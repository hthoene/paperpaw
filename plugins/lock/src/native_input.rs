//! What an input does to the lock screen's steps (`Flow`): an action the host
//! resolved from a button, a tap, a finger on the pattern. Interface 6's
//! `on_action`, `on_gesture` and `on_pointer`, made free of the kernel. While
//! unlocking, everything but what passes the lock (sleeping, refreshing the
//! screen, night mode) is the lock's, and while the next try has to wait
//! nothing but *Back* is anything.
use paperpaw_sdk::native::{Error, callbacks::PointerPhase};
use paperpaw_sdk::{Point, Rect};

use crate::native_flow::{Context, Effect, Effects, Flow, Lockstate, PATTERN_OK, Step};
use crate::native_pad::Layout;

/// What passes the lock: a locked device still sleeps, and its screen can be
/// refreshed and turned dark.
const PASSING: [&str; 3] = [
    "core.sleep",
    "core.refresh-screen",
    "core.toggle-night-mode",
];

/// What an action does to the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// The lock screen's to handle (the host does nothing with it).
    Taken,
    /// Not the lock screen's: the host goes on with it.
    Passed,
    /// Where the kinds are listed: the list decides.
    List,
}

/// One step of the focus: `delta` of 1 or -1 goes round past either end;
/// larger steps (a row of keys) stop at the end.
#[must_use]
pub fn step_focus(focus: usize, delta: isize, len: usize) -> usize {
    let Some(last) = len.checked_sub(1) else {
        return 0;
    };
    let target = isize::try_from(focus)
        .unwrap_or(isize::MAX)
        .saturating_add(delta);
    let last_i = isize::try_from(last).unwrap_or(isize::MAX);
    if delta.abs() == 1 && target < 0 {
        last
    } else if delta.abs() == 1 && target > last_i {
        0
    } else {
        usize::try_from(target.clamp(0, last_i)).unwrap_or(0)
    }
}

impl Flow {
    /// An action. [`Act::List`] where the screen lists the kinds: the list
    /// takes the action then, unless it is not for it (*Home* and the rest still
    /// work while setting up: it locks nothing).
    ///
    /// # Errors
    /// As [`Flow::submit`].
    pub fn act(
        &mut self,
        action: &str,
        lock: &mut Lockstate,
        ctx: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<Act, Error> {
        if PASSING.contains(&action) {
            return Ok(Act::Passed);
        }
        if self.released(lock, ctx, fx) {
            return Ok(Act::Taken);
        }
        if action == "core.back" {
            self.back(fx);
            return Ok(Act::Taken);
        }
        if Self::waiting(lock, ctx.now_ms) {
            return Ok(Act::Taken);
        }
        if self.step() == Step::Choose {
            return Ok(Act::List);
        }
        let delta: isize = match action {
            "core.left" | "core.previous" => -1,
            "core.right" | "core.next" => 1,
            // A row up or down; page buttons where there are no others.
            "core.up" | "core.prev-page" => -3,
            "core.down" | "core.next-page" => 3,
            "core.confirm" => {
                if self.shown {
                    self.choose(lock, ctx, fx)?;
                } else {
                    self.shown = true;
                    fx.push(Effect::Redraw);
                }
                return Ok(Act::Taken);
            }
            // Locked, every other action is the lock's too.
            _ if self.step() == Step::Unlock => return Ok(Act::Taken),
            _ => return Ok(Act::Passed),
        };
        if self.shown {
            self.focus = step_focus(self.focus, delta, self.targets(ctx.touch));
        }
        self.shown = true;
        fx.push(Effect::Redraw);
        Ok(Act::Taken)
    }

    /// A tap at `at`, where `layout` and the `back` arrow were drawn:
    /// everything is taken. `Act::List` where the kinds are listed.
    ///
    /// # Errors
    /// As [`Flow::submit`].
    pub fn tap(
        &mut self,
        at: Point,
        drawn: (&Layout, Option<Rect>),
        lock: &mut Lockstate,
        ctx: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<Act, Error> {
        if self.released(lock, ctx, fx) || Self::waiting(lock, ctx.now_ms) {
            return Ok(Act::Taken);
        }
        let (layout, back) = drawn;
        if back.is_some_and(|rect| rect.contains(at)) {
            fx.push(Effect::Back);
            return Ok(Act::Taken);
        }
        if self.step() == Step::Choose {
            return Ok(Act::List);
        }
        self.shown = false;
        match self.kind() {
            "pattern" => {}
            "password" => {
                if layout.password.contains(at) {
                    self.tapped_password(fx);
                }
            }
            _ => {
                if let Some(key) = layout.key_at(at) {
                    self.press(key, lock, ctx, fx)?;
                }
            }
        }
        Ok(Act::Taken)
    }

    /// A finger draws a pattern from dot to dot. Whether it was for the
    /// screen: not while the kinds are listed, nor for another kind, nor while
    /// the next try waits.
    ///
    /// # Errors
    /// As [`Flow::submit`].
    pub fn pointer(
        &mut self,
        phase: PointerPhase,
        id: u32,
        at: Point,
        layout: &Layout,
        lock: &mut Lockstate,
        ctx: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<bool, Error> {
        if self.kind() != "pattern"
            || self.step() == Step::Choose
            || Self::waiting(lock, ctx.now_ms)
        {
            return Ok(false);
        }
        let dot = layout.dot_at(at);
        match phase {
            PointerPhase::Down => self.finger_down(id, at, dot, fx),
            PointerPhase::Move => self.finger_moved(id, at, dot, fx).map(|()| true),
            PointerPhase::Up => self.finger_up(lock, ctx, fx).map(|()| true),
            _ => {
                self.finger_cancelled(fx);
                Ok(true)
            }
        }
    }
}

/// The pattern's *OK* where there is no touch is the tenth target.
const _: () = assert!(PATTERN_OK == 9);
