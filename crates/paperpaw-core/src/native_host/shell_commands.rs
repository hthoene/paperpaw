//! One callback-local command stage, drained only after every host borrow ends.
use super::{Batch, Context, DispatchDepth, Error, Registration};
use crate::{Kernel, abi7::core_frame::CoreFrame, native_host::commands::CommandStage};
use core::pin::Pin;
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, IdError, ScreenKey};

/// Host-resolved source instance and bounded command-chain depth.
pub(crate) struct CommandTarget {
    pub(crate) screen: Option<ScreenKey>,
    pub(crate) hop: u8,
}

impl Kernel {
    /// Admission, settings authority and command limits are shared by every
    /// token callback. The invocation may install its environment and canvas
    /// scopes, but must end them before returning. No second queue is retained.
    #[allow(
        dead_code,
        reason = "native screen publication wires this callback path next"
    )]
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
                env: &env,
                meter: &self.shared.meter,
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
                let output = stage.into_ui_output();
                Ok::<_, Error>((status, owner, output))
            })??
        };
        self.drain_native_ui(&owner, commands, redraw);
        Ok(status)
    }
}
