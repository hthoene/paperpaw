//! Updates over the air as the firmware runs them, on any flash: what a
//! start finds (an update on trial, one that failed it, one undone), the
//! trial and its marks, and an install, the update file read from the card
//! a piece at a time, checked as it is written into the app partition not
//! running and chosen for the next start only if it holds. The firmware
//! (`shells/xteink/src/ota.rs`) gives it the chip's flash, where the
//! partitions are and which one runs, and restarts where it says so; the
//! rest is here, so that a computer runs it as the device does
//! (`tests/update_path.rs`).
//!
//! The bootloader espflash brings does not undo updates itself, so the
//! firmware does (a stock bootloader with app rollback marks a new image
//! `PendingVerify` itself at its first start; the firmware tells that
//! from its own mark of a trial, `ota::Start::Marked`): a new firmware's first start marks it as on trial
//! (`PendingVerify`) as early as the shell can, before the kernel is built
//! (what the shell builds before it, the firmware's description, cannot
//! fail), and it is marked as
//! good (`Valid`) once it ran [`TRIAL_MS`] in all. Going to sleep or
//! restarting before marks it new again with the time it ran, so the next
//! start (on the X4 also a wake from sleep) goes on with its trial. Found
//! on trial at a start (the last run ended otherwise, e.g. in a panic or a
//! hang the watchdog ended), it is marked bad and the device restarts into
//! the firmware before it, which says so ([`Updates::firmware`]'s
//! `undone`) and takes its place back. These marks go to the running
//! partition's entry, also when an update was chosen meanwhile.
//!
//! Only update files signed with a key the firmware has built in or one
//! the reader accepted for the install, and newer ones unless the reader
//! chose an older one, are installed (`paperpaw_sdk::firmware`); the
//! signature is checked while the image is written, and the new firmware
//! chosen only if it holds. Its first word is written only then
//! ([`Image`]): until it is, the partition holds no image the bootloader
//! would start, even if it tried that partition. An update installed and
//! not started yet is given up before another is written, which would
//! otherwise go into the partition chosen to start.
//!
//! Another firmware's app image (the way back to CrossPoint) installs
//! the same way, only when the reader chose it as such
//! (`FirmwareCommand::InstallForeign`): unsigned, it is checked as the
//! bootloader would start it (`ForeignCheck`: its segments, checksum and
//! hash, the chip, CrossPoint's board tag) and chosen only if that holds.

use alloc::boxed::Box;
use alloc::string::String;

use paperpaw_platform::Storage;
use paperpaw_sdk::firmware::{
    Check, Firmware, FirmwareState, ForeignCheck, ForeignImage, InstallFailure, Update,
};

use crate::dual_boot::Source;
use crate::ota::{self, Flash, FlashError, Image, OtaData, Start, State};
use crate::partitions::Slots;

/// PaperPaw's keys: updates signed with them install without asking.
pub const KEYS: &[[u8; 32]] = &[
    // The release key, fingerprint 55A9 F626 DB17 EB90.
    [
        0x55, 0xa9, 0xf6, 0x26, 0xdb, 0x17, 0xeb, 0x90, 0x58, 0x70, 0x73, 0x23, 0xd2, 0x9f, 0x52,
        0x47, 0x0b, 0xfc, 0xd0, 0x96, 0x38, 0x89, 0x8e, 0x17, 0xbd, 0x01, 0x78, 0x4e, 0xf9, 0xdf,
        0x0c, 0x51,
    ],
];
/// Read from the card and written a piece at a time.
const PIECE: usize = 4096;
/// Written per [`Updates::step`]: the main loop goes on in between.
const STEP: u32 = ota::BLOCK;
/// An update on trial is taken as good once it ran this long in all,
/// across starts (its start and the book it opens at once behind it).
pub const TRIAL_MS: u64 = 30_000;

/// A partition of the flash: the flash in addresses of its own.
struct Partition<'a, F> {
    flash: &'a mut F,
    offset: u32,
}

impl<F: Flash> Flash for Partition<'_, F> {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.flash.read(self.offset + at, buf)
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        self.flash.program(self.offset + at, data)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.flash.erase(self.offset + from, self.offset + to)
    }
}

