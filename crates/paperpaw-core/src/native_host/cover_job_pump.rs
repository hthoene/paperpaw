//! The kernel's side of the library's cover job (`doc_cover_job`): once a pass,
//! from the service pump and never inside a callback, one look at the job. A
//! job whose preparation is over is ended there (the format gets its `close`
//! through the pump's route, as a preparing step's call goes) and the library
//! is told, which raises `event.library_changed` bit 6 for the screens. The
//! slot is the one of either job's kind: a details job's book is read before
//! it is closed and kept here (`library::keep_native_details`), and its end
//! goes to the details job's own end (`details_job_pump`).
use super::{doc_service::JobStep, native_host_call::HostRoute};

/// How many passes of the services and the commands the start runs before its
/// first frame, while the pump asks for another at once: the native reader
/// hears `Started` in one and opens the last book in the next, so home is not
/// drawn first. Work still going after them goes on at the next tick (home
/// stays, as when the start is slow).
const START_SETTLE_PASSES: u32 = 6;

impl crate::Kernel {
    /// One look at the background job; nothing to do for a pass that finds
    /// none.
    #[inline(never)]
    pub(crate) fn native_cover_job_pump(&mut self) {
        let instances = &self.shared.native_instances;
        // Never inside a callback: the next pass looks again.
        if instances.depth() != 0 {
            return;
        }
        let Some(hosts) = instances.hosts() else {
            return;
        };
        let Some(documents) = hosts.documents() else {
            return;
        };
        // A job held back (the start, a book open for reading, a displacement)
        // may begin again: the screens are told to ask again (bit 6).
        if let Some(library) = hosts.library() {
            let given_up = documents.0.cover_job_given_up();
            if given_up != 0 {
                library.0.cover_job_left(given_up);
            }
            if documents.0.cover_jobs_released() {
                library.0.cover_jobs_began();
            }
        }
        if !documents.0.cover_job_active() {
            return;
        }
        let step = {
            let env = self.shared.env(self.now);
            let route = HostRoute {
                registry: instances,
                env: &env,
                meter: &self.shared.meter,
            };
            // What a details job read is kept as the book's details.
            let mut keep = |path: &str, metadata: &paperpaw_sdk::document::Metadata| {
                paperpaw_sdk::library::keep_native_details(&env, path, metadata).is_ok()
            };
            documents.0.cover_job_step(hosts, &route, &mut keep)
        };
        // What the format staged runs now, outside any callback.
        self.native_outbox_run();
        match step {
            JobStep::Ended { ready, key } => {
                if let Some(library) = self
                    .shared
                    .native_instances
                    .hosts()
                    .and_then(|h| h.library())
                {
                    library.0.cover_job_ended(ready, key);
                }
            }
            JobStep::DetailsEnded { filled, key } => self.native_details_job_ended(filled, key),
            JobStep::None | JobStep::Wait => {}
        }
    }
}

/// The plugin whose screens are the library's lists: the job takes no step
/// while one of them is on the stack.
const LIST_PLUGIN: &str = "files";

/// How long the device is left alone after the start or an input before a
/// cover is made, converted or started: the zip and XML of a book (some
/// 20 KiB) never come beside the user's screen or the start's peak (#248).
pub(crate) const COVER_QUIET_MS: u64 = 1_000;

impl crate::Kernel {
    /// Tells the document host whether a library list screen is up, before the
    /// pass steps any preparation (the cover job holds back while one is: the
    /// conversion's peak must not come beside the list's buffers).
    #[inline(never)]
    pub(crate) fn native_cover_list(&mut self) {
        let quiet_until = self.cover_quiet_until();
        let instances = &self.shared.native_instances;
        let Some(documents) = instances.hosts().and_then(|hosts| hosts.documents()) else {
            return;
        };
        let due = documents
            .0
            .cover_job_list(instances.plugin_has_screen(LIST_PLUGIN), quiet_until);
        // The list left: nothing prepares until the quiet moment is over, and
        // an idle device would not look again before the next tick. A timed
        // wake, as for the job's own gap (`doc_pump`); other steps keep theirs.
        if let Some(at) = due
            && !documents.0.preparing()
        {
            instances.set_doc_retry(Some(instances.doc_retry().map_or(at, |old| old.min(at))));
        }
    }

    /// The end of the quiet time after the user's last input (or the start,
    /// or the standby begun): no cover is made, converted or started before
    /// it. No job begins on the way into standby; one asked for there waits
    /// for the quiet time after waking, as it would with its format parked
    /// (#248).
    fn cover_quiet_until(&self) -> u64 {
        let since = self
            .suspended_at
            .map_or(self.last_activity, |at| at.max(self.last_activity));
        since.millis().saturating_add(COVER_QUIET_MS)
    }

    /// An input came (`Kernel::handle`): the quiet time begins anew before
    /// its callbacks run, which may ask for covers.
    pub(crate) fn native_cover_input(&self) {
        let until = self.cover_quiet_until();
        if let Some(documents) = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.documents())
        {
            documents.0.cover_job_input(until);
        }
    }

    /// The end of `Kernel::start`: the commands first (home opens, `Started` is
    /// queued), so that the services' pass inside `finish` tells native plugins
    /// `Started` and a book the reader opens at the start is opened before the
    /// first frame, as interface 6 did inside the start. Home is not drawn first.
    pub(crate) fn finish_start(&mut self) -> crate::Effects {
        self.run_commands();
        self.finish_started()
    }

    /// The end of `Kernel::start`: its effects, then cover jobs of the library
    /// may begin (a format's crash inside start would count as a failed start,
    /// `doc_cover_job`).
    pub(crate) fn finish_started(&mut self) -> crate::Effects {
        let effects = self.finish_settled(START_SETTLE_PASSES);
        // The quiet second counts from the start as from an input.
        self.native_cover_input();
        self.native_cover_jobs_allowed();
        effects
    }

    /// The kernel's start has returned: cover jobs may begin from now on.
    fn native_cover_jobs_allowed(&mut self) {
        let instances = &self.shared.native_instances;
        let Some(hosts) = instances.hosts() else {
            return;
        };
        let left = hosts
            .documents()
            .map_or(0, |documents| documents.0.allow_cover_jobs());
        // The book a reset ended the job of counts as failed (no cover job
        // for it), before anything asks for its cover again. A `cover` held
        // back until now is released by the pump's next look.
        if left != 0
            && let Some(library) = hosts.library()
        {
            library.0.cover_job_left(left);
        }
    }
}

impl crate::Kernel {
    /// The device goes to sleep or stops: the job ends first, before anything
    /// else of the sleep, and nothing restarts it after the wake until a new
    /// `cover` asks, or (a details job) a walk arms it again (`doc_cover_job`).
    pub(crate) fn native_cover_job_suspend(&mut self) {
        let instances = &self.shared.native_instances;
        let Some(hosts) = instances.hosts() else {
            return;
        };
        let Some(documents) = hosts.documents() else {
            return;
        };
        if let Some(details) = documents.0.details_job() {
            details.run().suspend();
        }
        if !documents.0.cover_job_active() {
            return;
        }
        // Not inside a callback: the job ends at the pump's next look.
        if instances.depth() != 0 {
            documents.0.cover_job_suspend_later();
            return;
        }
        let env = self.shared.env(self.now);
        let route = HostRoute {
            registry: instances,
            env: &env,
            meter: &self.shared.meter,
        };
        documents.0.cover_job_abort(hosts, &route);
        self.native_outbox_run();
    }
}
