//! Bounded per-instance EVT state. This owns no registry, dispatcher or plugin.
//! Core detaches one Ready item before entering foreign code or draining again.
use super::{registrations as wire, shared::Shared};
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE};

const EDGES: usize = 16;
const PLUGIN_BYTES: usize = 4096;
const DEVICE_BYTES: usize = 8192;
const BITS: u8 = 6;
const REREAD: u8 = 5;
const ORDER: usize = EDGES + BITS as usize;

/// Supplied by the same host predicate that authorizes the settings-read slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Visibility {
    Hidden,
    Redacted,
    Readable,
}

/// One shared byte budget, independent of runtime ownership and retained leases.
pub(crate) struct EventBudget(Shared<Cell<usize>>);
impl EventBudget {
    pub(crate) fn try_new() -> Result<Self, i32> {
        Shared::try_new(Cell::new(0))
            .map(Self)
            .map_err(|_| ERR_NO_MEMORY)
    }
    pub(crate) fn pending(&self) -> Result<Pending, i32> {
        Ok(Pending {
            budget: Shared::try_new(PluginBudget {
                device: self.0.try_clone().ok_or(ERR_NO_MEMORY)?,
                bytes: Cell::new(0),
                edges: Cell::new(0),
            })
            .map_err(|_| ERR_NO_MEMORY)?,
            edges: Vec::new(),
            order: [0; ORDER],
            len: 0,
        })
    }
    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.0.get()
    }
}

pub(crate) struct Pending {
    budget: Shared<PluginBudget>,
    edges: Vec<HeldEdge>,
    // IDs 0..5 are hint/overflow bits; IDs 6..21 name dense held-edge slots.
    // Coalescing moves its ID to the back. There is no sequence counter to wrap.
    order: [u8; ORDER],
    len: u8,
}

struct PluginBudget {
    device: Shared<Cell<usize>>,
    bytes: Cell<usize>,
    edges: Cell<usize>,
}

/// A queued or detached edge keeps its original plugin and device charges.
/// The ledger remains alive when plugin teardown drops its Pending queue.
pub(crate) struct HeldEdge {
    bytes: Vec<u8>,
    budget: Shared<PluginBudget>,
}
impl Drop for HeldEdge {
    fn drop(&mut self) {
        self.budget
            .bytes
            .set(self.budget.bytes.get() - self.bytes.len());
        self.budget
            .device
            .set(self.budget.device.get() - self.bytes.len());
        self.budget.edges.set(self.budget.edges.get() - 1);
    }
}

/// Detached from Pending, so delivery holds no queue or registry borrow.
pub(crate) enum Ready {
    Edge(HeldEdge),
    Hint(u32),
    Reread,
}
impl Ready {
    /// Recheck visibility at delivery too: revocation must not leak a queued old value.
    pub(crate) fn setting_record(
        &self,
        now_ms: u64,
        visibility: impl FnOnce(&str) -> Visibility,
    ) -> Result<Option<wire::Record<'_>>, i32> {
        let event = match self {
            Self::Reread => wire::EventSettingChanged {
                now_ms,
                key: "",
                old: &[],
            },
            Self::Edge(edge) => match wire::records_decode_single(&edge.bytes)? {
                wire::Record::EventSettingChanged(event) => event,
                _ => return Err(ERR_INVALID),
            },
            Self::Hint(_) => return Err(ERR_INVALID),
        };
        if event.key.is_empty() {
            return Ok(Some(wire::Record::EventSettingChanged(event)));
        }
        Ok(match visibility(event.key) {
            Visibility::Hidden => None,
            Visibility::Redacted => Some(wire::Record::EventSettingChanged(
                wire::EventSettingChanged { old: &[], ..event },
            )),
            Visibility::Readable => Some(wire::Record::EventSettingChanged(event)),
        })
    }
}

impl Pending {
    /// An error is a dropped edge: the caller must log event_dropped with its status.
    /// The allocation-free reread bit is set on every failure, even with a full store.
    pub(crate) fn setting(
        &mut self,
        now_ms: u64,
        key: &str,
        old: Result<&[u8], i32>,
        visibility: Visibility,
    ) -> Result<(), i32> {
        if visibility == Visibility::Hidden {
            return Ok(());
        }
        if key.is_empty() {
            // Scope switches request a reread without holding an edge or
            // reporting a dropped event.
            self.touch(REREAD);
            return Ok(());
        }
        let result = if visibility == Visibility::Readable {
            old.and_then(|old| self.push_setting(now_ms, key, old))
        } else {
            self.push_setting(now_ms, key, &[])
        };
        if result.is_err() {
            self.touch(REREAD);
        }
        result
    }

    fn push_setting(&mut self, now_ms: u64, key: &str, old: &[u8]) -> Result<(), i32> {
        if key.is_empty() {
            return Err(ERR_INVALID);
        }
        let record =
            wire::Record::EventSettingChanged(wire::EventSettingChanged { now_ms, key, old });
        let length = 1 + wire::record_framed_len(&record)?;
        if self.budget.edges.get() == EDGES
            || length > PLUGIN_BYTES - self.budget.bytes.get()
            || length > DEVICE_BYTES - self.budget.device.get()
        {
            return Err(ERR_NO_SPACE);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
        bytes.resize(length, 0);
        wire::records_encode(&[record], &mut bytes)?;
        self.edges.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
        let budget = self.budget.try_clone().ok_or(ERR_NO_MEMORY)?;
        let id = BITS + self.edges.len() as u8;
        self.edges.push(HeldEdge { bytes, budget });
        self.budget.bytes.set(self.budget.bytes.get() + length);
        self.budget.device.set(self.budget.device.get() + length);
        self.budget.edges.set(self.budget.edges.get() + 1);
        self.touch(id);
        Ok(())
    }

    /// Hints keep only an ordered pending bit; encode their current state at delivery.
    pub(crate) fn hint(&mut self, tag: u32) -> Result<(), i32> {
        let id = tag
            .checked_sub(wire::EVENT_PLUGINS_CHANGED_TAG)
            .filter(|id| *id < 5)
            .ok_or(ERR_INVALID)? as u8;
        self.touch(id);
        Ok(())
    }

    fn touch(&mut self, id: u8) {
        let len = usize::from(self.len);
        if let Some(at) = self.order[..len].iter().position(|entry| *entry == id) {
            self.order.copy_within(at + 1..len, at);
            self.len -= 1;
        }
        // At most 16 edge IDs and six distinct bits can be present.
        self.order[usize::from(self.len)] = id;
        self.len += 1;
    }

    pub(crate) fn take(&mut self) -> Option<Ready> {
        if self.len == 0 {
            return None;
        }
        let id = self.order[0];
        self.order.copy_within(1..usize::from(self.len), 0);
        self.len -= 1;
        if id == REREAD {
            return Some(Ready::Reread);
        }
        if id < BITS {
            return Some(Ready::Hint(wire::EVENT_PLUGINS_CHANGED_TAG + u32::from(id)));
        }
        let edge = self.edges.remove(usize::from(id - BITS));
        for next in &mut self.order[..usize::from(self.len)] {
            if *next > id {
                *next -= 1;
            }
        }
        Some(Ready::Edge(edge))
    }

    /// Teardown drops all held edges and pending hints without entering plugin code.
    pub(crate) fn clear(&mut self) {
        self.edges.clear();
        self.len = 0;
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[path = "event_pending_tests.rs"]
mod tests;
