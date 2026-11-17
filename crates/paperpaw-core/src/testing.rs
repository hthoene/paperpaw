//! Screens of tests (#293): a test's screen is a native screen, its plugin
//! the test itself. The kernel calls it as it calls every plugin's screen
//! (`NativeScreen`, the wire records of `docs/abi/screens-input.md`), and
//! the test's [`Fake`] hears what the records say: its actions, gestures,
//! pointers and keys while it is on top, that it is shown and closed. What
//! it does goes to the kernel through the call's context, as a plugin's
//! commands come through its stage. Only for tests (feature `testing`).
use crate::{Place, Turn};
use alloc::borrow::Cow;
use alloc::boxed::Box;
use core::cell::RefCell;

use crate::native_host::registrations::{self as wire, Record};
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Rect;
use paperpaw_platform::{Instant, Point, PointerEvent, PointerPhase, PointerTool};
use paperpaw_sdk::realtime::ScreenMode;
use paperpaw_sdk::{
    CloseCause, Direction, Gesture, GestureKind, Handled, Hint, HintVisibility, Id, Instances,
    Params, RefreshMode, ScreenKey,
};

/// It guards the device (`screen.spec.flags` bit 1).
pub const GUARDS: u32 = 1 << 1;
/// An overlay over the screen below, which shows through (bit 2).
pub const OVERLAY: u32 = 1 << 2;
/// What it shows is secret (bit 4).
pub const SECRET: u32 = 1 << 4;
/// It holds the edge swipes (bit 5).
pub const HOLDS_EDGES: u32 = 1 << 5;
/// No row of hints under it (bit 8).
pub const HINTS_HIDDEN: u32 = crate::native_host::declarations::SCREEN_HINTS_HIDDEN;
/// Best in grays (bit 9).
pub const GRAYS: u32 = crate::native_host::declarations::SCREEN_GRAYS;
/// It dims what is below (bit 10).
pub const DIMS_BELOW: u32 = crate::native_host::declarations::SCREEN_DIMS_BELOW;

/// What a test's screen does; each call says nothing, by default.
pub trait Fake {
    /// Its declaration's flags ([`GUARDS`], [`OVERLAY`] …), read once as it
    /// is made, with those of its [`FakeSpec`].
    fn flags(&self) -> u32 {
        0
    }
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
        let _ = (canvas, env);
    }
    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        let _ = (action, cx);
        Handled::No
    }
    /// A gesture that ended at `gesture.end` (where it started is not told).
    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Turn<'_, '_>) -> Handled {
        let _ = (gesture, cx);
        Handled::No
    }
    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Turn<'_, '_>) -> Handled {
        let _ = (event, cx);
        Handled::No
    }
    /// A key going down (`pressed`) or up at `at`, before it is an action.
    fn on_key(&mut self, name: &str, pressed: bool, at: Instant, cx: &mut Turn<'_, '_>) -> Handled {
        let _ = (name, pressed, at, cx);
        Handled::No
    }
    /// It is shown on top (again, or at another size).
    fn on_show(&mut self, cx: &mut Turn<'_, '_>) {
        let _ = cx;
    }
    /// It was opened again with `params`.
    fn on_reopen(&mut self, params: &Params, cx: &mut Turn<'_, '_>) {
        let _ = (params, cx);
    }
    fn on_close(&mut self, cx: &mut Turn<'_, '_>) {
        let _ = cx;
    }
    /// Text typed into it, the last part `done`.
    fn on_text(&mut self, text: &str, done: bool, cx: &mut Turn<'_, '_>) {
        let _ = (text, done, cx);
    }
    // What only the host's own screens have, for the kernel's paths they
    // take: a part of the screen of its own, a time, its hints, a close
    // it keeps from.
    /// The part of its area it covers, an overlay (its declaration's
    /// [`OVERLAY`] covers all of it).
    fn region(&self, env: &Place<'_>) -> Option<Rect> {
        let _ = env;
        None
    }
    /// Its time came ([`Turn::wake_at`]).
    fn on_timer(&mut self, cx: &mut Turn<'_, '_>) {
        let _ = cx;
    }
    /// Whether the row of hints shows under it.
    fn hints(&self) -> Option<HintVisibility> {
        None
    }
    /// The hint of `action`.
    fn hint(&self, action: &Id) -> Option<Hint> {
        let _ = action;
        None
    }
    /// It is asked to close: [`Handled::Yes`] keeps it open.
    fn on_close_request(&mut self, why: CloseCause, cx: &mut Turn<'_, '_>) -> Handled {
        let _ = (why, cx);
        Handled::No
    }
    /// How it runs (a native screen's: [`ScreenMode::raw_keys`]).
    fn mode(&self) -> Option<ScreenMode> {
        None
    }
    /// An event emitted to the screens.
    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Turn<'_, '_>) {
        let _ = (event, params, cx);
    }
}

