//! The kernel's side of a firmware install and of a firmware check
//! (`docs/abi/device.md` 5.1, 5.3, 5.4): the state machine, the check results
//! and the steps it drives on the device's [`FirmwareHost`]. Memory only: a
//! restart forgets all of it, and the image that was written is found by the
//! boot loader, not by this state.
//!
//! * **One job at a time.** The queue (shared with plugin installs, which
//!   write the code flash too) hands this machine one [`Job`] with
//!   [`FirmwareInstall::start`] when it is idle; it is stepped from the main
//!   loop ([`FirmwareInstall::step`], one bounded device step per call, never
//!   inside a plugin's callback) until it ends in an [`Outcome`].
//! * **Ask, always.** An install is `Checking` (pass 1: chip, size, version,
//!   signer, the SHA-256 and the built-ins the image carries), then
//!   `Asking`: the machine returns an [`Ask`] and does nothing until
//!   [`FirmwareInstall::answer`] (decision 134: no grant skips it, a foreign
//!   image and a key that is not PaperPaw's included). Only a yes writes. The
//!   host confirmation, its 10 minutes and its input isolation are the
//!   caller's; this machine knows yes, no and dismissed.
//! * **Bound to what was asked.** The image stays on the card where the
//!   plugin saved it. The write is bound to the digest of the check the user
//!   saw, and the device re-checks it while writing and on the read-back; a
//!   mismatch ends the install (reason 18) and the slot never starts.
//! * **State 3.** A committed image waits for its restart
//!   ([`State::Installed`]). Only its asker may ask to restart
//!   ([`FirmwareInstall::request_restart`]: `ERR_DENIED` for another plugin,
//!   `ERR_STATE` with none waiting); the host asks again, whatever grant. No
//!   or dismissed leaves the image waiting. A firmware request sent while one
//!   waits ends with reason 21.
//! * **Check results.** One per plugin, at most [`RESULTS_MAX`] plugins, kept
//!   until that plugin's next check (or its stop); each holds the verdict, the
//!   check's facts and **one bit per built-in plugin the image has** (12 bytes
//!   for 96 built-ins, no list of names), from which op 16 lists what the
//!   image leaves out. The room is reserved at submit
//!   ([`FirmwareInstall::reserve_check`]) so a finished check never fails to
//!   keep its result.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};
use paperpaw_platform::{
    Storage,
    firmware::{CheckFacts, FirmwareHost, Reason, Signer, Step, Text, VERSION_MAX},
};
use paperpaw_sdk::Version;

use super::plugin_job::PluginFacts;

/// Most plugins that keep a firmware check result.
pub(crate) const RESULTS_MAX: usize = 4;
/// Most built-in plugins one bit is kept for.
pub(crate) const BUILT_INS_MAX: usize = 96;
/// `firmware_changed` comes at most this often while installing.
pub(crate) const PROGRESS_EVERY_MS: u64 = 250;

/// `install_firmware.flags` bit 0: an older or the same version may install.
pub(crate) const FLAG_OLDER: u32 = 1;
/// `install_firmware.flags` bit 1: another firmware than PaperPaw.
pub(crate) const FLAG_FOREIGN: u32 = 2;

/// `install_done.kind` of a firmware install (and of its restart).
pub(crate) const KIND_FIRMWARE: u32 = 1;
/// `install_done.kind` of a file check.
pub(crate) const KIND_CHECK: u32 = 3;

/// Reason 2 (declined), 3 (dismissed), 21 (a firmware waits for its restart).
const DECLINED: u8 = 2;
const DISMISSED: u8 = 3;
const RESTART_PENDING: u8 = 21;
/// Reason 14: an older or the same version, not allowed.
const OLDER: u8 = 14;

/// What the kernel knows of the built-in plugins, to number the ones an image
/// names (by plugin id, as op 4's rows give them).
pub(crate) trait BuiltIns {
    /// How many plugins are built into the running image.
    fn count(&self) -> usize;
    /// The number (below [`Self::count`]) of the built-in plugin of this
    /// plugin id (op 4's `BuiltinRow.id`); `None` for a name the running image does not have.
    fn index_of(&self, plugin_id: &str) -> Option<usize>;
}

/// The built-in plugins an image has, one bit each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Bits {
    has: [u8; BUILT_INS_MAX / 8],
    /// How many built-ins of the running image the bits speak of.
    count: u8,
}

impl Bits {
    const NONE: Self = Self {
        has: [0; BUILT_INS_MAX / 8],
        count: 0,
    };

