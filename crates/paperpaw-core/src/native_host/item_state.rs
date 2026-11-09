//! The item states plugins push with `command.set_item_state` (60,
//! `docs/abi/screens-input.md` 3.1): what a menu shows of one of the pushing
//! plugin's own actions, settings or setup steps, kept in memory by the host
//! and read live by the registry (menu rows, setup steps).
//!
//! Limits (3.1): at most 16 items with a state per plugin and 2,048 bytes of
//! item ids, keys and arguments together; device-wide at most 128 items and
//! 6,144 bytes, what an item costs besides those bytes included. They are
//! checked on a batch's net result: a record that replaces an item's state
//! counts the new one instead of the old, one that removes it frees its
//! share, so a plugin at its limit can still change or clear its items
//! (`ERR_FULL`, whole).
//!
//! What is charged is what the heap holds. An item is one [`ItemPush`] of 24
//! bytes on the device (a pointer and a length, five 16-bit lengths, the
//! flags and the plugin's slot) and one allocation for all of its texts
//! (about 8 bytes of allocator header); a plugin's id is kept once, in a slot
//! of the plugin table, however many items it has. A test pins the entry
//! size. The table is a small vector searched linearly (128 entries at
//! most) and grows by exactly what is needed.
//!
//! Memory is reserved at submit ([`ItemStates::reserve`]): the entries a
//! batch can add and the plugin's slot. Applying the batch after the
//! callback then allocates nothing, so a batch is never half applied.
//!
//! Not here yet: the kept file (`keep` states surviving a stop or a restart,
//! written at most once a minute and restored at boot) and
//! `event.registry_changed` bit 0. Until the kept file exists the `keep` flag
//! changes nothing beyond what it must: every state of a plugin ends when the
//! plugin is unloaded (stopped, updated or removed), the kept ones included.
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_MEMORY};

/// Items with a state per plugin and on the device.
pub(crate) const PER_PLUGIN_ITEMS: usize = 16;
pub(crate) const DEVICE_ITEMS: usize = 128;
/// Bytes of ids, keys and arguments per plugin, and on the device (what an
/// item and a plugin's slot cost included).
pub(crate) const PER_PLUGIN_BYTES: usize = 2048;
pub(crate) const DEVICE_BYTES: usize = 6144;
/// An entry on the device, in bytes (pinned by a test).
const ENTRY_BYTES: usize = 24;
/// The allocator's header of one allocation, about.
const ALLOC_OVERHEAD: usize = 8;
/// What an item costs besides its bytes: the entry and the header of its
/// texts' allocation.
const ITEM_BYTES: usize = ENTRY_BYTES + ALLOC_OVERHEAD;
/// A plugin's slot in the table, besides its id's bytes.
const SLOT_BYTES: usize = 8 + ALLOC_OVERHEAD;
/// No slot yet.
const NO_SLOT: u16 = u16::MAX;
/// The flags the command knows (bits 0-4).
const KNOWN: u32 = HIDDEN | FAINT | SWITCH | ON | KEEP;

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

/// One pushed state: the item (relative to the plugin) and what it shows, in
/// one allocation: item, label, label arguments, badge, badge arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemPush {
    data: Box<[u8]>,
    lens: [u16; 5],
    flags: u8,
    plugin: u16,
}

impl ItemPush {
    /// A copy of the command's fields in one allocation, reserved fallibly.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`; `ERR_INVALID` for flags the command does not know or
    /// a field the wire could not have carried.
    pub(crate) fn copied(
        item: &str,
        flags: u32,
        label: (&str, &[u8]),
        badge: (&str, &[u8]),
    ) -> Result<Self, i32> {
        if flags & !KNOWN != 0 {
            return Err(ERR_INVALID);
        }
        let parts: [&[u8]; 5] = [
            item.as_bytes(),
            label.0.as_bytes(),
            label.1,
            badge.0.as_bytes(),
            badge.1,
        ];
        let mut lens = [0_u16; 5];
        let mut total = 0_usize;
        for (len, part) in lens.iter_mut().zip(parts) {
            *len = u16::try_from(part.len()).map_err(|_| ERR_INVALID)?;
            total += part.len();
        }
        let mut data = Vec::new();
        data.try_reserve_exact(total).map_err(|_| ERR_NO_MEMORY)?;
        for part in parts {
            data.extend_from_slice(part);
        }
        Ok(Self {
            data: data.into_boxed_slice(),
            lens,
            flags: flags as u8,
            plugin: NO_SLOT,
        })
    }

