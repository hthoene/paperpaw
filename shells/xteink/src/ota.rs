//! Updates over the air: the flash has two app partitions (see
//! `partitions.csv`). An update is written to the one not running and
//! chosen for the next start; the bootloader's choice is the OTA data
//! partition's. How a start, a trial and an install go is
//! `paperpaw_x4::updates`, tested on a computer as it runs here
//! (`crates/paperpaw-x4/tests/update_path.rs`); this is what only the
//! device has: its flash, where the partitions are and which one runs,
//! PaperPaw's keys and this firmware's name, the log and the restarts.
//!
//! The flash's table says where the partitions are and how large, also
//! another firmware's (CrossPoint's), which PaperPaw never writes.

use alloc::boxed::Box;

use esp_bootloader_esp_idf::partitions::{self, FlashStorage, PARTITION_TABLE_MAX_LEN};
use log::{info, warn};
use paperpaw_platform::Storage;
use paperpaw_sdk::firmware::{Firmware, FirmwareState, InstallFailure, Plugins};
use paperpaw_x4::ota::{self, FlashError, Start};
use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN};
use paperpaw_x4::updates::{Installing, Updates};

// The plugin crates built in (`PLUGINS`), written by `build.rs`.
include!(concat!(env!("OUT_DIR"), "/plugins.rs"));

/// The chip id in this device's images: the X4's ESP32-C3, the ESP32-S3
/// of the X4 Pro and the X4 Classic.
#[cfg(feature = "esp32c3")]
const CHIP: u16 = paperpaw_sdk::firmware::CHIP_ESP32_C3;
#[cfg(feature = "esp32s3")]
const CHIP: u16 = paperpaw_sdk::firmware::CHIP_ESP32_S3;
/// Which firmware this is (the image says it, `main.rs`): updates must be
/// the same. The X4 Pro and the X4 Classic share their chip, not their
/// boards.
#[cfg(feature = "x4")]
pub const PROJECT: &str = "paperpaw-xteink";
#[cfg(feature = "x3")]
pub const PROJECT: &str = "paperpaw-xteink-x3";
#[cfg(feature = "x4-pro")]
pub const PROJECT: &str = "paperpaw-xteink-pro";
#[cfg(feature = "x4-classic")]
pub const PROJECT: &str = "paperpaw-xteink-classic";
/// This device in CrossPoint's board tag (its `FirmwareBoardTag`): the
/// X4's and the X3's one image is `x4`.
#[cfg(not(feature = "esp32s3"))]
const BOARD_TAG: &str = "x4";
#[cfg(feature = "x4-pro")]
const BOARD_TAG: &str = "x4pro";
#[cfg(feature = "x4-classic")]
const BOARD_TAG: &str = "x4c";

/// The flash, one handle for updates and native plugins (`native`): a
/// second one would find its size anew, and not always.
pub type Flash = alloc::rc::Rc<core::cell::RefCell<FlashStorage<'static>>>;

/// The flash as `paperpaw_x4::updates` reads and writes it, in the chip's
/// addresses. A handle somebody else holds is a flash error, never a
/// panic: a panic before the trial mark would repeat on every start.
struct Chip(Flash);

impl ota::Flash for Chip {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.0
            .try_borrow_mut()
            .map_err(|_| FlashError)?
            .read(at, buf)
            .map_err(|_| FlashError)
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        self.0
            .try_borrow_mut()
            .map_err(|_| FlashError)?
            .write_nor(at, data)
            .map_err(|_| FlashError)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.0
            .try_borrow_mut()
            .map_err(|_| FlashError)?
            .erase(from, to)
            .map_err(|_| FlashError)
    }
}

/// Updates on this device.
pub struct Ota(Updates<Chip>);

