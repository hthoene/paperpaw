//! Host screen routing for one native declaration token.
use super::registrations::{self as wire, Record};
use crate::native_host::codec;
use crate::native_host::ui_store::Info;
#[cfg(test)]
use alloc::rc::Rc;
use alloc::vec::Vec;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_platform::{PointerEvent, PointerPhase, Rect, buttons};
use paperpaw_sdk::{
    Cx, Env, Gesture, GestureKind, Handled, Id, Screen, ScreenKey,
    builtin::{events, slots},
    event::EventData as _,
    realtime,
};

/// The most text one `type_text` call hands to a screen (64 KiB: 64 events of
/// 1,024 bytes); the contract fixes only the event's own 1,024.
const TYPED_TOTAL_MAX: usize = 64 * 1024;

/// The owner/runtime supplies admission and the callback-scoped host views.
/// Implementations keep no input, frame, canvas or environment borrow.
pub(crate) trait Dispatch {
    /// What the `screen_opened` now sent says of the hand-over its open
    /// named, if the kernel set it (none for an open without one).
    fn take_open_info(&self) -> Option<Info> {
        None
    }
    fn event(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32>;
    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32>;
    fn draw(
        &self,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32>;
    /// Screen wrappers supply their own key, never a plugin-provided value.
    fn screen_callback(
        &self,
        instance: Option<ScreenKey>,
        token: u32,
        bytes: &[u8],
        env: &Env<'_>,
        input: bool,
    ) -> Result<i32, i32> {
        let _ = instance;
        if input {
            self.input(token, bytes, env)
        } else {
            self.event(token, bytes, env)
        }
    }
    fn screen_draw(
        &self,
        instance: Option<ScreenKey>,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        let _ = instance;
        self.draw(token, frame, env, canvas)
    }
    fn stage(
        &self,
        _token: u32,
        _bytes: &[u8],
        _out: &mut [u8],
        _env: &Env<'_>,
    ) -> Result<i32, i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
    /// Whether the kernel holds the instance `key` secret now (its run-time
    /// policy, `command.screen_policy`, included; a missing row counts as
    /// secret), `None` where the dispatcher cannot tell: the screen's own
    /// declaration then decides.
    fn secret_now(&self, key: Option<ScreenKey>) -> Option<bool> {
        let _ = key;
        None
    }
    fn dropped(&self, token: u32, status: i32);
}

#[cfg(test)]
impl<T: Dispatch + ?Sized> Dispatch for Rc<T> {
    fn take_open_info(&self) -> Option<Info> {
        (**self).take_open_info()
    }
    fn event(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        (**self).event(token, bytes, env)
    }
    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        (**self).input(token, bytes, env)
    }
    fn draw(
        &self,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        (**self).draw(token, frame, env, canvas)
    }
    fn screen_callback(
        &self,
        instance: Option<ScreenKey>,
        token: u32,
        bytes: &[u8],
        env: &Env<'_>,
        input: bool,
    ) -> Result<i32, i32> {
        (**self).screen_callback(instance, token, bytes, env, input)
    }
    fn screen_draw(
        &self,
        instance: Option<ScreenKey>,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        (**self).screen_draw(instance, token, frame, env, canvas)
    }
    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], env: &Env<'_>) -> Result<i32, i32> {
        (**self).stage(token, bytes, out, env)
    }
    fn secret_now(&self, key: Option<ScreenKey>) -> Option<bool> {
        (**self).secret_now(key)
    }
    fn dropped(&self, token: u32, status: i32) {
        (**self).dropped(token, status);
    }
}

impl<T: Dispatch> Dispatch for super::shared::Shared<T> {
    fn take_open_info(&self) -> Option<Info> {
        (**self).take_open_info()
    }
    fn event(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        (**self).event(token, bytes, env)
    }
    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        (**self).input(token, bytes, env)
    }
    fn draw(
        &self,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        (**self).draw(token, frame, env, canvas)
    }
    fn screen_callback(
        &self,
        instance: Option<ScreenKey>,
        token: u32,
        bytes: &[u8],
        env: &Env<'_>,
        input: bool,
    ) -> Result<i32, i32> {
        (**self).screen_callback(instance, token, bytes, env, input)
    }
    fn screen_draw(
        &self,
        instance: Option<ScreenKey>,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        (**self).screen_draw(instance, token, frame, env, canvas)
    }
    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], env: &Env<'_>) -> Result<i32, i32> {
        (**self).stage(token, bytes, out, env)
    }
    fn secret_now(&self, key: Option<ScreenKey>) -> Option<bool> {
        (**self).secret_now(key)
    }
    fn dropped(&self, token: u32, status: i32) {
        (**self).dropped(token, status);
    }
}

