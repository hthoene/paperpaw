//! `command.set_item_state` (60, `docs/abi/screens-input.md` 3.1) at submit:
//! the checks of a whole batch and the states it stages, applied to the
//! host's [`ItemStates`] after the callback with the other commands.
//!
//! A batch's records are checked together, in this order, and a refusal
//! stages nothing: the host shows item states (`ERR_UNSUPPORTED` until it
//! does), at most one record per item (`ERR_INVALID`), the item is one of
//! the plugin's own actions, settings or setup steps (`ERR_NOT_FOUND`;
//! `switch` on a setting is `ERR_INVALID`), the label and badge keys are in
//! the catalogs (`ERR_NOT_FOUND`), and the net result fits the limits
//! (`ERR_FULL`). The argument batches are checked when the item is shown
//! (an item whose key cannot be resolved then shows its title).
use super::{CommandStage, WireCommand};
use crate::native_host::{
    declaration_store::Kind,
    item_state::{ItemPush, ItemStates, SWITCH},
    native_declarations::NativeDeclarations,
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};

/// Where the item states live: with the declarations (the registry reads
/// them there too).
#[derive(Clone, Copy)]
pub(crate) struct ItemAdmit<'a> {
    pub(crate) directory: &'a NativeDeclarations,
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
    /// callback (call it before the stage is consumed).
    #[must_use]
    pub(crate) fn take_item_states(&mut self) -> Vec<ItemPush> {
        core::mem::take(&mut self.staged_items)
    }

    /// Whether `command` is an item state.
    pub(super) fn is_item_state(command: &WireCommand<'_>) -> bool {
        matches!(command, WireCommand::SetItemState { .. })
    }

    /// The item-state records of `batch`, checked and copied; their bytes
    /// are the second part of the answer.
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
            if pushes.iter().any(|p| &*p.item == item) {
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
            for key in [label, badge] {
                if !key.is_empty() && env.i18n().get(key).is_none() {
                    return Err(ERR_NOT_FOUND);
                }
            }
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
        admit
            .states()?
            .fits(self.owner.as_str(), &all)
            .map_err(|status| if status == ERR_FULL { ERR_FULL } else { status })?;
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
