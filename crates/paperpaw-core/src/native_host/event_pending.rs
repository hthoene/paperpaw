//! Bounded per-instance EVT state. This owns no registry, dispatcher or plugin.
//! Core detaches one Ready item before entering foreign code or draining again.
use super::{registrations as wire, shared::Shared};
use crate::native_host::codec;
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE};

const EDGES: usize = 16;
const PLUGIN_BYTES: usize = 4096;
const DEVICE_BYTES: usize = 8192;
/// Pending bits: five hints, the reread bit, then the coalesced lifecycle
/// events (tick, resume, suspend, stopping; see [`CORE_TAGS`]).
const BITS: u8 = 10;
const REREAD: u8 = 5;
const CORE_FIRST: u8 = 6;
/// The events that keep a pending bit, in bit order from `CORE_FIRST`: a
/// second one before the first was told is one (tick, resume) or, for suspend
/// and stopping, never lost to a full store (`events::bypasses_queue`).
pub(crate) const CORE_TAGS: [u32; 4] = [
    wire::EVENT_TICK_TAG,
    wire::EVENT_RESUME_TAG,
    wire::EVENT_SUSPEND_TAG,
    wire::EVENT_STOPPING_TAG,
];
const ORDER: usize = EDGES + BITS as usize;

pub(crate) use crate::abi7::settings_visibility::Visibility;

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
    // IDs 0..9 are hint, overflow and lifecycle bits; IDs 10..25 name dense held-edge slots.
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
    /// The tag of the record held.
    tag: u32,
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
    /// A coalesced lifecycle event (tick, resume, suspend, stopping), encoded
    /// from the clock at delivery.
    Core(u32),
    Reread,
}
impl Ready {
    /// The encoded record of a held edge that is not a setting change
    /// (`event.started`, `battery_measured`, `page_turned`).
    pub(crate) fn event_bytes(&self) -> Option<(u32, &[u8])> {
        match self {
            Self::Edge(edge) if edge.tag != wire::EVENT_SETTING_CHANGED_TAG => {
                Some((edge.tag, &edge.bytes))
            }
            _ => None,
        }
    }

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
            Self::Edge(edge) => match codec::decode_single(&edge.bytes)? {
                wire::Record::EventSettingChanged(event) => event,
                _ => return Err(ERR_INVALID),
            },
            Self::Hint(_) | Self::Core(_) => return Err(ERR_INVALID),
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
        self.push_record(wire::EVENT_SETTING_CHANGED_TAG, &record)
    }

    /// Holds one encoded edge (`record`, of event tag `tag`) behind the ones
    /// held. `ERR_NO_SPACE` when 16 are held, or the plugin's 4,096 bytes or
    /// the device's 8,192 are used; `ERR_NO_MEMORY` when the room cannot be
    /// reserved. The caller logs the drop; nothing is retried.
    pub(crate) fn push_record(&mut self, tag: u32, record: &wire::Record<'_>) -> Result<(), i32> {
        let length = 1 + codec::framed_len(record)?;
        if self.budget.edges.get() == EDGES
            || length > PLUGIN_BYTES - self.budget.bytes.get()
            || length > DEVICE_BYTES - self.budget.device.get()
        {
            return Err(ERR_NO_SPACE);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
        bytes.resize(length, 0);
        codec::encode_batch(core::slice::from_ref(record), &mut bytes)?;
        self.edges.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
        let budget = self.budget.try_clone().ok_or(ERR_NO_MEMORY)?;
        let id = BITS + self.edges.len() as u8;
        self.edges.push(HeldEdge { bytes, tag, budget });
        self.budget.bytes.set(self.budget.bytes.get() + length);
        self.budget.device.set(self.budget.device.get() + length);
        self.budget.edges.set(self.budget.edges.get() + 1);
        self.touch(id);
        Ok(())
    }

    /// Whether a held edge waits: a later edge goes behind it.
    pub(crate) fn has_edges(&self) -> bool {
        !self.edges.is_empty()
    }

    /// Marks the lifecycle event `tag` (one of [`CORE_TAGS`]) pending: a
    /// second one before the first was told is one. No allocation.
    pub(crate) fn core(&mut self, tag: u32) -> Result<(), i32> {
        let at = CORE_TAGS
            .iter()
            .position(|&known| known == tag)
            .ok_or(ERR_INVALID)?;
        self.touch(CORE_FIRST + at as u8);
        Ok(())
    }

    /// Whether nothing waits (no edge, no hint, no reread).
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
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
        // At most 16 edge IDs and ten distinct bits can be present.
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
        if id < REREAD {
            return Some(Ready::Hint(wire::EVENT_PLUGINS_CHANGED_TAG + u32::from(id)));
        }
        if id < BITS {
            return Some(Ready::Core(CORE_TAGS[usize::from(id - CORE_FIRST)]));
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
