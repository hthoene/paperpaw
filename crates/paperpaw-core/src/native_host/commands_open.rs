//! `command.open` (1547): the four modes and the hand-over an open names
//! (`docs/abi/ui.md` 1.1, section 4). The stage claims a hand-over once the
//! whole batch is checked and every reservation is made; the kernel gives it
//! to the instance when the open applies (`Kernel::drain_native_opens`).
use super::{CommandBatch, CommandStage, WireCommand, ui};
use crate::native_host::ui_store::{Claims, Stamp, UiStore};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_sdk::{Command, Id, Params, ScreenKey};

/// The hand-over store and the host stamp of the callback the stage serves.
#[derive(Clone, Copy)]
pub(super) struct Handovers<'a> {
    pub(super) store: &'a UiStore,
    pub(super) stamp: Stamp,
}

/// One staged open, for the drain.
pub(crate) struct StagedOpen {
    /// Its command's index among the staged commands.
    pub(crate) index: usize,
    /// The hand-over it names (none: 0).
    pub(crate) handover: Option<u32>,
    /// Mode 2: the submitter's own top screen it replaces. The drain applies
    /// it only if that screen is still on top then; whatever else is, is not
    /// the submitter's to close.
    pub(crate) replaces: Option<ScreenKey>,
}

/// What the drain needs of the staged opens, and the claims to roll back
/// once it is done (or if the stage is dropped instead).
#[derive(Default)]
pub(crate) struct Opens {
    pub(crate) at: Vec<StagedOpen>,
    pub(crate) claims: Vec<Claims>,
}

impl<'a> CommandStage<'a> {
    /// Opens of this stage may name hand-overs `who` made in this callback.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the production command path takes it with the shell wiring (Input's core.ui, todo 119)"
        )
    )]
    #[must_use]
    pub(crate) fn with_handovers(mut self, store: &'a UiStore, stamp: Stamp) -> Self {
        self.handovers = Some(Handovers { store, stamp });
        self
    }

    /// The staged opens, for the drain; the claims go with them.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the production command path takes it with the shell wiring (Input's core.ui, todo 119)"
        )
    )]
    pub(super) fn take_opens(&mut self) -> Opens {
        core::mem::take(&mut self.opens)
    }

    /// As `into_ui_output`, with the opens the drain needs (hand-overs and
    /// the screens they replace). Without them the claims are rolled back
    /// when the stage is dropped.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the production command path takes it with the shell wiring (Input's core.ui, todo 119)"
        )
    )]
    pub(crate) fn into_ui_output_with_opens(
        mut self,
    ) -> (Vec<Command>, Option<ui::Redraw>, u8, Opens) {
        let opens = self.take_opens();
        let (commands, redraw, hop) = self.into_ui_output();
        (commands, redraw, hop, opens)
    }

    /// One open of a batch: modes, slot, the target's existence.
    pub(super) fn validate_open(&self, target: &str, mode: u32, slot: &str) -> Result<usize, i32> {
        let ui = self.ui.as_ref().ok_or(ERR_UNSUPPORTED)?;
        // `slot` is empty except for mode 3, where it is named.
        if mode > 3 || (mode == 3) == slot.is_empty() {
            return Err(ERR_INVALID);
        }
        crate::native_host::declarations::full_id(target)?;
        if !slot.is_empty() {
            crate::native_host::declarations::full_id(slot)?;
        }
        if ui.hop() >= 8 {
            return Err(ERR_BUSY);
        }
        let known = if mode == 1 {
            Id::try_parse(target).is_ok_and(|slot| ui.env().has_slot(&slot))
        } else {
            ui.env()
                .screens()
                .iter()
                .any(|(id, _)| id.as_str() == target)
        };
        if !known {
            return Err(ERR_NOT_FOUND);
        }
        if mode == 2 {
            self.own_top()?;
        }
        Ok(target.len() + slot.len())
    }

    /// The hand-overs the batch's opens name: each made by this callback,
    /// committed, and named once (`ERR_INVALID`; a begun text not committed is
    /// `ERR_STATE`). The store changes nothing here.
    #[inline(never)]
    pub(super) fn validate_handovers(&self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let (ids, count) = named(batch);
        let ids = &ids[..count];
        for (index, id) in ids.iter().enumerate() {
            let named_before = ids[..index].contains(id)
                || self
                    .opens
                    .at
                    .iter()
                    .any(|before| before.handover == Some(*id));
            let Some(named) = self.handovers.filter(|_| !named_before) else {
                return Err(ERR_INVALID);
            };
            named
                .store
                .handover_check(named.stamp, *id)
                .map_err(|status| {
                    if status == ERR_STATE {
                        ERR_STATE
                    } else {
                        ERR_INVALID
                    }
                })?;
        }
        Ok(())
    }

    /// After every reservation: the batch's hand-overs are claimed, all or
    /// none. The claims wait in the stage until the drain.
    #[inline(never)]
    pub(super) fn claim_handovers(
        &mut self,
        batch: &CommandBatch<'_>,
    ) -> Result<Option<Claims>, i32> {
        let (ids, count) = named(batch);
        let Some(named) = self.handovers.filter(|_| count > 0) else {
            return Ok(None);
        };
        named.store.claim(named.stamp, &ids[..count]).map(Some)
    }

    /// Reserve what recording the batch's opens takes, so recording cannot fail.
    #[inline(never)]
    pub(super) fn reserve_opens(&mut self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let opens = batch
            .iter()
            .filter(|c| matches!(c, WireCommand::Open { .. }))
            .count();
        self.opens
            .at
            .try_reserve(opens)
            .and(self.opens.claims.try_reserve(1))
            .map_err(|_| paperpaw_native::host_glue::ERR_NO_MEMORY)
    }

    /// The batch failed after its claim: free exactly those. A busy store
    /// hands the claims back to the stage, which retries when it is dropped.
    pub(super) fn unclaim(&mut self, claims: Option<Claims>) {
        let Some(claims) = claims else {
            return;
        };
        let Some(named) = self.handovers else {
            return;
        };
        if let Err((_, claims)) = named.store.rollback(claims) {
            self.opens.claims.push(claims);
        }
    }

    /// The batch is staged: its claims are kept for the drain.
    pub(super) fn keep_claims(&mut self, claims: Option<Claims>) {
        if let Some(claims) = claims {
            self.opens.claims.push(claims);
        }
    }

    /// Records an open staged at `index`.
    #[inline(never)]
    pub(super) fn record_open(
        &mut self,
        index: usize,
        mode: u32,
        handover: u32,
    ) -> Result<(), i32> {
        let replaces = if mode == 2 {
            Some(self.own_top()?)
        } else {
            None
        };
        // Room was reserved for every open of the batch.
        self.opens.at.push(StagedOpen {
            index,
            handover: (handover != 0).then_some(handover),
            replaces,
        });
        Ok(())
    }
}

