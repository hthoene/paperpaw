//! The stops of the firmware driver: suspend, shutdown or a card swap
//! ([`Driver::abort_all`]), one plugin stopping ([`Driver::sender_stopped`]),
//! and the bounded steps of a plugin job that must finish after either
//! ([`Driver::finish_stopped`]).
use super::super::firmware_install::{BuiltIns, KIND_CHECK, KIND_FIRMWARE, Outcome};
use super::super::plugin_job::PluginJob;
use super::{Driver, Entry, FINISH_STEPS, Queued, STOPPED};
use paperpaw_platform::Storage;

impl Driver {
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

    pub(super) fn kind_of(&self, job: &Queued) -> u32 {
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
