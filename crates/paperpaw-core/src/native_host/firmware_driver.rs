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
//! * **Not here.** Nothing of this runs inside a plugin's callback; the pump
//!   is called from the main loop. Plugin installs (kind 0) are a seam: they
//!   share the queue and its limit but have no job type yet.
use super::firmware_install::{
    Answer, Ask, BuiltIns, Event, FirmwareInstall, Job, KIND_CHECK, KIND_FIRMWARE, Outcome,
    Question,
};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_platform::{Storage, firmware::FirmwareHost};

/// Most install, removal, firmware and check requests waiting device-wide.
pub(crate) const REQUESTS_MAX: usize = 4;
/// Most open requests one plugin has, waiting or not yet told (a restart
/// counts too), so one
/// plugin cannot take every device-wide place.
pub(crate) const PER_PLUGIN_MAX: usize = 2;
/// Most outcomes kept for op 13 (one per plugin and kind).
pub(crate) const KEPT_MAX: usize = 8;
/// Reason 20 (`ERR_CANCELLED`): the host stopped the job.
const STOPPED: u8 = 20;

/// One accepted request that has not ended.
#[derive(Clone, Copy)]
struct Entry {
    owner: u64,
    request: u32,
    kind: u32,
}

/// A kept outcome: `None` while the plugin's request of that kind waits.
#[derive(Clone, Copy)]
struct Kept {
    owner: u64,
    kind: u32,
    /// The request the slot waits for: only its own outcome fills it.
    request: u32,
    outcome: Option<Outcome>,
}

/// The firmware request table, queue and pump.
pub(crate) struct Driver {
    host: Box<dyn FirmwareHost>,
    machine: Rc<RefCell<FirmwareInstall>>,
    entries: Vec<Entry>,
    queue: Vec<Job>,
    /// Ended, not yet delivered; room for every entry is reserved.
    ready: Vec<Outcome>,
    kept: Vec<Kept>,
    /// Outcomes that found no room (never in practice: see [`Self::end`]).
    lost: u32,
    /// Answers and aborts that found the machine borrowed and did nothing
    /// (never from a callback; counted so a test or a log can see it).
    skipped: u32,
}

impl Driver {
    /// A driver over the device's firmware and the machine the ops read.
    pub(crate) fn new(host: Box<dyn FirmwareHost>, machine: Rc<RefCell<FirmwareInstall>>) -> Self {
        Self {
            host,
            machine,
            entries: Vec::new(),
            queue: Vec::new(),
            ready: Vec::new(),
            kept: Vec::new(),
            lost: 0,
            skipped: 0,
        }
    }

    /// How many answers and aborts found the machine borrowed and did nothing.
    #[cfg(test)]
    pub(crate) fn skipped(&self) -> u32 {
        self.skipped
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

    fn accept(&mut self, job: Job, kind: u32) {
        let (owner, request) = (job.owner, job.request);
        self.take_slot(owner, kind, request);
        self.entries.push(Entry {
            owner,
            request,
            kind,
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
        self.accept(job, KIND_FIRMWARE);
        Ok(())
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
        self.accept(job, KIND_CHECK);
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
            .position(|job| (job.owner, job.request) == (owner, request));
        if let Some(at) = queued {
            let job = self.queue.remove(at);
            let kind = self.kind_of(&job);
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
    pub(crate) fn machine(&self) -> Rc<RefCell<FirmwareInstall>> {
        Rc::clone(&self.machine)
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
        });
        Ok(ask)
    }

    /// The user answered the open confirmation.
    pub(crate) fn answer(&mut self, question: Question, answer: Answer) -> Event {
        let event = match self.machine.try_borrow_mut() {
            Ok(mut machine) => machine.answer(question, answer, &mut *self.host),
            Err(_) => {
                self.skipped = self.skipped.saturating_add(1);
                return Event::Quiet;
            }
        };
        self.note(event)
    }

    /// One bounded step: the queue's head starts when the machine is idle,
    /// else the open job takes one device step. Never called from a callback.
    pub(crate) fn pump(
        &mut self,
        storage: &dyn Storage,
        built_ins: &dyn BuiltIns,
        now_ms: u64,
    ) -> Event {
        let Ok(mut machine) = self.machine.try_borrow_mut() else {
            return Event::Quiet;
        };
        let event = if machine.busy() || self.queue.is_empty() {
            machine.step(&mut *self.host, storage, built_ins, now_ms)
        } else {
            let job = self.queue.remove(0);
            match machine.start(job, &mut *self.host) {
                Some(outcome) => Event::Ended(outcome),
                None => Event::Quiet,
            }
        };
        drop(machine);
        self.note(event)
    }

    /// Records what ended: the entry goes, the outcome waits for delivery.
    fn note(&mut self, event: Event) -> Event {
        if let Event::Ended(outcome) = event {
            self.end(outcome);
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
        let mut got = true;
        let aborted = match self.machine.try_borrow_mut() {
            Ok(mut machine) => machine.abort(&mut *self.host),
            Err(_) => {
                self.skipped = self.skipped.saturating_add(1);
                got = false;
                None
            }
        };
        if let Some(outcome) = aborted {
            self.end(outcome);
        }
        for job in core::mem::take(&mut self.queue) {
            let kind = if self.kind_of(&job) == KIND_CHECK {
                KIND_CHECK
            } else {
                KIND_FIRMWARE
            };
            if kind == KIND_CHECK
                && let Ok(mut machine) = self.machine.try_borrow_mut()
            {
                // The check never ran: its result slot says why, so ops 6 and
                // 16 do not answer `ERR_BUSY` until its plugin's next check.
                machine.stopped_check(job.owner, job.request, STOPPED);
            }
            self.end(Outcome {
                owner: job.owner,
                request: job.request,
                kind,
                reason: STOPPED,
            });
        }
        // What is left has no job: a restart that waits for its answer. With
        // the machine borrowed the running job was not stopped and its entry
        // is still its own: it ends (once) when the job does.
        while got && let Some(entry) = self.entries.first().copied() {
            self.end(Outcome {
                owner: entry.owner,
                request: entry.request,
                kind: entry.kind,
                reason: STOPPED,
            });
        }
    }

    fn kind_of(&self, job: &Job) -> u32 {
        self.entries
            .iter()
            .find(|entry| (entry.owner, entry.request) == (job.owner, job.request))
            .map_or(KIND_FIRMWARE, |entry| entry.kind)
    }

    /// `owner` stopped: its waiting requests, its undelivered outcomes, its
    /// kept ones and its check result go. A job of its that runs is abandoned.
    pub(crate) fn sender_stopped(&mut self, owner: u64) {
        // The one entry that is not queued is the job that runs.
        let running = self
            .entries
            .iter()
            .any(|entry| entry.owner == owner && !self.queue_has(entry));
        self.queue.retain(|job| job.owner != owner);
        if let Ok(mut machine) = self.machine.try_borrow_mut() {
            if running {
                // Its sender is gone: nobody is told.
                let _ = machine.abort(&mut *self.host);
            }
            machine.forget(owner);
        } else {
            self.skipped = self.skipped.saturating_add(1);
        }
        self.entries.retain(|entry| entry.owner != owner);
        self.ready.retain(|outcome| outcome.owner != owner);
        self.kept.retain(|kept| kept.owner != owner);
    }

    fn queue_has(&self, entry: &Entry) -> bool {
        self.queue
            .iter()
            .any(|job| (job.owner, job.request) == (entry.owner, entry.request))
    }
}

#[cfg(test)]
#[path = "firmware_driver_tests.rs"]
mod tests;
