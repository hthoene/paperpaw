//! The kernel's side of the document preparing loop (`docs/abi/document.md`
//! 2.1, "Preparing"): once a pass, from the service pump and never inside a
//! callback, the next preparing document's format gets one op 3 `prepare`
//! (`doc_prepare` picks the document, calls the format and records its
//! answer; this file only finds the moment and keeps the retry).
//!
//! The format is entered through the retained instance registry, at depth 1,
//! as its own call: no opener callback runs. A step that cannot be made now
//! (the format is not started, a table is held) is tried again after
//! [`RETRY_MS`], not at once, so a format that is slow to start does not
//! keep the device awake; a step that was made asks for the next pass at once
//! while a document still prepares (the format itself works for at most 50 ms
//! of its own clock per call, and the pump runs between inputs).
use super::{doc_service::PrepareStep, native_host_call::HostRoute};

/// The wait before a step that could not be made is tried again.
const RETRY_MS: u64 = 500;

impl crate::Kernel {
    /// One step of the preparing loop; `true` while more are to be made
    /// (see the module).
    pub(crate) fn native_doc_pump(&mut self) -> bool {
        let instances = &self.shared.native_instances;
        // Waiting for its time after a step that could not be made.
        if instances
            .doc_retry()
            .is_some_and(|at| self.now.millis() < at)
        {
            return false;
        }
        let Some(hosts) = instances.hosts() else {
            return false;
        };
        let Some(documents) = hosts.documents() else {
            return false;
        };
        // Never inside a callback: the pump asks for another pass instead.
        if instances.depth() != 0 {
            return false;
        }
        // What a callback staged runs before the format is entered.
        if instances.outbox.full() {
            return true;
        }
        let step = {
            let env = self.shared.env(self.now);
            let route = HostRoute {
                registry: instances,
                env: &env,
                meter: &self.shared.meter,
            };
            documents.0.prepare_step(hosts, &route)
        };
        // What the format staged runs now, outside any callback.
        self.native_outbox_run();
        let instances = &self.shared.native_instances;
        match step {
            PrepareStep::Idle => {
                instances.set_doc_retry(None);
                false
            }
            PrepareStep::More => {
                instances.set_doc_retry(None);
                true
            }
            PrepareStep::Later => {
                instances.set_doc_retry(Some(self.now.millis() + RETRY_MS));
                false
            }
        }
    }
}
