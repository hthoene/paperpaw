//! Inactive callback staging, exercised through kernel-backed dispatch fixtures.
use super::{Batch, Context, DispatchDepth, Error, Registration};
use crate::{Kernel, abi7::core_frame::CoreFrame, native_host::commands::CommandStage};
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
        let (status, owner, (commands, redraw, _hop)) = {
            let env = self.shared.env(now);
            let frame = CoreFrame {
                callback: None,
                instance: None,
                env: &env,
                meter: &self.shared.meter,
                directory: None,
                font_removals: &self.shared.font_removals,
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
                    8,
                    4096,
                )
                .with_ui(
                    &env,
                    target.screen,
                    target.hop,
                    self.screen_keys_remaining(),
                );
                // SAFETY: the admitted owner structurally pins the command scope.
                let scope = unsafe { context.map_unchecked(|value| &value.commands) };
                let status = scope.scope(&mut stage, || invoke(context, &frame))??;
                let valid =
                    status == 0 || (matches!(target.result, CommandResult::Input) && status == 1);
                if !valid {
                    return Err(Error::Status(if status < 0 { status } else { ERR_INVALID }));
                }
                let output = stage.into_ui_output();
                Ok::<_, Error>((status, owner, output))
            })??
        };
        self.drain_native_ui(&owner, commands, redraw);
        Ok(status)
    }
}
