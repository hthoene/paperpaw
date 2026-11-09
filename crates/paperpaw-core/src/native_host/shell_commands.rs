//! Inactive callback staging, exercised through kernel-backed dispatch fixtures.
use super::{Batch, Context, DispatchDepth, Error, Registration};
use crate::{
    Kernel,
    abi7::core_frame::CoreFrame,
    native_host::commands::{CALLBACK_COMMAND_BYTES, CALLBACK_COMMANDS, CommandStage},
};
use core::pin::Pin;
use paperpaw_native::host_glue::ERR_INVALID;
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
        let (status, owner, (commands, redraw, _hop), item_states) = {
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
                );
                if let Some(directory) = self.shared.registry.native_declarations.as_ref() {
                    stage = stage
                        .with_item_states(crate::native_host::commands::ItemAdmit { directory });
                }
                // SAFETY: the admitted owner structurally pins the command scope.
                let scope = unsafe { context.map_unchecked(|value| &value.commands) };
                let ran = (|| {
                    let status = scope.scope(&mut stage, || invoke(context, &frame))??;
                    let valid = status == 0
                        || (matches!(target.result, CommandResult::Input) && status == 1);
                    if valid {
                        Ok(status)
                    } else {
                        Err(Error::Status(if status < 0 { status } else { ERR_INVALID }))
                    }
                })();
                let status = match ran {
                    Ok(status) => status,
                    Err(error) => {
                        // Nothing is applied: the room and the slot go back.
                        stage.drop_item_states();
                        return Err(error);
                    }
                };
                let item_states = stage.take_item_states();
                let output = stage.into_ui_output();
                Ok::<_, Error>((status, owner, output, item_states))
            })??
        };
        self.drain_native_ui(&owner, commands, redraw);
        self.apply_item_states(&owner, item_states);
        Ok(status)
    }

    /// The states the callback pushed, applied in submit order after its
    /// other commands. The checks were made at submit; a state the host
    /// cannot store now (no memory) is dropped with a warning, the table
    /// unchanged.
    fn apply_item_states(
        &mut self,
        owner: &Id,
        (pushes, held): (
            alloc::vec::Vec<crate::native_host::item_state::ItemPush>,
            usize,
        ),
    ) {
        let Some(directory) = self.shared.registry.native_declarations.as_ref() else {
            return;
        };
        if pushes.is_empty() {
            crate::native_host::commands::ItemAdmit { directory }.finish(owner.as_str(), held);
            return;
        }
        let mut failed = false;
        match directory.items.try_borrow_mut() {
            Ok(mut states) => {
                for push in pushes {
                    failed |= states.apply(owner.as_str(), push).is_err();
                }
            }
            Err(_) => failed = true,
        }
        crate::native_host::commands::ItemAdmit { directory }.finish(owner.as_str(), held);
        if failed {
            self.warn_runtime("item state dropped: the host could not keep it");
        }
    }
}
