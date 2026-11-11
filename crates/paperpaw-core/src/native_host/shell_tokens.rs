//! Owner-admitted callbacks selected by declared screen or action tokens.
use super::registrations;
use super::{
    DispatchDepth, Error, Published, Registration, borrowed, host_glue, no_reply, status_only,
};
use paperpaw_graphics::Canvas;

/// A host-produced event or input for a declared screen or action token.
/// Tokens come from committed declarations, never a fabricated registration.
#[allow(
    dead_code,
    reason = "native built-in adapter follows this prerequisite"
)]
pub(crate) enum TokenInvocation<'a> {
    Event(&'a [u8]),
    Input(&'a [u8]),
}

/// Dispatch a token resolved by the host from one committed declaration.
/// The frame remains scoped to this synchronous callback and owner admission.
#[allow(
    dead_code,
    reason = "native built-in adapter follows this prerequisite"
)]
pub(crate) fn dispatch_token<S: 'static, M>(
    batch: &Published<'_, S, M, Registration>,
    token: u32,
    depth: &DispatchDepth,
    frame: &crate::abi7::core_frame::CoreFrame<'_>,
    invocation: TokenInvocation<'_>,
) -> Result<i32, Error> {
    batch.dispatch_owner(depth, |context| {
        invoke_token(context, token, frame, invocation)
    })?
}

/// As [`dispatch_token`], with the command stage of the callback lent to
/// the plugin's `commands.submit` for its duration (every callback but
/// `draw` gets one). The stage is the caller's: it reads what was accepted
/// after the callback returned.
pub(crate) fn dispatch_token_staged<S: 'static, M>(
    batch: &Published<'_, S, M, Registration>,
    token: u32,
    depth: &DispatchDepth,
    frame: &crate::abi7::core_frame::CoreFrame<'_>,
    invocation: TokenInvocation<'_>,
    stage: &mut super::super::commands::CommandStage<'_>,
) -> Result<i32, Error> {
    batch.dispatch_owner(depth, |context| {
        stage.admit_images(context.image_admit());
        if let Some(dev) = context.dev_context() {
            stage.admit_dev(dev);
        }
        // SAFETY: the admitted owner structurally pins the command scope.
        let scope = unsafe { context.map_unchecked(|value| &value.commands) };
        scope.scope(stage, || invoke_token(context, token, frame, invocation))?
    })?
}

pub(super) fn invoke_token<S: 'static>(
    context: core::pin::Pin<&super::Context<S>>,
    token: u32,
    frame: &crate::abi7::core_frame::CoreFrame<'_>,
    invocation: TokenInvocation<'_>,
) -> Result<i32, Error> {
    let instance = context.instance.get().ok_or(Error::Attempt)?;
    // SAFETY: the owner structurally pins the field; nothing moves it.
    let scope = unsafe { context.map_unchecked(|value| &value.environment) };
    scope.scope_core(frame, || match invocation {
        TokenInvocation::Event(bytes) => {
            let callback = instance
                .event
                .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
            // SAFETY: the admitted owner retains state and code. The
            // host-produced payload is borrowed only until this returns.
            Ok(status_only(context.run_plugin(|| unsafe {
                callback(instance.state, token, borrowed(bytes), no_reply())
            })))
        }
        TokenInvocation::Input(bytes) => {
            let Some(callback) = instance.input else {
                return Ok(0);
            };
            // SAFETY: the admitted owner retains state and code, and this
            // host-produced input is borrowed only for the callback.
            let status =
                context.run_plugin(|| unsafe { callback(instance.state, token, borrowed(bytes)) });
            Ok(if status > 1 {
                host_glue::ERR_INVALID
            } else {
                status
            })
        }
    })?
}

/// Draw a declared screen token with its full pane and separate clip rows.
/// The adapter supplies the complete frame rather than deriving a pane from
/// the clipped canvas, which may hold only one band of a larger screen.
#[allow(
    dead_code,
    reason = "native built-in adapter follows this prerequisite"
)]
pub(crate) fn dispatch_token_draw<S: 'static, M>(
    batch: &Published<'_, S, M, Registration>,
    token: u32,
    depth: &DispatchDepth,
    frame: &crate::abi7::core_frame::CoreFrame<'_>,
    draw_frame: &registrations::DrawFrame<'_>,
    canvas: &mut Canvas<'_>,
) -> Result<i32, Error> {
    if canvas.clip().width == 0 || canvas.clip().height == 0 {
        return Ok(host_glue::OK);
    }
    let mut bytes = [0; 320];
    let len = super::super::frames::encode(draw_frame, &mut bytes).map_err(Error::Status)?;
    batch.dispatch_owner(depth, |context| {
        // A child's draw reached from another plugin's (compose) is charged to
        // the child; a screen's own draw was entered by the screen router.
        let _running = frame.meter.enter_unless_running(context.plugin_id());
        let instance = context.instance.get().ok_or(Error::Attempt)?;
        let callback = instance
            .draw
            .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
        // SAFETY: the owner structurally pins these fields; nothing moves them.
        let env_scope = unsafe { context.map_unchecked(|value| &value.environment) };
        let canvas_scope = unsafe { context.map_unchecked(|value| &value.canvas) };
        env_scope
            .scope_core(frame, || {
                canvas_scope.scope(canvas, || {
                    // SAFETY: the admitted owner retains state and code. Encoded
                    // bytes and the canvas scope live only for this callback.
                    status_only(context.run_plugin(|| unsafe {
                        callback(instance.state, token, borrowed(&bytes[..len]))
                    }))
                })
            })?
            .map_err(Into::into)
    })?
}

// The accessors the command stage reads (`shell.rs` is at its line limit).
impl<S: 'static> super::Context<S> {
    /// The host owner key of the plugin (the key of its native instance).
    #[allow(
        dead_code,
        reason = "read by the staged command path (with_native_commands), which the shell wires"
    )]
    pub(crate) fn owner_key(&self) -> u64 {
        self.resources.owner_id()
    }

    /// What a command stage checks the plugin's image commands against:
    /// whether root 1 is open to it (`uses.storage`) and how many more
    /// requests it may queue now.
    #[allow(
        dead_code,
        reason = "read by the staged command path (with_native_commands), which the shell wires"
    )]
    pub(crate) fn image_admit(&self) -> super::super::image_service::commands::ImageAdmit {
        super::super::image_service::commands::ImageAdmit {
            storage_root: self
                .declared_services
                .features()
                .feature("uses.storage")
                .is_ok(),
            room: self
                .resources
                .hosts()
                .images()
                .prepare_room(self.resources.owner_id()),
        }
    }

    /// What a command stage judges the plugin's power, Wi-Fi and firmware
    /// commands with (`commands_dev.rs`): its admitted features, the
    /// kernel's Wi-Fi table and, where wired, the firmware table. Built for
    /// one callback; none before the kernel wired its Wi-Fi table, and then
    /// those commands stay `ERR_UNSUPPORTED`.
    pub(crate) fn dev_context(&self) -> Option<super::super::commands::dev::DevContext<'static>> {
        let hosts = self.resources.hosts();
        let context = super::super::commands::dev::DevContext::new(
            self.declared_services.features(),
            hosts.radio_requests()?,
        );
        Some(match hosts.firmware_requests() {
            Some(driver) => {
                context.with_firmware(super::super::commands::dev::FirmwareContext::new(
                    alloc::rc::Rc::clone(driver),
                    self.resources.owner_id(),
                ))
            }
            None => context,
        })
    }
}
