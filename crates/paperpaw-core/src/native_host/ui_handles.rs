//! The handle table behind slot 35 and `core.ui` ops 4, 5, 14 and 15
//! (`docs/abi/ui.md` sections 5.1 to 5.3), and the compose algorithm that
//! uses it.
//!
//! Two kinds of handle:
//!
//! - **Widget instances** (op 4 makes, op 5 drops): kept until dropped, until
//!   the screen that made them closes, or until a plugin stops. At most 32
//!   device-wide, 16 on the X4, 16 bytes each.
//! - **Status handles** (ops 14 and 15 grant): valid only inside the callback
//!   that got them (section 5.3), so each carries the callback id of the
//!   granting plugin and is gone when that callback is.
//!
//! Plugins are named by small indices (`u16`, the host's plugin table), so a
//! made instance costs 16 bytes and no string. Everything is fallible: a
//! refusal is `ERR_FULL` or `ERR_NO_MEMORY`, never a panic, and ids come from
//! checked counters, never 0 and never reused while the host runs.
//!
//! The table decides; it never calls plugin code. [`compose`] is the whole of
//! `Frame::compose`: resolve the handle, then ask a [`ChildDraw`] (the
//! owner's `draw`, run by Core's dispatcher) to draw the child.
use super::registrations::DrawFrame;
use crate::abi7::context::{ComposeKind, ComposeRequest};
use alloc::vec::Vec;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND,
};

/// Most widget instances device-wide.
pub(crate) const MAX_WIDGETS: usize = 32;
/// Most widget instances on the X4.
pub(crate) const MAX_WIDGETS_X4: usize = 16;
/// Most status handles live at once (one op 14 asks for at most 8 items).
pub(crate) const MAX_STATUS: usize = 16;
/// Most status handles live at once on the X4.
pub(crate) const MAX_STATUS_X4: usize = 8;
/// Plugin indices are below this (the top bit of an owner marks `private`).
pub(crate) const MAX_PLUGIN: u16 = 0x8000;

const PRIVATE: u16 = 0x8000;

/// The two things the table needs to know about the stack's guard.
pub(crate) trait GuardState {
    /// The guard's plugin, if any instance guards.
    fn guard_plugin(&self) -> Option<u16>;
    /// Whether the screen instance lies below the topmost guard.
    fn below_guard(&self, screen_instance: u32) -> bool;
}

/// No guard: for tests and for a stack without one.
#[allow(dead_code, reason = "tests and the unguarded stack")]
pub(crate) struct NoGuard;
impl GuardState for NoGuard {
    fn guard_plugin(&self) -> Option<u16> {
        None
    }
    fn below_guard(&self, _: u32) -> bool {
        false
    }
}

/// A made widget instance: 16 bytes.
#[derive(Clone, Copy, Debug)]
struct Widget {
    id: u32,
    /// The screen instance that made it (its maker's), for the guard rule and
    /// for dropping it when the screen closes.
    screen: u32,
    token: u32,
    /// The owner's plugin index, `PRIVATE` set for a `private` widget.
    owner: u16,
    maker: u16,
}

/// A status handle, valid for one callback of `caller`.
#[derive(Clone, Copy, Debug)]
struct Status {
    handle: u32,
    token: u32,
    callback: u64,
    caller: u16,
    owner: u16,
    line: bool,
}

/// Who calls slot 35, and from where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    pub(crate) plugin: u16,
    /// The caller's current callback id (`EnvironmentScope::callback_id`).
    pub(crate) callback: u64,
    /// 1 for a draw the host called, 2 inside a composed child.
    pub(crate) depth: u8,
}

/// What a handle resolved to: the owner's plugin and its callback token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) owner: u16,
    pub(crate) token: u32,
}

/// The table.
#[derive(Debug)]
pub(crate) struct UiHandles {
    widgets: Vec<Widget>,
    status: Vec<Status>,
    next_widget: u32,
    next_status: u32,
    widget_cap: usize,
    status_cap: usize,
}