    fn set(&mut self, index: usize) {
        if let Some(byte) = self.has.get_mut(index / 8) {
            *byte |= 1 << (index % 8);
        }
    }

    fn has(&self, index: usize) -> bool {
        self.has
            .get(index / 8)
            .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
    }

    /// Whether the running built-in `index` is left out.
    fn leaves_out(&self, index: usize) -> bool {
        index < usize::from(self.count) && !self.has(index)
    }
}

/// One firmware job: an install or a file check.
pub(crate) struct Job {
    pub(crate) owner: u64,
    pub(crate) request: u32,
    /// The image's path on the card, resolved from the plugin's root.
    path: String,
    /// `Some(key)`: the install of the image the plugin named a key for (the
    /// user must still accept it); the flags of `install_firmware`.
    install: Option<InstallArgs>,
    /// What an install's flag 1 said; `None` for a check, which finds out
    /// from the image alone.
    foreign: Option<bool>,
}

#[derive(Clone, Copy)]
struct InstallArgs {
    key: Option<[u8; 32]>,
    older_ok: bool,
}

impl Job {
    /// An install of the image at `path` (`flags` as `install_firmware`'s).
    ///
    /// # Errors
    /// `ERR_INVALID` for a flag bit other than 0 and 1, `ERR_NO_MEMORY`.
    pub(crate) fn install(
        owner: u64,
        request: u32,
        path: &str,
        key: Option<[u8; 32]>,
        flags: u32,
    ) -> Result<Self, i32> {
        if flags & !(FLAG_OLDER | FLAG_FOREIGN) != 0 {
            return Err(ERR_INVALID);
        }
        Self::make(
            owner,
            request,
            path,
            Some(flags & FLAG_FOREIGN != 0),
            Some(InstallArgs {
                key,
                older_ok: flags & FLAG_OLDER != 0,
            }),
        )
    }

    /// A check of the image at `path` (`check_file` kind 1).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`.
    pub(crate) fn check(owner: u64, request: u32, path: &str) -> Result<Self, i32> {
        Self::make(owner, request, path, None, None)
    }

    fn make(
        owner: u64,
        request: u32,
        path: &str,
        foreign: Option<bool>,
        install: Option<InstallArgs>,
    ) -> Result<Self, i32> {
        let mut text = String::new();
        text.try_reserve_exact(path.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        text.push_str(path);
        Ok(Self {
            owner,
            request,
            path: text,
            install,
            foreign,
        })
    }
}

/// How a job ended, for `install_done` (device.md 4) of its sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) owner: u64,
    pub(crate) request: u32,
    /// [`KIND_FIRMWARE`] or [`KIND_CHECK`].
    pub(crate) kind: u32,
    /// The reason of 5.5; 0 done (a check: fits).
    pub(crate) reason: u8,
    /// `install_done.flags` (a plugin job's: bit 1 the change reaches code
    /// at the next start).
    pub(crate) flags: u32,
    /// The plugin the job installed or removed, for `install_done.id`; `None`:
    /// the plugin that asked.
    pub(crate) id: Option<Text<{ super::plugin_job::ID_MAX }>>,
}

/// What the confirmation shows and asks (device.md 5.0, 5.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ask {
    pub(crate) owner: u64,
    pub(crate) request: u32,
    /// The image's project and version, as read from the image.
    pub(crate) project: Text<{ paperpaw_platform::firmware::PROJECT_MAX }>,
    pub(crate) version: Text<VERSION_MAX>,
    pub(crate) signer: Signer,
    pub(crate) foreign: bool,
    /// Older or the same version, allowed by the plugin's flag.
    pub(crate) older: bool,
    /// A key that is not PaperPaw's: the user accepts this very key or not.
    pub(crate) other_key: bool,
    /// The question is the restart's, not the install's.
    pub(crate) restart: bool,
}

/// What the user answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Yes,
    /// A plugin removal's second yes: with the plugin's data.
    YesAndData,
    No,
    Dismissed,
}

/// What a step or an answer calls for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Event {
    /// Nothing the caller shows or tells.
    Quiet,
    /// `firmware_changed`: `done` of `total` bytes written.
    Progress {
        owner: u64,
        request: u32,
        done: u32,
        total: u32,
    },
    /// Ask the user.
    Ask(Ask),
    /// The job ended.
    Ended(Outcome),
    /// A yes to the restart: save what plugins keep, then
    /// [`FirmwareInstall::restart`].
    Restart,
}

/// `dev_firmware.state` (device.md 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Idle,
    WaitingForUser,
    Installing,
    Installed,
    Failed,
}

