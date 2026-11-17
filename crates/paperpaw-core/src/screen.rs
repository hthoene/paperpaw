//! What the kernel's screen stack holds (#293, Decision 209): a plugin's
//! native screen or one of the host's own ([`HostScreen`]).
use alloc::boxed::Box;

use crate::host_screen::HostScreen;
use crate::native_host::{native_instances::NativeLease, native_screen::NativeScreen};
use paperpaw_graphics::Canvas;
use paperpaw_platform::{PointerEvent, Rect};
use paperpaw_sdk::{
    CloseCause, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Params, ShowsThrough,
};

/// A screen on the kernel's stack.
pub(crate) enum Screen {
    /// A plugin's, of interface 7.
    Native(Box<NativeScreen<NativeLease>>),
    /// The host's own.
    Host(HostScreen),
}

impl Screen {
    /// The screen that holds a place while it is made anew.
    pub(crate) const GONE: Self = Self::Host(HostScreen::Gone);

    pub(crate) fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        match self {
            Self::Native(s) => s.draw(canvas, env),
            Self::Host(h) => h.draw(canvas, env),
        }
    }

    /// The part of its area it covers, an overlay; `None` for all of it.
    pub(crate) fn region(&self, env: &Env<'_>) -> Option<Rect> {
        match self {
            Self::Native(s) => s.region(env),
            Self::Host(h) => h.region(),
        }
    }

    pub(crate) fn grays(&self) -> bool {
        match self {
            Self::Native(s) => s.grays(),
            Self::Host(_) => false,
        }
    }

    pub(crate) fn dims_below(&self) -> bool {
        match self {
            Self::Native(s) => s.dims_below(),
            Self::Host(h) => h.dims_below(),
        }
    }

    pub(crate) fn shows_through(&self) -> ShowsThrough {
        match self {
            Self::Native(s) => s.shows_through(),
            Self::Host(_) => ShowsThrough::Nothing,
        }
    }

    pub(crate) fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Native(s) => s.on_action(action, cx),
            Self::Host(h) => h.on_action(action, cx),
        }
    }

    pub(crate) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Native(s) => s.on_gesture(gesture, cx),
            Self::Host(h) => h.on_gesture(gesture, cx),
        }
    }

    pub(crate) fn holds_edges(&self) -> bool {
        match self {
            Self::Native(s) => s.holds_edges(),
            Self::Host(h) => h.holds_edges(),
        }
    }

    pub(crate) fn guards(&self) -> bool {
        match self {
            Self::Native(s) => s.guards(),
            Self::Host(h) => h.guards(),
        }
    }

    pub(crate) fn secret(&self) -> bool {
        match self {
            Self::Native(s) => s.secret(),
            Self::Host(_) => false,
        }
    }

    pub(crate) fn hints(&self) -> Option<HintVisibility> {
        match self {
            Self::Native(s) => s.hints(),
            Self::Host(h) => h.hints(),
        }
    }

    pub(crate) fn on_close_request(&mut self, why: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Native(s) => s.on_close_request(why, cx),
            Self::Host(h) => h.on_close_request(why, cx),
        }
    }

    pub(crate) fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Native(s) => s.on_pointer(event, cx),
            Self::Host(h) => h.on_pointer(event),
        }
    }

    pub(crate) fn mode(&self) -> paperpaw_sdk::realtime::ScreenMode {
        match self {
            Self::Native(s) => s.mode(),
            Self::Host(_) => paperpaw_sdk::realtime::ScreenMode::new(),
        }
    }

    pub(crate) fn on_key(
        &mut self,
        key: &paperpaw_sdk::realtime::KeyEvent,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        match self {
            Self::Native(s) => s.on_key(key, cx),
            Self::Host(_) => Handled::No,
        }
    }

    pub(crate) fn hint(&self, action: &Id) -> Hint {
        match self {
            Self::Native(s) => s.hint(action),
            Self::Host(h) => h.hint(action),
        }
    }

    pub(crate) fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        match self {
            Self::Native(s) => s.on_show(cx),
            Self::Host(h) => h.on_show(cx),
        }
    }

    pub(crate) fn on_reopen(&mut self, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Self::Native(s) = self {
            s.on_reopen(params, cx);
        }
    }

    pub(crate) fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Self::Native(s) = self {
            s.on_event(event, params, cx);
        }
    }

    pub(crate) fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        if let Self::Native(s) = self {
            s.on_close(cx);
        }
    }

    pub(crate) fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        match self {
            Self::Native(s) => s.on_timer(cx),
            Self::Host(h) => h.on_timer(cx),
        }
    }

    pub(crate) fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        match self {
            Self::Native(s) => s.on_resize(cx),
            Self::Host(_) => Handled::No,
        }
    }

    /// Where it is kept on the heap, to tell one from another (tests).
    #[cfg(test)]
    pub(crate) fn addr(&self) -> *const u8 {
        match self {
            Self::Native(s) => core::ptr::from_ref(&**s).cast(),
            Self::Host(HostScreen::Notice(n)) => core::ptr::from_ref(&**n).cast(),
            Self::Host(_) => core::ptr::null(),
        }
    }
}
