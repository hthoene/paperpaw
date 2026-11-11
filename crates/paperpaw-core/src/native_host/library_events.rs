//! `event.library_changed` (91, `docs/abi/library.md` section 3) to the
//! plugins: what `core.library` says changed is held by the library host
//! (`lib_changed`, coalesced) and queued here, between callbacks, for every
//! running plugin that subscribed, the changer too, in the same per-plugin
//! queue as every other kernel event ([`Kernel::native_queue_event`]).
//!
//! The host never reaches the queues itself. A change that does not fit a
//! queue is dropped and logged there, once until the queue is empty again: the
//! event is a hint ("re-read what you show"), so a later change tells again.
use super::registrations as wire;
use crate::Kernel;

impl Kernel {
    /// Takes the library's waiting change and queues it for the subscribers.
    /// `true` is never answered: one change is coalesced, so one pass takes it.
    // Its own frame: the pump keeps the services' passes apart.
    #[inline(never)]
    pub(crate) fn native_library_pump(&mut self) -> bool {
        let now_ms = self.now.millis();
        let Some(host) = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.library())
            .map(|library| library.0.clone())
        else {
            return false;
        };
        let mut change = None;
        host.take_changed(&mut |what, generation, path| {
            // The path, at most 512 bytes, is held for the queueing only.
            let mut own = alloc::string::String::new();
            if own.try_reserve_exact(path.len()).is_ok() {
                own.push_str(path);
            }
            change = Some((what, generation, own));
        });
        if let Some((what, generation, path)) = change {
            self.native_queue_event(wire::EVENT_LIBRARY_CHANGED_TAG, &|_| true, &|queue| {
                let record = wire::Record::EventLibraryChanged(wire::EventLibraryChanged {
                    now_ms,
                    what,
                    generation,
                    path: &path,
                });
                queue.push_record(wire::EVENT_LIBRARY_CHANGED_TAG, &record)
            });
        }
        false
    }
}

#[cfg(test)]
#[path = "library_events_tests.rs"]
mod tests;
