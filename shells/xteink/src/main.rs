//! PaperPaw on the Xteink X4 (ESP32-C3), the X4 Pro (ESP32-S3, with a
//! touch screen) and the X4 Classic (ESP32-S3, the X4's buttons): the
//! kernel with the standard app, its screen, buttons, battery and SD card.
//! The feature `x4`, `x4-pro`, `x4-classic` or `x3` says which.
//!
//! - **Board**: which pin does what, in one place (`board`, for the X4 Pro
//!   `board_pro`, for the X4 Classic `board_classic`; the card of both
//!   `card_pro`).
//! - **Screen**: the kernel draws in bands of [`FRAME_ROWS`] rows; each
//!   goes straight into the display controller's memory, which keeps the
//!   frame (see `paperpaw_x4::display`).
//! - **Buttons**: read in a timer's interrupt and debounced, so presses
//!   count while PaperPaw is busy (`input`, `paperpaw_x4::buttons`).
//! - **Storage**: the SD card's FAT file system, with long file names
//!   (`paperpaw_fat`). Without a card, PaperPaw starts with nothing to
//!   read.
//! - **Sleep**: the sleep screen stays shown; the battery's power is cut,
//!   and the power button starts the X4 again, where it was.
//! - **Main loop**: `firmware`; remote control over USB for testing
//!   (`remote`) with the `remote` feature, on by default.

#![no_std]
#![no_main]

extern crate alloc;

#[cfg(all(feature = "test-image", feature = "release"))]
compile_error!("a test image (the high-water log, decision 156) must never be a release");

#[cfg(all(feature = "textprobe", not(feature = "x4")))]
compile_error!("textprobe is restricted to the X4 ESP32-C3 test build");

#[cfg(all(feature = "bluetooth", not(feature = "x4-pro")))]
compile_error!("Bluetooth is built for the X4 Pro alone (ADR 0027)");

#[cfg(feature = "remote")]
mod bench;
#[cfg(feature = "bluetooth")]
mod bluetooth;
#[cfg(feature = "x4")]
mod board;
#[cfg(feature = "x3")]
#[path = "board_x3.rs"]
mod board;
#[cfg(feature = "x4-pro")]
#[path = "board_pro.rs"]
mod board;
#[cfg(feature = "x4-classic")]
#[path = "board_classic.rs"]
mod board;
#[cfg(feature = "esp32c3")]
mod card;
#[cfg(feature = "esp32s3")]
#[path = "card_pro.rs"]
mod card;
mod clock;
mod crash;
mod device;
#[cfg(feature = "ppn5")]
mod device_host;
mod dual;
mod entropy;
mod firmware;
#[cfg(feature = "remote")]
mod gray_test;
#[cfg(feature = "test-image")]
mod high_water;
#[cfg(feature = "ppn5")]
mod host_net;
mod http;
mod input;
#[cfg(feature = "waveform-lab")]
mod lab;
#[cfg(feature = "x4-pro")]
mod light;
mod link;
mod native;
mod net;
mod network;
mod ota;
#[cfg(feature = "remote")]
mod peak;
mod power;
mod probe;
mod provision;
#[cfg(feature = "remote")]
mod remote;
mod roots;
mod safe_mode;
#[cfg(feature = "ppn5")]
mod secrets;
mod serve;
#[cfg(feature = "ppn5")]
mod serve7;
mod sockets;
mod stack;
mod startup;
#[cfg(feature = "textprobe")]
mod textprobe;
mod tls;
mod tls_slots;
#[cfg(feature = "remote")]
mod traced;
mod usb_lines;
mod usb_power;
mod volume;
mod wifi;

#[cfg(any(
    all(feature = "esp32c3", feature = "esp32s3"),
    all(feature = "x4", feature = "x3"),
    all(feature = "x4-pro", feature = "x4-classic"),
    all(feature = "esp32c3", not(any(feature = "x4", feature = "x3"))),
    all(
        feature = "esp32s3",
        not(any(feature = "x4-pro", feature = "x4-classic"))
    ),
))]
compile_error!("build for one device: the feature `x4`, `x4-pro`, `x4-classic` or `x3`");

use alloc::boxed::Box;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::main;
use esp_hal::rtc_cntl::sleep::LowPower;
use esp_hal::timer::PeriodicTimer;
use log::{info, warn};
use paperpaw_core::Kernel;
#[cfg(feature = "x4-pro")]
use paperpaw_platform::buttons as names;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, NoStorage, PixelFormat, Rotation,
    Storage,
};
use paperpaw_sdk::builtin::settings;
#[cfg(not(feature = "x4-pro"))]
use paperpaw_x4::buttons;
#[cfg(feature = "x4")]
use paperpaw_x4::display;
#[cfg(feature = "esp32s3")]
use paperpaw_x4::uc8179 as display;

use crate::board::Board;

/// The keyboard's size by default: large keys to tap with a finger on the
/// X4 Pro's 4.3" screen; small ones, chosen with the buttons, on the
/// others, leaving more of the page in view (the rows stay a computer
/// keyboard's whatever the size).
#[cfg(feature = "x4-pro")]
const KEYBOARD_SIZE: &str = "keyboard.size.large";
#[cfg(not(feature = "x4-pro"))]
const KEYBOARD_SIZE: &str = "keyboard.size.small";
use crate::firmware::{Firmware, State};

