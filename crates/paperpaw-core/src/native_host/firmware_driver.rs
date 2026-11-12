//! What the kernel keeps around the firmware install machine
//! ([`FirmwareInstall`], `docs/abi/device.md` 3, 4, 5.3): the device-wide
//! request table, the job queue, the outcomes kept for `install_done` and
//! op 13, and the pump that gives the machine one bounded device step per call.
//!
//! * **Requests.** At most [`REQUESTS_MAX`] firmware and check requests wait
//!   device-wide (running one included). A request is accepted whole or not at
//!   all: its table entry, its place in the queue, its room for the outcome
//!   and, for a check, its result slot are all reserved at submit
//!   (`ERR_NO_MEMORY` or `ERR_FULL` before anything changes), so an accepted
//!   request always has room to end and is never dropped later.
//! * **One at a time.** The queue hands the machine its head only when the
//!   machine is idle. A firmware request that comes while another runs, or
//!   while a check runs, waits in the queue; it is not refused.
//! * **Outcomes.** A job that ends puts its [`Outcome`] in `ready`, in room
//!   reserved at submit. The shell's event path takes it with
//!   [`Driver::take_ready`] and, once the edge is delivered, calls
//!   [`Driver::delivered`], which keeps it for op 13 ([`Driver::kept`]) until
//!   that plugin's next request of the same kind or its stop.
//! * **Stopping.** [`Driver::abort_all`] (suspend, shutdown, card swap) ends
//!   the running job and every waiting one with reason 20;
//!   [`Driver::sender_stopped`] drops one plugin's requests.
//! * **Plugin jobs.** A [`PluginJob`] (a plugin check, an install, a removal:
//!   `plugin_job`) shares the queue, the request limit and the outcomes; it
//!   runs when it is the queue's head and no firmware job runs, and takes one
//!   bounded step per pump ([`JobEnv`]). The jobs are kept inline as
//!   [`PluginJobs`] in a vector of their own, reserved exactly at submit (a
//!   firmware-only device keeps none, and a queue entry stays small); the
//!   queue only names them. A firmware job waits behind it
//!   and the other way round: never two at once. Without a plugin host
//!   ([`Driver::set_plugin_host`]) none is accepted.
//! * **Not here.** Nothing of this runs inside a plugin's callback; the pump
//!   is called from the main loop.
//!
//! [`FirmwareInstall`]: super::firmware_install::FirmwareInstall
use super::dual_boot_job::After;
use super::firmware_cell::{DriverParts, MachineRef};
use super::firmware_install::{
    Answer, Ask, BuiltIns, Event, Job, KIND_CHECK, KIND_FIRMWARE, Outcome, Question,
};
use super::plugin_job::{
    Finished, ID_MAX, InstallGrants, JobEnv, JobStep, KIND_INSTALL, KIND_REMOVAL, PluginHost,
    PluginJob, PluginQuestion,
};
use super::plugin_jobs::PluginJobs;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::Cell;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_platform::{
    Storage,
    firmware::{FirmwareHost, Text},
};
use paperpaw_sdk::native::keys::{self, Revocation};

/// Most install, removal, firmware and check requests waiting device-wide.
pub(crate) const REQUESTS_MAX: usize = 4;
/// Most open requests one plugin has, waiting or not yet told (a restart
/// counts too), so one
/// plugin cannot take every device-wide place.
pub(crate) const PER_PLUGIN_MAX: usize = 2;
/// Most outcomes kept for op 13 (one per plugin and kind).
pub(crate) const KEPT_MAX: usize = 8;
/// Revocations the driver holds before it keeps them (two batches).
pub(crate) const MAX_REVOCATIONS_QUEUED: usize = 16;
/// Reason 20 (`ERR_CANCELLED`): the host stopped the job.
const STOPPED: u8 = 20;
/// Most steps [`Driver::finish_stopped`] takes at once.
const FINISH_STEPS: u32 = 256;

/// What op 13 reads of one request (`dev_install.state`: 0 waiting in the
/// queue, 1 running, 2 ended, its outcome in `outcome`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lookup {
    pub(crate) kind: u32,
    pub(crate) state: u32,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) done: u64,
    pub(crate) total: u64,
}

/// One accepted request that has not ended.
#[derive(Clone, Copy)]
struct Entry {
    owner: u64,
    request: u32,
    kind: u32,
    /// `firmware_changed` is due: set by progress, cleared once told. One
    /// mark per request however many steps pass, so a progress flood never
    /// queues anything; the event is encoded from the state at delivery.
    changed: bool,
}

/// A kept outcome: `None` while the plugin's request of that kind waits.
#[derive(Clone, Copy)]
pub(crate) struct Kept {
    pub(super) owner: u64,
    kind: u32,
    /// The request the slot waits for: only its own outcome fills it.
    request: u32,
    pub(super) outcome: Option<Outcome>,
}

/// One job in the queue: a firmware install or check, or a plugin job.
enum Queued {
    Firmware(Job),
    /// A plugin job, kept in [`Driver`]'s `plugin_jobs` under this key.
    Plugin {
        owner: u64,
        request: u32,
    },
}

impl Queued {
    fn owner(&self) -> u64 {
        match self {
            Self::Firmware(job) => job.owner,
            Self::Plugin { owner, .. } => *owner,
        }
    }

    fn request(&self) -> u32 {
        match self {
            Self::Firmware(job) => job.request,
            Self::Plugin { request, .. } => *request,
        }
    }
}