    /// Field `n` (0 item, 1 label, 2 label arguments, 3 badge, 4 badge
    /// arguments).
    fn part(&self, n: usize) -> &[u8] {
        let start: usize = self.lens[..n].iter().map(|len| usize::from(*len)).sum();
        let end = start + usize::from(self.lens[n]);
        self.data.get(start..end).unwrap_or(&[])
    }

    fn text(&self, n: usize) -> &str {
        core::str::from_utf8(self.part(n)).unwrap_or("")
    }

    /// The item, relative to the plugin.
    pub(crate) fn item(&self) -> &str {
        self.text(0)
    }
    /// The label's catalog key, empty for none.
    pub(crate) fn label(&self) -> &str {
        self.text(1)
    }
    pub(crate) fn label_args(&self) -> &[u8] {
        self.part(2)
    }
    /// The badge's catalog key, empty for none.
    pub(crate) fn badge(&self) -> &str {
        self.text(3)
    }
    pub(crate) fn badge_args(&self) -> &[u8] {
        self.part(4)
    }

    /// The default state removes the item's state (the kept copy included).
    pub(crate) fn is_default(&self) -> bool {
        u32::from(self.flags) & !KEEP == 0 && self.lens[1] == 0 && self.lens[3] == 0
    }

    /// Bytes of ids, keys and arguments.
    pub(crate) fn bytes(&self) -> usize {
        self.data.len()
    }

    /// The flags (`HIDDEN` .. `KEEP`).
    pub(crate) fn flags(&self) -> u32 {
        u32::from(self.flags)
    }
    pub(crate) fn hidden(&self) -> bool {
        u32::from(self.flags) & HIDDEN != 0
    }
    pub(crate) fn faint(&self) -> bool {
        u32::from(self.flags) & FAINT != 0
    }
    /// `Some(on)` for an item shown with a switch.
    pub(crate) fn switch(&self) -> Option<bool> {
        let flags = u32::from(self.flags);
        (flags & SWITCH != 0).then_some(flags & ON != 0)
    }
}

/// The states pushed so far, by plugin and item. A plugin's id is kept once
/// in `plugins`; an entry names it by its slot.
#[derive(Default)]
pub(crate) struct ItemStates {
    entries: Vec<ItemPush>,
    plugins: Vec<Option<Box<str>>>,
}

impl core::fmt::Debug for ItemStates {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ItemStates")
            .field("items", &self.entries.len())
            .finish()
    }
}

impl ItemStates {
    fn slot_of(&self, plugin: &str) -> Option<u16> {
        let at = self
            .plugins
            .iter()
            .position(|slot| slot.as_deref() == Some(plugin))?;
        u16::try_from(at).ok()
    }

    /// The state `plugin` last pushed for `item` (relative to it).
    pub(crate) fn get(&self, plugin: &str, item: &str) -> Option<&ItemPush> {
        let slot = self.slot_of(plugin)?;
        self.entries
            .iter()
            .find(|e| e.plugin == slot && e.item() == item)
    }