/// An install going on.
struct Install {
    path: String,
    image: Image,
    /// Where in the flash the image goes.
    offset: u32,
    /// The partition it goes to.
    slot: u32,
    check: Checking,
    buf: Box<[u8; PIECE]>,
    /// The last twentieth of the image said to be written.
    said: u32,
}

/// How the image is checked as it is written: one at a time, in the
/// install under way.
#[expect(
    clippy::large_enum_variant,
    reason = "one at a time; the signature's state stays where it was"
)]
enum Checking {
    /// PaperPaw's update: its signature.
    Signed(Check),
    /// Another firmware's image: whole, for this device.
    Foreign(ForeignCheck),
}

impl Checking {
    fn absorb(&mut self, piece: &[u8]) {
        match self {
            Checking::Signed(check) => check.absorb(piece),
            Checking::Foreign(check) => check.absorb(piece),
        }
    }

    fn finish(self) -> Result<(), InstallFailure> {
        match self {
            Checking::Signed(check) => check.finish().map_err(InstallFailure::from),
            Checking::Foreign(check) => check.finish(),
        }
    }
}

/// The running firmware on trial.
struct Trial {
    /// How long it ran in starts before this one.
    before: u64,
    /// Since when it runs now (ms since the start; anew after pending
    /// work cancelled sleep).
    since: u64,
}

/// An install begun: the update's file and where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Installing {
    /// The image's length.
    pub len: u32,
    /// Where in the flash it goes.
    pub offset: u32,
}

/// How the trial of the running firmware stands ([`Updates::end_of_trial`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialEnd {
    /// Not on trial, or not yet run long enough.
    Running,
    /// Marked as good: the update is kept.
    Kept,
    /// It ran long enough, but the mark could not be written: the OTA data
    /// still says on trial, and the update may go back.
    Unmarked,
}

/// Updates on the flash `F` (in the chip's own addresses), for the
/// firmware this is.
pub struct Updates<F> {
    flash: F,
    /// Where the OTA data partition is.
    data: u32,
    /// Where the two app partitions are, and their sizes.
    apps: [(u32, u32); 2],
    /// The app partition running.
    running: u32,
    /// The firmware running, as plugins see it, without its room, state
    /// and whether an update was undone.
    this: Firmware,
    /// This device in CrossPoint's board tag, for another firmware's image.
    board: &'static str,
    trial: Option<Trial>,
    /// An update did not start cleanly, and this firmware came back.
    undone: bool,
    install: Option<Install>,
    /// This is a declared dual-boot setup (`dual_boot`): the other app
    /// partition is another firmware's, nothing is installed there.
    dual: Option<Source>,
    /// What the start did at once (`open`), for [`Updates::settle`] to say.
    early: Option<Option<Start>>,
    /// The OTA data as the start found it, where the start would write it:
    /// held until [`Updates::settle`] knows whether it may.
    pending: Option<OtaData>,
}

impl<F> core::fmt::Debug for Updates<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Updates")
            .field("running", &self.running)
            .field("on_trial", &self.trial.is_some())
            .field("undone", &self.undone)
            .field("installing", &self.install.is_some())
            .finish_non_exhaustive()
    }
}

impl<F: Flash> Updates<F> {
    /// Looks at what started and why (see the [module documentation](self)):
    /// `slots` from the flash's table, `running` the one the bootloader
    /// started, `this` the firmware (version, project, chip, keys, plugins)
    /// and `board` its board in CrossPoint's tag. With what the start found:
    /// [`Start::Failed`] means an update failed its trial and is marked bad,
    /// and the device restarts now into the firmware before it; `None`
    /// where the start could not be marked. `Err` where the OTA data cannot
    /// be read. Writes the OTA data at once where the start needs it; a
    /// firmware that may share the flash with another (`dual_boot`) uses
    /// [`Updates::open`] and [`Updates::settle`] instead.
    pub fn start(
        flash: F,
        slots: &Slots,
        running: u32,
        this: Firmware,
        board: &'static str,
    ) -> Result<(Self, Option<Start>), FlashError> {
        let (mut updates, _) = Self::open(flash, slots, running, this, board)?;
        let start = updates.settle(None);
        Ok((updates, start))
    }

