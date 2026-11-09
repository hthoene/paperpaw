//! Inactive callback staging, exercised through kernel-backed dispatch fixtures.
use super::{Batch, Context, DispatchDepth, Error, Registration};
use crate::{
    Kernel,
    abi7::core_frame::CoreFrame,
    native_host::commands::{CALLBACK_COMMAND_BYTES, CALLBACK_COMMANDS, CommandStage},
};
use core::pin::Pin;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, IdError, ScreenKey};

/// Valid result values for the callback family lent this command stage.
pub(crate) enum CommandResult {
    Status,
    Input,
}

/// Host-resolved source instance and bounded command-chain depth.
pub(crate) struct CommandTarget {
    pub(crate) screen: Option<ScreenKey>,
    pub(crate) hop: u8,
    pub(crate) result: CommandResult,
}

/// `event.registry_changed.what` bit 0: menus and the item states they show.
const REGISTRY_MENUS: u32 = 1;

impl Kernel {
    /// Admission, settings authority and command limits are shared by event and
    /// input callbacks. The invocation ends its environment scope before returning.
    /// Draw is render-only and must never use this helper. No second queue is kept.
    pub(crate) fn with_native_commands<S, M>(
        &mut self,
        batch: &Batch<'_, S, M, Registration>,
        depth: &DispatchDepth,
        target: CommandTarget,
        now: Instant,
        invoke: impl FnOnce(Pin<&Context<S>>, &CoreFrame<'_>) -> Result<i32, Error>,
    ) -> Result<i32, Error> {
        let (status, owner, owner_key, images, (commands, redraw, _hop), item_states, presets, hints) =
            {
            let env = self.shared.env(now);
            let frame = CoreFrame {
                callback: None,
                instance: None,
                env: &env,
                meter: &self.shared.meter,
                depth: None,
                directory: None,
                stack_generation: self.shared.stack_rows.generation(),
                stack_rows: &self.shared.stack_rows,
                font_removals: &self.shared.font_removals,
                registry_generation: &self.shared.registry_generation,
                usage: &self.shared.usage,
                plugins: &self.shared.plugins,
                work: &self.shared.work_states,
            };
            batch.dispatch_owner(depth, |context| {
                let owner = Id::try_parse(&context.id).map_err(|error| match error {
                    IdError::NoMemory => Error::Allocation,
                    _ => Error::Status(ERR_INVALID),
                })?;
                let mut stage = CommandStage::new(
                    &owner,
                    env.settings(),
                    Some(self.command_settings_authority(&owner)),
                    CALLBACK_COMMANDS,
                    CALLBACK_COMMAND_BYTES,
                )
                .with_ui(
                    &env,
                    target.screen,
                    target.hop,
                    self.screen_keys_remaining(),
                )
                .with_images({
                    // Requests held outside the table count against the room.
                    let mut admit = context.image_admit();
                    admit.room = admit
                        .room
                        .saturating_sub(self.image_held(context.owner_key()));
                    admit
                })
                .with_hint_labels(
                    self.shared.stack_rows.labels(),
                    !self.hint.buttons.is_empty(),
                );
                if let Some(directory) = self.shared.registry.native_declarations.as_ref() {
                    stage = stage
                        .with_item_states(crate::native_host::commands::ItemAdmit { directory });
                }
                // SAFETY: the admitted owner structurally pins the command scope.
                let scope = unsafe { context.map_unchecked(|value| &value.commands) };
                // A failed callback applies nothing: the stage's room guard
                // gives its room and slot back when the stage drops.
                let status = scope.scope(&mut stage, || invoke(context, &frame))??;
                let valid =
                    status == 0 || (matches!(target.result, CommandResult::Input) && status == 1);
                if !valid {
                    return Err(Error::Status(if status < 0 { status } else { ERR_INVALID }));
                }
                let item_states = stage.take_item_states();
                let presets = stage.take_presets();
                let images = stage.take_image_commands();
                let hints = stage.take_hint_sets();
                let output = stage.into_ui_output();
                Ok::<_, Error>((
                    status,
                    owner,
                    context.owner_key(),
                    images,
                    output,
                    item_states,
                    presets,
                    hints,
                ))
            })??
        };
        self.drain_native_ui(&owner, commands, redraw);
        // The picture requests it admitted, after the callback.
        self.native_images_apply(owner_key, images);
        self.apply_item_states(&owner, item_states);
        let authority = self.command_settings_authority(&owner);
        self.apply_native_presets(&owner, authority, presets);
        // The labels the callback set; room was taken when they were checked.
        // Nothing holds the table across a callback, so a failed borrow is a
        // host fault: the callback fails (`ERR_BUSY`) rather than dropping them.
        if !hints.is_empty() {
            let Ok(mut labels) = self.shared.stack_rows.labels().try_borrow_mut() else {
                return Err(Error::Status(ERR_BUSY));
            };
            labels.apply(hints);
        }
        Ok(status)
    }

    /// The states the callback pushed, applied in submit order after its
    /// other commands. The checks were made at submit; a state the host
    /// cannot store now (no memory) is dropped with a warning, the table
    /// unchanged.
    fn apply_item_states(
        &mut self,
        owner: &Id,
        (pushes, room): (
            alloc::vec::Vec<crate::native_host::item_state::ItemPush>,
            Option<crate::native_host::commands::ItemRoom>,
        ),
    ) {
        let Some(directory) = self.shared.registry.native_declarations.as_ref() else {
            return;
        };
        let (mut failed, mut changed) = (false, false);
        match directory.items.try_borrow_mut() {
            Ok(mut states) => {
                for push in pushes {
                    match states.apply(owner.as_str(), push) {
                        Ok(did) => changed |= did,
                        Err(_) => failed = true,
                    }
                }
                // A plugin that cleared its last state needs no slot (a batch
                // of defaults holds no room guard that would free it).
                states.settle(owner.as_str());
            }
            Err(_) => failed = !pushes.is_empty(),
        }
        // The room and a slot no state needs go back (nothing is borrowed).
        drop(room);
        if changed {
            // Menus show item states: bit 0 of `event.registry_changed`.
            self.native_registry_changed(REGISTRY_MENUS);
        }
        if failed {
            self.warn_runtime("item state dropped: the host could not keep it");
        }
    }
}