/// Where the screen `key` is on `stack` when nothing but overlays lies above it
/// (none, for the top one): the one rule by which such a screen is drawn as if on
/// top (`ui.md` 1.2) and may redraw (`redraw_screen`), so the two never differ.
pub(crate) fn only_overlays_above(
    stack: &[paperpaw_sdk::StackEntry],
    key: ScreenKey,
) -> Option<usize> {
    let at = stack.iter().rposition(|entry| entry.key == key)?;
    stack[at + 1..]
        .iter()
        .all(|entry| entry.overlay)
        .then_some(at)
}

/// Where the screen `key` of `owner` is on `stack` when it may redraw: only
/// overlays above it, and none of them guards (a guard hides what is below it,
/// `ui.md` 4.3, an overlay or not). `None` otherwise: a full screen or a guard
/// above, or not the owner's.
pub(crate) fn redrawable(
    stack: &[paperpaw_sdk::StackEntry],
    key: ScreenKey,
    owner: Option<&str>,
) -> Option<usize> {
    let at = only_overlays_above(stack, key)?;
    let mine = owner.is_none_or(|owner| stack[at].screen.namespace() == owner);
    (mine && !stack[at + 1..].iter().any(|entry| entry.guards)).then_some(at)
}

pub(crate) struct NativeScreen<D> {
    id: Id,
    token: u32,
    dispatch: D,
    key: Option<ScreenKey>,
    flags: u32,
    opened: bool,
    params_count: u32,
}

impl<D: Dispatch> NativeScreen<D> {
    pub(crate) fn new(id: Id, token: u32, dispatch: D) -> Self {
        Self {
            id,
            token,
            dispatch,
            key: None,
            flags: 0,
            opened: false,
            params_count: 0,
        }
    }

    pub(crate) fn with_flags(mut self, flags: u32) -> Self {
        self.flags = flags;
        self
    }

    pub(crate) fn with_params_count(mut self, count: u32) -> Self {
        self.params_count = count;
        self
    }

    fn shown(&self, env: &Env<'_>) -> bool {
        let pane = env.content();
        let record = Record::EventScreenShown(wire::EventScreenShown {
            now_ms: env.now().millis(),
            screen: self.id.as_str(),
            width: u32::from(pane.width),
            height: u32::from(pane.height),
            instance: self.key.map(ScreenKey::serial),
            last_input: match env.last_input() {
                paperpaw_sdk::keys::InputMode::Buttons => 1,
                paperpaw_sdk::keys::InputMode::Touch => 2,
                paperpaw_sdk::keys::InputMode::Pen => 3,
                _ => 0,
            },
        });
        let mut bytes = [0; 192];
        let result = codec::encode_batch(&[record], &mut bytes).and_then(|len| {
            self.dispatch
                .screen_callback(self.key, self.token, &bytes[..len], env, false)
        });
        self.status(result)
    }

