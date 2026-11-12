//! The handle table behind slot 35 and `core.ui` ops 4, 5, 14 and 15
//! (`docs/abi/ui.md` sections 5.1 to 5.3), and the compose algorithm that
//! uses it.
//!
//! Two kinds of handle:
//!
//! - **Widget instances** (op 4 makes, op 5 drops): kept until dropped, until
//!   the screen that made them closes, or until a plugin stops. At most 32
//!   device-wide, 16 on the X4, 32 bytes each.
//! - **Status handles** (ops 14 and 15 grant): valid only inside the callback
//!   that got them (section 5.3), so each carries the callback id of the
//!   granting plugin and is gone when that callback is.
//!
//! Memory: each table is one allocation of its whole cap, made at its first
//! insert and kept (no growth, no realloc, so no old-plus-new peak): widgets
//! 32 B x 32 = 1,024 B (x 16 = 512 B on the X4), status handles 40 B x 16 =
//! 640 B (x 8 = 320 B on the X4), and the drops to tell 16 B x 32 = 512 B (x 16 =
//! 256 B on the X4); on the X4 at most 1,088 B once all three are used.
//!
//! Plugins are named by their full-width host owner id (`u64`, the one
//! `OwnerResources` id, never reused, no second namespace), so a made
//! instance costs 32 bytes and no string. Everything is fallible: a
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
/// The two things the table needs to know about the stack's guard.
pub(crate) trait GuardState {
    /// The guard's plugin, if any instance guards.
    fn guard_plugin(&self) -> Option<u64>;
    /// Whether the screen instance lies below the topmost guard.
    fn below_guard(&self, screen_instance: u32) -> bool;
    /// Whether the screen instance is still on the stack. A reader that does
    /// not know says so (`true`): only a delivery between callbacks asks.
    fn on_stack(&self, _screen_instance: u32) -> bool {
        true
    }
}

/// No guard: for tests and for a stack without one.
#[allow(dead_code, reason = "tests and the unguarded stack")]
pub(crate) struct NoGuard;
impl GuardState for NoGuard {
    fn guard_plugin(&self) -> Option<u64> {
        None
    }
    fn below_guard(&self, _: u32) -> bool {
        false
    }
}

/// A made widget instance: 32 bytes.
#[derive(Clone, Copy, Debug)]
struct Widget {
    id: u32,
    /// The screen instance that made it (its maker's), for the guard rule and
    /// for dropping it when the screen closes.
    screen: u32,
    token: u32,
    /// The owner's host owner id.
    owner: u64,
    maker: u64,
    /// The widget is `private` (ui.md 4.3).
    private: bool,
}

/// A dropped widget instance its owner has yet to hear of (`event.widget_dropped`,
/// 85, at the owner's token): 16 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Dropped {
    /// The owner's host owner id.
    pub(crate) owner: u64,
    /// The widget's callback token of the owner.
    pub(crate) token: u32,
    pub(crate) instance: u32,
}

impl Widget {
    fn notice(&self) -> Dropped {
        Dropped {
            owner: self.owner,
            token: self.token,
            instance: self.id,
        }
    }
}

/// A status handle, valid for one callback of `caller`.
#[derive(Clone, Copy, Debug)]
struct Status {
    handle: u32,
    token: u32,
    callback: u64,
    caller: u64,
    owner: u64,
    line: bool,
}

/// Who calls slot 35, and from where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    pub(crate) plugin: u64,
    /// The caller's current callback id (`EnvironmentScope::callback_id`).
    pub(crate) callback: u64,
    /// 1 for a draw the host called, 2 inside a composed child.
    pub(crate) depth: u8,
}

/// A widget instance as its maker's layout ops see it (`widget_of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WidgetRef {
    pub(crate) owner: u64,
    pub(crate) token: u32,
    /// The maker's screen instance that made it.
    pub(crate) screen: u32,
    pub(crate) private: bool,
}

/// What a handle resolved to: the owner's plugin and its callback token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) owner: u64,
    pub(crate) token: u32,
}

