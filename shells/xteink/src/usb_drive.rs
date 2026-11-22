//! The card shown to a computer as a USB drive (#32), on the ESP32-S3
//! devices: the chip's USB OTG core takes the cable's two pins from the USB
//! serial unit (the log, the cable transfer, the web flasher and the remote
//! are gone for the time), enumerates as a mass-storage device (embassy-usb's
//! device stack, `paperpaw_x4::usb_drive`'s Bulk-Only Transport and SCSI on
//! the card's blocks) and ends where the computer ejects the drive, the
//! reader presses Back, the cable goes or nobody came
//! (`paperpaw_x4::usb_drive::Session`).
//!
//! Nothing runs in the mode but this loop: the kernel is suspended, its
//! file system dropped and the card lent (`paperpaw_fat::CardSwap::lend`),
//! so the computer is the card's only writer. The loop polls the stack's
//! futures (no executor: the OTG interrupt only fills the endpoints'
//! buffers) and the reader's Back key between them.

use alloc::vec;
use core::future::poll_fn;
use core::pin::pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};

use embassy_futures::join::join;
use embassy_futures::select::{Either, select};
use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use embassy_usb::driver::{EndpointError, EndpointIn, EndpointOut};
use embassy_usb::{Builder, Handler};
use esp_hal::peripherals::{GPIO19, GPIO20, LPWR, USB_FS, USB_WRAP};
use esp_hal::usb::otg::Usb;
use esp_hal::usb::otg::embassy_usb_device::{Config as OtgConfig, Driver};
use log::{info, warn};
use paperpaw_fat::Blocks;
use paperpaw_x4::usb_drive::{Begin, Disk, Msc, Session, Stall, Why};

use crate::now;

/// A test vendor and product number (pid.codes' "Test PID"): a release
/// needs a product number of its own (owner's decision, #32).
const VENDOR: u16 = 0x1209;
const PRODUCT: u16 = 0x0001;
/// The full-speed bulk endpoints' packet size.
const PACKET: usize = 64;
/// What moves between the card and the endpoints at once: sixteen blocks, as
/// the card's driver moves them.
const IO: usize = 16 * paperpaw_x4::usb_drive::BLOCK;

/// The computer reset the interface (the class request).
static RESET: AtomicBool = AtomicBool::new(false);
/// A computer has set the device up, and has not suspended it.
static CONFIGURED: AtomicBool = AtomicBool::new(false);
static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// The status of the command that ejected the drive went back.
static EJECT_SENT: AtomicBool = AtomicBool::new(false);

/// The card's blocks as the drive's disk.
struct Card<'a>(&'a mut dyn Blocks);

impl Disk for Card<'_> {
    fn blocks(&mut self) -> Option<u32> {
        self.0.blocks().ok()
    }

    fn read(&mut self, lba: u32, data: &mut [u8]) -> bool {
        self.0.read_blocks(lba, data).is_ok()
    }

    fn write(&mut self, lba: u32, data: &[u8]) -> bool {
        self.0.write_blocks(lba, data).is_ok()
    }
}

/// What the stack asks of the class: the interface's reset and the number of
/// units, and what it tells of the bus.
struct Control;

impl Handler for Control {
    fn reset(&mut self) {
        CONFIGURED.store(false, Ordering::Relaxed);
        // A bus reset ends a suspend too: embassy-usb clears its own flag
        // then without telling `suspended(false)`, and a computer that
        // resumes with a reset would leave the drive "suspended" for good
        // (the mode ending `BusGone` while in use).
        SUSPENDED.store(false, Ordering::Relaxed);
    }

    fn configured(&mut self, configured: bool) {
        CONFIGURED.store(configured, Ordering::Relaxed);
    }

    fn suspended(&mut self, suspended: bool) {
        SUSPENDED.store(suspended, Ordering::Relaxed);
    }

    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        // Bulk-Only Mass Storage Reset.
        let ours = req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xff;
        ours.then(|| {
            RESET.store(true, Ordering::Relaxed);
            OutResponse::Accepted
        })
    }

    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        // Get Max LUN: one unit.
        let ours = req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xfe;
        let byte = buf.first_mut().filter(|_| ours)?;
        *byte = 0;
        Some(InResponse::Accepted(&buf[..1]))
    }
}

/// Shows `blocks` (the card) to a computer until the mode ends; `pressed`
/// is asked each turn (it also feeds the watchdog) and says whether the
/// reader asked to end it. Switches the cable's pins back to the USB serial
/// unit before it returns.
#[inline(never)]
pub fn run(blocks: &mut dyn Blocks, mut pressed: impl FnMut() -> bool) -> Why {
    RESET.store(false, Ordering::Relaxed);
    CONFIGURED.store(false, Ordering::Relaxed);
    SUSPENDED.store(false, Ordering::Relaxed);
    EJECT_SENT.store(false, Ordering::Relaxed);
    info!("USB drive: the cable goes to the card");
    let why = serve(blocks, &mut pressed);
    give_back_pins();
    info!("USB drive: the cable is the log's again ({why:?})");
    why
}

/// The USB serial unit has the pins again (the OTG core's switch is flipped
/// back and its pads off), as at the start.
fn give_back_pins() {
    LPWR::regs()
        .usb_conf()
        .modify(|_, w| w.sw_usb_phy_sel().clear_bit());
    USB_WRAP::regs()
        .otg_conf()
        .modify(|_, w| w.usb_pad_enable().clear_bit());
}