    fn opened(&self, again: bool, info: Info, env: &Env<'_>) {
        let Some(key) = self.key else {
            return;
        };
        let slot = env
            .stack()
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.slot.as_ref())
            .map_or("", Id::as_str);
        self.event(
            Record::EventScreenOpened(wire::EventScreenOpened {
                now_ms: env.now().millis(),
                screen: self.id.as_str(),
                instance: key.serial(),
                slot,
                flags: u32::from(again),
                handover: info.handover,
                params: info.params,
                text_bytes: info.text_bytes,
            }),
            env,
        );
    }

    fn event(&self, record: Record<'_>, env: &Env<'_>) {
        let mut bytes = [0; 384];
        let result = codec::encode_batch(&[record], &mut bytes).and_then(|len| {
            self.dispatch
                .screen_callback(self.key, self.token, &bytes[..len], env, false)
        });
        self.status(result);
    }

    /// Sends `text` as `event.text_typed` records of at most 1,024 bytes,
    /// cut between characters, `done` only on the last; nothing for no text
    /// without `done`. At most [`TYPED_TOTAL_MAX`] bytes per call are taken:
    /// more is dropped whole (not cut, and never confirmed), so a long paste
    /// cannot cause unbounded callbacks. The buffer is on the heap (the X4's
    /// frames are short), as large as one chunk needs, fallibly reserved
    /// (a refusal drops the screen's token) and wiped before it is freed (the
    /// text may be a password).
    fn typed(&self, text: &str, done: bool, env: &Env<'_>) {
        const MAX: usize = wire::EVENT_TEXT_TYPED_TEXT_MAX_BYTES;
        // Tag, length and fixed fields around the text.
        const FRAME: usize = 64;
        if text.len() > TYPED_TOTAL_MAX {
            return;
        }
        let size = text.len().min(MAX) + FRAME;
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(size).is_err() {
            self.dispatch.dropped(self.token, ERR_NO_MEMORY);
            return;
        }
        bytes.resize(size, 0);
        self.typed_chunks(text, done, env, &mut bytes);
        crate::native_host::secret_bytes::wipe(&mut bytes);
    }

    fn typed_chunks(&self, text: &str, done: bool, env: &Env<'_>, bytes: &mut [u8]) {
        const MAX: usize = wire::EVENT_TEXT_TYPED_TEXT_MAX_BYTES;
        let mut rest = text;
        loop {
            let mut cut = rest.len().min(MAX);
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            let (chunk, tail) = rest.split_at(cut);
            if chunk.is_empty() && !done {
                return;
            }
            let record = Record::EventTextTyped(wire::EventTextTyped {
                now_ms: env.now().millis(),
                text: chunk,
                flags: u32::from(done && tail.is_empty()),
            });
            let result = codec::encode_batch(&[record], bytes).and_then(|len| {
                self.dispatch
                    .screen_callback(self.key, self.token, &bytes[..len], env, false)
            });
            if !self.status(result) || tail.is_empty() {
                return;
            }
            rest = tail;
        }
    }

    fn status(&self, result: Result<i32, i32>) -> bool {
        match result {
            Ok(0) => true,
            Ok(status) | Err(status) => {
                self.dispatch
                    .dropped(self.token, if status < 0 { status } else { ERR_INVALID });
                false
            }
        }
    }

    fn is_top(&self, cx: &Cx<'_, '_>) -> bool {
        self.key.is_some() && self.key == cx.key() && cx.is_on_top()
    }

    fn input(&self, record: Record<'_>, env: &Env<'_>) -> Handled {
        let mut bytes = [0; 192];
        let result = codec::encode_batch(&[record], &mut bytes).and_then(|len| {
            self.dispatch
                .screen_callback(self.key, self.token, &bytes[..len], env, true)
        });
        match result {
            Ok(0) => Handled::No,
            Ok(1) => Handled::Yes,
            Ok(status) | Err(status) => {
                self.dispatch
                    .dropped(self.token, if status < 0 { status } else { ERR_INVALID });
                Handled::No
            }
        }
    }

    /// Whether the kernel's draw of this screen is to reach the plugin: it is on
    /// the stack, and the screens above it (none, for the top one) are all
    /// overlays. Under an overlay of another plugin a screen that is secret now
    /// (the kernel's run-time answer, a guard included), or that is on a stack
    /// with a guard (it guards, lies above one, or is under one), is not drawn:
    /// its area stays blank, so the sleep screen over a locked device never
    /// shows the page or the PIN screen.
    fn shown_below(&self, env: &Env<'_>) -> bool {
        let Some(key) = self.key else {
            return false;
        };
        let stack = env.stack();
        let Some(at) = only_overlays_above(stack, key) else {
            return false;
        };
        let above = &stack[at + 1..];
        let secret = self
            .dispatch
            .secret_now(self.key)
            .unwrap_or_else(|| self.secret());
        let guarded = secret || stack.iter().any(|entry| entry.guards);
        let foreign = above
            .iter()
            .any(|entry| entry.screen.namespace() != self.id.namespace());
        !(guarded && foreign)
    }

    fn draw_visible(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let pane = env.content();
        let pane = Rect::new(0, 0, pane.width, pane.height);
        let Some(clip) = pane.intersection(&canvas.clip()) else {
            return;
        };
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        let top = i32::from(clip.y);
        let frame = super::frames::screen(
            0,
            0,
            u32::from(pane.width),
            u32::from(pane.height),
            (top, top + i32::from(clip.height)),
        );
        let result =
            self.dispatch
                .screen_draw(self.key, self.token, &frame, env, &mut canvas.clipped(pane));
        self.status(result);
    }
}