/// The table.
#[derive(Debug)]
pub(crate) struct UiHandles {
    widgets: Vec<Widget>,
    /// Drops the owners have not been told of yet: never lost, so while live
    /// instances and these fill the cap new instances are refused (`ERR_FULL`)
    /// until Core delivers some.
    dropped: Vec<Dropped>,
    /// Instances their owners said show something new
    /// (`command.widget_changed`), told to the makers by `event.widget_changed`
    /// (86): a hint, one entry per instance. Room for every live instance is
    /// taken with the instance ([`Self::make_widget`]), so marking never
    /// allocates.
    changed: Vec<u32>,
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
            dropped: Vec::new(),
            changed: Vec::new(),
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
    /// `ERR_INVALID` for screen 0.
    pub(crate) fn make_widget(
        &mut self,
        owner: u64,
        maker: u64,
        screen: u32,
        token: u32,
        private: bool,
    ) -> Result<u32, i32> {
        if screen == 0 {
            return Err(ERR_INVALID);
        }
        // Live instances and undelivered drops share the cap, so the drops
        // always fit the one allocation below.
        if self.widgets.len() + self.dropped.len() >= self.widget_cap {
            return Err(ERR_FULL);
        }
        let id = self.next_widget;
        let next = id.checked_add(1).ok_or(ERR_FULL)?;
        reserve(&mut self.widgets, self.widget_cap)?;
        // Room for every notice there can be, taken once: the drop paths
        // (op 5, a screen close, a stopped maker) never allocate, so a drop
        // cannot fail for the heap and no notice is lost.
        reserve(&mut self.dropped, self.widget_cap)?;
        reserve(&mut self.changed, self.widget_cap)?;
        self.widgets.push(Widget {
            id,
            screen,
            token,
            owner,
            maker,
            private,
        });
        self.next_widget = next;
        Ok(id)
    }

    /// Notices that fit without allocating.
    #[cfg(test)]
    pub(crate) fn dropped_room(&self) -> usize {
        self.dropped.capacity() - self.dropped.len()
    }

