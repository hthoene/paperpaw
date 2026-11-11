//! The kernel's side of the document preparing loop (`docs/abi/document.md`
//! 2.1, "Preparing"): once a pass, from the service pump and never inside a
//! callback, the next preparing document's format gets one op 3 `prepare`
//! (`doc_prepare` picks the document, calls the format and records its
//! answer; this file only finds the moment and keeps the retry).
//!
//! The format is entered through the retained instance registry, outside any
//! callback (depth 0), as its own call: no opener callback runs. A step that
//! cannot be made now (the format is not started, a table is held) is tried
//! again after [`RETRY_MS`], doubled for each failure in a row up to
//! [`RETRY_MAX_MS`], not at once, so a format that is slow to start (or never
//! starts) does not keep the device awake; a step that was made asks for the next pass at once
//! while a document still prepares (the format itself works for at most 50 ms
//! of its own clock per call, and the pump runs between inputs). The library's
//! cover job is the exception: after its step the pump waits [`JOB_GAP_MS`]
//! (a timed wake), so it goes on while the device is idle.
use super::{doc_service::PrepareStep, native_host_call::HostRoute};

/// The wait before a step that could not be made is tried again; it doubles
/// for each failure in a row (as the library move job's does) ...
const RETRY_MS: u64 = 500;
/// The wait between two steps of the library's cover job (`doc_cover_job`),
/// the only document that prepares: a timed wake, not the "more" signal, so
/// the job goes on while the device is idle (nothing else wakes the pump) and
/// still never holds the idle or the sleep timer off (it is no input, and no
/// wake counts as one).
const JOB_GAP_MS: u64 = 100;
/// The same where the device saves power (the power modes Saving and Offline):
/// the job goes on ten times slower, so it costs little battery.
const JOB_GAP_SAVING_MS: u64 = 1_000;
/// ... up to this (500 ms shifted at most seven times).
const RETRY_MAX_MS: u64 = RETRY_MS << 7;

/// The wait after `failures` steps in a row that could not be made.
const fn wait(failures: u8) -> u64 {
    let shift = failures.saturating_sub(1);
    let shift = if shift > 7 { 7 } else { shift };
    let ms = RETRY_MS << shift;
    if ms > RETRY_MAX_MS { RETRY_MAX_MS } else { ms }
}

impl crate::Kernel {
    /// One step of the preparing loop; `true` while more are to be made
    /// (see the module).
    #[inline(never)]
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
        // Nothing prepares: no environment, no route.
        if !documents.0.preparing() {
            instances.doc_ok();
            instances.set_doc_retry(None);
            return false;
        }
        // What a callback staged runs before the format is entered.
        if instances.outbox.full() {
            self.native_outbox_run();
            let instances = &self.shared.native_instances;
            if instances.outbox.full() {
                // It did not drain (a plugin that cannot be called): no busy
                // loop, a later pass looks again.
                let failures = instances.doc_failed();
                instances.set_doc_retry(Some(self.now.millis() + wait(failures)));
                return false;
            }
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
        let job_only = documents.0.only_cover_job_preparing();
        // What the format staged runs now, outside any callback.
        self.native_outbox_run();
        let instances = &self.shared.native_instances;
        match step {
            PrepareStep::Idle => {
                instances.doc_ok();
                instances.set_doc_retry(None);
                false
            }
            PrepareStep::More => {
                instances.doc_ok();
                // The library's cover job is background work: its next step
                // comes after a short gap, as a timed wake and not as "more",
                // so it goes on on an idle device and never holds the idle or
                // the sleep timer off.
                let gap = if self.shared.power.level == paperpaw_sdk::power::PowerLevel::Normal {
                    JOB_GAP_MS
                } else {
                    JOB_GAP_SAVING_MS
                };
                instances.set_doc_retry(job_only.then(|| self.now.millis() + gap));
                !job_only
            }
            PrepareStep::Later => {
                let failures = instances.doc_failed();
                instances.set_doc_retry(Some(self.now.millis() + wait(failures)));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::native_instances::NativeInstances;

    #[test]
    fn the_wait_doubles_to_its_cap_and_starts_over_after_a_step() {
        let instances = NativeInstances::default();
        let mut last = 0;
        for _ in 0..12 {
            let now = wait(instances.doc_failed());
            assert!(now >= last && now <= RETRY_MAX_MS, "{now}");
            last = now;
        }
        assert_eq!(wait(1), RETRY_MS);
        assert_eq!(wait(2), 2 * RETRY_MS);
        assert_eq!(last, RETRY_MAX_MS);
        assert_eq!(RETRY_MAX_MS, 64_000);
        // A step that was made: the back-off starts over.
        instances.doc_ok();
        assert_eq!(wait(instances.doc_failed()), RETRY_MS);
    }
}