/// The firmware request table, queue and pump.
pub(crate) struct Driver {
    host: Box<dyn FirmwareHost>,
    machine: MachineRef,
    entries: Vec<Entry>,
    queue: Vec<Queued>,
    /// The plugin jobs, waiting and running, room reserved at submit.
    plugin_jobs: Vec<PluginJobs>,
    /// The plugin job that runs, `(owner, request)` (the machine is idle
    /// meanwhile).
    running: Option<(u64, u32)>,
    /// The running job was aborted but must finish ([`PluginJob::must_finish`])
    /// or runs on without its sender: it is stepped on to its end, and the
    /// flag says whether its sender is still there to be told.
    finishing: Option<bool>,
    /// What the device tells a plugin job; `None`: no plugin job is accepted.
    plugin_host: Option<PluginHost>,
    /// Which plugins hold the trusted-installer grant (`None`: none does).
    grants: Option<Box<dyn InstallGrants>>,
    /// The host's reading of the grants, before each step (no allocation).
    fed: super::install_grants::Granted,
    /// The running plugin job asked the user and waits for the answer.
    plugin_question: Option<PluginQuestion>,
    /// The nonce of the confirmation that shows `plugin_question`: only an
    /// answer that carries it is taken, once.
    plugin_shown: Option<u128>,
    /// The running plugin job asks the host to close this plugin's instances
    /// and waits for [`Self::plugin_unloaded`] (a removal).
    plugin_unload: Option<Text<ID_MAX>>,
    /// The running removal asks the host to forget this plugin's secrets and
    /// grants and waits for [`Self::plugin_forgotten`].
    plugin_forget: Option<Text<ID_MAX>>,
    /// Revocations handed in (`trust_revoked`), kept at the next pump with no
    /// plugin job running; at most [`MAX_REVOCATIONS_QUEUED`].
    revocations: Vec<Revocation>,
    /// Ended, not yet delivered; room for every entry is reserved.
    ready: Vec<Outcome>,
    kept: Vec<Kept>,
    /// Outcomes that found no room (never in practice: see [`Self::end`]).
    lost: u32,
    /// Answers and aborts that found the machine borrowed and did nothing
    /// (never from a callback; counted so a test or a log can see it).
    skipped: Rc<Cell<u32>>,
    /// Until when the pump does not try to tell again (a plugin that could
    /// not be called); `None` when nothing is held back.
    blocked_until: Option<u64>,
    /// The confirmation the host must show (an install's or a restart's),
    /// until [`Self::take_ask`].
    ask: Option<Ask>,
    /// The confirmation that shows: its nonce and the question it asks. Only
    /// an answer that carries the nonce is taken, once.
    shown: Option<(u128, Question)>,
    /// A yes to the restart came: the kernel restarts the device.
    restart_wanted: bool,
    /// The plugins were told PaperPaw stops for a restart; it comes next pass.
    stopping: bool,
    /// The restart wanted is a dual-boot change's: of the running image, no
    /// new one waits ([`FirmwareHost::restart_running`]).
    dual_restart: bool,
    /// The host opened `firmware_dual_boot` ([`Self::open_dual_boot`]).
    dual_open: bool,
    /// What a dual-boot change wrote that the machine did not take yet (it
    /// was borrowed): applied at the next pass, never dropped, so the
    /// latch of mode 0 protects as soon as the machine is free.
    dual_pending: Option<After>,
    /// `paperpaw.ini`'s `dual-boot` was read for `dev_firmware.flags` (once,
    /// at the first idle pass with the card).
    ini_read: bool,
}

impl Driver {
    /// A driver over the device's firmware and the machine the ops read.
    #[cfg(test)]
    pub(crate) fn new(host: Box<dyn FirmwareHost>, machine: MachineRef) -> Self {
        Self::from_parts(DriverParts::new(host), machine, Rc::new(Cell::new(0)))
    }

    /// A driver made of `parts`, over `machine`, counting its skips in
    /// `skipped`.
    pub(crate) fn from_parts(
        parts: DriverParts,
        machine: MachineRef,
        skipped: Rc<Cell<u32>>,
    ) -> Self {
        let DriverParts {
            host,
            plugin_host,
            grants,
            kept,
            machine: _,
        } = parts;
        if let Ok(mut machine) = machine.try_borrow_mut() {
            machine.set_dual_host(host.dual_boot_capable(), host.dual_boot_declared());
        }
        Self {
            host,
            machine,
            entries: Vec::new(),
            queue: Vec::new(),
            plugin_jobs: Vec::new(),
            running: None,
            finishing: None,
            plugin_host,
            grants,
            fed: super::install_grants::Granted::NONE,
            plugin_question: None,
            plugin_shown: None,
            plugin_unload: None,
            plugin_forget: None,
            revocations: Vec::new(),
            ready: Vec::new(),
            kept,
            lost: 0,
            skipped,
            blocked_until: None,
            ask: None,
            shown: None,
            restart_wanted: false,
            stopping: false,
            dual_restart: false,
            dual_open: false,
            dual_pending: None,
            ini_read: false,
        }
    }

    /// What the driver is made of, taken out in place for it to rest
    /// ([`super::firmware_cell::FirmwareCell::rest`]); it keeps a host that
    /// is gone and is dropped. The outcomes kept for op 13 go with them (a
    /// slot that waits for no request is dropped), and so does the machine
    /// while it keeps a check's result or a failed install.
    pub(crate) fn take_parts(&mut self) -> DriverParts {
        let mut kept = core::mem::take(&mut self.kept);
        kept.retain(|kept| kept.outcome.is_some());
        let keeps = self
            .machine
            .try_borrow()
            .map_or(true, |machine| !machine.at_rest());
        let machine = if keeps {
            self.machine.try_clone()
        } else {
            None
        };
        DriverParts {
            host: core::mem::replace(&mut self.host, Box::new(super::firmware_cell::Gone)),
            plugin_host: self.plugin_host.take(),
            grants: self.grants.take(),
            kept,
            machine,
        }
    }

    /// Whether nothing is left to run, ask or tell, and the machine runs
    /// nothing either ([`FirmwareInstall::quiet`]): made again from its parts
    /// (which keep the outcomes and results plugins may still read), a driver
    /// is the same. Never for a device that can boot two firmwares (its latch
    /// and `paperpaw.ini` reading are kept).
    ///
    /// [`FirmwareInstall::quiet`]: super::firmware_install::FirmwareInstall::quiet
    pub(crate) fn at_rest(&self) -> bool {
        self.entries.is_empty()
            && self.queue.is_empty()
            && self.plugin_jobs.is_empty()
            && self.running.is_none()
            && self.finishing.is_none()
            && self.plugin_question.is_none()
            && self.plugin_shown.is_none()
            && self.plugin_unload.is_none()
            && self.plugin_forget.is_none()
            && self.revocations.is_empty()
            && self.ready.is_empty()
            && self.blocked_until.is_none()
            && self.ask.is_none()
            && self.shown.is_none()
            && !self.restart_wanted
            && !self.stopping
            && !self.dual_restart
            && !self.dual_open
            && self.dual_pending.is_none()
            && !self.host.dual_boot_capable()
            && self.machine.is_unique()
            && self
                .machine
                .try_borrow()
                .is_ok_and(|machine| machine.quiet())
    }

    /// Takes a revocation to keep at the next idle pump; `false`: no room (the
    /// library lists it again), and it is dropped.
    pub(crate) fn queue_revocation(&mut self, revocation: Revocation) -> bool {
        if self.revocations.len() >= MAX_REVOCATIONS_QUEUED
            || self.revocations.try_reserve(1).is_err()
        {
            return false;
        }
        self.revocations.push(revocation);
        true
    }

