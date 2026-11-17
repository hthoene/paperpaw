//! What the kernel's screen stack holds: [`Layer`] (#244, slice D5).
//!
//! The kernel's own screens, the shells' and those of interface-7 plugins
//! are layers; the old SDK `Screen` trait is gone (slice D6b).

use alloc::boxed::Box;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{PointerEvent, Rect};
use paperpaw_sdk::{
    CloseCause, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Instances, Outside, Pane,
    Params, RefreshMode, SCREEN_REVISION, ShowsThrough,
};

/// A screen on the kernel's stack: the same calls, and the same answers when
/// it says nothing, as the old SDK `Screen` had.
pub trait Layer {
    fn revision(&self) -> u16 {
        SCREEN_REVISION
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>);

    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        let _ = env;
        None
    }

    fn grays(&self) -> bool {
        false
    }

    fn pictures_in_grays(&self) -> bool {
        true
    }

    fn dims_below(&self) -> bool {
        false
    }

    fn shows_through(&self) -> ShowsThrough {
        ShowsThrough::Nothing
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled;

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (gesture, cx);
        Handled::No
    }

    fn holds_edges(&self) -> bool {
        false
    }

    fn takes_double_taps(&self) -> bool {
        false
    }

    fn guards(&self) -> bool {
        false
    }

    fn secret(&self) -> bool {
        false
    }

    fn outside(&self) -> Outside {
        Outside::Close
    }

    fn passes_through(&self) -> bool {
        false
    }

    fn direction(&self) -> paperpaw_sdk::lang::Direction {
        paperpaw_sdk::lang::Direction::Auto
    }

    fn hints(&self) -> Option<HintVisibility> {
        None
    }

    fn on_close_request(&mut self, why: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (why, cx);
        Handled::No
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (event, cx);
        Handled::No
    }

    fn takes_buttons(&self) -> bool {
        false
    }

    fn on_button(&mut self, name: &str, long: bool, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (name, long, cx);
        Handled::No
    }

    fn mode(&self) -> paperpaw_sdk::realtime::ScreenMode {
        paperpaw_sdk::realtime::ScreenMode::new()
    }

    fn on_key(&mut self, key: &paperpaw_sdk::realtime::KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (key, cx);
        Handled::No
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        let _ = (action, env);
        Hint::Default
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let _ = cx;
    }

    fn on_reopen(&mut self, params: &Params, cx: &mut Cx<'_, '_>) {
        let _ = (params, cx);
    }

    fn on_squeeze(
        &mut self,
        squeeze: &paperpaw_sdk::realtime::Squeeze,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        let _ = (squeeze, cx);
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let _ = (event, params, cx);
    }

    fn on_hide(&mut self) {}

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        let _ = cx;
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let _ = cx;
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = cx;
        Handled::No
    }

    fn pane(&self) -> Pane {
        Pane::FULL
    }

    fn times_under_overlays(&self) -> bool {
        false
    }
}

/// What makes a layer a plugin added ([`LayerSpec`]): it may hold state.
pub type LayerFactory = Box<dyn Fn(&Env<'_>, &Params) -> Box<dyn Layer>>;

/// A screen a plugin adds that the kernel makes as a [`Layer`]
/// ([`Items::add_layer`](crate::Items::add_layer)): as the old SDK's
/// `ScreenSpec` (#244, slice D6a).
pub struct LayerSpec {
    pub id: Id,
    /// Translation key of the screen's display name.
    pub title: alloc::borrow::Cow<'static, str>,
    pub factory: LayerFactory,
    /// How the panel is refreshed when it opens, in place of the kernel's
    /// choice.
    pub refresh_on_open: Option<RefreshMode>,
    /// How many of it may be open at once.
    pub instances: Instances,
}

impl LayerSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(
        id: Id,
        title: impl Into<alloc::borrow::Cow<'static, str>>,
        factory: LayerFactory,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            factory,
            refresh_on_open: None,
            instances: Instances::PER_PARAMS,
        }
    }

    /// How the panel is refreshed when it opens.
    #[must_use]
    pub fn refresh_on_open(mut self, mode: RefreshMode) -> Self {
        self.refresh_on_open = Some(mode);
        self
    }

    /// How many of it may be open at once.
    #[must_use]
    pub fn instances(mut self, instances: Instances) -> Self {
        self.instances = instances;
        self
    }
}

impl core::fmt::Debug for LayerSpec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("LayerSpec").field(&self.id).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer saying `revision`, as one built before later items would.
    struct Built(u16);

    impl Layer for Built {
        fn revision(&self) -> u16 {
            self.0
        }

        fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

        fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
            Handled::No
        }

        fn pane(&self) -> Pane {
            Pane::DETAIL
        }
    }

    #[test]
    fn newer_items_are_called_only_on_layers_that_have_them() {
        use crate::screen::{Screen, call_since};
        let ask = |revision| {
            let mut screen = Screen::Layer(Box::new(Built(revision)));
            call_since(&mut screen, 2, Pane::FULL, |screen| screen.pane())
        };
        assert_eq!(ask(1), Pane::FULL, "built before: never asked");
        assert_eq!(ask(2), Pane::DETAIL);
        assert_eq!(ask(3), Pane::DETAIL);
    }

    #[test]
    fn layers_say_this_builds_revision() {
        struct Plain;
        impl Layer for Plain {
            fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
            fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
                Handled::No
            }
        }
        assert_eq!(Plain.revision(), SCREEN_REVISION);
        assert_eq!(Plain.pane(), Pane::FULL);
    }
}