// PaperPaw's version and which firmware this is, read from updates.
esp_bootloader_esp_idf::esp_app_desc!(
    env!("PAPERPAW_VERSION"),
    ota::PROJECT,
    esp_bootloader_esp_idf::BUILD_TIME,
    esp_bootloader_esp_idf::BUILD_DATE,
    esp_bootloader_esp_idf::ESP_IDF_COMPATIBLE_VERSION,
    esp_bootloader_esp_idf::MMU_PAGE_SIZE,
    0,
    u16::MAX,
    esp_bootloader_esp_idf::SECURE_VERSION
);

/// Heap for PaperPaw: what the emulator's X4 profile allows, less what
/// Bluetooth takes of the same memory for good, and less the X4's arena
/// for plugins' hot code, which the heap gets once plugins are loaded as
/// far as they did not take it (`native::HOT_ARENA_LEN`).
const HEAP_SIZE: usize =
    (192 - BLUETOOTH_KB - S3_BAND_KB - X4_BAND_KB) * 1024 - native::HOT_ARENA_LEN;
/// The X4's band at the stack's end and its reserve (4 KB and the band's 2 KB,
/// `paperpaw_x4::stack_band`) take the top 7.7 KB of a stack that had only
/// 37.4 KB left once the interface-7 plugins were built in: the start's path
/// (some 33 KB) tripped the band in the first test image (#2556). The stack
/// grows by as much as the heap gives (`build.rs` checks its size), as the
/// ESP32-S3's does (`S3_BAND_KB`).
const X4_BAND_KB: usize = if cfg!(all(target_arch = "riscv32", feature = "ppn5")) {
    8
} else {
    0
};
/// The ESP32-S3's band at the stack's end and its reserve, in KB, where
/// interface-7 plugins are built in (`ppn5`): the stack grows by as much (the
/// heap gives it, `build.rs` checks the stack's size). 16 KB: 14,336 bytes of
/// reserve and the band's 2,048 (`paperpaw_x4::s3_band::SPARE`).
const S3_BAND_KB: usize = if cfg!(all(target_arch = "xtensa", feature = "ppn5")) {
    (paperpaw_x4::s3_band::SPARE / 1024) as usize
} else {
    0
};
/// Bluetooth's fixed share of the internal RAM, in KB, as measured: its
/// controller's code in IRAM (20.6 KB) and statics (4.4 KB). The stack,
/// after them, would lose it: the heap gives it back (`build.rs` checks
/// the stack's size).
const BLUETOOTH_KB: usize = if cfg!(feature = "bluetooth") { 26 } else { 0 };
/// More heap in the memory the bootloader used (65 KB are free after it):
/// room for the frame kept for the panel (see `paperpaw_x4::sent`).
const RECLAIMED_SIZE: usize = 64 * 1024;
/// Rows the kernel draws at once: 11.3 KiB of black and white. Fewer
/// bands draw faster: each costs the screens a pass over what they show.
const FRAME_ROWS: u16 = 192;
/// The X4 starts again if its main loop does not come by for this long
/// (the longest work, a book's first opening, takes some 5 s): by the
/// RTC's watchdog (counting in light sleep too), and the timer group's.
const WATCHDOG_S: (u64, u64) = (20, 30);
/// Until then, from the first moment, the X4 starts again if its start
/// hangs for this long: room for a display that does not answer (each
/// wait for it ends in an error after 10 s).
const START_WATCHDOG_S: u64 = 60;

#[cfg(feature = "x4")]
const DEVICE: DeviceInfo = DeviceInfo::new(
    "xteink-x4",
    "Xteink X4",
    DisplayInfo::new(
        display::WIDTH,
        display::HEIGHT,
        220,
        // What the panel shows until its four grays come: drawing in gray
        // only to send black and white costs time and memory.
        PixelFormat::Mono1,
        Rotation::Deg90,
        None,
    ),
    InputCaps::new(true, false, false, &buttons::ALL),
    MemoryClass::Small,
);

/// The X3 (experimental, not yet tried on one): the X4's buttons and
/// memory, a 3.68" panel of 528 × 792 upright (about 260 dpi). Uncertain
/// without a device: whether it lies turned as the X4's does.
#[cfg(feature = "x3")]
const DEVICE: DeviceInfo = DeviceInfo::new(
    "xteink-x3",
    "Xteink X3",
    DisplayInfo::new(528, 792, 260, PixelFormat::Mono1, Rotation::Deg90, None),
    InputCaps::new(true, false, false, &buttons::ALL),
    MemoryClass::Small,
);

/// The X4 Pro: touch besides its page and power buttons, and the memory
/// of larger devices (8 MB of PSRAM).
#[cfg(feature = "x4-pro")]
const DEVICE: DeviceInfo = DeviceInfo::new(
    "xteink-x4-pro",
    "Xteink X4 Pro",
    DisplayInfo::new(
        display::WIDTH,
        display::HEIGHT,
        217,
        PixelFormat::Mono1,
        Rotation::Deg90,
        None,
    ),
    InputCaps::new(
        true,
        true,
        false,
        &[
            names::PAGE_BACK,
            names::PAGE_FORWARD,
            names::HOME,
            names::POWER,
        ],
    ),
    MemoryClass::Large,
);

