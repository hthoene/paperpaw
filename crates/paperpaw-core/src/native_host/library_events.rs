//! `event.library_changed` (91, `docs/abi/library.md` section 3) to the
//! plugins: what `core.library` says changed is held by the library host
//! (`lib_changed`, coalesced) and queued here, between callbacks, for every
//! running plugin that subscribed, the changer too, in the same per-plugin
//! queue as every other kernel event ([`Kernel::native_queue_event`]).
//!
//! The host never reaches the queues itself. A change that does not fit a
//! queue is dropped and logged there, once until the queue is empty again: the
//! event is a hint ("re-read what you show"), so a later change tells again.
//!
//! The same pass wants the catalog (`core.catalog-wanted`) when a view of the
//! library first reads it since the start or the last wake (#243): the
//! kernel's walk starts from that event only, which Beta 3's library view
//! raised when it showed and a native view has no op for.
use super::registrations as wire;
use crate::Kernel;
use paperpaw_sdk::builtin::events;

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
        // The first view of the library since the start or the wake: the
        // kernel walks the catalog where it was not walked since, as for an
        // interface 6 view that showed (`library::request_catalog`).
        if host.take_catalog_wanted() {
            self.output
                .commands
                .push(events::CATALOG_WANTED.emit(events::CatalogWanted::new(false)));
        }
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

    /// Woken: the next view of the library wants the catalog again.
    pub(crate) fn native_library_woke(&self) {
        if let Some(library) = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.library())
        {
            library.0.catalog_ask_again();
        }
    }
}

#[cfg(test)]
#[path = "library_events_tests.rs"]
mod tests;
