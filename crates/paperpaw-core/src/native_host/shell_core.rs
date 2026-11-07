//! Test-only kernel-backed dispatch without exposing borrowed CoreFrame internals.
use super::{
    Batch, DispatchDepth, Error, Invocation, Registration, invoke_registration, screen_frame,
};
use crate::{Kernel, abi7::core_frame::CoreFrame};
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::OK;
use paperpaw_platform::Instant;

impl Kernel {
    /// Dispatch a registered native callback with this kernel's current views.
    /// Usage, plugins and work notes borrow real core state for this call only.
    /// Owner admission precedes frame installation; frames clear on every exit.
    ///
    /// # Errors
    /// Returns the owner or frame refusal, or a missing callback error.
    pub(crate) fn dispatch_native_for_test<S, M>(
        &self,
        batch: &Batch<'_, S, M, Registration>,
        index: usize,
        depth: &DispatchDepth,
        invocation: Invocation<'_>,
        now: Instant,
    ) -> Result<i32, Error> {
        let env = self.shared.env(now);
        let frame = CoreFrame {
            env: &env,
            meter: &self.shared.meter,
            font_removals: &self.shared.font_removals,
            usage: &self.shared.usage,
            plugins: &self.shared.plugins,
            work: &self.shared.work_states,
        };
        batch.dispatch(index, depth, |context, record| {
            let _running = self.shared.meter.enter(&context.id);
            // SAFETY: the owner structurally pins this scope for the call.
            let scope = unsafe { context.map_unchecked(|value| &value.environment) };
            scope.scope_core(&frame, || invoke_registration(context, *record, invocation))?
        })?
    }

    /// Draw a registered native callback against a caller-owned canvas.
    /// The frame uses this kernel's real usage, plugins and work-note views.
    /// The visible clip defines the encoded frame, as in shell::dispatch_draw.
    ///
    /// # Errors
    /// Returns an owner, frame or canvas refusal, or a missing callback error.
    pub(crate) fn draw_native_for_test<S, M>(
        &self,
        batch: &Batch<'_, S, M, Registration>,
        index: usize,
        depth: &DispatchDepth,
        now: Instant,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, Error> {
        let mut bytes = [0; 64];
        let Some(len) = screen_frame(canvas.clip(), &mut bytes)? else {
            return Ok(OK);
        };
        let env = self.shared.env(now);
        let frame = CoreFrame {
            env: &env,
            meter: &self.shared.meter,
            font_removals: &self.shared.font_removals,
            usage: &self.shared.usage,
            plugins: &self.shared.plugins,
            work: &self.shared.work_states,
        };
        batch.dispatch(index, depth, |context, record| {
            let _running = self.shared.meter.enter(&context.id);
            // SAFETY: the owner structurally pins both scoped fields.
            let environment = unsafe { context.map_unchecked(|value| &value.environment) };
            let drawing = unsafe { context.map_unchecked(|value| &value.canvas) };
            environment.scope_core(&frame, || {
                drawing.scope(canvas, || {
                    invoke_registration(context, *record, Invocation::Draw(&bytes[..len]))
                })
            })??
        })?
    }
}

impl Kernel {
    /// The test caller resolves the token from its checked native screen.
    /// Input and its real command stage use the current kernel screen identity.
    pub(crate) fn input_native_for_test<S, M>(
        &mut self,
        batch: &Batch<'_, S, M, Registration>,
        token: u32,
        depth: &DispatchDepth,
        bytes: &[u8],
        now: Instant,
    ) -> Result<i32, Error> {
        use crate::native_host::commands::CommandStage;
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
                let owner =
                    paperpaw_sdk::Id::try_parse(&context.id).map_err(|error| match error {
                        paperpaw_sdk::IdError::NoMemory => Error::Allocation,
                        _ => Error::Status(paperpaw_native::host_glue::ERR_INVALID),
                    })?;
                let screen = env.stack().last().map(|entry| entry.key);
                let mut stage = CommandStage::new(
                    &owner,
                    env.settings(),
                    Some(self.command_settings_authority(&owner)),
                    8,
                    4096,
                )
                .with_ui(&env, screen, 0, u32::MAX - self.next_serial);
                // SAFETY: the command scope is structurally pinned by the owner.
                let scope = unsafe { context.map_unchecked(|value| &value.commands) };
                let status = scope.scope(&mut stage, || {
                    super::tokens::invoke_token(
                        context,
                        token,
                        &frame,
                        super::TokenInvocation::Input(bytes),
                    )
                })??;
                let output = stage.into_ui_output();
                Ok::<_, Error>((status, owner, output))
            })??
        };
        self.drain_native_ui(&owner, commands, redraw);
        Ok(status)
    }
}