/// The X4 Classic: the X4's buttons, the X4 Pro's screen and memory.
#[cfg(feature = "x4-classic")]
const DEVICE: DeviceInfo = DeviceInfo::new(
    "xteink-x4-classic",
    "Xteink X4 Classic",
    DisplayInfo::new(
        display::WIDTH,
        display::HEIGHT,
        217,
        PixelFormat::Mono1,
        Rotation::Deg90,
        None,
    ),
    InputCaps::new(true, false, false, &buttons::ALL),
    MemoryClass::Large,
);

/// Random bytes from the chip's generator: fully random while the
/// radio is on, pseudo-random before (see `esp_hal::rng`).
fn random(buf: &mut [u8]) -> bool {
    esp_hal::rng::Rng::new().read(buf);
    true
}

/// One line of the host's own diagnostics (`KernelBuilder::device_log`): to
/// the log. A plain function without state that never calls the kernel, as
/// the builder asks; the line is at most 120 bytes of plugin ids and numbers
/// (never plugin text), and `log` formats it in place, so nothing is
/// allocated.
fn device_log(line: &str) {
    log::info!("host: {line}");
}

/// How this start came, from the chip's reason for its reset: a sleep
/// that cut the power ends by the power coming again.
fn started() -> paperpaw_sdk::power::Start {
    use esp_hal::rtc_cntl::SocResetReason;
    use paperpaw_sdk::power::Start;
    match power::last_run().2 {
        Some(SocResetReason::ChipPowerOn) => Start::PowerOn,
        Some(SocResetReason::CoreDeepSleep) => Start::Wake,
        Some(_) => Start::Restart,
        None => Start::Unknown,
    }
}

/// The heap and the stack free now, for diagnostics and plugins
/// (`paperpaw_sdk::memory`).
fn memory() -> Option<paperpaw_sdk::memory::MemoryUse> {
    let (stack_free, stack_size) = stack::left();
    let bytes = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let free = esp_alloc::HEAP.free();
    let mut used =
        paperpaw_sdk::memory::MemoryUse::new(bytes(free), bytes(free + esp_alloc::HEAP.used()))
            .with_stack(bytes(stack_free), bytes(stack_size))
            .with_largest_free(bytes(largest_block()));
    // The X4 Pro's PSRAM beside its internal RAM.
    let stats = esp_alloc::HEAP.stats();
    if stats.region_stats.iter().flatten().count() > 1 {
        for region in stats.region_stats.iter().flatten() {
            let part =
                paperpaw_sdk::memory::MemoryRegion::new(bytes(region.free), bytes(region.size));
            let external = region
                .capabilities
                .contains(esp_alloc::MemoryCapability::External);
            used = used.with_region(if external { part.external() } else { part });
        }
    }
    Some(used)
}

/// The largest block of memory that could be had now (found by asking for
/// blocks, given back at once; not counted in the heap's peak).
pub fn largest_block() -> usize {
    #[cfg(feature = "remote")]
    return peak::probing(search_largest_block);
    #[cfg(not(feature = "remote"))]
    search_largest_block()
}

fn search_largest_block() -> usize {
    let (mut can, mut cannot) = (0, esp_alloc::HEAP.free() + 1);
    while cannot - can > 64 {
        let size = (can + cannot) / 2;
        if alloc::vec::Vec::<u8>::new().try_reserve_exact(size).is_ok() {
            can = size;
        } else {
            cannot = size;
        }
    }
    can
}

/// Microseconds since start, for measuring what plugins take.
pub fn micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

fn now() -> Instant {
    Instant(
        esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_millis(),
    )
}

#[main]
fn main() -> ! {
    // Counted first: a start that dies before anything else (#2556) must
    // not go on dying; after two, this one leaves the band off and goes
    // without native plugins (`safe_mode::check`).
    let earlier = safe_mode::begin();
    stack::paint();
    if !paperpaw_x4::start_count::early::refuses(earlier) {
        stack::arm_band();
    }
    let mut firmware = start();
    // The radios the kernel has are told to the device once, now that the
    // kernel exists (it answers their state itself).
    #[cfg(feature = "ppn5")]
    {
        device_host::present(&firmware.device, &firmware.kernel);
        device_host::check(&firmware.device, &firmware.kernel);
    }
    firmware.run()
}

