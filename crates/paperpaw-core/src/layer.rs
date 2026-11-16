//! What the kernel's screen stack holds: [`Layer`] (#244, slice D5).
//!
//! The kernel's own screens and those of interface-7 plugins are layers;
//! a screen of the old SDK [`Screen`] trait is held in [`Old`], its adapter,
//! until the last one is gone (slice D6).

use alloc::boxed::Box;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{PointerEvent, Rect};
use paperpaw_sdk::{
    CloseCause, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Instances, Outside, Pane,
    Params, RefreshMode, SCREEN_REVISION, Screen, ShowsThrough, TitleEntry,
};

/// A screen on the kernel's stack: the same calls, and the same answers when
/// it says nothing, as the old [`Screen`].
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

/// What makes a [`KernelScreen`] with the params it is opened with.
pub type KernelFactory = fn(&Env<'_>, &Params) -> Box<dyn Layer>;

/// A screen kept in the program that the kernel makes as a [`Layer`]
/// ([`Items::add_kernel_screens`](crate::Items::add_kernel_screens)): the
/// kernel's own, and those of the shells (#244, slice D5b).
pub struct KernelScreen {
    entry: TitleEntry,
    factory: KernelFactory,
    refresh_on_open: Option<RefreshMode>,
    instances: Instances,
}

impl KernelScreen {
    /// Screen `id`, called `title` (a translation key), made by `factory`.
    #[must_use]
    pub const fn new(id: Id, title: &'static str, factory: KernelFactory) -> Self {
        Self {
            entry: TitleEntry::new(id, title),
            factory,
            refresh_on_open: None,
            instances: Instances::PER_PARAMS,
        }
    }

    /// How the panel is refreshed when it opens.
    #[must_use]
    pub const fn refresh_on_open(mut self, mode: RefreshMode) -> Self {
        self.refresh_on_open = Some(mode);
        self
    }

    /// How many of it may be open at once.
    #[must_use]
    pub const fn instances(mut self, instances: Instances) -> Self {
        self.instances = instances;
        self
    }

    #[must_use]
    pub const fn id(&self) -> &Id {
        &self.entry.id
    }

    /// Its id and the translation key of its name.
    #[must_use]
    pub const fn entry(&self) -> &TitleEntry {
        &self.entry
    }

    pub(crate) fn make(&self, env: &Env<'_>, params: &Params) -> Box<dyn Layer> {
        (self.factory)(env, params)
    }

    pub(crate) const fn refresh(&self) -> Option<RefreshMode> {
        self.refresh_on_open
    }

    pub(crate) const fn how_many(&self) -> Instances {
        self.instances
    }
}

impl core::fmt::Debug for KernelScreen {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("KernelScreen").field(&self.entry.id).finish()
    }
}

/// An old [`Screen`] on the stack.
pub(crate) struct Old(pub(crate) Box<dyn Screen>);

impl Layer for Old {
    fn revision(&self) -> u16 {
        self.0.revision()
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.0.draw(canvas, env)
    }

    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        self.0.region(env)
    }

    fn grays(&self) -> bool {
        self.0.grays()
    }

    fn pictures_in_grays(&self) -> bool {
        self.0.pictures_in_grays()
    }

    fn dims_below(&self) -> bool {
        self.0.dims_below()
    }

    fn shows_through(&self) -> ShowsThrough {
        self.0.shows_through()
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_action(action, cx)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_gesture(gesture, cx)
    }

    fn holds_edges(&self) -> bool {
        self.0.holds_edges()
    }

    fn takes_double_taps(&self) -> bool {
        self.0.takes_double_taps()
    }

    fn guards(&self) -> bool {
        self.0.guards()
    }

    fn secret(&self) -> bool {
        self.0.secret()
    }

    fn outside(&self) -> Outside {
        self.0.outside()
    }

    fn passes_through(&self) -> bool {
        self.0.passes_through()
    }

    fn direction(&self) -> paperpaw_sdk::lang::Direction {
        self.0.direction()
    }

    fn hints(&self) -> Option<HintVisibility> {
        self.0.hints()
    }

    fn on_close_request(&mut self, why: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_close_request(why, cx)
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_pointer(event, cx)
    }

    fn takes_buttons(&self) -> bool {
        self.0.takes_buttons()
    }

    fn on_button(&mut self, name: &str, long: bool, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_button(name, long, cx)
    }

    fn mode(&self) -> paperpaw_sdk::realtime::ScreenMode {
        self.0.mode()
    }

    fn on_key(&mut self, key: &paperpaw_sdk::realtime::KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_key(key, cx)
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        self.0.hint(action, env)
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.0.on_show(cx)
    }

    fn on_reopen(&mut self, params: &Params, cx: &mut Cx<'_, '_>) {
        self.0.on_reopen(params, cx)
    }

    fn on_squeeze(
        &mut self,
        squeeze: &paperpaw_sdk::realtime::Squeeze,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        self.0.on_squeeze(squeeze, cx)
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        self.0.on_event(event, params, cx)
    }

    fn on_hide(&mut self) {
        self.0.on_hide()
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        self.0.on_close(cx)
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.0.on_timer(cx)
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        // Reached through `call_since` on this layer: its revision is the held screen's.
        self.0.on_resize(cx)
    }

    fn pane(&self) -> Pane {
        // Reached through `call_since` on this layer: its revision is the held screen's.
        self.0.pane()
    }

    fn times_under_overlays(&self) -> bool {
        // Reached through `call_since` on this layer: its revision is the held screen's.
        self.0.times_under_overlays()
    }
}