/// What the kernel asks of a test's screen beside its records, through its
/// native screen (`Dispatch::test_screen`).
pub trait TestScreen {
    fn region(&self, env: &Place<'_>) -> Option<Rect>;
    fn on_timer(&self, cx: &mut Turn<'_, '_>);
    fn hints(&self) -> Option<HintVisibility>;
    fn hint(&self, action: &Id) -> Option<Hint>;
    fn on_close_request(&self, why: CloseCause, cx: &mut Turn<'_, '_>) -> Handled;
    fn mode(&self) -> Option<ScreenMode>;
    fn on_event(&self, event: &Id, params: &Params, cx: &mut Turn<'_, '_>);
    fn on_reopen(&self, params: &Params, cx: &mut Turn<'_, '_>);
}

impl TestScreen for FakeInstance {
    fn region(&self, env: &Place<'_>) -> Option<Rect> {
        self.0.try_borrow().ok()?.region(env)
    }
    fn on_timer(&self, cx: &mut Turn<'_, '_>) {
        if let Ok(mut fake) = self.0.try_borrow_mut() {
            fake.on_timer(cx);
        }
    }
    fn hints(&self) -> Option<HintVisibility> {
        self.0.try_borrow().ok()?.hints()
    }
    fn hint(&self, action: &Id) -> Option<Hint> {
        self.0.try_borrow().ok()?.hint(action)
    }
    fn on_close_request(&self, why: CloseCause, cx: &mut Turn<'_, '_>) -> Handled {
        self.0
            .try_borrow_mut()
            .map_or(Handled::No, |mut fake| fake.on_close_request(why, cx))
    }
    fn mode(&self) -> Option<ScreenMode> {
        self.0.try_borrow().ok()?.mode()
    }
    fn on_event(&self, event: &Id, params: &Params, cx: &mut Turn<'_, '_>) {
        if let Ok(mut fake) = self.0.try_borrow_mut() {
            fake.on_event(event, params, cx);
        }
    }
    fn on_reopen(&self, params: &Params, cx: &mut Turn<'_, '_>) {
        if let Ok(mut fake) = self.0.try_borrow_mut() {
            fake.on_reopen(params, cx);
        }
    }
}

/// What makes a test's screen with the params it is opened with.
pub type FakeFactory = Box<dyn Fn(&Place<'_>, &Params) -> Box<dyn Fake>>;

/// A test's screen to add ([`Items::add_fake`](crate::Items::add_fake)).
pub struct FakeSpec {
    pub(crate) id: Id,
    pub(crate) title: Cow<'static, str>,
    pub(crate) factory: FakeFactory,
    pub(crate) flags: u32,
    pub(crate) refresh_on_open: Option<RefreshMode>,
    pub(crate) instances: Instances,
}

impl FakeSpec {
    /// Screen `id`, called `title`, made by `factory`.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, factory: FakeFactory) -> Self {
        Self {
            id,
            title: title.into(),
            factory,
            flags: 0,
            refresh_on_open: None,
            instances: Instances::PER_PARAMS,
        }
    }

    /// With the declaration's flags ([`GUARDS`], [`OVERLAY`] …).
    #[must_use]
    pub fn flags(mut self, flags: u32) -> Self {
        self.flags = flags;
        self
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

impl FakeSpec {
    pub(crate) fn id(&self) -> &Id {
        &self.id
    }
}

impl core::fmt::Debug for FakeSpec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("FakeSpec").field(&self.id).finish()
    }
}

/// The plugin of one opened test screen.
pub(crate) struct FakeInstance(RefCell<Box<dyn Fake>>);

impl FakeInstance {
    pub(crate) fn new(fake: Box<dyn Fake>) -> Self {
        Self(RefCell::new(fake))
    }