/// Builds the stack and polls it, and the class, until the session ends.
#[inline(never)]
fn serve(blocks: &mut dyn Blocks, pressed: &mut dyn FnMut() -> bool) -> Why {
    let mut ep_out_buffer = vec![0u8; 256];
    let mut config_buf = vec![0u8; 128];
    let mut bos_buf = vec![0u8; 16];
    let mut control_buf = vec![0u8; 64];
    let mut io = vec![0u8; IO];
    // SAFETY: the pins and the core are PaperPaw's alone for the mode; the
    // USB serial unit does not use them while the OTG core has them.
    let usb = unsafe { Usb::new_fs(USB_FS::steal(), GPIO20::steal(), GPIO19::steal()) };
    let driver = Driver::new(usb, &mut ep_out_buffer, OtgConfig::default());
    let mut config = embassy_usb::Config::new(VENDOR, PRODUCT);
    config.manufacturer = Some("PaperPaw");
    config.product = Some("PaperPaw reader card");
    config.serial_number = Some("PAPERPAW0001");
    // The class is the interface's.
    config.device_class = 0;
    config.device_sub_class = 0;
    config.device_protocol = 0;
    config.composite_with_iads = false;
    config.max_power = 250;
    let mut control = Control;
    let mut builder = Builder::new(
        driver,
        config,
        &mut config_buf,
        &mut bos_buf,
        &mut [],
        &mut control_buf,
    );
    // Mass storage, SCSI transparent command set, Bulk-Only Transport.
    let (mut ep_in, mut ep_out) = {
        let mut function = builder.function(0x08, 0x06, 0x50);
        let mut interface = function.interface();
        let mut alt = interface.alt_setting(0x08, 0x06, 0x50, None);
        (
            alt.endpoint_bulk_in(None, PACKET as u16),
            alt.endpoint_bulk_out(None, PACKET as u16),
        )
    };
    builder.handler(&mut control);
    let mut usb = builder.build();
    let mut disk = Card(blocks);
    let mut work = pin!(join(
        async {
            usb.run().await;
        },
        class(&mut ep_in, &mut ep_out, &mut disk, &mut io),
    ));
    let mut cx = Context::from_waker(Waker::noop());
    let mut session = Session::new(now().0);
    loop {
        if work.as_mut().poll(&mut cx) != Poll::Pending {
            warn!("USB drive: the stack stopped");
            return Why::Failed;
        }
        let bus = CONFIGURED.load(Ordering::Relaxed) && !SUSPENDED.load(Ordering::Relaxed);
        let back = pressed();
        if let Some(why) = session.step(now().0, bus, back, EJECT_SENT.load(Ordering::Relaxed)) {
            return why;
        }
    }
}

/// Waits until the computer reset the interface.
async fn reset_asked() {
    poll_fn(|_| {
        if RESET.swap(false, Ordering::Relaxed) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

/// The class: one command after the other for as long as the stack runs; a
/// reset of the interface, or a bus reset, drops what was under way.
async fn class<I: EndpointIn, O: EndpointOut>(
    ep_in: &mut I,
    ep_out: &mut O,
    disk: &mut Card<'_>,
    io: &mut [u8],
) {
    let mut msc = Msc::new();
    loop {
        ep_out.wait_enabled().await;
        ep_in.wait_enabled().await;
        match select(command(ep_in, ep_out, disk, &mut msc, io), reset_asked()).await {
            Either::First(Ok(())) => {}
            Either::First(Err(_)) | Either::Second(()) => msc.reset(),
        }
    }
}

/// One command: its block, the data and the status.
async fn command<I: EndpointIn, O: EndpointOut>(
    ep_in: &mut I,
    ep_out: &mut O,
    disk: &mut Card<'_>,
    msc: &mut Msc,
    io: &mut [u8],
) -> Result<(), EndpointError> {
    let mut block = [0u8; PACKET];
    let read = ep_out.read(&mut block).await?;
    let mut sent = 0usize;
    match msc.begin(disk, &block[..read]) {
        // Not a command: the next block is read; the computer resets the
        // interface if it wants to be heard.
        Begin::Invalid => return Ok(()),
        Begin::Done => {}
        Begin::In(_) => {
            while let Some(n) = msc.data_in(disk, io) {
                if n == 0 {
                    break;
                }
                for packet in io[..n].chunks(PACKET) {
                    ep_in.write(packet).await?;
                }
                sent += n;
            }
        }
        Begin::Out(len) => {
            let mut left = len as usize;
            while left > 0 {
                let want = left.min(io.len());
                let mut have = 0;
                while have < want {
                    let end = (have + PACKET).min(io.len());
                    have += ep_out.read(&mut io[have..end]).await?;
                }
                // A packet past the end (the computer sends more than the
                // command takes) is not kept.
                let take = have.min(want);
                msc.data_out(disk, &io[..take]);
                left -= take;
            }
        }
    }
    // Data the computer expected and did not get is ended as a stall would:
    // a short packet; data it sends and nothing takes is read and dropped.
    match msc.stall() {
        Stall::No => {}
        Stall::In => {
            if sent.is_multiple_of(PACKET) {
                ep_in.write(&[]).await?;
            }
        }
        Stall::Out => {
            let mut left = msc.unread() as usize;
            while left > 0 {
                left = left.saturating_sub(ep_out.read(&mut block).await?);
            }
        }
    }
    ep_in.write(&msc.csw()).await?;
    if msc.ejected() {
        EJECT_SENT.store(true, Ordering::Relaxed);
    }
    Ok(())
}