impl UiHandles {
    /// An empty table with the caps of the device (`x4`: the small ones).
    pub(crate) const fn new(x4: bool) -> Self {
        Self {
            widgets: Vec::new(),
            status: Vec::new(),
            next_widget: 1,
            next_status: 1,
            widget_cap: if x4 { MAX_WIDGETS_X4 } else { MAX_WIDGETS },
            status_cap: if x4 { MAX_STATUS_X4 } else { MAX_STATUS },
        }
    }

    /// Op 4: `maker`'s screen instance `screen` makes an instance of the
    /// widget `owner` declared with callback `token`. `ERR_FULL` at the cap
    /// or when the counter is spent, `ERR_NO_MEMORY` when the heap refuses,
    /// `ERR_INVALID` for an index out of range.
    pub(crate) fn make_widget(
        &mut self,
        owner: u16,
        maker: u16,
        screen: u32,
        token: u32,
        private: bool,
    ) -> Result<u32, i32> {
        if owner >= MAX_PLUGIN || maker >= MAX_PLUGIN || screen == 0 {
            return Err(ERR_INVALID);
        }
        if self.widgets.len() >= self.widget_cap {
            return Err(ERR_FULL);
        }
        let id = self.next_widget;
        let next = id.checked_add(1).ok_or(ERR_FULL)?;
        self.widgets.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        self.widgets.push(Widget {
            id,
            screen,
            token,
            owner: owner | if private { PRIVATE } else { 0 },
            maker,
        });
        self.next_widget = next;
        Ok(id)
    }