/// Everything before the main loop, in frames of their own: what only the
/// start needs is off the stack once the loop runs, where only the
/// firmware stays (in `main`'s frame). The kernel is built last, over
/// little else: its start goes deepest.
#[inline(never)]
fn start() -> Firmware {
    let mut ready = prepare();
    let building = micros();
    info!("startup: building the kernel");
    let bundle = ready.staged.other_slot.take();
    let flash = ready.flash.clone();
    #[cfg(all(feature = "native-on", feature = "esp32c3"))]
    let device_table = match &ready.code {
        CodePlugins::Load(loader) => Some(loader.device_table()),
        _ => None,
    };
    let firmware = Firmware {
        panel: ready.panel,
        network: ready.network,
        ota: ready.ota,
        power_latch: ready.power_latch,
        usb_power: ready.usb_power,
        delay: ready.delay,
        usb: ready.usb,
        #[cfg(feature = "remote")]
        remote: remote::Remote,
        #[cfg(feature = "x4-pro")]
        light: ready.light,
        card_swap: ready.card_swap,
        #[cfg(feature = "ppn5")]
        device: ready.device,
        #[cfg(feature = "ppn5")]
        secrets: secrets::Secrets::new(ready.secrets),
        #[cfg(all(feature = "native-on", feature = "esp32c3"))]
        device_table,
        state: State::default(),
        // Last, and built right in its place: the kernel is large.
        kernel: kernel(
            ready.storage,
            ready.card_trouble,
            ready.firmware,
            ready.code,
            ready.staged,
        ),
    };
    // Its plugins installed: the web flasher's bundle goes (see
    // `native::bundle`).
    if let Some(at) = bundle {
        native::bundle::forget(&flash, at);
    }
    // Nothing here borrows `firmware`: a local that is borrowed after it is
    // built cannot be returned in place, and the copy takes 7.8 KB more of the
    // stack (#2556: with the band, the start's path no longer fitted). What
    // needs the kernel built runs in `main`, on the firmware where it lies.
    power::mark(power::at::START_KERNEL);
    info!(
        "startup: kernel ready in {} ms",
        (micros() - building) / 1000
    );
    firmware
}

/// What [`prepare`] made ready for the firmware and its kernel.
struct Ready {
    panel: board::Panel,
    network: network::Network,
    ota: Option<ota::Ota>,
    power_latch: esp_hal::gpio::Output<'static>,
    usb_power: board::UsbPower,
    delay: esp_hal::delay::Delay,
    usb: usb_lines::UsbLines,
    #[cfg(feature = "x4-pro")]
    light: Option<light::Light>,
    storage: Box<dyn Storage>,
    /// What watches the card being taken out and put back, if there is one.
    card_swap: Option<Box<dyn volume::Swap>>,
    /// Why there is no card to keep things on, if there is none.
    card_trouble: Option<paperpaw_core::CardTrouble>,
    firmware: Option<paperpaw_sdk::firmware::Firmware>,
    code: CodePlugins,
    /// The device behind `core.device`.
    #[cfg(feature = "ppn5")]
    device: alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
    /// What the loader leaves of `core.secrets` for the main loop.
    #[cfg(feature = "ppn5")]
    secrets: secrets::Kept,
    staged: native::bundle::Staged,
    /// The flash, to let the web flasher's bundle go once installed.
    flash: ota::Flash,
}

