//! Inactive callback staging, exercised through kernel-backed dispatch fixtures.
use super::{Context, DispatchDepth, Error, Published, Registration};
use crate::{
    Kernel,
    abi7::core_frame::CoreFrame,
    native_host::screen_stage::{StageParts, build_stage, staged_of},
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

impl crate::Shared {
    /// The frame a command-staging callback runs with: no callback, instance,
    /// directory, depth or screen of its own.
    fn command_frame<'a>(&'a self, env: &'a crate::Env<'a>) -> CoreFrame<'a> {
        CoreFrame {
            callback: None,
            instance: None,
            env,
            meter: &self.meter,
            depth: None,
            directory: None,
            stack_generation: self.stack_rows.generation(),
            stack_rows: &self.stack_rows,
            font_removals: &self.font_removals,
            registry_generation: &self.registry_generation,
            usage: &self.usage,
            plugins: &self.plugins,
            work: &self.work_states,
            log: self.device_log,
            ui: None,
        }
    }
}

impl Kernel {
    /// Admission, settings authority and command limits are shared by event and
    /// input callbacks. The invocation ends its environment scope before returning.
    /// Draw is render-only and must never use this helper. No second queue is kept.
    pub(crate) fn with_native_commands<S: 'static, M>(
        &mut self,
        batch: &Published<'_, S, M, Registration>,
        depth: &DispatchDepth,
        target: CommandTarget,
        now: Instant,
        invoke: impl FnOnce(Pin<&Context<S>>, &CoreFrame<'_>) -> Result<i32, Error>,
    ) -> Result<i32, Error> {
        let (status, owner, staged) = {
            let env = self.shared.env(now);
            let frame = self.shared.command_frame(&env);
            batch.dispatch_owner(depth, |context| {
                let owner = Id::try_parse(context.plugin_id()).map_err(|error| match error {
                    IdError::NoMemory => Error::Allocation,
                    _ => Error::Status(ERR_INVALID),
                })?;
                let owner_key = context.owner_key();
                let inputs = self.stage_inputs(owner.as_str(), owner_key);
                let mut stage = build_stage(StageParts {
                    owner: &owner,
                    owner_key,
                    env: &env,
                    screen: target.screen,
                    hop: target.hop,
                    inputs,
                    stack_rows: &self.shared.stack_rows,
                    ui: self.shared.native_instances.ui_services(),
                    directory: self.shared.registry.native_declarations.as_ref(),
                })
                .with_images({
                    // Requests held outside the table count against the room.
                    let mut admit = context.image_admit();
                    admit.room = admit.room.saturating_sub(inputs.images_held);
                    admit
                });
                if let Some(dev) = context.dev_context() {
                    stage.admit_dev(dev);
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
                let staged = staged_of(stage, owner_key);
                Ok::<_, Error>((status, owner, staged))
            })??
        };
        self.apply_staged(&owner, staged)?;
        Ok(status)
    }
}
