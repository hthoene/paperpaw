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
pub struct Ota {
    updates: Updates<Chip>,
    /// The milliseconds a trial ran before it failed (`Ota::failed_trial`).
    failed: Option<u32>,
    /// The bootloader marked the start pending (`Ota::marked_by_bootloader`).
    marked: bool,
    /// The start found an update that failed its trial (`Ota::plans_to_go_back`).
    going_back: bool,
    /// The start is done (`Ota::settle`).
    settled: bool,
    /// The flash, to look at the bootloader again.
    shared: Flash,
}

/// Whether the bootloader carries the marker of a dual-boot setup
/// (`paperpaw_x4::dual_boot`), looked for on the flash `shared`. Where the
/// flash cannot be read it counts as there: nothing is written to the OTA
/// data where it cannot be told.
pub fn marker(shared: &Flash) -> Option<paperpaw_x4::dual_boot::Source> {
    paperpaw_x4::dual_boot::scan_marker(&mut Chip(shared.clone())).unwrap_or_else(|_| {
        warn!("dual boot: the bootloader could not be read, so it counts as marked");
        Some(paperpaw_x4::dual_boot::Source::Marker)
    })
}

impl Ota {
    /// Looks at what started and why (see `paperpaw_x4::updates`), and
    /// writes nothing: [`Ota::settle`] does what the start needs once the
    /// card has said whether this is a dual-boot setup. `None` without two
    /// app partitions (flashed before they came: the web flasher sets them
    /// up once).
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
        let (updates, plan) = Updates::open_with(
            Chip(shared.clone()),
            &slots,
            running,
            this,
            BOARD_TAG,
            || marker(&shared),
        )
        .ok()?;
        let mut this = Self {
            updates,
            failed: None,
            marked: false,
            going_back: matches!(plan, Start::Failed { .. }),
            settled: false,
            shared,
        };
        // An update this firmware installed, or one that failed its trial,
        // was marked at once (`Updates::open_with`): before the card, since a
        // hang in the board's start would leave it unmarked for good; not
        // where the bootloader already carries a dual-boot marker (#222),
        // nothing is written to the OTA data then.
        if plan != Start::Kept && !this.updates.start_pending() {
            this.settle(None);
        }
        Some(this)
    }

    /// Whether the start found an update that failed its trial: unless the
    /// setup turns out a dual-boot one, it goes back to the firmware before
    /// it ([`Ota::settle`], [`Ota::failed_trial`]).
    pub fn plans_to_go_back(&self) -> bool {
        self.going_back
    }

    /// Whether the start is done: marked at once, or settled.
    pub fn settled(&self) -> bool {
        self.settled
    }

    /// The flash, for the bootloader's marker.
    pub fn flash(&self) -> &Flash {
        &self.shared
    }

    /// Says whether this is a declared dual-boot setup (`dual_boot::decide`,
    /// once the card gave `paperpaw.ini`) and does what the start needs:
    /// the OTA data is written, an update put on trial, one undone or marked
    /// bad, only where it is not (see `Updates::settle`).
    pub fn settle(&mut self, source: Option<paperpaw_x4::dual_boot::Source>) {
        let running = self.updates.running();
        self.settled = true;
        let start = self.updates.settle(source);
        if source.is_some() {
            warn!("OTA: a declared dual-boot setup: the OTA data is left as it is");
        }
        match start {
            Some(Start::Kept) => info!("OTA: started {running}"),
            Some(Start::Undone) => warn!("OTA: the update did not start, {running} is back"),
            Some(Start::Trial { ran }) => {
                info!("OTA: an update on trial in {running}, {ran} ms run before");
            }
            Some(Start::Marked { ran }) => {
                // The bootloader (with app rollback) marked the start as
                // pending itself: the update's first start, on trial.
                info!(
                    "OTA: an update on trial in {running}, marked pending by the bootloader, {ran} ms run before"
                );
                self.marked = true;
            }
            Some(Start::Failed { ran }) => {
                // Marked bad already: whatever happens now, the bootloader
                // passes over this firmware at its next start. It goes back
                // once the card has the note (`main`, `failed_trial`).
                warn!("OTA: the update failed on trial ({ran} ms run before): going back");
                self.failed = Some(ran);
            }
            None => warn!("OTA: could not mark the start of {running}"),
        }
    }

    /// An update on trial that cannot work on this device (an X3 whose
    /// display it does not drive, see `main`): marked bad, and the device
    /// starts again into the firmware before it, as after a failed trial.
    /// Nothing where none is on trial.
    #[cfg_attr(not(feature = "x3"), expect(dead_code, reason = "the X3's"))]
    pub fn give_up(&mut self, why: &str) {
        match self.updates.give_up() {
            Ok(false) => return,
            Ok(true) => warn!("OTA: {why}: back to the firmware before"),
            Err(_) => warn!("OTA: {why}: could not mark the update as bad"),
        }
        esp_hal::system::software_reset();
    }

    /// This start found the update's trial had not ended cleanly (the
    /// milliseconds it ran before): the update is marked bad and the start
    /// goes back to the firmware before it ([`Ota::go_back`]) after it left
    /// its note.
    pub fn failed_trial(&self) -> Option<u32> {
        self.failed
    }

    /// The trial is over: the note of its start (`main::install_note`) has
    /// done its work.
    pub fn trial_over(storage: &dyn paperpaw_platform::Storage) {
        let _ = storage.remove(paperpaw_x4::install_note::FILE);
    }

    /// Milliseconds the update on trial ran in earlier starts (0: none, or
    /// not on trial).
    pub fn ran_before(&self) -> u32 {
        let left = self.trial_left(0).unwrap_or(paperpaw_x4::updates::TRIAL_MS);
        u32::try_from(paperpaw_x4::updates::TRIAL_MS.saturating_sub(left)).unwrap_or(u32::MAX)
    }

    /// The bootloader marked this start `PendingVerify` itself (it has app
    /// rollback): the trial's first start, not a failure of one.
    pub fn marked_by_bootloader(&self) -> bool {
        self.marked
    }

    /// Restarts into the firmware before, after a failed trial.
    pub fn go_back(&self) -> ! {
        esp_hal::system::software_reset()
    }

    /// Whether the running firmware is on trial.
    pub fn on_trial(&self) -> bool {
        self.updates.on_trial()
    }

    /// How long the running firmware is still on trial, at `now` (ms
    /// since the start); `None` where it is not.
    pub fn trial_left(&self, now: u64) -> Option<u64> {
        self.updates.trial_left(now)
    }

    /// The running firmware on trial ran long enough, at `now` (ms since
    /// the start): it is marked as good, if the mark could be written.
    pub fn check_trial(&mut self, now: u64) -> paperpaw_x4::updates::TrialEnd {
        let end = self.updates.end_of_trial(now);
        match end {
            paperpaw_x4::updates::TrialEnd::Running => {}
            paperpaw_x4::updates::TrialEnd::Kept => info!("OTA: the update works"),
            paperpaw_x4::updates::TrialEnd::Unmarked => {
                warn!("OTA: could not mark the update as good");
            }
        }
        end
    }

    /// The running firmware goes to sleep or restarts at `now`, ending
    /// cleanly: on trial, the next start goes on with the trial.
    pub fn pause_trial(&mut self, now: u64) {
        if self.updates.pause_trial(now).is_err() {
            warn!("OTA: could not note the time on trial");
        }
    }

    /// Awake again at `now` after pending work cancelled sleep: on
    /// trial again.
    pub fn resume_trial(&mut self, now: u64) {
        if self.updates.resume_trial(now).is_err() {
            warn!("OTA: could not put the update on trial again");
        }
    }

    /// The firmware running, for `core.device` (interface 7 builds).
    #[cfg(feature = "ppn5")]
    pub fn running(&self) -> paperpaw_platform::firmware::Running<'_> {
        paperpaw_platform::firmware::FirmwareHost::running(&self.updates)
    }

    /// The firmware as plugins see it.
    pub fn firmware(&self) -> Firmware {
        self.updates.firmware()
    }

    /// Whether an install goes on (the device neither sleeps nor waits
    /// meanwhile).
    pub fn installing(&self) -> bool {
        self.updates.installing()
    }

    /// Whether, and by what, the setup is a declared dual-boot one.
    pub fn dual(&self) -> Option<paperpaw_x4::dual_boot::Source> {
        self.updates.dual()
    }

    /// The setup is declared a dual-boot one, or not, again (before an
    /// install, `dual::guard`): nothing is written to the OTA data by it.
    pub fn declare(&mut self, source: Option<paperpaw_x4::dual_boot::Source>) {
        self.updates.settle(source);
    }

    /// The partition running, 0 or 1.
    pub fn slot(&self) -> u32 {
        self.updates.slot()
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
        if crate::dual::guard(self, storage) {
            return said(
                path,
                Err(InstallFailure::Other(paperpaw_x4::dual_boot::REFUSED)),
            );
        }
        let begun = self.updates.begin(storage, path, key, older);
        said(path, begun)
    }

    /// Starts installing another firmware's image at `path`, which the
    /// reader chose as such (see `FirmwareCommand::InstallForeign`).
    pub fn begin_foreign(&mut self, storage: &dyn Storage, path: &str) -> FirmwareState {
        if crate::dual::guard(self, storage) {
            return said(
                path,
                Err(InstallFailure::Other(paperpaw_x4::dual_boot::REFUSED)),
            );
        }
        let begun = self
            .updates
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
        let state = self.updates.step(storage)?;
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

/// On the ESP32-C3 the MMU's entry for this code's page (the first entry,
/// which `booted_partition` reads, is the bootloader's to map as it likes:
/// a dualbooter's does not map the app's first page there, #167).
#[cfg(not(feature = "esp32s3"))]
fn running_from() -> Option<u32> {
    crate::native::partition::code_address().ok()
}

/// The partitions updates use (`paperpaw_x4::partitions`, as the release
/// checks each image's table); `None` without two app partitions (and
/// only two) or when which runs cannot be told.
fn partitions_of(flash: &mut FlashStorage<'static>) -> Option<Partitions> {
    // Fallible: without room the device goes without updates, not without
    // starting.
    let mut table = alloc::vec::Vec::new();
    if table.try_reserve_exact(TABLE_LEN).is_err() {
        warn!("OTA: no memory to read the partition table");
        return None;
    }
    table.resize(TABLE_LEN, 0u8);
    flash.read(TABLE_AT, &mut table[..]).ok()?;
    let slots = match Slots::find(&table[..]) {
        Ok(slots) => slots,
        Err(why) => {
            warn!("OTA: the partition table has no room for updates: {why:?}");
            return None;
        }
    };
    // Where this code runs, by the MMU's entry for its page (the first
    // entry is the bootloader's to map as it likes, and is a guess a
    // dualbooter's gets wrong), else as the MMU's first entry says.
    let running = running_from()
        .and_then(|at| slots.slot_at(at))
        .or_else(|| {
            partitions::read_partition_table(flash, &mut table[..PARTITION_TABLE_MAX_LEN])
                .ok()
                .and_then(|pt| pt.booted_partition().ok().flatten())
                .and_then(|p| slots.slot_at(p.offset()))
        })?;
    Some(Partitions { slots, running })
}

/// The firmware host the kernel's plugin jobs share (`KernelBuilder::firmware_host`,
/// #2654): plugin installs and checks queue on the host's driver, which is
/// built around one. The X4's firmware updates stay with [`Ota`], where
/// interface 6's update plugin drives them; this one says what runs (`room` 0:
/// no firmware is installed through it) and refuses every firmware job of
/// interface 7 as `Reason::NOT_HERE`, as a device shared with another firmware
/// does, until an interface-7 update is wired. It owns two short strings and
/// holds no buffer, so it costs next to nothing at idle.
#[cfg(feature = "ppn5")]
pub struct PluginJobsHost {
    version: alloc::string::String,
    project: alloc::string::String,
    chip: u16,
    undone: bool,
}

#[cfg(feature = "ppn5")]
impl PluginJobsHost {
    /// The host for the firmware `firmware` describes.
    pub fn new(firmware: &Firmware) -> Self {
        Self {
            version: firmware.version.clone(),
            project: firmware.project.clone(),
            chip: firmware.chip,
            undone: firmware.undone,
        }
    }
}

#[cfg(feature = "ppn5")]
impl paperpaw_platform::firmware::FirmwareHost for PluginJobsHost {
    fn running(&self) -> paperpaw_platform::firmware::Running<'_> {
        paperpaw_platform::firmware::Running::new(
            &self.version,
            &self.project,
            self.chip,
            0,
            self.undone,
        )
    }

    fn begin_check(&mut self, _: &str) -> Result<(), paperpaw_platform::firmware::Reason> {
        Err(paperpaw_platform::firmware::Reason::NOT_HERE)
    }

    fn check_step(
        &mut self,
        _: &dyn Storage,
        _: &mut dyn FnMut(&str),
    ) -> paperpaw_platform::firmware::Step<paperpaw_platform::firmware::CheckFacts> {
        paperpaw_platform::firmware::Step::Failed(paperpaw_platform::firmware::Reason::NOT_HERE)
    }

    fn begin_write(
        &mut self,
        _: &str,
        _: [u8; 32],
        _: Option<[u8; 32]>,
        _: bool,
    ) -> Result<(), paperpaw_platform::firmware::Reason> {
        Err(paperpaw_platform::firmware::Reason::NOT_HERE)
    }

    fn write_step(
        &mut self,
        _: &dyn Storage,
    ) -> paperpaw_platform::firmware::Step<()> {
        paperpaw_platform::firmware::Step::Failed(paperpaw_platform::firmware::Reason::NOT_HERE)
    }

    fn readback_step(&mut self) -> paperpaw_platform::firmware::Step<()> {
        paperpaw_platform::firmware::Step::Failed(paperpaw_platform::firmware::Reason::NOT_HERE)
    }

    fn commit(&mut self) -> Result<(), paperpaw_platform::firmware::Reason> {
        Err(paperpaw_platform::firmware::Reason::NOT_HERE)
    }

    fn abandon(&mut self) {}

    fn restart(&mut self) -> Result<(), paperpaw_platform::firmware::Reason> {
        Err(paperpaw_platform::firmware::Reason::NOT_HERE)
    }
}
