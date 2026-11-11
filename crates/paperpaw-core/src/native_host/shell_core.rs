//! Test-only kernel-backed dispatch without exposing borrowed CoreFrame internals.
use super::{
    DispatchDepth, Error, Invocation, Published, Registration, invoke_registration, screen_frame,
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
        batch: &Published<'_, S, M, Registration>,
        index: usize,
        depth: &DispatchDepth,
        invocation: Invocation<'_>,
        now: Instant,
    ) -> Result<i32, Error> {
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
            log: self.shared.device_log,
            ui: None,
        };
        batch.dispatch(index, depth, |context, record| {
            let _running = self.shared.meter.enter(context.plugin_id());
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
        batch: &Published<'_, S, M, Registration>,
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
            log: self.shared.device_log,
            ui: None,
        };
        batch.dispatch(index, depth, |context, record| {
            let _running = self.shared.meter.enter(context.plugin_id());
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
        batch: &Published<'_, S, M, Registration>,
        token: u32,
        depth: &DispatchDepth,
        bytes: &[u8],
        now: Instant,
        hop: u8,
    ) -> Result<i32, Error> {
        let screen = self.shared.stack.last().map(|entry| entry.key);
        self.with_native_commands(
            batch,
            depth,
            super::command_dispatch::CommandTarget {
                screen,
                hop,
                result: super::command_dispatch::CommandResult::Input,
            },
            now,
            |context, frame| {
                super::tokens::invoke_token(
                    context,
                    token,
                    frame,
                    super::TokenInvocation::Input(bytes),
                )
            },
        )
    }
}