impl State {
    /// The number `dev_firmware.state` carries.
    pub(crate) fn number(self) -> u32 {
        match self {
            Self::Idle => 0,
            Self::WaitingForUser => 1,
            Self::Installing => 2,
            Self::Installed => 3,
            Self::Failed => 4,
        }
    }
}

/// The committed image that waits for its restart.
#[derive(Clone, Copy)]
struct Waiting {
    owner: u64,
    request: u32,
    project: Text<{ paperpaw_platform::firmware::PROJECT_MAX }>,
    version: Text<VERSION_MAX>,
    signer: Signer,
    foreign: bool,
}

/// The job that runs (a waiting image is [`FirmwareInstall::waiting`], not a
/// phase: a check can run while an installed image waits for its restart).
enum Phase {
    Idle,
    Checking(Job, Bits),
    Asking(Job, CheckFacts),
    Writing(Job, CheckFacts, Option<u64>),
    Reading(Job, CheckFacts),
    /// The restart's confirmation of the waiting image shows (or, after a
    /// yes, waits for [`FirmwareInstall::restart`]); the request is the
    /// restart's own.
    RestartAsking(u32),
}

/// What an answer answers: the question as the confirmation was shown. An
/// answer that names another question (a late yes from the dialog of a job
/// that was aborted) is ignored, never applied to the job that runs now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub(crate) owner: u64,
    pub(crate) request: u32,
    /// The restart's question, not an install's.
    pub(crate) restart: bool,
}

impl Ask {
    /// The question this confirmation asks.
    pub(crate) fn question(&self) -> Question {
        Question {
            owner: self.owner,
            request: self.request,
            restart: self.restart,
        }
    }
}

/// One plugin's last firmware check: `None` while it runs.
type Slot = Option<CheckResult>;

#[derive(Clone, Copy)]
struct CheckResult {
    request: u32,
    reason: u8,
    facts: Facts,
    bits: Bits,
    /// `dev_check.needs_draft` (a plugin check that ended with reason 10).
    needs_draft: u32,
}

/// What a finished check found: a firmware image's facts or a plugin's. One
/// slot holds either, so a result is no bigger than the larger of the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Facts {
    None,
    Firmware(CheckFacts),
    Plugin(PluginFacts),
}

/// A finished firmware check, for op 6 and op 16.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) request: u32,
    pub(crate) reason: u8,
    facts: Facts,
    bits: Bits,
    needs_draft: u32,
}

impl Checked {
    /// `dev_check.needs_draft`: the interface revision a plugin file needs,
    /// 0 for any other result.
    pub(crate) fn needs_draft(&self) -> u32 {
        self.needs_draft
    }

    /// The firmware image's facts, `None` for a plugin's check or one that
    /// found none.
    pub(crate) fn facts(&self) -> Option<&CheckFacts> {
        match &self.facts {
            Facts::Firmware(facts) => Some(facts),
            _ => None,
        }
    }

    /// The plugin's facts, `None` for a firmware check or one that found none.
    pub(crate) fn plugin_facts(&self) -> Option<&PluginFacts> {
        match &self.facts {
            Facts::Plugin(facts) => Some(facts),
            _ => None,
        }
    }

    /// Whether this is the result of a plugin check (`check_file` kind 0).
    pub(crate) fn is_plugin(&self) -> bool {
        matches!(self.facts, Facts::Plugin(_))
    }

    /// `dev_check.flags` bit 5: the image leaves out plugins built in now.
    pub(crate) fn leaves_out_some(&self) -> bool {
        (0..usize::from(self.bits.count)).any(|index| self.bits.leaves_out(index))
    }

    /// Whether the running built-in `index` (op 4's order) is left out.
    pub(crate) fn leaves_out(&self, index: usize) -> bool {
        self.bits.leaves_out(index)
    }
}

/// The firmware install machine and the check results.
pub(crate) struct FirmwareInstall {
    phase: Phase,
    /// The committed image that waits for its restart (`dev_firmware.state` 3).
    waiting: Option<Waiting>,
    /// How the last install ended, if it failed (state 4).
    failed: Option<Outcome>,
    results: Vec<(u64, Slot)>,
    /// Bytes written and to write of the open install (`dev_firmware`).
    progress: (u64, u64),
    /// The dual-boot facts (`dev_firmware.flags`) and the latch.
    dual: Dual,
}

