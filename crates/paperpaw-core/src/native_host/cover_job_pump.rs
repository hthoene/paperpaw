//! The kernel's side of the library's cover job (`doc_cover_job`): once a pass,
//! from the service pump and never inside a callback, one look at the job. A
//! job whose preparation is over is ended there (the format gets its `close`
//! through the pump's route, as a preparing step's call goes) and the library
//! is told, which raises `event.library_changed` bit 6 for the screens.
use super::{doc_service::JobStep, native_host_call::HostRoute};

impl crate::Kernel {
    /// One look at the cover job; nothing to do for a pass that finds none.
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
            documents.0.cover_job_step(hosts, &route)
        };
        // What the format staged runs now, outside any callback.
        self.native_outbox_run();
        if let JobStep::Ended { ready, key } = step
            && let Some(library) = self
                .shared
                .native_instances
                .hosts()
                .and_then(|h| h.library())
        {
            library.0.cover_job_ended(ready, key);
        }
    }
}

/// The plugin whose screens are the library's lists: the job takes no step
/// while one of them is on the stack.
const LIST_PLUGIN: &str = "files";

impl crate::Kernel {
    /// Tells the document host whether a library list screen is up, before the
    /// pass steps any preparation (the cover job holds back while one is: the
    /// conversion's peak must not come beside the list's buffers).
    #[inline(never)]
    pub(crate) fn native_cover_list(&mut self) {
        let instances = &self.shared.native_instances;
        let Some(documents) = instances.hosts().and_then(|hosts| hosts.documents()) else {
            return;
        };
        documents
            .0
            .cover_job_list(instances.plugin_has_screen(LIST_PLUGIN));
    }

    /// The end of `Kernel::start`: its effects, then cover jobs of the library
    /// may begin (a format's crash inside start would count as a failed start,
    /// `doc_cover_job`).
    pub(crate) fn finish_started(&mut self) -> crate::Effects {
        let effects = self.finish();
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
    /// The device goes to sleep or stops: the cover job ends first, before
    /// anything else of the sleep, and nothing restarts it after the wake until
    /// a new `cover` asks (`doc_cover_job`).
    pub(crate) fn native_cover_job_suspend(&mut self) {
        let instances = &self.shared.native_instances;
        let Some(hosts) = instances.hosts() else {
            return;
        };
        let Some(documents) = hosts.documents() else {
            return;
        };
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