/// The chip, the board and the card, up to the kernel.
#[inline(never)]
fn prepare() -> Ready {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    // The Pro must keep its power before allocating memory or checking OTA.
    #[cfg(feature = "x4-pro")]
    let power_latch = Board::hold_power();
    // `init` stopped the watchdogs: a start that hangs begins again.
    power::watch_start(START_WATCHDOG_S);
    // The PSRAM of the ESP32-S3 devices first, so PaperPaw's memory comes
    // from there and the chip's own stays for the radio and DMA, which
    // need it.
    #[cfg(feature = "esp32s3")]
    // SAFETY: only taken here; the board does not use it.
    esp_alloc::psram_allocator!(
        unsafe { esp_hal::peripherals::PSRAM::steal() },
        esp_hal::psram
    );
    // The heap before the board: the board keeps its bus there.
    esp_alloc::heap_allocator!(size: HEAP_SIZE);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: RECLAIMED_SIZE);
    // Plugins choose internal RAM or PSRAM for their data (ADR 0030).
    paperpaw_sdk::memory::set_placer(native::api::alloc_in);
    esp_println::logger::init_logger_from_env();
    stack::report_band();
    // The test image says what the run before ended with, and starts counting.
    #[cfg(feature = "test-image")]
    high_water::start();
    // What hot code in internal RAM gains on this chip (test builds).
    #[cfg(feature = "remote")]
    bench::run();
    power::note_start();
    power::mark(power::at::START);
    let (place, naps, reason) = power::last_run();
    info!("PaperPaw starting (reset: {reason:?}, last at {place}, naps {naps})");
    // Before anything that could fail or hang, the board too: an update
    // on trial that failed is undone here (a restart into the firmware
    // before it). One handle on the flash, for updates and native
    // plugins.
    // SAFETY: only taken here; the board does not use it.
    let flash = unsafe { esp_hal::peripherals::FLASH::steal() };
    let flash: ota::Flash = alloc::rc::Rc::new(core::cell::RefCell::new(
        esp_bootloader_esp_idf::partitions::FlashStorage::new(flash),
    ));
    let ota = ota::Ota::start(flash.clone());
    power::mark(power::at::START_OTA);
    // Taken only now: an update that failed its trial goes back after its
    // note (`install_note`), which has the update's panic message.
    // A start that goes back (a failed trial) leaves the message where it
    // is: the firmware it goes back to reads it as its own. Whether it does
    // is known once the card has said this is no dual-boot setup (`dual`).
    let going_back = ota.as_ref().is_some_and(ota::Ota::plans_to_go_back);
    let panicked = if going_back {
        crash::last()
    } else {
        crash::take_last()
    };
    if let Some(message) = &panicked {
        warn!("the last run panicked: {message}");
    }
    #[cfg(not(any(feature = "x4-classic", feature = "x4-pro")))]
    let board = Board::take(p);
    #[cfg(feature = "x4-pro")]
    let board = Board::take(p, power_latch);
    // The X4 Classic's panel as the factory wrote it down, as its stock
    // firmware reads it.
    #[cfg(feature = "x4-classic")]
    let board = Board::take(p, probe::factory_controller(&flash));
    power::mark(power::at::START_BOARD);
    let (storage, card_swap, card_trouble) = match card::mount(board.card) {
        Ok(mounted) => (mounted.storage, Some(mounted.swap), None),
        Err(trouble) => (Box::new(NoStorage) as Box<dyn Storage>, None, Some(trouble)),
    };
    // The reader's choice and the bootloader say whether this is a dual-boot
    // setup, nothing in the other firmware's partition or the OTA data
    // changed: only then the start does what it needs (a tool's update
    // marked as on trial, one undone), as before anything else is written
    // to the card. An update this firmware installed, or one that failed its
    // trial, was marked before the board's start (`Ota::start`), so that a
    // hang there still ends in the way back.
    let mut ota = ota;
    let dual = dual::settle(&mut ota, &flash, storage.as_ref());
    // A failed trial that the setup left alone: the message is no longer
    // the other firmware's to read.
    if going_back && ota.as_ref().is_some_and(|ota| ota.failed_trial().is_none()) {
        crash::take_last();
    }
    // After the card, which the note below goes to; before anything else
    // is written to it.
    #[cfg(feature = "x3")]
    let ota = x3_panel_or_back(ota, &board.display, storage.as_ref());
    power::mark(power::at::START_CARD);
    install_note(ota.as_ref(), storage.as_ref(), panicked.as_deref());
    // What the card keeps under PaperPaw's old name, moved before anything
    // is written to its folder (the kernel does it again, finding nothing).
    paperpaw_sdk::data::take_over_old_name(storage.as_ref());
    // Wi-Fi the web flasher wrote with the firmware: to the card, for the
    // kernel to keep at its start.
    provision::take(&flash, storage.as_ref());
    // For bug reports: on the card, and said over USB.
    if let Some(message) = &panicked {
        crash::keep(storage.as_ref(), message);
    }
    #[cfg(feature = "remote")]
    remote::note_panic(panicked);
    let mut panel = board.panel;
    // For reports from devices with other controllers: what it answered.
    info!(
        "display: {}, driven as {}",
        board.display,
        panel.controller()
    );
    // What a UC8179's OTP holds, if it was read (developer builds).
    #[cfg(feature = "waveform-lab")]
    probe::log_otp();
    #[cfg(any(feature = "waveform-lab", feature = "x4-pro"))]
    panel.set_trial_power(probe::otp_power());
    // The X4 Pro's menus take the waveform of the registers where its
    // controller is a UC8179 (`device`, `firmware`).
    device::set_uc8179(
        cfg!(feature = "x4-pro") && panel.controller() == paperpaw_x4::probe::Controller::Uc8179,
    );
    // Only an SSD1677's waveforms can be tried or set (`device`, `lab`).
    #[cfg(feature = "x4")]
    device::set_ssd1677(panel.controller() == paperpaw_x4::probe::Controller::Ssd1677);
    // The waveform lab tries those of the SSD1677 and the UC8179.
    #[cfg(feature = "waveform-lab")]
    lab::set_support(match panel.controller() {
        paperpaw_x4::probe::Controller::Ssd1677 => lab::Support::Ssd1677,
        paperpaw_x4::probe::Controller::Uc8179 => lab::Support::Uc8179,
        _ => lab::Support::None,
    });
    if let Err(e) = panel.init() {
        warn!("display: {e:?}");
    }
    power::watch(
        esp_hal::rtc_cntl::Rtc::new(board.rtc),
        board.timers1.wdt,
        WATCHDOG_S,
    );
    // The radio (on when the kernel says so) and what goes over it.
    #[cfg_attr(not(feature = "ppn5"), allow(unused_mut))]
    let mut network = network::Network::new(wifi::Parts {
        timer: board.timers1.timer0,
        interrupt: board.radio.1,
        wifi: board.radio.0,
    });
    input::start(input::Sampler::new(
        PeriodicTimer::new(board.timers.timer0),
        board.keys,
        board.power_button,
    ));
    // The clock chip of the ESP32-S3 devices and the X3 kept the time
    // while it was off.
    #[cfg(any(feature = "esp32s3", feature = "x3"))]
    if let Some(unix_ms) = board.clock_ms {
        clock::set(now(), unix_ms);
    }
    // Plugins the web flasher wrote behind the image, onto the card first.
    let staged = {
        let mut startup = startup::Startup::new(&mut panel);
        let staged = native::bundle::stage(&flash, storage.as_ref(), !dual, &mut |progress| {
            startup.update(progress)
        })
        .unwrap_or_default();
        startup.starting_if_needed();
        staged
    };
    #[cfg(feature = "ppn5")]
    let host_net = host_net::Bridge::new();
    #[cfg(feature = "ppn5")]
    network.attach(host_net.clone());
    // The device behind `core.device`, made before the code plugins are
    // decided so that the loader can wire it.
    #[cfg(feature = "ppn5")]
    let device = device_host::make(
        ota.as_ref().map(ota::Ota::running).as_ref(),
        &staged.not_installed,
        started(),
    );
    // English hyphenation is a data plugin on the card: the flasher puts it
    // there, but an update of a firmware that had English built in (Beta 3)
    // leaves none. Put it there once if no start did (`paperpaw-default-packs`);
    // after the flasher's bundle, so that its own copy is found, not doubled.
    native::seed::hyphenation(storage.as_ref());
    // What the loader leaves of `core.secrets` for the main loop.
    #[cfg(feature = "ppn5")]
    let kept = secrets::Kept::default();
    let code = code_plugins(
        ota.as_ref(),
        &flash,
        #[cfg(feature = "ppn5")]
        host_net,
        #[cfg(feature = "ppn5")]
        device.clone(),
        #[cfg(feature = "ppn5")]
        kept.clone(),
    );
    #[cfg(feature = "ppn5")]
    network.attach_device(device.clone());
    // Safe mode is known only now: no loader is made then, so the device is told.
    #[cfg(feature = "ppn5")]
    if let CodePlugins::SafeMode(mode) = &code {
        device_host::tell_native(&device, Some(mode));
    }
    // Light sleeps while idle on battery (allowed only by the main loop),
    // the deep one at the end.
    power::init(LowPower::new(board.low_power));
    Ready {
        firmware: ota.as_ref().map(ota::Ota::firmware),
        panel,
        network,
        ota,
        #[cfg(not(feature = "x3"))]
        power_latch: board.power_latch,
        // The X3's GPIO 13 is its card's power, let go before deep sleep
        // as the others' latch.
        #[cfg(feature = "x3")]
        power_latch: board.card_power,
        usb_power: board.usb_power,
        delay: board.delay,
        // Lines from a computer on USB; the log goes out the same way.
        usb: usb_lines::UsbLines::new(board.usb),
        #[cfg(feature = "x4-pro")]
        light: board.light,
        storage,
        card_swap,
        card_trouble,
        code,
        #[cfg(feature = "ppn5")]
        device,
        #[cfg(feature = "ppn5")]
        secrets: kept,
        staged,
        flash,
    }
}

