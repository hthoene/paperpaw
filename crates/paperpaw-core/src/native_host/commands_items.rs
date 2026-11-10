//! `command.set_item_state` (60, `docs/abi/screens-input.md` 3.1) at submit:
//! the checks of a whole batch and the states it stages, applied to the
//! host's [`ItemStates`] after the callback with the other commands.
//!
//! A batch's records are checked together, in this order, and a refusal
//! stages nothing: the host shows item states (`ERR_UNSUPPORTED` until it
//! does), at most one record per item (`ERR_INVALID`), the item is one of
//! the plugin's own actions, settings or setup steps (`ERR_NOT_FOUND`;
//! `switch` on a setting is `ERR_INVALID`), the label and badge keys are in
//! the catalogs and resolve with their argument batches, as slot 27 would
//! (`ERR_NOT_FOUND`, `ERR_INVALID`, `ERR_NO_SPACE`; arguments without a key
//! are `ERR_INVALID`), and the net result fits the limits (`ERR_FULL`). The
//! room for the states is reserved before the batch is answered `OK`
//! (`ERR_NO_MEMORY` otherwise), so applying it after the callback allocates
//! nothing. The texts are resolved again each time the item is shown.
use super::{CommandStage, WireCommand};
use crate::native_host::shared::Shared;
use crate::native_host::{
    declaration_store::Kind,
    item_state::{ItemPush, ItemStates, SWITCH},
    native_declarations::NativeDeclarations,
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};

/// The longest label and badge text a menu shows (`registry.md`: a title is
/// at most 128 bytes, a badge 64); a longer one is `ERR_NO_SPACE` at submit.
const LABEL_ROOM: usize = 128;
const BADGE_ROOM: usize = 64;

/// A pushed label or badge at submit: no key means no arguments; otherwise
/// the key resolves with them, in the reader's language now.
fn resolves(
    translator: &paperpaw_i18n::Translator,
    key: &str,
    args: &[u8],
    room: &mut [u8],
) -> Result<(), i32> {
    if key.is_empty() {
        return if args.is_empty() {
            Ok(())
        } else {
            Err(ERR_INVALID)
        };
    }
    crate::native_host::formatting::format_into(translator, key, args, room).map(|_| ())
}

/// Where the item states live: with the declarations (the registry reads
/// them there too).
#[derive(Clone, Copy)]
pub(crate) struct ItemAdmit<'a> {
    pub(crate) directory: &'a Shared<NativeDeclarations>,
}

/// The room a callback holds in the item-state table (entries reserved at
/// submit, see [`ItemStates::reserve`]) and the claim on its plugin's slot.
/// Dropping it, on any path (applied, failed, refused, unwound), gives the
/// room back and frees the slot if the plugin holds no state.
pub(crate) struct ItemRoom {
    directory: Shared<NativeDeclarations>,
    plugin: alloc::boxed::Box<str>,
    held: usize,
}

impl ItemRoom {
    fn new(directory: &Shared<NativeDeclarations>, plugin: &str) -> Result<Self, i32> {
        let mut name = alloc::string::String::new();
        name.try_reserve_exact(plugin.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        name.push_str(plugin);
        Ok(Self {
            directory: directory.try_clone().ok_or(ERR_NO_MEMORY)?,
            plugin: name.into_boxed_str(),
            held: 0,
        })
    }
}

impl Drop for ItemRoom {
    fn drop(&mut self) {
        let room = &self.directory.item_room;
        room.set(room.get().saturating_sub(self.held));
        if let Ok(mut states) = self.directory.items.try_borrow_mut() {
            states.settle(&self.plugin);
        }
    }
}

impl ItemAdmit<'_> {
    fn states(&self) -> Result<core::cell::Ref<'_, ItemStates>, i32> {
        self.directory
            .items
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)
    }
}

/// `<owner>.<item>` on the stack (an id is at most 192 bytes).
fn full<'b>(buffer: &'b mut [u8; 192], owner: &str, item: &str) -> Option<&'b str> {
    let end = owner.len().checked_add(1)?.checked_add(item.len())?;
    let slot = buffer.get_mut(..end)?;
    slot[..owner.len()].copy_from_slice(owner.as_bytes());
    slot[owner.len()] = b'.';
    slot[owner.len() + 1..].copy_from_slice(item.as_bytes());
    core::str::from_utf8(slot).ok()
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the production native command path takes it with the shell wiring"
    )
)]
impl<'a> CommandStage<'a> {
    /// Admits `command.set_item_state` for this callback, kept in the
    /// declarations' item states; otherwise it is `ERR_UNSUPPORTED`.
    #[must_use]
    pub(crate) fn with_item_states(mut self, items: ItemAdmit<'a>) -> Self {
        self.items = Some(items);
        self
    }