impl Drop for CommandStage<'_> {
    /// A callback whose result drops its staged commands frees what its submits
    /// claimed, never what an earlier callback still has waiting.
    fn drop(&mut self) {
        let (Some(named), claims) = (self.handovers, core::mem::take(&mut self.opens.claims))
        else {
            return;
        };
        for claims in claims {
            // A store borrowed now keeps them claimed: the owner's stop frees them.
            let _ = named.store.rollback(claims);
        }
    }
}

/// The hand-over ids of a batch's opens, in order, at most one per record.
pub(super) fn named(
    batch: &CommandBatch<'_>,
) -> (
    [u32; crate::abi7::commands_wire::COMMAND_MAX_RECORDS],
    usize,
) {
    let mut ids = [0; crate::abi7::commands_wire::COMMAND_MAX_RECORDS];
    let mut count = 0;
    for command in batch.iter() {
        if let WireCommand::Open { handover, .. } = command
            && handover != 0
            && let Some(slot) = ids.get_mut(count)
        {
            *slot = handover;
            count += 1;
        }
    }
    (ids, count)
}

/// An open's wire fields as the kernel command that applies it.
pub(super) fn command(target: &str, mode: u32, slot: &str) -> Result<Command, i32> {
    let target = ui::copy_id(target)?;
    Ok(match mode {
        0 => Command::OpenScreen(target, Params::new()),
        1 => Command::OpenSlot(target, Params::new()),
        2 => Command::Replace(target, Params::new()),
        _ => Command::OpenInSlot(ui::copy_id(slot)?, target, Params::new()),
    })
}