impl<D: Dispatch> Screen for NativeScreen<D> {
    fn guards(&self) -> bool {
        self.flags & (1 << 1) != 0
    }
    fn secret(&self) -> bool {
        self.guards() || self.flags & (1 << 4) != 0
    }
    fn holds_edges(&self) -> bool {
        self.flags & (1 << 5) != 0
    }
    fn hints(&self) -> Option<paperpaw_sdk::HintVisibility> {
        // Declared hidden (`screen.spec.flags` bit 8); else as its plugin said.
        (self.flags & super::declarations::SCREEN_HINTS_HIDDEN != 0)
            .then_some(paperpaw_sdk::HintVisibility::Hidden)
    }
    fn grays(&self) -> bool {
        // Declared (`screen.spec.flags` bit 9); the run-time policy of
        // `ActiveScreen::grays` takes the place of it once one is set.
        self.flags & super::declarations::SCREEN_GRAYS != 0
    }
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        (self.flags & (1 << 2) != 0)
            .then(|| Rect::new(0, 0, env.content().width, env.content().height))
    }
    fn shows_through(&self) -> paperpaw_sdk::ShowsThrough {
        if self.flags & (1 << 2) != 0 {
            paperpaw_sdk::ShowsThrough::All
        } else {
            paperpaw_sdk::ShowsThrough::Nothing
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        // Only the top screen has input, but one below overlays only is drawn as if
        // it were on top, the overlays after it (`ui.md` 1.2, bit 2 `overlay`): the
        // page under a menu or a dialog shows. Instance keys are compared, since
        // several instances may have the same id.
        if self.shown_below(env) {
            self.draw_visible(canvas, env);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.key = cx.key();
        if !self.opened && self.key.is_some() {
            self.opened = true;
            let info = Info {
                params: self.params_count,
                ..Info::NONE
            };
            self.opened(false, info, cx.env());
        }
        self.shown(cx.env());
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        self.shown(cx.env());
        // The host owns the viewport; native instances are never remade for size.
        Handled::Yes
    }

    fn on_reopen(&mut self, params: &paperpaw_sdk::Params, cx: &mut Cx<'_, '_>) {
        let Ok(count) = u32::try_from(params.iter().count()) else {
            self.dispatch.dropped(self.token, ERR_INVALID);
            return;
        };
        // The open's hand-over, if it named one, comes from the kernel.
        let info = self.dispatch.take_open_info().unwrap_or(Info {
            params: count,
            ..Info::NONE
        });
        self.params_count = info.params;
        let again = self.opened;
        self.key = cx.key();
        self.opened = self.key.is_some();
        self.opened(again, info, cx.env());
        if again && self.is_top(cx) {
            self.shown(cx.env());
        }
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        if self.opened {
            self.opened = false;
            if let Some(key) = self.key {
                self.event(
                    Record::EventScreenClosed(wire::EventScreenClosed {
                        now_ms: cx.env().now().millis(),
                        screen: self.id.as_str(),
                        instance: key.serial(),
                    }),
                    cx.env(),
                );
            }
        }
    }

    /// Text typed from outside (`core.type-text`, wire `event.text_typed`)
    /// goes only to this screen on top when it is the one offered on slot
    /// `core.keyboard`; no other event is passed on (`screens-input.md`
    /// section 2).
    fn on_event(&mut self, event: &Id, params: &paperpaw_sdk::Params, cx: &mut Cx<'_, '_>) {
        if *event != events::TYPE_TEXT
            || !self.is_top(cx)
            || cx.env().slot_screen(slots::KEYBOARD.as_str()) != Some(&self.id)
        {
            return;
        }
        let Some(typed) = events::TypeText::from_params(params) else {
            return;
        };
        self.typed(&typed.text, typed.done.unwrap_or(false), cx.env());
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.is_top(cx) {
            return Handled::No;
        }
        self.input(
            Record::InputAction(wire::InputAction {
                now_ms: cx.env().now().millis(),
                action: action.as_str(),
            }),
            cx.env(),
        )
    }

    fn mode(&self) -> realtime::ScreenMode {
        realtime::ScreenMode::new().raw_keys()
    }

    fn on_key(&mut self, key: &realtime::KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.is_top(cx) || key.name() == buttons::POWER {
            return Handled::No;
        }
        self.input(
            Record::InputKey(wire::InputKey {
                now_ms: key.at().millis(),
                key: key.name(),
                state: u32::from(!key.is_press()),
            }),
            cx.env(),
        )
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.is_top(cx) {
            return Handled::No;
        }
        let phase = match event.phase {
            PointerPhase::Down => 0,
            PointerPhase::Move => 1,
            PointerPhase::Up => 2,
            PointerPhase::Cancel => 3,
            _ => return Handled::No,
        };
        self.input(
            Record::InputPointer(wire::InputPointer {
                now_ms: cx.env().now().millis(),
                x: event.point.x,
                y: event.point.y,
                phase,
                // Tells simultaneous contacts apart (interface 6 `PointerEvent::id`).
                contact: Some(u32::from(event.id)),
            }),
            cx.env(),
        )
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.is_top(cx) {
            return Handled::No;
        }
        let kind = match gesture.kind {
            GestureKind::Tap => 0,
            GestureKind::LongPress => 1,
            GestureKind::Swipe(direction) => match direction {
                paperpaw_sdk::Direction::Left => 2,
                paperpaw_sdk::Direction::Right => 3,
                paperpaw_sdk::Direction::Up => 4,
                paperpaw_sdk::Direction::Down => 5,
                _ => return Handled::No,
            },
            _ => return Handled::No,
        };
        self.input(
            Record::InputGesture(wire::InputGesture {
                now_ms: cx.env().now().millis(),
                kind,
                x: gesture.end.x,
                y: gesture.end.y,
            }),
            cx.env(),
        )
    }
}

#[cfg(test)]
#[path = "native_screen_tests.rs"]
mod tests;