    /// The first half of [`Updates::start`]: looks at the OTA data and what
    /// it says this start is ([`OtaData::plan`], returned), and writes
    /// nothing. Whether the setup is a dual-boot one is not known yet (the
    /// reader's choice is on the card): [`Updates::settle`] says, and does
    /// what the start needs only where it is not.
    #[inline(never)]
    pub fn open(
        mut flash: F,
        slots: &Slots,
        running: u32,
        this: Firmware,
        board: &'static str,
    ) -> Result<(Self, Start), FlashError> {
        let ota_data = OtaData::read(&mut Partition {
            flash: &mut flash,
            offset: slots.data,
        })?;
        let plan = ota_data.plan(running);
        let early = ota_data.acts_early(running);
        let mut updates = Self {
            flash,
            data: slots.data,
            apps: slots.apps,
            running,
            this,
            board,
            trial: None,
            undone: false,
            install: None,
            dual: None,
            pending: (plan != Start::Kept).then_some(ota_data),
            early: None,
        };
        // What an update this firmware installed needs, or one that failed,
        // is done now (a hang before the card would leave it undone for
        // good); the rest waits for [`Updates::settle`].
        if early {
            updates.early = Some(updates.apply());
        }
        Ok((updates, plan))
    }

    /// The second half of [`Updates::start`]: whether the setup is a
    /// declared dual-boot one (`dual_boot::decide`, from the reader's choice
    /// and the bootloader). If it is, nothing is written to the OTA data,
    /// no trial begins, and installs are refused from now on: the start is
    /// [`Start::Kept`]. If not, what the start needs is done now, and its
    /// result returned (`None` where it could not be marked). Once only;
    /// again it says [`Start::Kept`] and changes what is declared.
    #[inline(never)]
    pub fn settle(&mut self, declared: Option<Source>) -> Option<Start> {
        self.dual = declared;
        if let Some(done) = self.early.take() {
            return done;
        }
        if self.pending.is_none() || declared.is_some() {
            self.pending = None;
            return Some(Start::Kept);
        }
        self.apply()
    }

    /// What the start needs, written (the OTA data held in `pending`).
    fn apply(&mut self) -> Option<Start> {
        let mut ota_data = self.pending.take()?;
        let partition = &mut Partition {
            flash: &mut self.flash,
            offset: self.data,
        };
        let start = ota_data.start(partition, self.running);
        match start {
            Some(Start::Undone) => self.undone = true,
            Some(Start::Trial { ran } | Start::Marked { ran }) => {
                self.trial = Some(Trial {
                    before: u64::from(ran),
                    since: 0,
                });
            }
            Some(Start::Kept | Start::Failed { .. }) | None => {}
        }
        start
    }

    /// The app partition running.
    pub fn running(&self) -> u32 {
        self.running
    }

    /// The flash, e.g. for a test to look at what is on it.
    pub fn flash(&self) -> &F {
        &self.flash
    }

    /// The flash back.
    pub fn into_flash(self) -> F {
        self.flash
    }

    /// An update on trial that cannot work on this device (an X3 whose
    /// display it does not drive): marked bad. Whether one was on trial:
    /// then the device starts again now into the firmware before it, as
    /// after a failed trial.
    pub fn give_up(&mut self) -> Result<bool, FlashError> {
        if self.trial.take().is_none() {
            return Ok(false);
        }
        self.mark(State::Invalid, 0)?;
        Ok(true)
    }

    /// Whether the running firmware is on trial (an update not yet run
    /// [`TRIAL_MS`]).
    pub fn on_trial(&self) -> bool {
        self.trial.is_some()
    }

    /// How long the running firmware is still on trial, at `now` (ms
    /// since the start); `None` where it is not.
    pub fn trial_left(&self, now: u64) -> Option<u64> {
        let trial = self.trial.as_ref()?;
        Some(TRIAL_MS.saturating_sub(trial.before + now.saturating_sub(trial.since)))
    }