/// What `dev_firmware.flags` tells (`device.md` 3, `docs/dual-boot.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Dual {
    /// The device has a second app partition to share.
    capable: bool,
    /// The setup was declared at the start (marker or `paperpaw.ini`).
    declared: bool,
    /// An install asked for found, or a change to `auto` made, the setup
    /// declared: it stays so until the next start whatever the file says.
    latched: bool,
    /// `paperpaw.ini` says `dual-boot = off`.
    ini_off: bool,
}

impl Default for FirmwareInstall {
    fn default() -> Self {
        Self::new()
    }
}

fn outcome(job: &Job, reason: u8) -> Outcome {
    Outcome {
        owner: job.owner,
        request: job.request,
        kind: if job.install.is_some() {
            KIND_FIRMWARE
        } else {
            KIND_CHECK
        },
        reason,
        flags: 0,
        id: None,
    }
}

/// What an image's check says an install would end with (device.md 5.5), `0`
/// where it fits.
fn verdict(facts: &CheckFacts, job: &Job, room: u64, running: &str) -> u8 {
    if !facts.chip_ok {
        return Reason::OTHER_CHIP.0;
    }
    if job.foreign.is_some_and(|named| named != facts.foreign) {
        // The image is not the kind the request named.
        return Reason::DAMAGED.0;
    }
    if room == 0 {
        return Reason::NOT_HERE.0;
    }
    if u64::from(facts.size) > room {
        return Reason::NO_ROOM.0;
    }
    if facts.foreign {
        return 0;
    }
    let args = job.install;
    match facts.signer {
        Signer::Unsigned => return Reason::UNSIGNED.0,
        Signer::Other { key } if args.and_then(|args| args.key) != Some(key) => {
            return Reason::UNKNOWN_KEY.0;
        }
        _ => {}
    }
    let older_ok = args.is_some_and(|args| args.older_ok);
    match (
        Version::parse(facts.version.as_str()),
        Version::parse(running),
    ) {
        (None, _) => Reason::DAMAGED.0,
        (Some(image), Some(now)) if image <= now && !older_ok => OLDER,
        _ => 0,
    }
}