    /// Whether the net result of `pending` (in order, the last of an item
    /// wins) over the table fits the limits; nothing is changed.
    ///
    /// # Errors
    /// `ERR_FULL`.
    pub(crate) fn fits(&self, plugin: &str, pending: &[&ItemPush]) -> Result<(), i32> {
        let own_slot = self.slot_of(plugin);
        // Count what the table holds, then replace or remove what `pending`
        // names; one pass over the entries, none over the pending ones twice.
        let (mut mine, mut mine_bytes) = (0_usize, 0_usize);
        let (mut all, mut all_bytes) = (0_usize, 0_usize);
        for e in &self.entries {
            let own = Some(e.plugin) == own_slot;
            if own && pending.iter().any(|p| p.item() == e.item()) {
                // Replaced or removed: its new share comes with `pending`.
                continue;
            }
            all += 1;
            all_bytes += e.bytes() + ITEM_BYTES;
            if own {
                mine += 1;
                mine_bytes += e.bytes();
            }
        }
        for (at, p) in pending.iter().enumerate() {
            // Only the last record of an item counts.
            let later = pending[at + 1..].iter().any(|q| q.item() == p.item());
            if later || p.is_default() {
                continue;
            }
            mine += 1;
            mine_bytes += p.bytes();
            all += 1;
            all_bytes += p.bytes() + ITEM_BYTES;
        }
        // The plugin table: every slot held, and the caller's if it has none.
        for slot in self.plugins.iter().flatten() {
            all_bytes += slot.len() + SLOT_BYTES;
        }
        if own_slot.is_none() && mine > 0 {
            all_bytes += plugin.len() + SLOT_BYTES;
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

    /// Reserves, for `plugin`, room for `new_entries` more states and the
    /// plugin's slot, so that [`apply`](Self::apply) allocates nothing. Call
    /// it with the entries every batch of the callback can add (the ones
    /// staged earlier included); reserving again for the same is free.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (nothing the caller sees changes).
    pub(crate) fn reserve(&mut self, plugin: &str, new_entries: usize) -> Result<(), i32> {
        if new_entries == 0 {
            return Ok(());
        }
        self.entries
            .try_reserve_exact(new_entries)
            .map_err(|_| ERR_NO_MEMORY)?;
        if self.slot_of(plugin).is_some() {
            return Ok(());
        }
        let id = copy(plugin)?;
        match self.plugins.iter().position(Option::is_none) {
            Some(free) => self.plugins[free] = Some(id),
            None => {
                if self.plugins.len() >= usize::from(NO_SLOT) {
                    return Err(ERR_NO_MEMORY);
                }
                self.plugins
                    .try_reserve_exact(1)
                    .map_err(|_| ERR_NO_MEMORY)?;
                self.plugins.push(Some(id));
            }
        }
        Ok(())
    }

    /// Replaces `plugin`'s state of the item (the default state removes it).
    /// With the room [`reserve`](Self::reserve) made it allocates nothing.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` when there was no room (the table is unchanged).
    pub(crate) fn apply(&mut self, plugin: &str, mut push: ItemPush) -> Result<(), i32> {
        let slot = self.slot_of(plugin);
        let at = slot.and_then(|slot| {
            self.entries
                .iter()
                .position(|e| e.plugin == slot && e.item() == push.item())
        });
        if push.is_default() {
            if let Some(at) = at {
                self.entries.remove(at);
                if self.entries.is_empty() {
                    self.entries = Vec::new();
                }
            }
            return Ok(());
        }
        let slot = slot.ok_or(ERR_NO_MEMORY)?;
        push.plugin = slot;
        match at {
            Some(at) => self.entries[at] = push,
            None => {
                if self.entries.len() == self.entries.capacity() {
                    self.entries
                        .try_reserve_exact(1)
                        .map_err(|_| ERR_NO_MEMORY)?;
                }
                self.entries.push(push);
            }
        }
        Ok(())
    }

    /// The plugin was updated or removed: every state of it ends.
    pub(crate) fn forget(&mut self, plugin: &str) {
        let Some(slot) = self.slot_of(plugin) else {
            return;
        };
        self.entries.retain(|e| e.plugin != slot);
        if self.entries.is_empty() {
            self.entries = Vec::new();
        }
        self.plugins[usize::from(slot)] = None;
        while matches!(self.plugins.last(), Some(None)) {
            self.plugins.pop();
        }
        if self.plugins.is_empty() {
            self.plugins = Vec::new();
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
#[path = "item_state_tests.rs"]
mod tests;