/// `screen` as a layer.
pub(crate) fn old(screen: Box<dyn Screen>) -> Box<dyn Layer> {
    Box::new(Old(screen))
}

/// `call` when `layer` is of `revision` or later, else `older`.
pub(crate) fn call_since<T>(
    layer: &mut dyn Layer,
    revision: u16,
    older: T,
    call: impl FnOnce(&mut dyn Layer) -> T,
) -> T {
    if layer.revision() >= revision {
        call(layer)
    } else {
        older
    }
}

/// A layer held as an old [`Screen`], for a test fixture that registers one
/// through an old [`paperpaw_sdk::ScreenSpec`].
#[cfg(test)]
pub(crate) struct AsOld<L>(pub(crate) L);

#[cfg(test)]
impl<L: Layer> Screen for AsOld<L> {
    fn revision(&self) -> u16 {
        self.0.revision()
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.0.draw(canvas, env)
    }

    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        self.0.region(env)
    }

    fn grays(&self) -> bool {
        self.0.grays()
    }

    fn pictures_in_grays(&self) -> bool {
        self.0.pictures_in_grays()
    }

    fn dims_below(&self) -> bool {
        self.0.dims_below()
    }

    fn shows_through(&self) -> ShowsThrough {
        self.0.shows_through()
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_action(action, cx)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_gesture(gesture, cx)
    }

    fn holds_edges(&self) -> bool {
        self.0.holds_edges()
    }

    fn takes_double_taps(&self) -> bool {
        self.0.takes_double_taps()
    }

    fn guards(&self) -> bool {
        self.0.guards()
    }

    fn secret(&self) -> bool {
        self.0.secret()
    }

    fn outside(&self) -> Outside {
        self.0.outside()
    }

    fn passes_through(&self) -> bool {
        self.0.passes_through()
    }

    fn direction(&self) -> paperpaw_sdk::lang::Direction {
        self.0.direction()
    }

    fn hints(&self) -> Option<HintVisibility> {
        self.0.hints()
    }

    fn on_close_request(&mut self, why: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_close_request(why, cx)
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_pointer(event, cx)
    }

    fn takes_buttons(&self) -> bool {
        self.0.takes_buttons()
    }

    fn on_button(&mut self, name: &str, long: bool, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_button(name, long, cx)
    }

    fn mode(&self) -> paperpaw_sdk::realtime::ScreenMode {
        self.0.mode()
    }

    fn on_key(&mut self, key: &paperpaw_sdk::realtime::KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_key(key, cx)
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        self.0.hint(action, env)
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.0.on_show(cx)
    }

    fn on_reopen(&mut self, params: &Params, cx: &mut Cx<'_, '_>) {
        self.0.on_reopen(params, cx)
    }

    fn on_squeeze(
        &mut self,
        squeeze: &paperpaw_sdk::realtime::Squeeze,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        self.0.on_squeeze(squeeze, cx)
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        self.0.on_event(event, params, cx)
    }

    fn on_hide(&mut self) {
        self.0.on_hide()
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        self.0.on_close(cx)
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.0.on_timer(cx)
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_resize(cx)
    }

    fn pane(&self) -> Pane {
        self.0.pane()
    }

    fn times_under_overlays(&self) -> bool {
        self.0.times_under_overlays()
    }
}