/// An X3 with the newer UC8279d, which this build does not drive (its
/// waveforms are not taken over): its screen would stay as it was. Said
/// in the log and in a note on the card (the firmware before it, which it
/// goes back to, says nothing of it), and an update on trial that brought
/// it goes back at once to that firmware.
#[cfg(feature = "x3")]
fn x3_panel_or_back(
    mut ota: Option<ota::Ota>,
    probe: &paperpaw_x4::probe::Probe,
    storage: &dyn Storage,
) -> Option<ota::Ota> {
    if probe.x3_uc8279d() {
        log::error!("display: a UC8279d ({probe}), which this build does not drive: nothing shows");
        // Only an update on trial goes back (`give_up` below); a cable or
        // web-flasher install, or one already kept, stays with a blank
        // screen, and the note says that instead.
        let note = alloc::format!(
            "{}",
            paperpaw_x4::probe::X3Note {
                probe,
                version: env!("PAPERPAW_VERSION"),
                went_back: ota.as_ref().is_some_and(ota::Ota::on_trial),
            }
        );
        // Not again at every start of the same note.
        let file = paperpaw_x4::probe::X3_NOTE_FILE;
        if storage.size(file).ok() != Some(note.len() as u64)
            && let Err(e) = storage.write(file, note.as_bytes())
        {
            log::warn!("card: the display note was not written: {e:?}");
        }
        if let Some(ota) = ota.as_mut() {
            ota.give_up("this X3's display is not driven");
        }
    }
    ota
}

/// The name of this device in the note on the card.
const DEVICE_NAME: &str = if cfg!(feature = "x3") {
    "Xteink X3"
} else if cfg!(feature = "x4-pro") {
    "Xteink X4 Pro"
} else if cfg!(feature = "x4-classic") {
    "Xteink X4 Classic"
} else {
    "Xteink X4"
};