impl FirmwareInstall {
    pub(crate) const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            waiting: None,
            failed: None,
            results: Vec::new(),
            progress: (0, 0),
            dual: Dual {
                capable: false,
                declared: false,
                latched: false,
                ini_off: false,
            },
        }
    }

    /// The host's dual-boot facts, read once when the driver is built.
    pub(crate) fn set_dual_host(&mut self, capable: bool, declared: bool) {
        self.dual.capable = capable;
        self.dual.declared = declared;
    }

    /// What `paperpaw.ini` says now (`dev_firmware.flags` bit 1).
    pub(crate) fn set_ini_off(&mut self, off: bool) {
        self.dual.ini_off = off;
    }

    /// From now on until the next start the setup counts as declared (a change
    /// to `auto`, Senior R4): an install ends with reason 19.
    pub(crate) fn latch_declared(&mut self) {
        self.dual.latched = true;
    }

    /// `dev_firmware.flags` (draft 35): bit 0 `shared`, bit 1 `ini_off`. A
    /// device with no second app partition to share says 0.
    /// Whether nothing runs, waits for its restart, failed or is kept for a
    /// plugin to read, on a device that boots one firmware: a new machine is
    /// the same ([`super::firmware_driver::Driver::at_rest`]).
    pub(crate) fn at_rest(&self) -> bool {
        self.quiet() && self.failed.is_none() && self.results.is_empty()
    }

    /// Whether nothing runs or waits for a restart: what is left (check
    /// results, a failed install's outcome) is only read, so the machine may
    /// rest with them ([`super::firmware_driver::Driver::into_parts`]).
    pub(crate) fn quiet(&self) -> bool {
        matches!(self.phase, Phase::Idle) && self.waiting.is_none() && self.dual == Dual::default()
    }

    pub(crate) fn dual_flags(&self) -> u32 {
        if !self.dual.capable {
            return 0;
        }
        u32::from(self.dual.declared || self.dual.latched) | (u32::from(self.dual.ini_off) << 1)
    }

    /// Whether a job runs or waits for the user (no other job starts).
    pub(crate) fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    /// Whether a device step can be taken now (not idle, not waiting for the
    /// user's answer).
    pub(crate) fn can_step(&self) -> bool {
        matches!(
            self.phase,
            Phase::Checking(..) | Phase::Writing(..) | Phase::Reading(..)
        )
    }

    /// `dev_firmware.state`.
    pub(crate) fn state(&self) -> State {
        match &self.phase {
            Phase::Asking(..) | Phase::RestartAsking(_) => State::WaitingForUser,
            Phase::Checking(job, _) if job.install.is_none() => self.resting(),
            Phase::Checking(..) | Phase::Writing(..) | Phase::Reading(..) => State::Installing,
            Phase::Idle => self.resting(),
        }
    }

    fn resting(&self) -> State {
        if self.waiting.is_some() {
            State::Installed
        } else if self.failed.is_some() {
            State::Failed
        } else {
            State::Idle
        }
    }

    /// Bytes written and bytes to write of the install that runs (`dev_firmware.done`,
    /// `total`); `(0, 0)` while none does.
    pub(crate) fn progress(&self) -> (u64, u64) {
        match self.phase {
            Phase::Writing(..) | Phase::Reading(..) => self.progress,
            _ => (0, 0),
        }
    }

    /// The request the state belongs to and the reason of the last end (0
    /// while none ended), as `owner` may know them: another plugin's request
    /// number is not told (`(0, 0)`), and a check is no install, so a check
    /// that runs is not the state's request either.
    pub(crate) fn last(&self, owner: u64) -> (u32, u8) {
        let (of, request, reason) = match &self.phase {
            Phase::Asking(job, _) | Phase::Writing(job, ..) | Phase::Reading(job, _) => {
                (job.owner, job.request, 0)
            }
            Phase::Checking(job, _) if job.install.is_some() => (job.owner, job.request, 0),
            Phase::Checking(..) | Phase::RestartAsking(_) | Phase::Idle => {
                match (&self.waiting, &self.failed) {
                    (Some(waiting), _) => (waiting.owner, waiting.request, 0),
                    (None, Some(outcome)) => (outcome.owner, outcome.request, outcome.reason),
                    (None, None) => return (0, 0),
                }
            }
        };
        if of == owner {
            (request, reason)
        } else {
            (0, 0)
        }
    }

    // ---- check results ----

    /// Reserves `owner`'s place for a check result at submit (its old result
    /// goes at once: the next check replaces it), so a finished check always
    /// has room.
    ///
    /// # Errors
    /// `ERR_FULL` when [`RESULTS_MAX`] other plugins keep one,
    /// `ERR_NO_MEMORY`.
    pub(crate) fn reserve_check(&mut self, owner: u64) -> Result<(), i32> {
        if let Some((_, slot)) = self.results.iter_mut().find(|(key, _)| *key == owner) {
            *slot = None;
            return Ok(());
        }
        if self.results.len() >= RESULTS_MAX {
            return Err(ERR_FULL);
        }
        self.results.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        self.results.push((owner, None));
        Ok(())
    }

    /// `owner`'s last finished check (op 6, op 16).
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` when it has none, `ERR_BUSY` while one runs.
    pub(crate) fn checked(&self, owner: u64) -> Result<Checked, i32> {
        match self.results.iter().find(|(key, _)| *key == owner) {
            None => Err(ERR_NOT_FOUND),
            Some((_, None)) => Err(ERR_BUSY),
            Some((_, Some(result))) => Ok(Checked {
                request: result.request,
                reason: result.reason,
                facts: result.facts,
                bits: result.bits,
                needs_draft: result.needs_draft,
            }),
        }
    }

    /// The rows of op 16 need an image whose built-ins PaperPaw can read.
    ///
    /// # Errors
    /// As [`Self::checked`], and `ERR_UNSUPPORTED` for a foreign image.
    pub(crate) fn leaves_out_of(&self, owner: u64) -> Result<Checked, i32> {
        let checked = self.checked(owner)?;
        match checked.facts {
            Facts::Firmware(facts) if !facts.foreign => Ok(checked),
            // The rows of op 16 are a firmware check's: the caller's last
            // check was a plugin's, so it has no finished firmware check.
            Facts::Plugin(_) => Err(ERR_NOT_FOUND),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    /// A plugin stopped: its result and place go.
    pub(crate) fn forget(&mut self, owner: u64) {
        self.results.retain(|(key, _)| *key != owner);
    }

    /// A restart forgets everything but the check that is running, which the
    /// caller ends with [`Self::abort`] first.
    pub(crate) fn clear_results(&mut self) {
        self.results.clear();
    }

    fn keep(&mut self, owner: u64, result: CheckResult) {
        if let Some((_, slot)) = self.results.iter_mut().find(|(key, _)| *key == owner) {
            *slot = Some(result);
        }
    }

    fn mark_running(&mut self, owner: u64) {
        if let Some((_, slot)) = self.results.iter_mut().find(|(key, _)| *key == owner) {
            *slot = None;
        }
    }

    // ---- the job ----

    /// Starts `job` (the queue's head, when nothing else runs): pass 1 begins.
    /// A job that cannot begin ends at once, and so does a firmware install
    /// sent while an image waits for its restart (reason 21).
    pub(crate) fn start(&mut self, job: Job, firmware: &mut dyn FirmwareHost) -> Option<Outcome> {
        if self.busy() {
            // The queue never does this: the job is refused whole.
            return Some(outcome(&job, Reason::OTHER.0));
        }
        if job.install.is_some() && self.waiting.is_some() {
            return Some(outcome(&job, RESTART_PENDING));
        }
        if job.install.is_some() && self.dual.capable && (self.dual.declared || self.dual.latched) {
            // Declared as dual boot: no firmware is installed here.
            return Some(outcome(&job, Reason::NOT_HERE.0));
        }
        if job.install.is_none() {
            self.mark_running(job.owner);
        }
        match firmware.begin_check(&job.path) {
            Ok(()) => {
                let mut bits = Bits::NONE;
                bits.count = u8::try_from(BUILT_INS_MAX).unwrap_or(u8::MAX);
                self.phase = Phase::Checking(job, bits);
                None
            }
            Err(reason) => Some(self.finish(job, reason.0, None, Bits::NONE)),
        }
    }

    /// One bounded device step of the open job.
    pub(crate) fn step(
        &mut self,
        firmware: &mut dyn FirmwareHost,
        storage: &dyn Storage,
        built_ins: &dyn BuiltIns,
        now_ms: u64,
    ) -> Event {
        match core::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Checking(job, bits) => self.check_step(job, bits, firmware, storage, built_ins),
            Phase::Writing(job, facts, last) => {
                self.write_step(job, facts, last, firmware, storage, now_ms)
            }
            Phase::Reading(job, facts) => self.read_step(job, facts, firmware),
            other => {
                // Nothing to step (idle, or waiting for the user's answer).
                self.phase = other;
                Event::Quiet
            }
        }
    }

    fn check_step(
        &mut self,
        job: Job,
        mut bits: Bits,
        firmware: &mut dyn FirmwareHost,
        storage: &dyn Storage,
        built_ins: &dyn BuiltIns,
    ) -> Event {
        bits.count = u8::try_from(built_ins.count().min(BUILT_INS_MAX)).unwrap_or(u8::MAX);
        let step = firmware.check_step(storage, &mut |name| {
            if let Some(index) = built_ins.index_of(name) {
                bits.set(index);
            }
        });
        match step {
            Step::Progress { .. } => {
                self.phase = Phase::Checking(job, bits);
                Event::Quiet
            }
            Step::Failed(reason) => Event::Ended(self.finish(job, reason.0, None, bits)),
            Step::Done(facts) => {
                let running = firmware.running();
                let reason = verdict(&facts, &job, running.room, running.version);
                if job.install.is_none() {
                    return Event::Ended(self.finish(job, reason, Some(facts), bits));
                }
                if reason != 0 {
                    return Event::Ended(self.finish(job, reason, Some(facts), bits));
                }
                let ask = Ask {
                    owner: job.owner,
                    request: job.request,
                    project: facts.project,
                    version: facts.version,
                    signer: facts.signer,
                    foreign: facts.foreign,
                    older: matches!(
                        (Version::parse(facts.version.as_str()), Version::parse(running.version)),
                        (Some(image), Some(now)) if image <= now
                    ),
                    other_key: matches!(facts.signer, Signer::Other { .. }),
                    restart: false,
                };
                self.phase = Phase::Asking(job, facts);
                Event::Ask(ask)
            }
        }
    }

    fn write_step(
        &mut self,
        job: Job,
        facts: CheckFacts,
        last: Option<u64>,
        firmware: &mut dyn FirmwareHost,
        storage: &dyn Storage,
        now_ms: u64,
    ) -> Event {
        match firmware.write_step(storage) {
            Step::Failed(reason) => Event::Ended(self.finish(job, reason.0, None, Bits::NONE)),
            Step::Done(()) => {
                self.phase = Phase::Reading(job, facts);
                Event::Quiet
            }
            Step::Progress { done, total } => {
                self.progress = (u64::from(done), u64::from(total));
                let (owner, request) = (job.owner, job.request);
                let due = last.is_none_or(|last| now_ms.saturating_sub(last) >= PROGRESS_EVERY_MS);
                self.phase = Phase::Writing(job, facts, if due { Some(now_ms) } else { last });
                if due {
                    Event::Progress {
                        owner,
                        request,
                        done,
                        total,
                    }
                } else {
                    Event::Quiet
                }
            }
        }
    }

    fn read_step(&mut self, job: Job, facts: CheckFacts, firmware: &mut dyn FirmwareHost) -> Event {
        match firmware.readback_step() {
            Step::Failed(reason) => Event::Ended(self.finish(job, reason.0, None, Bits::NONE)),
            Step::Progress { .. } => {
                self.phase = Phase::Reading(job, facts);
                Event::Quiet
            }
            Step::Done(()) => match firmware.commit() {
                Err(reason) => Event::Ended(self.finish(job, reason.0, None, Bits::NONE)),
                Ok(()) => {
                    self.failed = None;
                    self.waiting = Some(Waiting {
                        owner: job.owner,
                        request: job.request,
                        project: facts.project,
                        version: facts.version,
                        signer: facts.signer,
                        foreign: facts.foreign,
                    });
                    Event::Ended(outcome(&job, 0))
                }
            },
        }
    }

    /// Ends `job`: a check keeps its result (`facts` and `bits`), an install
    /// that failed is state 4. The job's buffers are freed with it.
    fn finish(&mut self, job: Job, reason: u8, facts: Option<CheckFacts>, bits: Bits) -> Outcome {
        let ended = outcome(&job, reason);
        // A check that failed partway (`facts` is `None`) and one of an image
        // that is damaged or for another chip tell nothing true about the
        // image: no facts, no built-ins. A check that finished keeps both even
        // where it refuses (unsigned, an unknown key, older, no room): op 6
        // gives the update plugin the key to ask for, the version and the size.
        let (facts, bits) = match facts {
            Some(_) if matches!(reason, r if r == Reason::OTHER_CHIP.0 || r == Reason::DAMAGED.0) => {
                (Facts::None, Bits::NONE)
            }
            Some(facts) => (Facts::Firmware(facts), bits),
            None => (Facts::None, Bits::NONE),
        };
        if job.install.is_none() {
            self.keep(
                job.owner,
                CheckResult {
                    request: job.request,
                    reason,
                    facts,
                    bits,
                    needs_draft: 0,
                },
            );
        } else if reason != 0 {
            self.failed = Some(ended);
        }
        self.phase = Phase::Idle;
        ended
    }

    /// The user answered the confirmation of `question`. A yes begins pass 2,
    /// bound to the digest asked about; a no or a dismissal ends the job. An
    /// answer to a question that is not the open one changes nothing.
    pub(crate) fn answer(
        &mut self,
        question: Question,
        answer: Answer,
        firmware: &mut dyn FirmwareHost,
    ) -> Event {
        match core::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Asking(job, facts)
                if !question.restart
                    && (question.owner, question.request) == (job.owner, job.request) =>
            {
                match answer {
                    Answer::Yes => {
                        let key = job.install.and_then(|args| args.key);
                        match firmware.begin_write(&job.path, facts.sha256, key, facts.foreign) {
                            Ok(()) => {
                                self.phase = Phase::Writing(job, facts, None);
                                Event::Quiet
                            }
                            Err(reason) => {
                                Event::Ended(self.finish(job, reason.0, None, Bits::NONE))
                            }
                        }
                    }
                    other => Event::Ended(self.finish(job, declined(other), None, Bits::NONE)),
                }
            }
            Phase::RestartAsking(request)
                if question.restart
                    && question.request == request
                    && self
                        .waiting
                        .is_some_and(|waiting| waiting.owner == question.owner) =>
            {
                self.answer_restart(request, answer)
            }
            other => {
                self.phase = other;
                Event::Quiet
            }
        }
    }

    /// Whether the request runs for good: a check, or an install after the
    /// user's yes (copy, check, write). Before that, and for a restart that
    /// waits for its answer, it still waits (op 13 state 0).
    pub(crate) fn runs(&self, owner: u64, request: u32) -> bool {
        match &self.phase {
            Phase::Writing(job, ..) | Phase::Reading(job, _) => {
                (job.owner, job.request) == (owner, request)
            }
            Phase::Checking(job, _) => {
                job.install.is_none() && (job.owner, job.request) == (owner, request)
            }
            Phase::Asking(..) | Phase::RestartAsking(_) | Phase::Idle => false,
        }
    }

    // ---- the restart ----

    /// The submit-time gates of `firmware_restart`, changing nothing.
    ///
    /// # Errors
    /// `ERR_STATE` with no image waiting, `ERR_DENIED` for another plugin.
    pub(crate) fn restart_gate(&self, owner: u64) -> Result<(), i32> {
        match self.waiting {
            None => Err(ERR_STATE),
            Some(waiting) if waiting.owner != owner => Err(ERR_DENIED),
            Some(_) => Ok(()),
        }
    }

    /// The request of the image that waits for its restart.
    pub(crate) fn waiting_request(&self) -> Option<u32> {
        self.waiting.map(|waiting| waiting.request)
    }

    /// `owner`'s check `request` was stopped before it ran: its result says
    /// `reason` (no facts), so the result slot is not left running.
    pub(crate) fn stopped_check(&mut self, owner: u64, request: u32, reason: u8) {
        self.keep(
            owner,
            CheckResult {
                request,
                reason,
                facts: Facts::None,
                bits: Bits::NONE,
                needs_draft: 0,
            },
        );
    }

    /// `owner`'s plugin check was stopped before it ran or ended: its result
    /// slot says what it found (`facts`, none for a refusal).
    pub(crate) fn keep_plugin(
        &mut self,
        owner: u64,
        request: u32,
        reason: u8,
        facts: Option<PluginFacts>,
        needs_draft: u32,
    ) {
        self.keep(
            owner,
            CheckResult {
                request,
                reason,
                facts: facts.map_or(Facts::None, Facts::Plugin),
                bits: Bits::NONE,
                needs_draft,
            },
        );
    }

    /// `firmware_restart` at submit: only the plugin whose install waits may
    /// ask, and the host asks again.
    ///
    /// # Errors
    /// `ERR_STATE` with no image waiting, `ERR_DENIED` for another plugin,
    /// `ERR_BUSY` while a job runs (the caller retries after it).
    pub(crate) fn request_restart(&mut self, owner: u64, request: u32) -> Result<Ask, i32> {
        let waiting = self.waiting.ok_or(ERR_STATE)?;
        if waiting.owner != owner {
            return Err(ERR_DENIED);
        }
        if self.busy() {
            return Err(ERR_BUSY);
        }
        self.phase = Phase::RestartAsking(request);
        Ok(Ask {
            owner,
            request,
            project: waiting.project,
            version: waiting.version,
            signer: waiting.signer,
            foreign: waiting.foreign,
            older: false,
            other_key: false,
            restart: true,
        })
    }

    fn answer_restart(&mut self, request: u32, answer: Answer) -> Event {
        let Some(waiting) = self.waiting else {
            return Event::Quiet;
        };
        if answer == Answer::Yes {
            // Held in the phase until `restart` runs (what plugins keep is
            // saved first).
            self.phase = Phase::RestartAsking(request);
            return Event::Restart;
        }
        Event::Ended(Outcome {
            owner: waiting.owner,
            request,
            kind: KIND_FIRMWARE,
            reason: declined(answer),
            flags: 0,
            id: None,
        })
    }

    /// Starts the image that was chosen. It returns only where it cannot: the
    /// outcome says why, and the image keeps waiting.
    pub(crate) fn restart(&mut self, firmware: &mut dyn FirmwareHost) -> Option<Outcome> {
        let waiting = self.waiting?;
        let request = match core::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::RestartAsking(request) => request,
            other => {
                self.phase = other;
                return None;
            }
        };
        let reason = firmware.restart().err()?;
        Some(Outcome {
            owner: waiting.owner,
            request,
            kind: KIND_FIRMWARE,
            reason: reason.0,
            flags: 0,
            id: None,
        })
    }

    /// Ends the open job as the host stopped it (shutdown, card swap): the
    /// device's job is abandoned and its sender is told reason 20.
    pub(crate) fn abort(&mut self, firmware: &mut dyn FirmwareHost) -> Option<Outcome> {
        let job = match core::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Checking(job, _) | Phase::Asking(job, _) => job,
            Phase::Writing(job, ..) | Phase::Reading(job, _) => job,
            Phase::RestartAsking(request) => {
                // The image keeps waiting; the plugin that asked is told.
                return self.waiting.map(|waiting| Outcome {
                    owner: waiting.owner,
                    request,
                    kind: KIND_FIRMWARE,
                    reason: Reason::STOPPED.0,
                    flags: 0,
                    id: None,
                });
            }
            Phase::Idle => return None,
        };
        firmware.abandon();
        Some(self.finish(job, Reason::STOPPED.0, None, Bits::NONE))
    }
}

/// Reason 2 for a no, 3 for a dismissal.
fn declined(answer: Answer) -> u8 {
    if answer == Answer::No {
        DECLINED
    } else {
        DISMISSED
    }
}

#[cfg(test)]
#[path = "firmware_install_tests.rs"]
mod tests;