impl Ota {
    /// Looks at what started and why (see `paperpaw_x4::updates`): an
    /// update that failed its trial restarts into the firmware before it
    /// at once. `None` without two app partitions (flashed before they
    /// came: the web flasher sets them up once).
    // A frame of its own, off the stack before the kernel is built.
    #[inline(never)]
    pub fn start(shared: Flash) -> Option<Self> {
        let Some(Partitions { slots, running }) = shared
            .try_borrow_mut()
            .ok()
            .and_then(|mut flash| partitions_of(&mut flash))
        else {
            warn!("OTA: no partitions for updates");
            return None;
        };
        let this = Firmware::new(env!("PAPERPAW_VERSION"), PROJECT, CHIP)
            .keys(paperpaw_x4::updates::KEYS.to_vec())
            .plugins(Plugins::read_static(&PLUGINS).unwrap_or_default());
        let (updates, start) =
            Updates::start(Chip(shared), &slots, running, this, BOARD_TAG).ok()?;
        match start {
            Some(Start::Kept) => info!("OTA: started {running}"),
            Some(Start::Undone) => warn!("OTA: the update did not start, {running} is back"),
            Some(Start::Trial { ran }) => {
                info!("OTA: an update on trial in {running}, {ran} ms run before");
            }
            Some(Start::Failed) => {
                warn!("OTA: the update failed on trial: back to the one before");
                esp_hal::system::software_reset();
            }
            None => warn!("OTA: could not mark the start of {running}"),
        }
        Some(Self(updates))
    }

    /// An update on trial that cannot work on this device (an X3 whose
    /// display it does not drive, see `main`): marked bad, and the device
    /// starts again into the firmware before it, as after a failed trial.
    /// Nothing where none is on trial.
    #[cfg_attr(not(feature = "x3"), expect(dead_code, reason = "the X3's"))]
    pub fn give_up(&mut self, why: &str) {
        match self.0.give_up() {
            Ok(false) => return,
            Ok(true) => warn!("OTA: {why}: back to the firmware before"),
            Err(_) => warn!("OTA: {why}: could not mark the update as bad"),
        }
        esp_hal::system::software_reset();
    }

    /// Whether the running firmware is on trial.
    pub fn on_trial(&self) -> bool {
        self.0.on_trial()
    }

    /// How long the running firmware is still on trial, at `now` (ms
    /// since the start); `None` where it is not.
    pub fn trial_left(&self, now: u64) -> Option<u64> {
        self.0.trial_left(now)
    }

    /// The running firmware on trial ran long enough, at `now` (ms since
    /// the start): it is marked as good. Whether it was, just now.
    pub fn check_trial(&mut self, now: u64) -> bool {
        match self.0.check_trial(now) {
            Ok(false) => false,
            Ok(true) => {
                info!("OTA: the update works");
                true
            }
            Err(_) => {
                warn!("OTA: could not mark the update as good");
                true
            }
        }
    }

    /// The running firmware goes to sleep or restarts at `now`, ending
    /// cleanly: on trial, the next start goes on with the trial.
    pub fn pause_trial(&mut self, now: u64) {
        if self.0.pause_trial(now).is_err() {
            warn!("OTA: could not note the time on trial");
        }
    }

    /// Awake again at `now` after pending work cancelled sleep: on
    /// trial again.
    pub fn resume_trial(&mut self, now: u64) {
        if self.0.resume_trial(now).is_err() {
            warn!("OTA: could not put the update on trial again");
        }
    }

    /// The firmware as plugins see it.
    pub fn firmware(&self) -> Firmware {
        self.0.firmware()
    }

    /// Whether an install goes on (the device neither sleeps nor waits
    /// meanwhile).
    pub fn installing(&self) -> bool {
        self.0.installing()
    }

    /// Starts installing the update file at `path`, with the `key` the
    /// reader accepted and whether they chose an `older` one (see
    /// `FirmwareCommand::Install`).
    pub fn begin(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        key: Option<&[u8; 32]>,
        older: bool,
    ) -> FirmwareState {
        let begun = self.0.begin(storage, path, key, older);
        said(path, begun)
    }