/// The note of an update on trial (`paperpaw_x4::install_note`), for the
/// reader whose first start does not get through: the firmware before it
/// (CrossPoint) takes its place back and says nothing, and a USB-locked
/// device has no log. An update on trial writes "started" (the file is
/// there if PaperPaw got as far as the card; it goes when the trial is
/// over), and a start that finds the last one did not end cleanly writes
/// the reason and the place it ended, then goes back.
///
/// The way back never waits for this: the failed update is marked bad
/// before (`Ota::start`), so a hang or a crash here ends in the same
/// restart (the start watchdog, a panic). Nothing but the card is touched.
fn install_note(ota: Option<&ota::Ota>, storage: &dyn Storage, panicked: Option<&str>) {
    use paperpaw_x4::install_note::{FILE, InstallNote, Kind, is_started};
    let Some(ota) = ota else {
        return;
    };
    let failed = ota.failed_trial();
    if failed.is_none() && !ota.on_trial() {
        return;
    }
    // The first start of a trial writes the note (over an older one of an
    // earlier install); the starts that go on with it, after a sleep, do
    // not write it again.
    if failed.is_none() && ota.ran_before() > 0 {
        return;
    }
    let (place, _, reason) = power::last_run();
    let reset = alloc::format!("{reason:?}");
    let note = InstallNote {
        kind: if failed.is_some() {
            // Whether the note of the first start is there (and not an
            // older note of an earlier install).
            let mut head = [0u8; 64];
            let read = storage.read_at(FILE, 0, &mut head).unwrap_or(0);
            Kind::Failed {
                panic: panicked,
                started: is_started(&head[..read]),
            }
        } else {
            Kind::Started {
                by_bootloader: ota.marked_by_bootloader(),
            }
        },
        device: DEVICE_NAME,
        version: env!("PAPERPAW_VERSION"),
        ran_ms: failed.unwrap_or_else(|| ota.ran_before()),
        reset: &reset,
        place: power::place_name(place),
    };
    let text = alloc::format!("{note}");
    match storage.write(FILE, text.as_bytes()) {
        Ok(()) => info!("card: the install note was written ({} bytes)", text.len()),
        Err(e) => warn!("card: the install note was not written: {e:?}"),
    }
    if failed.is_some() {
        ota.go_back();
    }
}

/// How this start treats code plugins (ADR 0024).
enum CodePlugins {
    Load(native::Loader),
    /// Safe mode: none loaded.
    SafeMode(paperpaw_sdk::native::SafeMode),
    /// None this start: an update is on trial, which must show it starts
    /// by itself. What the firmware takes is told once it is kept (#164).
    OnTrial,
}

/// What this firmware loads, as plugins see it ([`paperpaw_sdk::Env`]),
/// with the plugins of the web flasher not installed.
fn native(
    not_installed: alloc::vec::Vec<paperpaw_sdk::native::NotInstalled>,
) -> paperpaw_sdk::native::Native {
    let mut native = crate::native::this_firmware();
    native.not_installed = not_installed;
    native
}

/// Loads nothing: in safe mode the kernel asks for no code.
struct NoCode;

impl paperpaw_core::CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_sdk::Plugin>, paperpaw_sdk::PluginProblem> {
        Err(paperpaw_sdk::PluginProblem::SafeMode)
    }
}

/// Whether code plugins are loaded at this start, after the key for safe
/// mode was looked for and the failed starts counted.
fn code_plugins(
    ota: Option<&ota::Ota>,
    flash: &ota::Flash,
    #[cfg(feature = "ppn5")] host_net: alloc::rc::Rc<host_net::Bridge>,
    #[cfg(feature = "ppn5")] device: alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
    #[cfg(feature = "ppn5")] secrets: secrets::Kept,
) -> CodePlugins {
    if ota.is_some_and(ota::Ota::on_trial) {
        info!("native: an update on trial starts without code plugins");
        native::no_hot_code();
        return CodePlugins::OnTrial;
    }
    if let Some(safe_mode) = safe_mode::check(input::held(safe_mode::KEY)) {
        warn!(
            "native: safe mode ({:?}), without code plugins",
            safe_mode.reason
        );
        native::no_hot_code();
        return CodePlugins::SafeMode(safe_mode);
    }
    #[cfg(feature = "esp32s3")]
    let place = {
        let _ = flash;
        // SAFETY: the board does not use SHA; this loader owns it until
        // all plugins are loaded, before the radio is started.
        native::Place::new(unsafe { esp_hal::peripherals::SHA::steal() })
    };
    #[cfg(feature = "esp32c3")]
    let place = native::Place::new(flash.clone());
    CodePlugins::Load(native::Loader::new(
        place,
        #[cfg(feature = "ppn5")]
        host_net,
        #[cfg(feature = "ppn5")]
        device,
        #[cfg(feature = "ppn5")]
        secrets::Wiring::new(flash.clone(), secrets),
    ))
}

/// The status lines of the X4s by default (see `setting_default_text`):
/// fewer items than on large screens.
const CALM_STATUS: [(&str, &str); 6] = [
    (
        "reader.status.bottom-left",
        "core.status.clock, reader.status.aloud, lock.touch-status",
    ),
    ("reader.status.bottom-center", ""),
    ("reader.status.bottom-right", "reader.status.page"),
    ("home.status.left", "core.status.clock"),
    ("home.status.center", ""),
    (
        "home.status.right",
        "lock.touch-status, core.status.battery",
    ),
];