    fn call(&self, bytes: &[u8], cx: &mut Turn<'_, '_>) -> Result<i32, i32> {
        let record = wire::records_decode_single(bytes).map_err(|_| ERR_INVALID)?;
        // A screen of the test calls nothing of the kernel back: a call that
        // comes while it runs is not taken.
        let mut fake = self.0.try_borrow_mut().map_err(|_| ERR_INVALID)?;
        let handled = match record {
            Record::InputAction(input) => {
                let action = Id::try_parse(input.action).map_err(|_| ERR_INVALID)?;
                fake.on_action(&action, cx)
            }
            Record::InputGesture(input) => {
                let kind = match input.kind {
                    0 => GestureKind::Tap,
                    1 => GestureKind::LongPress,
                    2 => GestureKind::Swipe(Direction::Left),
                    3 => GestureKind::Swipe(Direction::Right),
                    4 => GestureKind::Swipe(Direction::Up),
                    5 => GestureKind::Swipe(Direction::Down),
                    _ => return Err(ERR_INVALID),
                };
                let at = Point::new(input.x, input.y);
                fake.on_gesture(Gesture::new(kind, at, at, PointerTool::Finger), cx)
            }
            Record::InputPointer(input) => {
                let phase = match input.phase {
                    0 => PointerPhase::Down,
                    1 => PointerPhase::Move,
                    2 => PointerPhase::Up,
                    _ => PointerPhase::Cancel,
                };
                let id = input
                    .contact
                    .and_then(|c| u8::try_from(c).ok())
                    .unwrap_or(0);
                let event = PointerEvent::new(
                    id,
                    phase,
                    Point::new(input.x, input.y),
                    PointerTool::Finger,
                    0,
                );
                fake.on_pointer(&event, cx)
            }
            Record::InputKey(input) => {
                fake.on_key(input.key, input.state == 0, Instant(input.now_ms), cx)
            }
            Record::EventScreenShown(_) => {
                fake.on_show(cx);
                Handled::No
            }
            Record::EventScreenOpened(_) => Handled::No,
            Record::EventScreenClosed(_) => {
                fake.on_close(cx);
                Handled::No
            }
            Record::EventTextTyped(typed) => {
                fake.on_text(typed.text, typed.flags & 1 != 0, cx);
                Handled::No
            }
            _ => Handled::No,
        };
        Ok(i32::from(handled == Handled::Yes))
    }
}

impl crate::native_host::native_screen::Dispatch for FakeInstance {
    fn event(&self, _: u32, _: &[u8], _: &Place<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &Place<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Place<'_>,
        _: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn screen_callback(
        &self,
        _: Option<ScreenKey>,
        _: u32,
        bytes: &[u8],
        cx: &mut Turn<'_, '_>,
        _: bool,
    ) -> Result<i32, i32> {
        self.call(bytes, cx)
    }
    fn screen_draw(
        &self,
        _: Option<ScreenKey>,
        _: u32,
        _: &wire::DrawFrame<'_>,
        env: &Place<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        let mut fake = self.0.try_borrow_mut().map_err(|_| ERR_INVALID)?;
        fake.draw(canvas, env);
        Ok(OK)
    }
    fn dropped(&self, _: u32, _: i32) {}
    fn test_screen(&self, visit: &mut dyn FnMut(&dyn TestScreen)) -> bool {
        visit(self);
        true
    }
}

impl crate::native_host::native_instances::NativeInstance for FakeInstance {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
}