    /// Keeps the revocations that hold (`device.md` 5.1); the others are
    /// dropped, counted. One bounded write; never while a plugin job runs.
    fn keep_revocations(&mut self, storage: &dyn Storage) {
        let list = core::mem::take(&mut self.revocations);
        if keys::keep_revoked(storage, &list).is_err() {
            self.skip();
        }
    }

    /// How many answers and aborts found the machine borrowed and did nothing.
    #[cfg(test)]
    pub(crate) fn skipped(&self) -> u32 {
        self.skipped.get()
    }

    /// The counter of skips, for callers that cannot borrow the driver when
    /// they skip (a stage's release, the apply after a callback).
    pub(crate) fn skip_counter(&self) -> Rc<Cell<u32>> {
        Rc::clone(&self.skipped)
    }

    fn skip(&self) {
        self.skipped.set(self.skipped.get().saturating_add(1));
    }

    /// How many requests wait (the running one included).
    pub(crate) fn waiting(&self) -> usize {
        self.entries.len()
    }

    /// Whether `owner` has a waiting request numbered `request`.
    pub(crate) fn holds(&self, owner: u64, request: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| (entry.owner, entry.request) == (owner, request))
    }

    /// Room for one more request of `owner` and `kind`, reserved before
    /// anything is changed. Takes `owner`'s kept outcome of that kind (the
    /// next request replaces it) only once the request is `commit`ted.
    fn room(&mut self, owner: u64, request: u32, kind: u32) -> Result<(), i32> {
        if request == 0 || self.holds(owner, request) {
            return Err(ERR_INVALID);
        }
        // A request counts for its plugin until its `install_done` is told.
        let open = self.entries.iter().filter(|e| e.owner == owner).count()
            + self.ready.iter().filter(|o| o.owner == owner).count();
        if self.entries.len() >= REQUESTS_MAX || open >= PER_PLUGIN_MAX {
            return Err(ERR_FULL);
        }
        let has_slot = self
            .kept
            .iter()
            .any(|kept| (kept.owner, kept.kind) == (owner, kind));
        if !has_slot && self.kept.len() >= KEPT_MAX && !self.evictable() {
            return Err(ERR_FULL);
        }
        let wanted = self.entries.len() + 1;
        self.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        self.queue.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        // An outcome for every waiting request, besides those not yet taken.
        self.ready.try_reserve(wanted).map_err(|_| ERR_NO_MEMORY)?;
        if !has_slot && self.kept.len() < KEPT_MAX {
            self.kept.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        }
        Ok(())
    }

    /// Whether a kept outcome of a plugin with no waiting request can go.
    fn evictable(&self) -> bool {
        self.kept.iter().any(|kept| self.free_slot(kept))
    }

    /// A slot nobody waits for: no request of its plugin and kind waits or has
    /// an outcome not yet told. It may be empty (an outcome that never
    /// reached its plugin) or hold an old outcome.
    fn free_slot(&self, kept: &Kept) -> bool {
        let of_it = |owner: u64, kind: u32| (owner, kind) == (kept.owner, kept.kind);
        !self
            .entries
            .iter()
            .any(|entry| of_it(entry.owner, entry.kind))
            && !self
                .ready
                .iter()
                .any(|outcome| of_it(outcome.owner, outcome.kind))
    }

    /// Takes the plugin's slot for `kind`: its old outcome goes, and the slot
    /// waits (`None`) for the new request's. Room was reserved by [`Self::room`].
    fn take_slot(&mut self, owner: u64, kind: u32, request: u32) {
        if let Some(kept) = self
            .kept
            .iter_mut()
            .find(|kept| (kept.owner, kept.kind) == (owner, kind))
        {
            kept.outcome = None;
            kept.request = request;
            return;
        }
        if self.kept.len() >= KEPT_MAX
            && let Some(at) = self.kept.iter().position(|kept| self.free_slot(kept))
        {
            self.kept.remove(at);
        }
        self.kept.push(Kept {
            owner,
            kind,
            request,
            outcome: None,
        });
    }

    fn accept(&mut self, job: Queued, kind: u32) {
        let (owner, request) = (job.owner(), job.request());
        self.take_slot(owner, kind, request);
        self.entries.push(Entry {
            owner,
            request,
            kind,
            changed: false,
        });
        self.queue.push(job);
    }

    /// `install_firmware` at submit. The image stays on the card at `path`
    /// (the caller resolved the root); nothing is copied.
    ///
    /// # Errors
    /// `ERR_INVALID` for request 0, one that waits already or a flag beyond
    /// bits 0 and 1; `ERR_FULL` with [`REQUESTS_MAX`] waiting;
    /// `ERR_NO_MEMORY`. A refusal changes nothing.
    pub(crate) fn submit_install(
        &mut self,
        owner: u64,
        request: u32,
        path: &str,
        key: Option<[u8; 32]>,
        flags: u32,
    ) -> Result<(), i32> {
        let job = Job::install(owner, request, path, key, flags)?;
        self.room(owner, request, KIND_FIRMWARE)?;
        self.accept(Queued::Firmware(job), KIND_FIRMWARE);
        Ok(())
    }

    /// The device tells plugin jobs about itself (`PluginHost`): from now on
    /// [`Self::submit_plugin`] accepts them.
    pub(crate) fn set_plugin_host(&mut self, host: PluginHost) {
        self.plugin_host = Some(host);
    }

    /// The host's trusted-installer grants (`core.grant.install.<plugin>`,
    /// `device.md` 5.2): without them no install skips its question.
    pub(crate) fn set_install_grants(&mut self, grants: Box<dyn InstallGrants>) {
        self.grants = Some(grants);
    }

    /// The plugins that have a plugin job here, each once, into `owners`
    /// (the first [`REQUESTS_MAX`] of them: it is all the device holds);
    /// how many.
    pub(crate) fn plugin_owners(&self, owners: &mut [u64; REQUESTS_MAX]) -> usize {
        let mut count = 0;
        for job in &self.plugin_jobs {
            if count < owners.len() && !owners[..count].contains(&job.owner()) {
                owners[count] = job.owner();
                count += 1;
            }
        }
        count
    }

    /// Replaces the trusted-installer grants by the host's present reading
    /// (see [`super::install_grants`]).
    pub(crate) fn feed_install_grants(&mut self, granted: super::install_grants::Granted) {
        self.fed = granted;
    }

    /// Whether `owner` may install without a question: the host's reading,
    /// or (tests) a hook set by [`Self::set_install_grants`].
    fn granted(&self, owner: u64) -> bool {
        self.fed.holds(owner)
            || self
                .grants
                .as_ref()
                .is_some_and(|grants| grants.granted(owner))
    }

    #[cfg(test)]
    pub(crate) fn install_granted(&self, owner: u64) -> bool {
        self.granted(owner)
    }

    /// The question the running plugin job waits on, for the host to show
    /// (`None` when none waits). Answered by [`Self::answer_plugin`].
    pub(crate) fn plugin_question(&self) -> Option<&PluginQuestion> {
        self.plugin_question.as_ref()
    }

    /// The open question the host has not shown yet.
    pub(crate) fn peek_plugin_question(&self) -> Option<PluginQuestion> {
        self.plugin_question.filter(|_| self.plugin_shown.is_none())
    }

    /// The plugin whose instances the running job waits for the host to close.
    pub(crate) fn peek_unload(&self) -> Option<Text<ID_MAX>> {
        self.plugin_unload
    }

    /// The plugin whose secrets and grants the running removal waits for the
    /// host to forget.
    pub(crate) fn peek_forget(&self) -> Option<Text<ID_MAX>> {
        self.plugin_forget
    }

    /// The host forgot (or could not forget, `ok` false): the job goes on.
    pub(crate) fn plugin_forgotten(&mut self, ok: bool) {
        self.plugin_forget = None;
        if let Some((owner, request)) = self.running
            && let Some(at) = self.plugin_at(owner, request)
        {
            self.plugin_jobs[at].forgotten(ok);
        }
    }

    /// The host closed the instances asked for: the job goes on.
    pub(crate) fn plugin_unloaded(&mut self) {
        self.plugin_unload = None;
        if let Some((owner, request)) = self.running
            && let Some(at) = self.plugin_at(owner, request)
        {
            self.plugin_jobs[at].unloaded();
        }
    }

    /// The host opened the confirmation of `question` with `nonce`.
    pub(crate) fn plugin_question_shown(&mut self, nonce: u128, question: &PluginQuestion) {
        if self.plugin_question.as_ref() == Some(question) && self.plugin_shown.is_none() {
            self.plugin_shown = Some(nonce);
        }
    }

    /// The confirmation of `question` could not be shown: it ends as
    /// dismissed (reason 3), never as a yes.
    pub(crate) fn plugin_question_dismissed(&mut self, question: &PluginQuestion) {
        if self.plugin_question.as_ref() == Some(question) {
            self.answer_plugin(question.owner, question.request, Answer::Dismissed);
        }
    }

    /// The user's answer to [`Self::plugin_question`]; `false` for a question
    /// that is not (or no longer) the open one.
    pub(crate) fn answer_plugin(&mut self, owner: u64, request: u32, answer: Answer) -> bool {
        let open = self
            .plugin_question
            .as_ref()
            .is_some_and(|question| (question.owner, question.request) == (owner, request));
        if !open || self.running != Some((owner, request)) {
            return false;
        }
        let Some(at) = self.plugin_at(owner, request) else {
            return false;
        };
        self.plugin_question = None;
        self.plugin_shown = None;
        self.plugin_jobs[at].answer(answer);
        true
    }

    /// Whether plugin jobs are accepted (a plugin host is set).
    pub(crate) fn plugins_accepted(&self) -> bool {
        self.plugin_host.is_some()
    }

    /// A plugin job at submit (`check_file` kind 0, later an install and a
    /// removal): the same limits, request table and outcome room as a firmware
    /// request, accepted whole or not at all.
    ///
    /// # Errors
    /// `ERR_UNSUPPORTED` with no plugin host, a kind that is not a plugin
    /// job's; else as [`Self::submit_install`].
    pub(crate) fn submit_plugin(&mut self, job: PluginJobs) -> Result<(), i32> {
        let (owner, request, kind) = (job.owner(), job.request(), job.kind());
        let dual = matches!(job, PluginJobs::DualBoot(_));
        if self.plugin_host.is_none()
            || !(dual || matches!(kind, KIND_INSTALL | KIND_REMOVAL | KIND_CHECK))
        {
            return Err(ERR_UNSUPPORTED);
        }
        self.room(owner, request, kind)?;
        // Exactly the room of this job: nothing is kept for jobs to come.
        self.plugin_jobs
            .try_reserve_exact(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        if kind == KIND_CHECK {
            // The result slot last: it is the one reservation that changes the machine.
            self.machine
                .try_borrow_mut()
                .map_err(|_| ERR_BUSY)?
                .reserve_check(owner)?;
        }
        self.accept(Queued::Plugin { owner, request }, kind);
        self.plugin_jobs.push(job);
        Ok(())
    }

    /// Opens `command.firmware_dual_boot` to plugins: until then it is
    /// `ERR_UNSUPPORTED` at submit. The host opens it once `paperpaw.ini` and
    /// `.paperpaw/` are closed to plugins' file access (`files.md` 1.1), so the
    /// command is the only way a plugin changes the `[firmware]` section.
    pub(crate) fn open_dual_boot(&mut self) {
        self.dual_open = true;
    }

    /// Whether [`Self::open_dual_boot`] was called.
    pub(crate) fn dual_boot_open(&self) -> bool {
        self.dual_open
    }

    /// `firmware_dual_boot` at submit: a job of the queue, behind a running
    /// install too.
    ///
    /// # Errors
    /// As [`Self::submit_plugin`].
    pub(crate) fn submit_dual_boot(
        &mut self,
        owner: u64,
        request: u32,
        mode: super::dual_boot_ini::Mode,
    ) -> Result<(), i32> {
        let capable = self.host.dual_boot_capable();
        self.submit_plugin(PluginJobs::DualBoot(super::dual_boot_job::DualBoot::new(
            owner, request, mode, capable,
        )))
    }

    /// `check_file` kind 1 at submit.
    ///
    /// # Errors
    /// As [`Self::submit_install`]; `ERR_FULL` also when four other plugins
    /// keep a check result.
    pub(crate) fn submit_check(&mut self, owner: u64, request: u32, path: &str) -> Result<(), i32> {
        let job = Job::check(owner, request, path)?;
        self.room(owner, request, KIND_CHECK)?;
        // The result slot last: it is the one reservation that changes the machine.
        self.machine
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .reserve_check(owner)?;
        self.accept(Queued::Firmware(job), KIND_CHECK);
        Ok(())
    }

    /// Takes back a request accepted by `submit_*` whose batch never applied
    /// (a callback error, a refused drain). A job that already runs is left to
    /// end: only a request still waiting in the queue goes. Its kept slot,
    /// emptied at submit, stays until it is evicted or its plugin's next
    /// request.
    pub(crate) fn cancel(&mut self, owner: u64, request: u32) {
        let queued = self
            .queue
            .iter()
            .position(|job| (job.owner(), job.request()) == (owner, request));
        if let Some(at) = queued {
            let job = self.queue.remove(at);
            let kind = self.kind_of(&job);
            if matches!(job, Queued::Plugin { .. }) {
                self.plugin_jobs
                    .retain(|plugin| (plugin.owner(), plugin.request()) != (owner, request));
            }
            self.entries
                .retain(|entry| (entry.owner, entry.request) != (owner, request));
            // The result place reserved for the check goes with it, unless
            // another check of the plugin still needs it.
            let other_check = self
                .entries
                .iter()
                .any(|entry| entry.owner == owner && entry.kind == KIND_CHECK);
            if kind == KIND_CHECK
                && !other_check
                && let Ok(mut machine) = self.machine.try_borrow_mut()
            {
                machine.forget(owner);
            }
        }
    }

    /// The machine the device ops read (`HostServices::wire_firmware_install`).
    pub(crate) fn machine(&self) -> Option<MachineRef> {
        self.machine.try_clone()
    }

    /// The submit gates of `firmware_restart`, and the room its request will
    /// need (reserved now, so the apply after the callback cannot fail for
    /// room). Nothing else changes.
    ///
    /// # Errors
    /// `ERR_STATE` with none waiting, `ERR_DENIED` for another plugin,
    /// `ERR_BUSY`, `ERR_FULL` (device-wide or per plugin), `ERR_NO_MEMORY`.
    pub(crate) fn restart_gate(&mut self, owner: u64) -> Result<(), i32> {
        let request = {
            let machine = self.machine.try_borrow().map_err(|_| ERR_BUSY)?;
            machine.restart_gate(owner)?;
            machine.waiting_request().ok_or(ERR_STATE)?
        };
        self.room(owner, request, KIND_FIRMWARE)
    }

    /// The confirmation waiting to be shown, if no other shows.
    pub(crate) fn peek_ask(&self) -> Option<Ask> {
        self.ask.filter(|_| self.shown.is_none())
    }

    /// The host opened the confirmation of `ask` with `nonce`.
    pub(crate) fn ask_shown(&mut self, nonce: u128, ask: Ask) {
        if self
            .ask
            .is_some_and(|waiting| waiting.question() == ask.question())
        {
            self.ask = None;
            self.shown = Some((nonce, ask.question()));
        }
    }

    /// The confirmation of `ask` could not be shown: it ends as dismissed.
    pub(crate) fn ask_dismissed(&mut self, ask: Ask) {
        if self
            .ask
            .is_some_and(|waiting| waiting.question() == ask.question())
        {
            self.ask = None;
            self.answer(ask.question(), Answer::Dismissed);
        }
    }

    /// The user's answer to the confirmation shown with `nonce`; any other
    /// nonce, or none shown, changes nothing.
    pub(crate) fn answer_shown(&mut self, nonce: u128, answer: Answer) -> Option<Event> {
        if self.plugin_shown == Some(nonce)
            && let Some(question) = self.plugin_question
        {
            self.answer_plugin(question.owner, question.request, answer);
            return Some(Event::Quiet);
        }
        let (shown, question) = self.shown?;
        if shown != nonce {
            return None;
        }
        self.shown = None;
        Some(self.answer(question, answer))
    }

    /// The shown confirmation ends as dismissed (sleep); its nonce, so that
    /// the screen can be closed.
    pub(crate) fn dismiss_shown(&mut self) -> Option<u128> {
        if let Some(nonce) = self.plugin_shown
            && let Some(question) = self.plugin_question
        {
            self.answer_plugin(question.owner, question.request, Answer::Dismissed);
            return Some(nonce);
        }
        let (nonce, question) = self.shown.take()?;
        self.answer(question, Answer::Dismissed);
        Some(nonce)
    }

    /// A yes to the restart waits for the kernel to restart the device.
    pub(crate) fn restart_wanted(&self) -> bool {
        self.restart_wanted
    }

    /// Whether the plugins were told to stop for the restart; `now` sets it.
    pub(crate) fn stopping(&mut self, now: bool) -> bool {
        core::mem::replace(&mut self.stopping, now)
    }

    /// Restarts into the chosen image. It returns only where it cannot (then
    /// `true`): the request ends with the device's reason (through `end`, so its place and
    /// outcome are kept like any request's) and the image keeps waiting.
    pub(crate) fn restart_now(&mut self) -> bool {
        self.restart_wanted = false;
        if core::mem::take(&mut self.dual_restart) {
            // A dual-boot change's: the request ended already (reason 0).
            return self.host.restart_running().is_err();
        }
        let outcome = match self.machine.try_borrow_mut() {
            Ok(mut machine) => machine.restart(&mut *self.host),
            Err(_) => {
                self.skip();
                None
            }
        };
        match outcome {
            Some(outcome) => {
                self.end(outcome);
                true
            }
            None => false,
        }
    }

    /// `firmware_restart` at submit: only the plugin whose install waits may
    /// ask; the returned question is the host's to show. The restart is a
    /// request of its own: it takes a place in the table and room for its
    /// outcome like an install, and its `install_done` carries the number of
    /// the install it restarts (the plugin's own), so it can be matched.
    ///
    /// # Errors
    /// `ERR_STATE` with none waiting, `ERR_DENIED` for another plugin,
    /// `ERR_BUSY` while a job runs, `ERR_FULL` with [`REQUESTS_MAX`] waiting,
    /// `ERR_NO_MEMORY`. A refusal changes nothing.
    pub(crate) fn request_restart(&mut self, owner: u64) -> Result<Ask, i32> {
        let request = {
            let machine = self.machine.try_borrow().map_err(|_| ERR_BUSY)?;
            machine.restart_gate(owner)?;
            machine.waiting_request().ok_or(ERR_STATE)?
        };
        self.room(owner, request, KIND_FIRMWARE)?;
        let ask = self
            .machine
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .request_restart(owner, request)?;
        // Room was reserved above; the restart has no job in the queue.
        self.take_slot(owner, KIND_FIRMWARE, request);
        self.entries.push(Entry {
            owner,
            request,
            kind: KIND_FIRMWARE,
            changed: false,
        });
        self.ask = Some(ask);
        Ok(ask)
    }

    /// The user answered the open confirmation.
    pub(crate) fn answer(&mut self, question: Question, answer: Answer) -> Event {
        let event = match self.machine.try_borrow_mut() {
            Ok(mut machine) => machine.answer(question, answer, &mut *self.host),
            Err(_) => {
                self.skip();
                return Event::Quiet;
            }
        };
        self.note(event)
    }

    /// One bounded step: a plugin job that runs takes its step; else the
    /// queue's head starts when the machine is idle, else the open job takes
    /// one device step. Never called from a callback.
    pub(crate) fn pump(
        &mut self,
        storage: &dyn Storage,
        built_ins: &dyn BuiltIns,
        now_ms: u64,
    ) -> Event {
        if self.running.is_none() && !self.revocations.is_empty() {
            self.keep_revocations(storage);
        }
        if let Some(after) = self.dual_pending.take()
            && !self.dual_machine(after)
        {
            self.dual_pending = Some(after);
        }
        if !self.ini_read && self.running.is_none() && self.host.dual_boot_capable() {
            self.read_ini(storage);
        }
        if self.running.is_some() {
            // A job that asked the user waits for the answer; nothing to step.
            if self.plugin_question.is_some()
                || self.plugin_unload.is_some()
                || self.plugin_forget.is_some()
            {
                return Event::Quiet;
            }
            return self.plugin_step(storage, built_ins);
        }
        let Ok(mut machine) = self.machine.try_borrow_mut() else {
            return Event::Quiet;
        };
        if !machine.busy()
            && let Some(&Queued::Plugin { owner, request }) = self.queue.first()
        {
            self.queue.remove(0);
            drop(machine);
            self.running = Some((owner, request));
            // Its first step comes with the next pump.
            return Event::Quiet;
        }
        let event = if machine.busy() || self.queue.is_empty() {
            machine.step(&mut *self.host, storage, built_ins, now_ms)
        } else if let Queued::Firmware(job) = self.queue.remove(0) {
            match machine.start(job, &mut *self.host) {
                Some(outcome) => Event::Ended(outcome),
                None => Event::Quiet,
            }
        } else {
            Event::Quiet
        };
        drop(machine);
        self.note(event)
    }

    /// One bounded step of the plugin job that runs; its end is noted like a
    /// firmware job's, and a check's facts are kept in its owner's result slot.
    fn plugin_step(&mut self, storage: &dyn Storage, built_ins: &dyn BuiltIns) -> Event {
        let Some((owner, request)) = self.running else {
            return Event::Quiet;
        };
        let Some(at) = self.plugin_at(owner, request) else {
            // Not reachable: a running job is always kept.
            self.running = None;
            self.finishing = None;
            return Event::Quiet;
        };
        let kind = self.plugin_jobs[at].kind();
        let granted = self.granted(owner);
        // A machine borrowed elsewhere counts as "an image waits": a
        // dual-boot change then ends with reason 21, the safe side.
        let waiting = self
            .machine
            .try_borrow()
            .map_or(true, |machine| machine.waiting_request().is_some());
        self.plugin_jobs[at].image_waiting(waiting);
        let finished = match self.plugin_host.as_ref() {
            Some(host) => match self.plugin_jobs[at].step(&JobEnv {
                storage,
                host,
                built_ins,
                granted,
            }) {
                JobStep::More | JobStep::Wait => return Event::Quiet,
                JobStep::Ask(question) => {
                    self.plugin_question = Some(question);
                    return Event::Quiet;
                }
                JobStep::Unload(id) => {
                    self.plugin_unload = Some(id);
                    return Event::Quiet;
                }
                JobStep::Forget(id) => {
                    self.plugin_forget = Some(id);
                    return Event::Quiet;
                }
                JobStep::Done(finished) => finished,
            },
            // Not reachable (no job is accepted without a host): ends, told.
            None => Finished::refused(1),
        };
        // The job goes (its `Drop` releases what it holds).
        self.plugin_unload = None;
        self.plugin_forget = None;
        let after = self.plugin_jobs[at].after();
        // In place: `remove` would hand the job over on this frame.
        drop(self.plugin_jobs.drain(at..=at));
        self.dual_after(after);
        self.running = None;
        if self.finishing.take() == Some(false) {
            // Its sender stopped meanwhile: no entry, no outcome, no result.
            return Event::Quiet;
        }
        if kind == KIND_CHECK {
            match self.machine.try_borrow_mut() {
                Ok(mut machine) => {
                    machine.keep_plugin(
                        owner,
                        request,
                        finished.reason,
                        finished.facts,
                        finished.needs_draft,
                    );
                }
                Err(_) => self.skip(),
            }
        }
        self.note(Event::Ended(Outcome {
            owner,
            request,
            kind,
            reason: finished.reason,
            flags: finished.flags,
            id: finished.changed,
        }))
    }

    /// What a dual-boot change that wrote `paperpaw.ini` leaves behind: the
    /// flags follow the file, `auto` latches the setup as declared, and a yes
    /// to `off` restarts the device.
    pub(crate) fn dual_after(&mut self, after: After) {
        if after == After::Nothing {
            return;
        }
        if !self.dual_machine(after) {
            // The machine is borrowed: kept, applied at the next pass.
            self.dual_pending = Some(after);
        }
        if after == After::Restart {
            self.dual_restart = true;
            self.restart_wanted = true;
        }
    }

    /// Tells the machine what a dual-boot change wrote: the flags follow the
    /// file and `auto` latches the setup as declared. `false`: it is borrowed.
    fn dual_machine(&mut self, after: After) -> bool {
        let off = after == After::Restart;
        let Ok(mut machine) = self.machine.try_borrow_mut() else {
            return false;
        };
        machine.set_ini_off(off);
        if !off {
            machine.latch_declared();
        }
        true
    }

    /// Reads `paperpaw.ini`'s `dual-boot` once for `dev_firmware.flags`; a file
    /// that cannot be read says nothing (bit 1 stays 0).
    fn read_ini(&mut self, storage: &dyn Storage) {
        if core::mem::replace(&mut self.ini_read, true) {
            return;
        }
        let off = super::dual_boot_ini::card_choice(storage)
            .is_ok_and(|choice| choice == super::dual_boot_ini::Choice::Off);
        if let Ok(mut machine) = self.machine.try_borrow_mut() {
            machine.set_ini_off(off);
        }
    }

    /// The place of the plugin job `(owner, request)`.
    fn plugin_at(&self, owner: u64, request: u32) -> Option<usize> {
        self.plugin_jobs
            .iter()
            .position(|job| (job.owner(), job.request()) == (owner, request))
    }

    /// Records what ended: the entry goes, the outcome waits for delivery.
    fn note(&mut self, event: Event) -> Event {
        match event {
            Event::Ended(outcome) => self.end(outcome),
            Event::Ask(ask) => self.ask = Some(ask),
            Event::Restart => self.restart_wanted = true,
            Event::Progress { owner, request, .. } => {
                if let Some(entry) = self
                    .entries
                    .iter_mut()
                    .find(|entry| (entry.owner, entry.request) == (owner, request))
                {
                    entry.changed = true;
                }
            }
            _ => {}
        }
        event
    }

    fn end(&mut self, outcome: Outcome) {
        if let Some(at) = self.entries.iter().position(|entry| {
            (entry.owner, entry.request, entry.kind)
                == (outcome.owner, outcome.request, outcome.kind)
        }) {
            self.entries.remove(at);
        }
        // Room for every entry (a restart's included) was reserved at submit,
        // so this never grows; if it ever has to, it asks for room and, only
        // if that fails, counts the loss where a test can see it.
        if self.ready.len() == self.ready.capacity() && self.ready.try_reserve(1).is_err() {
            self.lost = self.lost.saturating_add(1);
            return;
        }
        self.ready.push(outcome);
    }

    /// What op 13 says of `owner`'s request `request`: waiting, running or
    /// ended (the outcome kept for it), `None` for one it has not got.
    pub(crate) fn lookup(&self, owner: u64, request: u32) -> Option<Lookup> {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| (entry.owner, entry.request) == (owner, request))
        {
            // Running only after the user's yes (or for a check): before it,
            // queued or in front of its confirmation, it waits.
            let machine = self.machine.try_borrow();
            let running = machine
                .as_ref()
                .is_ok_and(|machine| machine.runs(owner, request))
                || self.running == Some((owner, request));
            let waiting = !running;
            let (done, total) = machine.map_or((0, 0), |machine| machine.progress());
            return Some(Lookup {
                kind: entry.kind,
                state: if waiting { 0 } else { 1 },
                outcome: None,
                done: if waiting { 0 } else { done },
                total: if waiting { 0 } else { total },
            });
        }
        // Ended: its edge may not have reached the plugin yet.
        self.ready
            .iter()
            .copied()
            .find(|outcome| (outcome.owner, outcome.request) == (owner, request))
            .map(|outcome| Lookup {
                kind: outcome.kind,
                state: 2,
                outcome: Some(outcome),
                done: 0,
                total: 0,
            })
            .or_else(|| super::firmware_cell::kept_lookup(&self.kept, owner, request))
    }

    /// `dev_firmware.state` now (0 while the machine is in use).
    pub(crate) fn state(&self) -> u32 {
        self.machine
            .try_borrow()
            .map_or(0, |machine| machine.state().number())
    }

    fn tell_pending(&self) -> bool {
        !self.ready.is_empty() || self.entries.iter().any(|entry| entry.changed)
    }

    /// Whether the pump has something to do now: a job that can take a step,
    /// a queue head that can start, or something that can be told. A request
    /// that waits for the user's answer, or behind one, is no work (the
    /// answer or the dialog's end wakes the pump), and what could not be told
    /// waits for [`Self::due`], so the device may idle.
    pub(crate) fn has_work(&self, now: u64) -> bool {
        let held = self.blocked_until.is_some_and(|until| now < until);
        // A job that waits for the user's answer has nothing to step.
        let stepping = self.running.is_some() && self.plugin_question.is_none();
        let revoking = self.running.is_none() && !self.revocations.is_empty();
        if self.restart_wanted
            || self.dual_pending.is_some()
            || stepping
            || revoking
            || (!held && self.tell_pending())
        {
            return true;
        }
        self.machine.try_borrow().map_or(true, |machine| {
            machine.can_step() || (!machine.busy() && !self.queue.is_empty())
        })
    }

    /// When to look again for what could not be told, if anything waits.
    pub(crate) fn due(&self) -> Option<u64> {
        self.blocked_until.filter(|_| self.tell_pending())
    }

    /// Its plugin could not be called: no new try before `until`.
    pub(crate) fn hold_back(&mut self, until: u64) {
        self.blocked_until = Some(until);
    }

    /// Something was told: tries are free again.
    pub(crate) fn released(&mut self) {
        self.blocked_until = None;
    }

    /// The ended outcome of `owner`'s request, if it is still the oldest
    /// one waiting; another one (the ready list changed during the callback)
    /// is left alone.
    pub(crate) fn take_ready_if(&mut self, owner: u64, request: u32, kind: u32) -> Option<Outcome> {
        let first = self.ready.first()?;
        if (first.owner, first.request, first.kind) != (owner, request, kind) {
            return None;
        }
        self.take_ready()
    }

    /// The first request whose `firmware_changed` is due, left marked.
    pub(crate) fn peek_changed(&self) -> Option<(u64, u32)> {
        self.entries
            .iter()
            .find(|entry| entry.changed)
            .map(|entry| (entry.owner, entry.request))
    }

    /// Its `firmware_changed` was told (or has nobody to tell).
    pub(crate) fn clear_changed(&mut self, owner: u64, request: u32) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| (entry.owner, entry.request) == (owner, request))
        {
            entry.changed = false;
        }
    }

    /// The oldest ended request's outcome, left in place.
    pub(crate) fn peek_ready(&self) -> Option<Outcome> {
        self.ready.first().copied()
    }

    /// The oldest ended request's outcome, for its `install_done` edge.
    pub(crate) fn take_ready(&mut self) -> Option<Outcome> {
        if self.ready.is_empty() {
            None
        } else {
            Some(self.ready.remove(0))
        }
    }

    /// The edge of `outcome` reached its plugin: op 13 reads it from now on.
    pub(crate) fn delivered(&mut self, outcome: Outcome) {
        // A restart reuses its install's number: one asked for from the
        // callback just told waits with that number, and the old outcome must
        // not be written into its slot.
        let again = self.entries.iter().any(|entry| {
            (entry.owner, entry.kind, entry.request)
                == (outcome.owner, outcome.kind, outcome.request)
        });
        if again {
            return;
        }
        if let Some(kept) = self
            .kept
            .iter_mut()
            .find(|kept| (kept.owner, kept.kind) == (outcome.owner, outcome.kind))
            // A later request of the same kind (made from the callback that
            // was just told) has its own slot state: its number differs.
            .filter(|kept| kept.request == outcome.request)
        {
            kept.outcome = Some(outcome);
        }
    }

    /// The last delivered outcome of `owner`'s request of `kind` (op 13).
    pub(crate) fn kept(&self, owner: u64, kind: u32) -> Option<Outcome> {
        self.kept
            .iter()
            .find(|kept| (kept.owner, kept.kind) == (owner, kind))
            .and_then(|kept| kept.outcome)
    }

    /// Suspend, shutdown or a card swap: the running job and every waiting
    /// one end with reason 20, each for its sender.
    pub(crate) fn abort_all(&mut self) {
        // A restart that waited for the next pass does not come.
        self.restart_wanted = false;
        self.stopping = false;
        let mut got = true;
        let aborted = match self.machine.try_borrow_mut() {
            Ok(mut machine) => machine.abort(&mut *self.host),
            Err(_) => {
                self.skip();
                got = false;
                None
            }
        };
        if let Some(outcome) = aborted {
            self.end(outcome);
        }
        // The plugin job that runs is stopped where it stands (its `Drop`
        // releases what it holds) and ends like a waiting one. One that must
        // finish (past the point where the card holds half of a change)
        // stays, aborted: [`Self::finish_stopped`] steps it on.
        // Nobody is left to answer.
        self.plugin_question = None;
        self.plugin_shown = None;
        // The device is going down: its instances are not waited for.
        self.plugin_unload = None;
        self.plugin_forget = None;
        let mut running = None;
        if let Some((owner, request)) = self.running.take()
            && let Some(at) = self.plugin_at(owner, request)
        {
            self.plugin_jobs[at].abort(STOPPED);
            // A job whose sender stopped already (`Some(false)`) has no entry
            // and nobody to tell: that stays so.
            let orphan = self.finishing == Some(false);
            if self.plugin_jobs[at].must_finish() {
                self.running = Some((owner, request));
                self.finishing.get_or_insert(true);
            } else {
                // In place: `remove` would hand the job over on this frame.
                drop(self.plugin_jobs.drain(at..=at));
                self.finishing = None;
                if !orphan {
                    running = Some(Queued::Plugin { owner, request });
                }
            }
        }
        // The waiting jobs go with their queue entries, below.
        let kept = self.running;
        self.plugin_jobs
            .retain(|job| kept == Some((job.owner(), job.request())));
        for job in running.into_iter().chain(core::mem::take(&mut self.queue)) {
            let kind = self.kind_of(&job);
            if kind == KIND_CHECK
                && let Ok(mut machine) = self.machine.try_borrow_mut()
            {
                // The check never ran: its result slot says why, so ops 6 and
                // 16 do not answer `ERR_BUSY` until its plugin's next check.
                machine.stopped_check(job.owner(), job.request(), STOPPED);
            }
            self.end(Outcome {
                owner: job.owner(),
                request: job.request(),
                kind,
                reason: STOPPED,
                flags: 0,
                id: None,
            });
        }
        // What is left has no job: a restart that waits for its answer. With
        // the machine borrowed the running job was not stopped and its entry
        // is still its own: it ends (once) when the job does.
        while got
            && let Some(entry) = self
                .entries
                .iter()
                .find(|entry| !self.is_finishing(entry))
                .copied()
        {
            self.end(Outcome {
                owner: entry.owner,
                request: entry.request,
                kind: entry.kind,
                reason: STOPPED,
                flags: 0,
                id: None,
            });
        }
    }

    /// Whether `entry` is the plugin job that runs on after its abort.
    fn is_finishing(&self, entry: &Entry) -> bool {
        self.finishing.is_some() && self.running == Some((entry.owner, entry.request))
    }

    /// After [`Self::abort_all`] or [`Self::sender_stopped`]: steps a plugin
    /// job that must finish on, at most [`FINISH_STEPS`] steps; what is left
    /// (and a job that merely runs on without its sender, which may take
    /// long) is stepped by the pump.
    pub(crate) fn finish_stopped(&mut self, storage: &dyn Storage, built_ins: &dyn BuiltIns) {
        for _ in 0..FINISH_STEPS {
            let must = self
                .running
                .and_then(|(owner, request)| self.plugin_at(owner, request))
                .is_some_and(|at| self.finishing.is_some() && self.plugin_jobs[at].must_finish());
            if !must {
                break;
            }
            self.plugin_step(storage, built_ins);
        }
    }

    fn kind_of(&self, job: &Queued) -> u32 {
        self.entries
            .iter()
            .find(|entry| (entry.owner, entry.request) == (job.owner(), job.request()))
            .map_or(KIND_FIRMWARE, |entry| entry.kind)
    }

    /// `owner` stopped: its waiting requests, its undelivered outcomes, its
    /// kept ones and its check result go. A job of its that runs is abandoned.
    pub(crate) fn sender_stopped(&mut self, owner: u64) -> Option<u128> {
        // The one entry that is not queued is the job that runs: the plugin
        // job's, or the machine's.
        let plugin_runs = self.running.is_some_and(|(runs, _)| runs == owner);
        let running = !plugin_runs
            && self
                .entries
                .iter()
                .any(|entry| entry.owner == owner && !self.queue_has(entry));
        // Its question goes with its sender (or with the job); a confirmation
        // that shows it is closed by the caller.
        let mut closed = None;
        if plugin_runs {
            self.plugin_question = None;
            closed = self.plugin_shown.take();
        }
        if plugin_runs
            && let Some((runs, request)) = self.running
            && let Some(at) = self.plugin_at(runs, request)
        {
            // One the user confirmed runs on, untold; any other stops, and
            // nobody is told either (its sender is gone).
            let keep = self.plugin_jobs[at].keep_without_sender();
            if !keep {
                self.plugin_jobs[at].abort(STOPPED);
            }
            if keep || self.plugin_jobs[at].must_finish() {
                self.finishing = Some(false);
            } else {
                // In place: `remove` would hand the job over on this frame.
                drop(self.plugin_jobs.drain(at..=at));
                self.running = None;
            }
        }
        if self.running.is_none() {
            self.plugin_unload = None;
            self.plugin_forget = None;
        }
        // Its other jobs go; the one that runs on stays.
        let kept = self.running;
        self.plugin_jobs
            .retain(|job| job.owner() != owner || kept == Some((job.owner(), job.request())));
        self.queue.retain(|job| job.owner() != owner);
        if let Ok(mut machine) = self.machine.try_borrow_mut() {
            if running {
                // Its sender is gone: nobody is told.
                let _ = machine.abort(&mut *self.host);
            }
            machine.forget(owner);
        } else {
            self.skip();
        }
        self.entries.retain(|entry| entry.owner != owner);
        self.ready.retain(|outcome| outcome.owner != owner);
        self.kept.retain(|kept| kept.owner != owner);
        // Its confirmation goes with it: nobody is left to answer for.
        if self.ask.is_some_and(|ask| ask.owner == owner) {
            self.ask = None;
        }
        match self.shown {
            Some((nonce, question)) if question.owner == owner => {
                self.shown = None;
                Some(nonce)
            }
            _ => closed,
        }
    }

    fn queue_has(&self, entry: &Entry) -> bool {
        self.queue
            .iter()
            .any(|job| (job.owner(), job.request()) == (entry.owner, entry.request))
    }
}

#[cfg(test)]
#[path = "firmware_driver_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "firmware_driver_install_tests.rs"]
mod install_tests;
#[cfg(test)]
#[path = "firmware_driver_plugin_tests.rs"]
pub(crate) mod plugin_tests;