    /// Starts installing another firmware's image at `path`, which the
    /// reader chose as such (see `FirmwareCommand::InstallForeign`).
    pub fn begin_foreign(&mut self, storage: &dyn Storage, path: &str) -> FirmwareState {
        let begun = self
            .0
            .begin_foreign(storage, path)
            .map(|(installing, what)| {
                info!("OTA: {path} is another firmware ({what}), not signed");
                installing
            });
        said(path, begun)
    }

    /// Writes the next part; the new state when the install went on (every
    /// 5 %: the page shows each with a refresh), ended or failed.
    pub fn step(&mut self, storage: &dyn Storage) -> Option<FirmwareState> {
        let state = self.0.step(storage)?;
        match &state {
            FirmwareState::Installed => info!("OTA: installed, started at the next start"),
            FirmwareState::Failed(why) => warn!("OTA: the install failed: {why:?}"),
            _ => {}
        }
        Some(state)
    }
}

/// An install begun, in the log and as the page hears of it.
fn said(path: &str, begun: Result<Installing, InstallFailure>) -> FirmwareState {
    match begun {
        Ok(Installing { len, offset }) => {
            info!("OTA: installing {path} ({len} bytes) at {offset:#x}");
            FirmwareState::Installing {
                done: 0,
                total: u64::from(len),
            }
        }
        Err(why) => {
            warn!("OTA: {path}: {why:?}");
            FirmwareState::Failed(why)
        }
    }
}

/// The partitions updates use.
struct Partitions {
    slots: Slots,
    /// The app partition running.
    running: u32,
}

/// Where in the flash the code running is, on the ESP32-S3: its
/// bootloader maps only the last of the image's DROM segments, so the
/// MMU's first entry, which `booted_partition` reads, is not the app's
/// there. The entry that maps this function's page is.
#[cfg(feature = "esp32s3")]
fn running_from() -> Option<u32> {
    /// The MMU's table: one entry for each 64 KB page of the 32 MB the
    /// caches map (instructions and data alike).
    const MMU_TABLE: usize = 0x600C_5000;
    const INVALID: u32 = 1 << 14;
    const PSRAM: u32 = 1 << 15;
    let here = running_from as *const () as usize;
    let entry = (here & 0x1FF_FFFF) >> 16;
    // SAFETY: a read of the MMU table, within its 512 entries.
    let value = unsafe { ((MMU_TABLE + entry * 4) as *const u32).read_volatile() };
    if value & (INVALID | PSRAM) != 0 {
        return None;
    }
    Some(((value & 0x3FFF) << 16) | (here as u32 & 0xFFFF))
}

/// Elsewhere the MMU's first entry tells.
#[cfg(not(feature = "esp32s3"))]
fn running_from() -> Option<u32> {
    None
}

/// The partitions updates use (`paperpaw_x4::partitions`, as the release
/// checks each image's table); `None` without two app partitions (and
/// only two) or when which runs cannot be told.
fn partitions_of(flash: &mut FlashStorage<'static>) -> Option<Partitions> {
    let mut table = Box::new([0u8; TABLE_LEN]);
    flash.read(TABLE_AT, &mut table[..]).ok()?;
    let slots = match Slots::find(&table[..]) {
        Ok(slots) => slots,
        Err(why) => {
            warn!("OTA: the partition table has no room for updates: {why:?}");
            return None;
        }
    };
    // Where the bootloader started this: as the MMU's first entry says,
    // else (the ESP32-S3) by where this code is.
    let booted = partitions::read_partition_table(flash, &mut table[..PARTITION_TABLE_MAX_LEN])
        .ok()
        .and_then(|pt| pt.booted_partition().ok().flatten())
        .map(|p| p.offset());
    let running = booted
        .and_then(|at| slots.slot_at(at))
        .or_else(|| running_from().and_then(|at| slots.slot_at(at)))?;
    Some(Partitions { slots, running })
}
