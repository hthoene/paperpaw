//! The kernel's side of the library's cover job (`doc_cover_job`): once a pass,
//! from the service pump and never inside a callback, one look at the job. A
//! job whose preparation is over is ended there (the format gets its `close`
//! through the pump's route, as a preparing step's call goes) and the library
//! is told, which raises `event.library_changed` bit 6 for the screens. The
//! slot is the one of either job's kind: a details job's book is read before
//! it is closed and kept here (`library::keep_native_details`), and its end
//! goes to the details job's own end (`details_job_pump`).
use super::{doc_service::JobStep, native_host_call::HostRoute};

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

impl crate::Kernel {
    /// The end of `Kernel::start`: its effects, then cover jobs of the library
    /// may begin (a format's crash inside start would count as a failed start,
    /// `doc_cover_job`).
    pub(crate) fn finish_started(&mut self) -> crate::Effects {
        let effects = self.finish();
        self.native_cover_jobs_allowed();
        effects
    }

    /// The kernel's start has returned: cover jobs may begin from now on. A
    /// `cover` held back until now gets its bit 6 (it asks again).
    fn native_cover_jobs_allowed(&mut self) {
        let instances = &self.shared.native_instances;
        let Some(hosts) = instances.hosts() else {
            return;
        };
        let allowed = hosts
            .documents()
            .map(|documents| documents.0.allow_cover_jobs())
            .unwrap_or_default();
        let Some(library) = hosts.library() else {
            return;
        };
        // The book a reset ended the job of counts as failed (no cover job
        // for it), before anything asks for its cover again.
        if allowed.left != 0 {
            library.0.cover_job_left(allowed.left);
        }
        if allowed.held_back {
            library.0.cover_jobs_began();
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
