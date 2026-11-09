//! The item states plugins push with `command.set_item_state` (60,
//! `docs/abi/screens-input.md` 3.1): what a menu shows of one of the pushing
//! plugin's own actions, settings or setup steps, kept in memory by the host
//! and read live by the registry (menu rows, setup steps).
//!
//! Limits (3.1): at most 16 items with a state per plugin and 2,048 bytes of
//! item ids, keys and arguments together; device-wide at most 128 items and
//! 6,144 bytes, an item's index (24 bytes) included. They are checked on a
//! batch's net result: a record that replaces an item's state counts the new
//! one instead of the old, one that removes it frees its share, so a plugin
//! at its limit can still change or clear its items (`ERR_FULL`, whole).
//! Storage is reserved fallibly (`ERR_NO_MEMORY`); the table is a small
//! vector searched linearly (128 entries at most).
//!
//! Not here yet: the kept file (`keep` states surviving a stop or a restart,
//! written at most once a minute and restored at boot) and
//! `event.registry_changed` bit 0. Until the kept file exists the `keep` flag
//! changes nothing beyond what it must: every state of a plugin ends when the
//! plugin is unloaded (stopped, updated or removed), the kept ones included.
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_NO_MEMORY};

/// Items with a state per plugin and on the device.
pub(crate) const PER_PLUGIN_ITEMS: usize = 16;
pub(crate) const DEVICE_ITEMS: usize = 128;
/// Bytes of ids, keys and arguments per plugin, and on the device (index
/// included).
pub(crate) const PER_PLUGIN_BYTES: usize = 2048;
pub(crate) const DEVICE_BYTES: usize = 6144;
/// What an item's index costs.
const INDEX_BYTES: usize = 24;

/// `command.set_item_state` flags (`abi7::commands_wire::COMMAND_ITEM_*`).
pub(crate) const HIDDEN: u32 = 1;
pub(crate) const FAINT: u32 = 1 << 1;
pub(crate) const SWITCH: u32 = 1 << 2;
pub(crate) const ON: u32 = 1 << 3;
pub(crate) const KEEP: u32 = 1 << 4;

fn copy(text: &str) -> Result<Box<str>, i32> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    out.push_str(text);
    Ok(out.into_boxed_str())
}

fn copy_bytes(bytes: &[u8]) -> Result<Box<[u8]>, i32> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    out.extend_from_slice(bytes);
    Ok(out.into_boxed_slice())
}

/// One pushed state: the item (relative to the plugin) and what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemPush {
    pub(crate) item: Box<str>,
    pub(crate) flags: u32,
    pub(crate) label: Box<str>,
    pub(crate) label_args: Box<[u8]>,
    pub(crate) badge: Box<str>,
    pub(crate) badge_args: Box<[u8]>,
}

impl ItemPush {
    /// A copy of the command's fields, reserved fallibly.
    pub(crate) fn copied(
        item: &str,
        flags: u32,
        label: (&str, &[u8]),
        badge: (&str, &[u8]),
    ) -> Result<Self, i32> {
        Ok(Self {
            item: copy(item)?,
            flags,
            label: copy(label.0)?,
            label_args: copy_bytes(label.1)?,
            badge: copy(badge.0)?,
            badge_args: copy_bytes(badge.1)?,
        })
    }

    /// The default state removes the item's state (the kept copy included).
    pub(crate) fn is_default(&self) -> bool {
        self.flags & !KEEP == 0 && self.label.is_empty() && self.badge.is_empty()
    }

    /// Bytes of ids, keys and arguments.
    pub(crate) fn bytes(&self) -> usize {
        self.item.len()
            + self.label.len()
            + self.label_args.len()
            + self.badge.len()
            + self.badge_args.len()
    }

    pub(crate) fn hidden(&self) -> bool {
        self.flags & HIDDEN != 0
    }
    pub(crate) fn faint(&self) -> bool {
        self.flags & FAINT != 0
    }
    /// `Some(on)` for an item shown with a switch.
    pub(crate) fn switch(&self) -> Option<bool> {
        (self.flags & SWITCH != 0).then_some(self.flags & ON != 0)
    }
}

struct Entry {
    plugin: Box<str>,
    push: ItemPush,
}

/// The states pushed so far, by plugin and item.
#[derive(Default)]
pub(crate) struct ItemStates {
    entries: Vec<Entry>,
}

impl core::fmt::Debug for ItemStates {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ItemStates")
            .field("items", &self.entries.len())
            .finish()
    }
}

impl ItemStates {
    /// The state `plugin` last pushed for `item` (relative to it).
    pub(crate) fn get(&self, plugin: &str, item: &str) -> Option<&ItemPush> {
        self.entries
            .iter()
            .find(|e| &*e.plugin == plugin && &*e.push.item == item)
            .map(|e| &e.push)
    }

    /// Whether the net result of `pending` (in order, the last of an item
    /// wins) over the table fits the limits; nothing is changed.
    ///
    /// # Errors
    /// `ERR_FULL`.
    pub(crate) fn fits(&self, plugin: &str, pending: &[&ItemPush]) -> Result<(), i32> {
        // Count what the table holds, then replace or remove what `pending`
        // names; one pass over the entries, none over the pending ones twice.
        let (mut mine, mut mine_bytes) = (0_usize, 0_usize);
        let (mut all, mut all_bytes) = (0_usize, 0_usize);
        for e in &self.entries {
            let named = pending.iter().rev().find(|p| *p.item == *e.push.item);
            let own = &*e.plugin == plugin;
            if own && named.is_some() {
                // Replaced or removed: its new share comes with `pending`.
                continue;
            }
            all += 1;
            all_bytes += e.push.bytes() + INDEX_BYTES;
            if own {
                mine += 1;
                mine_bytes += e.push.bytes();
            }
        }
        for (at, p) in pending.iter().enumerate() {
            // Only the last record of an item counts.
            let later = pending[at + 1..].iter().any(|q| q.item == p.item);
            if later || p.is_default() {
                continue;
            }
            mine += 1;
            mine_bytes += p.bytes();
            all += 1;
            all_bytes += p.bytes() + INDEX_BYTES;
        }
        if mine > PER_PLUGIN_ITEMS
            || mine_bytes > PER_PLUGIN_BYTES
            || all > DEVICE_ITEMS
            || all_bytes > DEVICE_BYTES
        {
            return Err(ERR_FULL);
        }
        Ok(())
    }

    /// Replaces `plugin`'s state of the item (the default state removes it).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (the table is unchanged).
    pub(crate) fn apply(&mut self, plugin: &str, push: ItemPush) -> Result<(), i32> {
        let at = self
            .entries
            .iter()
            .position(|e| &*e.plugin == plugin && e.push.item == push.item);
        if push.is_default() {
            if let Some(at) = at {
                self.entries.remove(at);
            }
            return Ok(());
        }
        match at {
            Some(at) => self.entries[at].push = push,
            None => {
                self.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                let plugin = copy(plugin)?;
                self.entries.push(Entry { plugin, push });
            }
        }
        Ok(())
    }

    /// The plugin was updated or removed: every state of it ends.
    pub(crate) fn forget(&mut self, plugin: &str) {
        self.entries.retain(|e| &*e.plugin != plugin);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
#[path = "item_state_tests.rs"]
mod tests;
