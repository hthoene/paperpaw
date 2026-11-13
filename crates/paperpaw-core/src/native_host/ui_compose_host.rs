//! The production half of `Frame::compose` (slot 35, `docs/abi/ui.md` section
//! 5.1): the handle table and guard checks of [`ui_handles::compose`] with the
//! owner's `draw` run through the retained native registry.
//!
//! Core's frame holds a [`ComposeHost`] beside its stack view and answers
//! `Frame::compose` with [`ComposeHost::compose`], passing the callback stamp
//! it already has. Nothing here allocates, keeps a borrow past the call or
//! stores state: the table, the guard view, the instance source, the environment
//! and the depth counter are all borrowed for the one call.
//!
//! - **Depth.** The caller's `draw` runs at depth 1 (the dispatch entered it),
//!   the child's at depth 2 (its own `lease.draw` enters again); the depth the
//!   caller is at is read from the shared [`DispatchDepth`], so a compose from a
//!   child is `ERR_BUSY` and the dispatch refuses a third level besides.
//! - **The owner.** A running owner is drawn with the handle's token; one that
//!   is lazy or stopped cannot be, which is the child's failure (logged, the
//!   caller's frame stays, `Ok`).
//! - **Memory.** The child's dispatch (`dispatch_token_draw`) encodes the draw
//!   frame into a 320-byte stack array and calls the owner's `draw` with no
//!   reply buffer, so a nested draw allocates nothing on the heap: no
//!   fallible allocation is needed and an out-of-memory sweep has nothing to
//!   refuse. The pool and directory handles it holds are counted clones.
//! - **The X4.** Until its stack gate passes (two plugin frames and the host
//!   frame between them on the main stack, section 5.1) the answer is
//!   `ERR_UNSUPPORTED`: `gate_open` is false there. A status handle of the
//!   host (the kernel's own item, `host_status`) runs no plugin: it is drawn
//!   there too, in its own frame. The host frames this adds
//!   hold no buffers (about 100 bytes each), inside the #254 budget of 2,047
//!   bytes per frame that CI checks on the `ppn5` build.
use super::{
    DispatchDepth,
    native_declarations::NativeDeclarations,
    native_instances::{InstanceState, NativeInstances, NativeLease},
    native_screen::Dispatch,
    registrations::DrawFrame,
    stack_rows::StackRows,
    ui_guard,
    ui_handles::{self, Caller, ChildDraw, GuardState, Target, UiHandles},
    ui_widget_ops::host_status,
};
use crate::abi7::{
    context::{ComposeKind, ComposeRequest, Frame},
    core_frame::{CallbackStamp, CoreFrame},
};
use core::cell::RefCell;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_CANCELLED, ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_sdk::Env;

/// Where a target's retained instance comes from: the registry (tests, the
/// kernel's own view) or the declaration directory (what a plugin's frame has).
pub(crate) trait Leases {
    /// The owner's live instance.
    ///
    /// # Errors
    /// `ERR_STATE` for one that is lazy (not started), `ERR_CANCELLED` for a
    /// stopped or unknown one, `ERR_BUSY` when the table is borrowed.
    fn lease(&self, owner: u64) -> Result<NativeLease, i32>;
}

impl Leases for NativeInstances {
    fn lease(&self, owner: u64) -> Result<NativeLease, i32> {
        match self.state(owner)? {
            InstanceState::Running(lease) => Ok(lease),
            InstanceState::Lazy => Err(ERR_STATE),
            InstanceState::Stopped => Err(ERR_CANCELLED),
        }
    }
}

impl Leases for NativeDeclarations {
    fn lease(&self, owner: u64) -> Result<NativeLease, i32> {
        // The borrow ends with the statement: the child may declare or stop.
        self.store
            .try_borrow()
            .map_err(|_| ERR_BUSY)?
            .native_lease(owner)
    }
}

/// Runs a target's `draw` on its owner's retained instance.
pub(crate) struct RegistryDraw<'a, 'e> {
    pub(crate) leases: &'a dyn Leases,
    pub(crate) env: &'a Env<'e>,
    /// The caller's screen callback, kept on the stack tracker while the
    /// child draws (`network.md` 5: a server is tied to that screen).
    pub(crate) screen: Option<(&'a StackRows, u32)>,
}

impl ChildDraw for RegistryDraw<'_, '_> {
    fn draw(
        &self,
        target: &Target,
        frame: &DrawFrame<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        let lease = self.leases.lease(target.owner)?;
        // The child's callback runs on behalf of the caller's screen: the
        // tracker names the child's owner on that screen until it returns
        // (the previous entry comes back). Another owner's row never answers
        // `showing_screen` for the child, so a nested server refuses, safely.
        // TODO(#228, Core): charge the running-plugin tracker here too, once
        // Core's central nested-dispatch path exists.
        let _screen = self
            .screen
            .map(|(rows, instance)| rows.enter_callback(target.owner, instance));
        lease.draw(target.token, frame, self.env, canvas)
    }