    fn widget(&self, id: u32) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == id)
    }

    /// Op 5: the instance's maker drops it. Another plugin's is `ERR_DENIED`,
    /// an unknown or already dropped one `ERR_NOT_FOUND`.
    pub(crate) fn drop_widget(&mut self, maker: u16, id: u32) -> Result<(), i32> {
        let at = self
            .widgets
            .iter()
            .position(|w| w.id == id)
            .ok_or(ERR_NOT_FOUND)?;
        if self.widgets[at].maker != maker {
            return Err(ERR_DENIED);
        }
        self.widgets.swap_remove(at);
        Ok(())
    }

    /// The screen instance closed: its widgets go (the owners get
    /// `widget_dropped`; the caller reads them first with
    /// [`Self::dropped_by_screen`]).
    pub(crate) fn drop_screen(&mut self, screen: u32) -> usize {
        let before = self.widgets.len();
        self.widgets.retain(|w| w.screen != screen);
        before - self.widgets.len()
    }

    /// `(instance, owner)` of the widgets `screen` made, to tell their owners.
    #[allow(dead_code, reason = "read by the applier when a screen closes")]
    pub(crate) fn dropped_by_screen(&self, screen: u32) -> impl Iterator<Item = (u32, u16)> + '_ {
        self.widgets
            .iter()
            .filter(move |w| w.screen == screen)
            .map(|w| (w.id, w.owner & !PRIVATE))
    }

    /// A plugin stopped: whatever it made or owns is gone, as are its status
    /// handles.
    pub(crate) fn drop_plugin(&mut self, plugin: u16) {
        self.widgets
            .retain(|w| w.maker != plugin && w.owner & !PRIVATE != plugin);
        self.status
            .retain(|s| s.caller != plugin && s.owner != plugin);
    }

    /// Ops 14 and 15: grants `caller` a handle for a drawn status item or line
    /// of `owner`, valid inside the caller's callback `callback` only.
    /// Handles of earlier callbacks of the caller are dropped first, so a
    /// caller never holds more than one callback's worth.
    pub(crate) fn grant_status(
        &mut self,
        caller: u16,
        callback: u64,
        owner: u16,
        token: u32,
        line: bool,
    ) -> Result<u32, i32> {
        if owner >= MAX_PLUGIN || caller >= MAX_PLUGIN {
            return Err(ERR_INVALID);
        }
        self.status
            .retain(|s| !(s.caller == caller && s.callback != callback));
        if self.status.len() >= self.status_cap {
            return Err(ERR_FULL);
        }
        let handle = self.next_status;
        let next = handle.checked_add(1).ok_or(ERR_FULL)?;
        self.status.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        self.status.push(Status {
            handle,
            token,
            callback,
            caller,
            owner,
            line,
        });
        self.next_status = next;
        Ok(handle)
    }

    /// A callback of `plugin` returned: its status handles end with it.
    pub(crate) fn end_callback(&mut self, plugin: u16, callback: u64) {
        self.status
            .retain(|s| !(s.caller == plugin && s.callback == callback));
    }

    /// Slot 35's lookup, in the contract's order (section 5.1): a call from a
    /// child (depth 2 or more) `ERR_BUSY`; an unknown, closed, expired or
    /// foreign handle `ERR_NOT_FOUND` (another plugin's handle is not
    /// revealed); a private widget or one made below a guard, for anyone but
    /// the guard's plugin, `ERR_DENIED`; the caller's own plugin `ERR_BUSY`.
    pub(crate) fn resolve(
        &self,
        caller: Caller,
        kind: ComposeKind,
        handle: u32,
        guard: &dyn GuardState,
    ) -> Result<Target, i32> {
        if caller.depth >= 2 {
            return Err(ERR_BUSY);
        }
        let guarded = guard.guard_plugin().is_some_and(|g| g != caller.plugin);
        let (target, hidden) = match kind {
            ComposeKind::Widget => {
                let w = self.widget(handle).ok_or(ERR_NOT_FOUND)?;
                if w.maker != caller.plugin {
                    return Err(ERR_NOT_FOUND);
                }
                let hidden = w.owner & PRIVATE != 0 || guard.below_guard(w.screen);
                (
                    Target {
                        owner: w.owner & !PRIVATE,
                        token: w.token,
                    },
                    hidden,
                )
            }
            ComposeKind::StatusItem | ComposeKind::StatusLine => {
                let line = kind == ComposeKind::StatusLine;
                let s = self
                    .status
                    .iter()
                    .find(|s| s.handle == handle && s.line == line)
                    .filter(|s| s.caller == caller.plugin && s.callback == caller.callback)
                    .ok_or(ERR_NOT_FOUND)?;
                (
                    Target {
                        owner: s.owner,
                        token: s.token,
                    },
                    false,
                )
            }
        };
        if guarded && hidden {
            return Err(ERR_DENIED);
        }
        if target.owner == caller.plugin {
            return Err(ERR_BUSY);
        }
        Ok(target)
    }

    /// Instances made now (for the heap gauge and tests).
    #[allow(dead_code, reason = "gauge")]
    pub(crate) fn widgets(&self) -> usize {
        self.widgets.len()
    }
}

/// Runs the owner's `draw`: Core's dispatcher implements it.
pub(crate) trait ChildDraw {
    /// Calls the target's `draw` with `frame` on `canvas` (already clipped to
    /// the visible part). The status is the callback's.
    fn draw(
        &self,
        target: &Target,
        frame: &DrawFrame<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32>;
    /// The child failed: log it (the owner is told `dropped`); nothing else.
    fn failed(&self, target: &Target, status: i32);
}

/// `Frame::compose` (section 5.1): resolve, then draw the child if anything is
/// visible. A child's failure is logged through [`ChildDraw::failed`] and the
/// call is `Ok`: the caller's frame stays.
pub(crate) fn compose(
    handles: &UiHandles,
    guard: &dyn GuardState,
    host: &dyn ChildDraw,
    caller: Caller,
    request: &ComposeRequest,
    canvas: &mut Canvas<'_>,
) -> Result<(), i32> {
    let target = handles.resolve(caller, request.kind, request.handle, guard)?;
    let Some(frame) = request.draw_frame() else {
        return Ok(());
    };
    match host.draw(&target, &frame, canvas) {
        Ok(0) => {}
        Ok(status) | Err(status) => host.failed(&target, status),
    }
    Ok(())
}

#[cfg(test)]
#[path = "ui_handles_tests.rs"]
mod tests;
