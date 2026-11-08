//! The production half of `Frame::compose` (slot 35, `docs/abi/ui.md` section
//! 5.1): the handle table and guard checks of [`ui_handles::compose`] with the
//! owner's `draw` run through the retained native registry.
//!
//! Core's frame holds a [`ComposeHost`] beside its stack view and answers
//! `Frame::compose` with [`ComposeHost::compose`], passing the callback stamp
//! it already has. Nothing here allocates, keeps a borrow past the call or
//! stores state: the table, the guard view, the registry, the environment and
//! the depth counter are all borrowed for the one call.
//!
//! - **Depth.** The caller's `draw` runs at depth 1 (the dispatch entered it),
//!   the child's at depth 2 (its own `lease.draw` enters again); the depth the
//!   caller is at is read from the shared [`DispatchDepth`], so a compose from a
//!   child is `ERR_BUSY` and the dispatch refuses a third level besides.
//! - **The owner.** A running owner is drawn with the handle's token; one that
//!   is lazy or stopped cannot be, which is the child's failure (logged, the
//!   caller's frame stays, `Ok`).
//! - **The X4.** Until its stack gate passes (two plugin frames and the host
//!   frame between them on the main stack, section 5.1) the answer is
//!   `ERR_UNSUPPORTED`: `gate_open` is false there. The host frames this adds
//!   hold no buffers (about 100 bytes each), inside the #254 budget of 2,047
//!   bytes per frame that CI checks on the `ppn5` build.
use super::{
    DispatchDepth,
    native_instances::{InstanceState, NativeInstances},
    native_screen::Dispatch,
    registrations::DrawFrame,
    ui_handles::{self, Caller, ChildDraw, GuardState, Target, UiHandles},
};
use crate::abi7::{context::ComposeRequest, core_frame::CallbackStamp};
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_CANCELLED, ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_sdk::Env;

/// Runs a target's `draw` on its owner's retained instance.
pub(crate) struct RegistryDraw<'a, 'e> {
    pub(crate) registry: &'a NativeInstances,
    pub(crate) env: &'a Env<'e>,
}

impl ChildDraw for RegistryDraw<'_, '_> {
    fn draw(
        &self,
        target: &Target,
        frame: &DrawFrame<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        match self.registry.state(target.owner)? {
            InstanceState::Running(lease) => lease.draw(target.token, frame, self.env, canvas),
            InstanceState::Lazy => Err(ERR_STATE),
            InstanceState::Stopped => Err(ERR_CANCELLED),
        }
    }

    fn failed(&self, target: &Target, status: i32) {
        // The owner is told; a stopped one has nobody to tell.
        if let Ok(InstanceState::Running(lease)) = self.registry.state(target.owner) {
            lease.dropped(target.token, status);
        }
    }
}

/// Everything `Frame::compose` needs, borrowed for one call.
pub(crate) struct ComposeHost<'a, 'e> {
    pub(crate) handles: &'a UiHandles,
    pub(crate) guard: &'a dyn GuardState,
    pub(crate) registry: &'a NativeInstances,
    pub(crate) env: &'a Env<'e>,
    pub(crate) depth: &'a DispatchDepth,
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
        if !self.gate_open {
            return Err(ERR_UNSUPPORTED);
        }
        let caller = Caller {
            plugin: stamp.owner,
            callback: stamp.epoch,
            depth: self.depth.0.current(),
        };
        let draw = RegistryDraw {
            registry: self.registry,
            env: self.env,
        };
        ui_handles::compose(self.handles, self.guard, &draw, caller, request, canvas)
    }
}

#[cfg(test)]
#[path = "ui_compose_host_tests.rs"]
mod tests;