    /// The running firmware on trial ran long enough, at `now` (ms since
    /// the start): it is marked as good. Whether it was, just now (`Err`
    /// where the mark could not be written).
    pub fn check_trial(&mut self, now: u64) -> Result<bool, FlashError> {
        let Some(trial) = &self.trial else {
            return Ok(false);
        };
        if trial.before + now.saturating_sub(trial.since) < TRIAL_MS {
            return Ok(false);
        }
        self.trial = None;
        self.mark(State::Valid, 0).map(|()| true)
    }

    /// [`Updates::check_trial`] as the shell needs it: `Kept` only where the
    /// mark of the update as good was written (a flash that fails to write
    /// leaves the OTA data on trial, and the update may still go back:
    /// `Unmarked`, #164).
    pub fn end_of_trial(&mut self, now: u64) -> TrialEnd {
        match self.check_trial(now) {
            Ok(false) => TrialEnd::Running,
            Ok(true) => TrialEnd::Kept,
            Err(_) => TrialEnd::Unmarked,
        }
    }

    /// The running firmware goes to sleep or restarts at `now`, ending
    /// cleanly: on trial, it is new again with the time it ran, so the
    /// next start goes on with the trial.
    pub fn pause_trial(&mut self, now: u64) -> Result<(), FlashError> {
        let Some(trial) = &mut self.trial else {
            return Ok(());
        };
        trial.before += now.saturating_sub(trial.since);
        trial.since = now;
        let ran = u32::try_from(trial.before).unwrap_or(u32::MAX - 1);
        self.mark(State::New, ran)
    }

    /// Awake again at `now` after pending work cancelled sleep: on
    /// trial again.
    pub fn resume_trial(&mut self, now: u64) -> Result<(), FlashError> {
        let Some(trial) = &mut self.trial else {
            return Ok(());
        };
        trial.since = now;
        let ran = u32::try_from(trial.before).unwrap_or(u32::MAX - 1);
        self.mark(State::PendingVerify, ran)
    }

    /// Marks the running partition's image (its entry, also with another
    /// chosen meanwhile).
    fn mark(&mut self, state: State, ran: u32) -> Result<(), FlashError> {
        let running = self.running;
        let partition = &mut self.data_partition();
        OtaData::read(partition)?.set_state(partition, running, state, ran)
    }

