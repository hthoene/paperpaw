//! `event.lookup_ready` (110, `docs/abi/document.md` 7) on its way to the
//! askers: a provider's `command.lookup_ready` ends the pending asks it
//! matches ([`LookupServices::ready_hash`]); the kernel queues one event for
//! each of their askers here, and its event pump hands them to the askers
//! that subscribed (`service_events.rs`), each taken as it is encoded.
//!
//! Memory: one [`Waiting`] (88 bytes) per event, at most one per pending ask
//! (16 on the X4: 1,408 bytes at most), in a list that holds nothing while no
//! event waits. A queue without room drops the event (the asker asks again
//! when its pending entry shows; nothing worse) and says so in the log.
use super::{LookupServices, NAME_MAX, pending, ready_event};
use crate::native_host::declaration_store::DeclarationStore;
use paperpaw_native::host_glue::ERR_BUSY;

/// One event waiting for its asker.
pub(crate) struct Waiting {
    asker: u64,
    /// The kernel's clock when the provider said ready.
    at_ms: u64,
    /// The provider's name, as the event carries it.
    name: [u8; NAME_MAX],
    len: u8,
}

impl Waiting {
    fn name(&self) -> &str {
        self.name
            .get(..usize::from(self.len))
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }
}

/// The name of `provider`'s lookup, as `event.lookup_ready` names it: its
/// first `lookup.spec` (a plugin that declares several is told by its first
/// name; the asker asks again from the start or that provider's cursor
/// either way). `None` for a plugin without one.
pub(crate) fn provider_name(
    store: &DeclarationStore,
    provider: u64,
) -> Option<([u8; NAME_MAX], u8)> {
    let mut found = None;
    store.each_lookup(&mut |decl| {
        if found.is_some() || decl.owner != provider {
            return;
        }
        let bytes = decl.spec.name.as_bytes();
        let mut name = [0; NAME_MAX];
        if let (Some(room), Ok(len)) = (name.get_mut(..bytes.len()), u8::try_from(bytes.len())) {
            room.copy_from_slice(bytes);
            found = Some((name, len));
        }
    });
    found
}

impl LookupServices {
    /// Queues `event.lookup_ready` for `asker`, from the provider named
    /// `name` (as [`provider_name`] gave it). Never fails: a queue without
    /// room drops it and logs a line.
    pub(crate) fn queue_ready(&self, asker: u64, name: &([u8; NAME_MAX], u8)) {
        let at_ms = self.now_ms();
        let queued = self.events.try_borrow_mut().is_ok_and(|mut events| {
            if events.len() >= pending::CAPACITY || events.try_reserve(1).is_err() {
                return false;
            }
            events.push(Waiting {
                asker,
                at_ms,
                name: name.0,
                len: name.1,
            });
            true
        });
        if !queued {
            self.note("lookup_ready: no room for the event, dropped");
        }
    }

    /// The oldest event of `owner`, encoded into `out` and taken; `None`
    /// when none waits. A failure takes nothing.
    pub(crate) fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<usize>, i32> {
        let mut events = self.events.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let Some(at) = events.iter().position(|event| event.asker == owner) else {
            return Ok(None);
        };
        let length = {
            let event = &events[at];
            ready_event(event.at_ms, event.name(), out)?
        };
        events.remove(at);
        if events.is_empty() {
            // Nothing waits: the list gives its room back.
            *events = alloc::vec::Vec::new();
        }
        Ok(Some(length))
    }

    /// Whether an event waits (a borrowed list counts: look again).
    pub(crate) fn events_waiting(&self) -> bool {
        self.events
            .try_borrow()
            .map_or(true, |events| !events.is_empty())
    }

    /// The kernel's clock, as the event pump tells it each pass.
    pub(crate) fn set_clock(&self, now_ms: u64) {
        self.clock.set(now_ms);
    }

    /// The events of `owner` end with it (it stopped).
    pub(super) fn drop_events_of(&self, owner: u64) {
        if let Ok(mut events) = self.events.try_borrow_mut() {
            events.retain(|event| event.asker != owner);
            if events.is_empty() {
                *events = alloc::vec::Vec::new();
            }
        }
    }

    /// Events waiting now.
    #[cfg(test)]
    pub(crate) fn events_len(&self) -> usize {
        self.events.borrow().len()
    }
}