    fn widget(&self, id: u32) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == id)
    }

    /// Whether `owner` may say instance `id` changed: `ERR_NOT_FOUND` for an
    /// unknown or dropped instance, `ERR_DENIED` for another plugin's widget.
    pub(crate) fn check_changed(&self, owner: u64, id: u32) -> Result<(), i32> {
        match self.widget(id) {
            None => Err(ERR_NOT_FOUND),
            Some(w) if w.owner != owner => Err(ERR_DENIED),
            Some(_) => Ok(()),
        }
    }

    /// `owner` says instance `id` shows something new: marked once however
    /// often it is said before the maker is told. An instance that is gone or
    /// not `owner`'s is ignored (it was checked at submit; a drop may have
    /// come since). Allocation-free.
    pub(crate) fn mark_changed(&mut self, owner: u64, id: u32) {
        if self.check_changed(owner, id).is_err() || self.changed.contains(&id) {
            return;
        }
        if self.changed.len() >= self.changed.capacity() {
            // Marks of instances dropped since: nobody is told of those.
            let live = &self.widgets;
            self.changed.retain(|id| live.iter().any(|w| w.id == *id));
        }
        if self.changed.len() < self.changed.capacity() {
            self.changed.push(id);
        }
    }

    /// The oldest change of a live instance not yet told, as (the maker, the
    /// instance), taken off the list: told once, never retried (a hint).
    pub(crate) fn take_changed(&mut self) -> Option<(u64, u32)> {
        while !self.changed.is_empty() {
            let id = self.changed.remove(0);
            if let Some(w) = self.widget(id) {
                return Some((w.maker, id));
            }
        }
        None
    }

    /// Whether a change waits.
    pub(crate) fn has_changed(&self) -> bool {
        !self.changed.is_empty()
    }

    /// Op 5: the instance's maker drops it. Another plugin's is `ERR_DENIED`,
    /// an unknown or already dropped one `ERR_NOT_FOUND`.
    pub(crate) fn drop_widget(&mut self, maker: u64, id: u32) -> Result<(), i32> {
        let at = self
            .widgets
            .iter()
            .position(|w| w.id == id)
            .ok_or(ERR_NOT_FOUND)?;
        if self.widgets[at].maker != maker {
            return Err(ERR_DENIED);
        }
        // Room for the notice was taken when the instance was made.
        let gone = self.widgets.swap_remove(at);
        self.dropped.push(gone.notice());
        Ok(())
    }

    /// Whether instance `id` is still made (not dropped).
    pub(crate) fn has_widget(&self, id: u32) -> bool {
        self.widget(id).is_some()
    }

    /// An instance for ops 6, 7, 12 and 13, for its maker only: instance 0 (an
    /// own widget, op 4's `flags` bit 0) `ERR_BUSY` (the plugin lays out and
    /// draws its own), an unknown or dropped one `ERR_NOT_FOUND`, another
    /// plugin's `ERR_DENIED` (as op 5).
    pub(crate) fn widget_of(&self, maker: u64, id: u32) -> Result<WidgetRef, i32> {
        if id == 0 {
            return Err(ERR_BUSY);
        }
        let widget = self.widget(id).ok_or(ERR_NOT_FOUND)?;
        if widget.maker != maker {
            return Err(ERR_DENIED);
        }
        Ok(WidgetRef {
            owner: widget.owner,
            token: widget.token,
            screen: widget.screen,
            private: widget.private,
        })
    }

    /// The screen instance closed: its widgets go and their owners are to be
    /// told ([`Self::next_dropped`]). Allocation-free (see [`Self::make_widget`]).
    pub(crate) fn drop_screen(&mut self, screen: u32) -> usize {
        let before = self.widgets.len();
        let dropped = &mut self.dropped;
        self.widgets.retain(|w| {
            let gone = w.screen == screen;
            if gone {
                dropped.push(w.notice());
            }
            !gone
        });
        before - self.widgets.len()
    }

    /// The first drop `owner` has not been told of. It stays listed until
    /// [`Self::dropped_delivered`] is given it, which ends it at the first
    /// delivery attempt whatever the outcome (returned, failed or trapped:
    /// `ui.md` 8.2), and at once when the owner stops ([`Self::drop_plugin`]).
    /// A dead owner never holds a place of the cap.
    pub(crate) fn next_dropped(&self, owner: u64) -> Option<Dropped> {
        self.dropped.iter().find(|d| d.owner == owner).copied()
    }

    /// The owner of the oldest drop not yet told: the next to hear of one.
    pub(crate) fn next_dropped_owner(&self) -> Option<u64> {
        self.dropped.first().map(|d| d.owner)
    }

    /// `notice` was attempted (not "succeeded"): it leaves the list.
    pub(crate) fn dropped_delivered(&mut self, notice: &Dropped) {
        if let Some(at) = self.dropped.iter().position(|d| d == notice) {
            self.dropped.remove(at);
        }
    }

    /// The lowest screen above `after` that still has widgets (the sweep of
    /// closed screens whose cleanup was refused).
    pub(crate) fn next_screen(&self, after: u32) -> Option<u32> {
        self.widgets
            .iter()
            .map(|w| w.screen)
            .filter(|screen| *screen > after)
            .min()
    }

    /// Whether `plugin` made or owns a widget or a status handle, or has a
    /// drop notice waiting (#248, D3).
    pub(crate) fn plugin_holds(&self, plugin: u64) -> bool {
        self.dropped.iter().any(|d| d.owner == plugin)
            || self
                .widgets
                .iter()
                .any(|w| w.maker == plugin || w.owner == plugin)
            || self
                .status
                .iter()
                .any(|s| s.caller == plugin || s.owner == plugin)
    }

    /// A plugin stopped: whatever it made or owns is gone, as are its status
    /// handles and what it was yet to be told. The owners of what it made are
    /// to be told. Allocation-free (see [`Self::make_widget`]), so nothing is
    /// lost to the heap.
    pub(crate) fn drop_plugin(&mut self, plugin: u64) {
        self.dropped.retain(|d| d.owner != plugin);
        let dropped = &mut self.dropped;
        self.widgets.retain(|w| {
            let gone = w.maker == plugin || w.owner == plugin;
            if gone && w.owner != plugin {
                dropped.push(w.notice());
            }
            !gone
        });
        self.status
            .retain(|s| s.caller != plugin && s.owner != plugin);
    }

    /// Ops 14 and 15: grants `caller` a handle for a drawn status item or line
    /// of `owner`, valid inside the caller's callback `callback` only.
    /// Handles of earlier callbacks of the caller are dropped first, so a
    /// caller never holds more than one callback's worth.
    pub(crate) fn grant_status(
        &mut self,
        caller: u64,
        callback: u64,
        owner: u64,
        token: u32,
        line: bool,
    ) -> Result<u32, i32> {
        self.status
            .retain(|s| !(s.caller == caller && s.callback != callback));
        if self.status.len() >= self.status_cap {
            return Err(ERR_FULL);
        }
        let handle = self.next_status;
        let next = handle.checked_add(1).ok_or(ERR_FULL)?;
        reserve(&mut self.status, self.status_cap)?;
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
    pub(crate) fn end_callback(&mut self, plugin: u64, callback: u64) {
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
                let hidden = w.private || guard.below_guard(w.screen);
                (
                    Target {
                        owner: w.owner,
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

    /// Status handles granted now (tests).
    #[cfg(test)]
    pub(crate) fn statuses(&self) -> usize {
        self.status.len()
    }

    /// Instances made now (for the heap gauge and tests).
    #[allow(dead_code, reason = "gauge")]
    pub(crate) fn widgets(&self) -> usize {
        self.widgets.len()
    }
}

/// Makes room for the whole cap in one allocation, the first time a table is
/// used: the table then never grows or reallocates, so there is no moment
/// when an old and a new buffer exist together, and a later insert cannot
/// fail for memory. `ERR_NO_MEMORY` when the heap refuses, nothing changed.
fn reserve<T>(items: &mut Vec<T>, cap: usize) -> Result<(), i32> {
    if items.capacity() < cap {
        items
            .try_reserve_exact(cap - items.len())
            .map_err(|_| ERR_NO_MEMORY)?;
    }
    Ok(())
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
    draw_target(host, target, request, canvas)
}

/// The second half of [`compose`]: draw the resolved `target` if anything is
/// visible, and log a child's failure. Callers that must not hold the table
/// across the child's `draw` resolve first and call this after releasing it.
pub(crate) fn draw_target(
    host: &dyn ChildDraw,
    target: Target,
    request: &ComposeRequest,
    canvas: &mut Canvas<'_>,
) -> Result<(), i32> {
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