    fn data_partition(&mut self) -> Partition<'_, F> {
        Partition {
            flash: &mut self.flash,
            offset: self.data,
        }
    }

    /// The firmware as plugins see it.
    pub fn firmware(&self) -> Firmware {
        let target = (self.running + 1) % ota::SLOTS;
        self.this
            .clone()
            // Nothing goes into another firmware's partition: no room.
            .room(if self.dual.is_some() {
                0
            } else {
                u64::from(self.apps[target as usize].1)
            })
            .undone(self.undone)
            .state(FirmwareState::Idle)
    }

    /// Whether an install goes on (the device neither sleeps nor waits
    /// meanwhile).
    pub fn installing(&self) -> bool {
        self.install.is_some()
    }

    /// Whether the start found something to write to the OTA data, held
    /// until [`Updates::settle`].
    pub fn start_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether, and by what, the setup is declared a dual-boot one.
    pub fn dual(&self) -> Option<Source> {
        self.dual
    }

    /// The partition running, for the note plugins read.
    pub fn slot(&self) -> u32 {
        self.running
    }

    /// Starts installing the update file at `path`, with the `key` the
    /// reader accepted and whether they chose an `older` one (see
    /// `FirmwareCommand::Install`): what it holds is checked first, the
    /// signature as it is written.
    pub fn begin(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        key: Option<&[u8; 32]>,
        older: bool,
    ) -> Result<Installing, InstallFailure> {
        if self.dual.is_some() {
            return Err(InstallFailure::Other(crate::dual_boot::REFUSED));
        }
        let update = Update::read(storage, path)?;
        update.may_install(&self.firmware(), key, older)?;
        let check = Checking::Signed(Check::new(&update.trailer)?);
        self.install_into_other(path, update.trailer.image_len, check)
    }

    /// Starts installing another firmware's image at `path`, which the
    /// reader chose as such (see `FirmwareCommand::InstallForeign`): its
    /// head is checked first, the whole image as it is written. Its
    /// project and version, for the log.
    pub fn begin_foreign(
        &mut self,
        storage: &dyn Storage,
        path: &str,
    ) -> Result<(Installing, String), InstallFailure> {
        if self.dual.is_some() {
            return Err(InstallFailure::Other(crate::dual_boot::REFUSED));
        }
        let image = ForeignImage::read(storage, path)?;
        image.fits(&self.firmware())?;
        let len = u32::try_from(image.len).map_err(|_| InstallFailure::TooLarge)?;
        let check = Checking::Foreign(ForeignCheck::new(self.this.chip, self.board, image.len));
        let installing = self.install_into_other(path, len, check)?;
        let what = alloc::format!("{} {}", image.info.project, image.info.version);
        Ok((installing, what))
    }

    /// An install of `len` bytes into the partition not running, checked
    /// so.
    fn install_into_other(
        &mut self,
        path: &str,
        len: u32,
        check: Checking,
    ) -> Result<Installing, InstallFailure> {
        let running = self.running;
        let partition = &mut self.data_partition();
        let slot = OtaData::read(partition)
            .and_then(|mut data| data.prepare_install(partition, running))
            .map_err(|_| InstallFailure::Write)?;
        let offset = self.apps[slot as usize].0;
        self.install = Some(Install {
            path: String::from(path),
            image: Image::new(len),
            offset,
            slot,
            check,
            buf: Box::new([0; PIECE]),
            said: 0,
        });
        Ok(Installing { len, offset })
    }

    /// Writes the next part; the new state when the install went on (every
    /// 5 %: the page shows each with a refresh), ended or failed.
    pub fn step(&mut self, storage: &dyn Storage) -> Option<FirmwareState> {
        let mut install = self.install.take()?;
        let (done, total) = match self.write(storage, &mut install) {
            Err(why) => return Some(FirmwareState::Failed(why)),
            Ok(()) => (install.image.done(), install.image.total()),
        };
        if done == total {
            return Some(self.finish(install));
        }
        let twentieth = (u64::from(done) * 20 / u64::from(total)) as u32;
        let state = (twentieth != install.said).then_some(FirmwareState::Installing {
            done: u64::from(done),
            total: u64::from(total),
        });
        install.said = twentieth;
        self.install = Some(install);
        state
    }

    fn write(
        &mut self,
        storage: &dyn Storage,
        install: &mut Install,
    ) -> Result<(), InstallFailure> {
        let image = &mut install.image;
        let end = (image.done() + STEP).min(image.total());
        let partition = &mut Partition {
            flash: &mut self.flash,
            offset: install.offset,
        };
        while image.done() < end {
            let len = (end - image.done()).min(PIECE as u32) as usize;
            let piece = &mut install.buf[..len];
            match storage.read_at(&install.path, u64::from(image.done()), piece) {
                Ok(n) if n == len => {}
                _ => return Err(InstallFailure::Unreadable),
            }
            install.check.absorb(piece);
            image
                .write(partition, &mut install.buf[..], len)
                .map_err(|_| InstallFailure::Write)?;
        }
        Ok(())
    }

    /// All written: the new firmware is made whole and chosen if its
    /// signature holds.
    fn finish(&mut self, mut install: Install) -> FirmwareState {
        if let Err(e) = install.check.finish() {
            return FirmwareState::Failed(e);
        }
        let made_whole = install.image.finish(
            &mut Partition {
                flash: &mut self.flash,
                offset: install.offset,
            },
            &mut install.buf[..],
        );
        let data = &mut self.data_partition();
        // State and choice in one entry, written whole.
        let chosen = made_whole
            .and_then(|()| OtaData::read(data)?.choose(data, install.slot, State::New, 0));
        match chosen {
            Ok(()) => FirmwareState::Installed,
            Err(_) => FirmwareState::Failed(InstallFailure::Write),
        }
    }
}