/// The kernel with the standard app, as the X4 wants it.
#[expect(
    clippy::expect_used,
    reason = "the plugins are built in: a kernel that cannot be built is a bug to see at once"
)]
// A frame of its own, over the little `start` keeps.
#[inline(never)]
fn kernel(
    storage: Box<dyn Storage>,
    card_trouble: Option<paperpaw_core::CardTrouble>,
    firmware: Option<paperpaw_sdk::firmware::Firmware>,
    code: CodePlugins,
    staged: native::bundle::Staged,
) -> Kernel {
    // A sleep cut the power or restarted the chip: waking is this start,
    // which plugins hear as `core.resume` after `core.started`. The mark
    // the sleep left is taken here, so that only this start finds it.
    let start = started();
    let after_sleep =
        paperpaw_x4::slept::take(storage.as_ref()) || start == paperpaw_sdk::power::Start::Wake;
    #[cfg(feature = "remote")]
    let storage: Box<dyn Storage> = Box::new(traced::Traced(storage));
    let builder = Kernel::builder(DEVICE)
        .storage_boxed(storage)
        .clock(micros)
        // Fully random while the radio is on, as when serving.
        .random(random)
        .device_log(device_log)
        .memory(memory)
        .started(start)
        .after_sleep(after_sleep)
        // Black and white, frames drawn and sent while none refreshes; the
        // refreshes' times as they are measured (ADR 0030).
        .panel(paperpaw_sdk::realtime::PanelInfo::new(2))
        // The panel refreshes on its own: a mark shows at once while slow
        // work goes on; for work that took as long as the refresh showing
        // it (~504 ms on the X4), so shorter work does not wait (#118).
        .busy_mark(true)
        .busy_mark_after(500)
        // Built-in packs are read in place; glyphs of packs on the card
        // (other scripts) are kept here once read. Empty until one is.
        .glyph_cache(12 * 1024)
        .frame_rows(FRAME_ROWS)
        .row_align(8)
        // The panel takes bands turned any way (`display`).
        .rotation(true)
        .sleep(true)
        // A plugin may ask to switch the device off (`Command::PowerOff`);
        // `slept_for` stays unknown after a switch-off: the card keeps no
        // clock, so plugins hear no time asleep for such a start.
        .power_off(true)
        // Buttons are read less often in Saving and Offline
        // (`buttons::read_interval_ms`); the clock stays (ADR 0025).
        .power(power_controls())
        // TLS sockets (and HTTPS with them): one session at a time on the
        // X4's 256 KiB, two on the X4 Pro and Classic (`tls_slots`).
        .tls_sessions(tls_slots::SESSIONS);
    // The X4's row of hints under the screen (the X4 Classic has its
    // buttons); the X4 Pro is touched.
    #[cfg(feature = "x4")]
    let builder = builder.button_row(&buttons::ROW).catalog_device("x4");
    #[cfg(feature = "x4-pro")]
    let builder = builder.catalog_device("x4-pro");
    #[cfg(feature = "x4-classic")]
    let builder = builder
        .button_row(&buttons::ROW)
        .catalog_device("x4-classic");
    // The X3 has the X4's buttons, and runs the X4's code plugins (the
    // same chip and fingerprint): the plugin library's X4 builds.
    #[cfg(feature = "x3")]
    let builder = builder.button_row(&buttons::ROW).catalog_device("x4");
    let builder = builder
        // A full refresh takes 3.5 s on its panel: screens change fast.
        .setting_default(&settings::FLASH_SCREENS, false)
        .setting_default(&settings::UI_SIZE, 8)
        .setting_default(&settings::FONT_SIZE, 9)
        // The keyboard's size, unless the reader chose another (see
        // `KEYBOARD_SIZE`).
        .setting_default_text("keyboard.size", KEYBOARD_SIZE);
    // A small screen: calm status lines, the time and the page while
    // reading (and the mark of reading aloud while it reads), the time
    // and the battery elsewhere. Every item stays to be chosen.
    let builder = CALM_STATUS
        .iter()
        .fold(builder, |builder, (id, value)| {
            builder.setting_default_text(id, value)
        })
        // PaperPaw switches the radio (`network`); on USB it stays on in a
        // network joined before, for transfers and for trying things.
        .wifi(true)
        .setting_default(&paperpaw_plugin_wifi::WHILE_CHARGING, true);
    #[cfg(feature = "bluetooth")]
    let builder = builder.bluetooth(bluetooth::LINKS as u8);
    let builder = match firmware {
        Some(firmware) => builder.firmware(firmware),
        None => builder,
    };
    let builder = match card_trouble {
        Some(trouble) => builder.card_trouble(trouble),
        None => builder,
    };
    let builder = match code {
        CodePlugins::Load(loader) => {
            builder.code_plugins(native(staged.not_installed), Box::new(loader))
        }
        CodePlugins::SafeMode(safe_mode) => builder
            .code_plugins(native(staged.not_installed), Box::new(NoCode))
            .safe_mode(safe_mode),
        CodePlugins::OnTrial => builder.native_after_trial(native(staged.not_installed)),
    };
    let builder = staged
        .installs
        .into_iter()
        .fold(builder, |builder, (id, folder)| {
            builder.install_plugin(id, folder)
        });
    // The plugins the store offers as files are not built in: the
    // firmware stays lean, and the reader installs those wanted.
    let builder = paperpaw_app::install_with(
        builder,
        paperpaw_app::Fonts::Packs,
        paperpaw_app::StorePlugins::Left,
    )
    .plugin(device::X4);
    #[cfg(feature = "frame-test")]
    let builder = builder.plugin(paperpaw_sample_frame_test::FrameTest);
    // Its warnings are logged as the main loop starts: taken here, the
    // kernel would be one more copy on the stack.
    builder.build().expect("kernel")
}

/// What the X4 shells change by the power level: the buttons' reading, and
/// the X4 Pro's front light.
fn power_controls() -> paperpaw_sdk::power::PowerControls {
    paperpaw_x4::controls::power_controls(cfg!(feature = "x4-pro"))
}