    fn failed(&self, target: &Target, status: i32) {
        // The owner is told; a stopped one has nobody to tell.
        if let Ok(lease) = self.leases.lease(target.owner) {
            lease.dropped(target.token, status);
        }
    }
}

/// The stack's guard as the handle table asks for it, read from the frame.
/// A stack the frame cannot read fails closed: guarded by no one's plugin,
/// everything below it.
pub(crate) struct StackGuard<'a>(pub(crate) &'a dyn Frame);

impl GuardState for StackGuard<'_> {
    fn guard_plugin(&self) -> Option<u64> {
        let mut guard = None;
        match self
            .0
            .with_stack(&mut |view| guard = ui_guard::guard_owner(view))
        {
            Ok(()) => guard,
            Err(_) => Some(0),
        }
    }

    fn below_guard(&self, screen_instance: u32) -> bool {
        let mut below = false;
        let read = self.0.with_stack(&mut |view| {
            let at = (0..view.len())
                .find(|&i| view.row(i).is_some_and(|r| r.instance == screen_instance));
            below = match (ui_guard::topmost_guard(view), at) {
                (Some(guard), Some(at)) => at <= guard,
                // A screen the stack does not hold, under a guard: fail closed.
                (Some(_), None) => true,
                (None, _) => false,
            };
        });
        read.is_err() || below
    }
}

/// `Frame::compose` of a plugin's own frame: the handle table of the device's
/// `core.ui` service, the declaration directory for the owner's instance, the
/// frame's stack for the guard, the frame's depth and callback stamp.
///
/// # Errors
/// `ERR_UNSUPPORTED` where no `core.ui` service or directory is wired and on the
/// X4 (the stack gate), `ERR_STATE` outside a callback; the rest as
/// [`ComposeHost::compose`].
pub(crate) fn compose_in(
    frame: &CoreFrame<'_>,
    request: &ComposeRequest,
    canvas: &mut Canvas<'_>,
) -> Result<(), i32> {
    let stamp = frame.callback.ok_or(ERR_STATE)?;
    let (ui, directory, depth) = match (frame.ui, frame.directory, frame.depth) {
        (Some(ui), Some(directory), Some(depth)) => (ui, directory, depth),
        _ => return Err(ERR_UNSUPPORTED),
    };
    let gate_open = !frame.nested_closed()?;
    ComposeHost {
        handles: &ui.widgets.handles,
        guard: &StackGuard(frame),
        leases: directory,
        env: frame.env,
        depth,
        screen: frame.instance.map(|instance| (frame.stack_rows, instance)),
        gate_open,
    }
    .compose(stamp, request, canvas)
}

/// Everything `Frame::compose` needs, borrowed for one call.
pub(crate) struct ComposeHost<'a, 'e> {
    pub(crate) handles: &'a RefCell<UiHandles>,
    pub(crate) guard: &'a dyn GuardState,
    pub(crate) leases: &'a dyn Leases,
    pub(crate) env: &'a Env<'e>,
    pub(crate) depth: &'a DispatchDepth,
    /// The caller's screen instance and the stack tracker, if it has a screen.
    pub(crate) screen: Option<(&'a StackRows, u32)>,
    /// False on the X4 until its nested-callback stack gate passes.
    pub(crate) gate_open: bool,
}

impl ComposeHost<'_, '_> {
    /// `Frame::compose` for the callback `stamp` made by the caller (its owner
    /// and callback epoch, host-resolved, never plugin bytes).
    pub(crate) fn compose(
        &self,
        stamp: CallbackStamp,
        request: &ComposeRequest,
        canvas: &mut Canvas<'_>,
    ) -> Result<(), i32> {
        let caller = Caller {
            plugin: stamp.owner,
            callback: stamp.epoch,
            depth: self.depth.0.current(),
        };
        let draw = RegistryDraw {
            leases: self.leases,
            env: self.env,
            screen: self.screen,
        };
        // Resolved under the table's borrow, drawn without it: the child's
        // callback may end its own status handles (`callback_ended`) when it
        // returns, which needs the table.
        let target = self.handles.try_borrow().map_err(|_| ERR_BUSY)?.resolve(
            caller,
            request.kind,
            request.handle,
            self.guard,
        );
        // The kernel's own item: drawn by the host, no plugin called, so the
        // X4's gate does not hold it back (todo 598).
        if let Ok(found) = &target
            && request.kind == ComposeKind::StatusItem
            && found.owner == host_status::HOST
        {
            return host_status::draw(self.env, found.token, request, canvas);
        }
        if !self.gate_open {
            return Err(ERR_UNSUPPORTED);
        }
        let target = target?;
        ui_handles::draw_target(&draw, target, request, canvas)
    }
}

#[cfg(test)]
#[path = "ui_compose_host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ui_compose_frame_tests.rs"]
mod frame_tests;
