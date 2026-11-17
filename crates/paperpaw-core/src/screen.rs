//! What the kernel's screen stack holds (#293, Decision 209): a plugin's
//! native screen or one of the host's own ([`HostScreen`]). Until the last
//! slice of #293 the shells' and the tests' screens are layers too.
use alloc::boxed::Box;

use crate::Layer;
use crate::host_screen::HostScreen;
use crate::native_host::{native_instances::NativeLease, native_screen::NativeScreen};
use paperpaw_graphics::Canvas;
use paperpaw_platform::{PointerEvent, Rect};
use paperpaw_sdk::{
    CloseCause, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Outside, Pane, Params,
    SCREEN_REVISION, ShowsThrough,
};

/// A screen on the kernel's stack.
pub(crate) enum Screen {
    /// A plugin's, of interface 7.
    Native(Box<NativeScreen<NativeLease>>),
    /// The host's own.
    Host(HostScreen),
    /// A shell's or a test's.
    Layer(Box<dyn Layer>),
}

/// `$call` on the screen: the same call on a native screen and a layer, the
/// host's own answer for its screens.
macro_rules! each {
    ($self:ident, $s:ident => $call:expr, $h:ident => $host:expr) => {
        match $self {
            Screen::Native($s) => $call,
            Screen::Layer($s) => $call,
            Screen::Host($h) => $host,
        }
    };
}

impl Screen {
    /// The screen that holds a place while it is made anew.
    pub(crate) const GONE: Self = Self::Host(HostScreen::Gone);

    pub(crate) fn revision(&self) -> u16 {
        each!(self, s => s.revision(), _h => SCREEN_REVISION)
    }

    pub(crate) fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        each!(self, s => s.draw(canvas, env), h => h.draw(canvas, env));
    }

    pub(crate) fn region(&self, env: &Env<'_>) -> Option<Rect> {
        each!(self, s => s.region(env), h => h.region())
    }

    pub(crate) fn grays(&self) -> bool {
        each!(self, s => s.grays(), _h => false)
    }

    pub(crate) fn pictures_in_grays(&self) -> bool {
        each!(self, s => s.pictures_in_grays(), _h => true)
    }

    pub(crate) fn dims_below(&self) -> bool {
        each!(self, s => s.dims_below(), h => h.dims_below())
    }

    pub(crate) fn shows_through(&self) -> ShowsThrough {
        each!(self, s => s.shows_through(), _h => ShowsThrough::Nothing)
    }

    pub(crate) fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        each!(self, s => s.on_action(action, cx), h => h.on_action(action, cx))
    }

    pub(crate) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        each!(self, s => s.on_gesture(gesture, cx), h => h.on_gesture(gesture, cx))
    }

    pub(crate) fn holds_edges(&self) -> bool {
        each!(self, s => s.holds_edges(), h => h.holds_edges())
    }

    pub(crate) fn takes_double_taps(&self) -> bool {
        each!(self, s => s.takes_double_taps(), _h => false)
    }

    pub(crate) fn guards(&self) -> bool {
        each!(self, s => s.guards(), h => h.guards())
    }

    pub(crate) fn secret(&self) -> bool {
        each!(self, s => s.secret(), _h => false)
    }

    pub(crate) fn outside(&self) -> Outside {
        each!(self, s => s.outside(), _h => Outside::Close)
    }

    pub(crate) fn passes_through(&self) -> bool {
        each!(self, s => s.passes_through(), _h => false)
    }

    pub(crate) fn direction(&self) -> paperpaw_sdk::lang::Direction {
        each!(self, s => s.direction(), _h => paperpaw_sdk::lang::Direction::Auto)
    }

    pub(crate) fn hints(&self) -> Option<HintVisibility> {
        each!(self, s => s.hints(), h => h.hints())
    }

    pub(crate) fn on_close_request(&mut self, why: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        each!(self, s => s.on_close_request(why, cx), h => h.on_close_request(why, cx))
    }

    pub(crate) fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        each!(self, s => s.on_pointer(event, cx), h => h.on_pointer(event))
    }

    pub(crate) fn takes_buttons(&self) -> bool {
        each!(self, s => s.takes_buttons(), _h => false)
    }

    pub(crate) fn on_button(&mut self, name: &str, long: bool, cx: &mut Cx<'_, '_>) -> Handled {
        each!(self, s => s.on_button(name, long, cx), _h => Handled::No)
    }

    pub(crate) fn mode(&self) -> paperpaw_sdk::realtime::ScreenMode {
        each!(self, s => s.mode(), _h => paperpaw_sdk::realtime::ScreenMode::new())
    }

    pub(crate) fn on_key(
        &mut self,
        key: &paperpaw_sdk::realtime::KeyEvent,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        each!(self, s => s.on_key(key, cx), _h => Handled::No)
    }

    pub(crate) fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        each!(self, s => s.hint(action, env), h => h.hint(action))
    }

    pub(crate) fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        each!(self, s => s.on_show(cx), h => h.on_show(cx));
    }

    pub(crate) fn on_reopen(&mut self, params: &Params, cx: &mut Cx<'_, '_>) {
        each!(self, s => s.on_reopen(params, cx), _h => {});
    }

    pub(crate) fn on_squeeze(
        &mut self,
        squeeze: &paperpaw_sdk::realtime::Squeeze,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        each!(self, s => s.on_squeeze(squeeze, cx), _h => Handled::No)
    }

    pub(crate) fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        each!(self, s => s.on_event(event, params, cx), _h => {});
    }

    pub(crate) fn on_hide(&mut self) {
        each!(self, s => s.on_hide(), _h => {});
    }

    pub(crate) fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        each!(self, s => s.on_close(cx), _h => {});
    }

    pub(crate) fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        each!(self, s => s.on_timer(cx), h => h.on_timer(cx));
    }

    // The three items newer than revision 1: the kernel asks them only
    // through `call_since`.
    pub(crate) fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        each!(self, s => Layer::on_resize(&mut **s, cx), _h => Handled::No)
    }

    pub(crate) fn pane(&self) -> Pane {
        each!(self, s => Layer::pane(&**s), _h => Pane::FULL)
    }

    pub(crate) fn times_under_overlays(&self) -> bool {
        each!(self, s => Layer::times_under_overlays(&**s), _h => false)
    }

    /// Where it is kept on the heap, to tell one from another (tests).
    #[cfg(test)]
    pub(crate) fn addr(&self) -> *const u8 {
        match self {
            Self::Native(s) => core::ptr::from_ref(&**s).cast(),
            Self::Layer(s) => core::ptr::from_ref(&**s).cast(),
            Self::Host(HostScreen::Notice(n)) => core::ptr::from_ref(&**n).cast(),
            Self::Host(_) => core::ptr::null(),
        }
    }
}

/// `call` when `screen` is of `revision` or later, else `older`.
pub(crate) fn call_since<T>(
    screen: &mut Screen,
    revision: u16,
    older: T,
    call: impl FnOnce(&mut Screen) -> T,
) -> T {
    if screen.revision() >= revision {
        call(screen)
    } else {
        older
    }
}