    /// The admitted states, in order, for the host to apply after the
    /// callback (call it before the stage is consumed), and the room this
    /// callback holds in the table: keep it until they are applied.
    #[must_use]
    pub(crate) fn take_item_states(&mut self) -> (Vec<ItemPush>, Option<ItemRoom>) {
        (
            core::mem::take(&mut self.staged_items),
            self.room.get_mut().take(),
        )
    }

    /// Whether `command` is an item state.
    pub(super) fn is_item_state(command: &WireCommand<'_>) -> bool {
        matches!(command, WireCommand::SetItemState { .. })
    }

    /// The item-state records of `batch`, checked and copied; their bytes
    /// are the second part of the answer.
    #[inline(never)]
    pub(super) fn validate_item_states(
        &self,
        batch: &super::CommandBatch<'_>,
    ) -> Result<(Vec<ItemPush>, usize), i32> {
        let mut pushes: Vec<ItemPush> = Vec::new();
        let count = batch.iter().filter(Self::is_item_state).count();
        if count == 0 {
            return Ok((pushes, 0));
        }
        let admit = self.items.ok_or(ERR_UNSUPPORTED)?;
        let ui = self.ui.as_ref().ok_or(ERR_UNSUPPORTED)?;
        let env = ui.env();
        pushes.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
        for command in batch.iter() {
            let WireCommand::SetItemState {
                item,
                flags,
                label,
                label_args,
                badge,
                badge_args,
            } = command
            else {
                continue;
            };
            if pushes.iter().any(|p| p.item() == item) {
                return Err(ERR_INVALID);
            }
            let mut buffer = [0_u8; 192];
            let id = full(&mut buffer, self.owner.as_str(), item).ok_or(ERR_INVALID)?;
            let setting = env.settings().spec(id).is_some();
            let declared =
                setting || env.actions().contains(id) || self.is_setup_step(admit, item)?;
            if !declared {
                return Err(ERR_NOT_FOUND);
            }
            if setting && flags & SWITCH != 0 {
                return Err(ERR_INVALID);
            }
            resolves(env.i18n(), label, label_args, &mut [0_u8; LABEL_ROOM])?;
            resolves(env.i18n(), badge, badge_args, &mut [0_u8; BADGE_ROOM])?;
            pushes.push(ItemPush::copied(
                item,
                flags,
                (label, label_args),
                (badge, badge_args),
            )?);
        }
        // The callback's earlier batches count too (they are applied later).
        let mut all: Vec<&ItemPush> = Vec::new();
        all.try_reserve_exact(self.staged_items.len() + pushes.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        all.extend(self.staged_items.iter());
        all.extend(pushes.iter());
        admit.states()?.fits(self.owner.as_str(), &all)?;
        // Room for every state this callback can add, on top of what the
        // callbacks it runs inside of hold, and the plugin's slot.
        let adding = all.iter().filter(|p| !p.is_default()).count();
        let room = &admit.directory.item_room;
        let mut mine = self.room.borrow_mut();
        let held = mine.as_ref().map_or(0, |held| held.held);
        let others = room.get().saturating_sub(held);
        if adding > 0 {
            if mine.is_none() {
                *mine = Some(ItemRoom::new(admit.directory, self.owner.as_str())?);
            }
            admit
                .directory
                .items
                .try_borrow_mut()
                .map_err(|_| ERR_BUSY)?
                .reserve(self.owner.as_str(), others + adding)?;
        }
        room.set(others + adding);
        if let Some(mine) = mine.as_mut() {
            mine.held = adding;
        }
        drop(mine);
        let bytes = pushes.iter().map(ItemPush::bytes).sum();
        Ok((pushes, bytes))
    }

    fn is_setup_step(&self, admit: ItemAdmit<'_>, name: &str) -> Result<bool, i32> {
        let store = admit
            .directory
            .store
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        Ok(store.find(self.owner.as_str(), Kind::SetupStep, name, &mut |_| {}))
    }
}
