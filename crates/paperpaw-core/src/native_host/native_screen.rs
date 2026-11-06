//! Host screen routing for one native declaration token.
use super::registrations::{self as wire, Record};
#[cfg(test)]
use alloc::rc::Rc;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_platform::{PointerEvent, PointerPhase, Rect, buttons};
use paperpaw_sdk::{Cx, Env, Gesture, GestureKind, Handled, Id, Screen, ScreenKey, realtime};

/// The owner/runtime supplies admission and the callback-scoped host views.
/// Implementations keep no input, frame, canvas or environment borrow.
pub(crate) trait Dispatch {
    fn event(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32>;
    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32>;
    fn draw(
        &self,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32>;
    fn dropped(&self, token: u32, status: i32);
}

#[cfg(test)]
impl<T: Dispatch + ?Sized> Dispatch for Rc<T> {
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
    fn dropped(&self, token: u32, status: i32) {
        (**self).dropped(token, status);
    }
}

pub(crate) struct NativeScreen<D> {
    id: Id,
    token: u32,
    dispatch: D,
    key: Option<ScreenKey>,
}

impl<D: Dispatch> NativeScreen<D> {
    pub(crate) fn new(id: Id, token: u32, dispatch: D) -> Self {
        Self {
            id,
            token,
            dispatch,
            key: None,
        }
    }

    fn shown(&self, env: &Env<'_>) -> bool {
        let pane = env.content();
        let record = Record::EventScreenShown(wire::EventScreenShown {
            now_ms: env.now().millis(),
            screen: self.id.as_str(),
            width: u32::from(pane.width),
            height: u32::from(pane.height),
        });
        let mut bytes = [0; 192];
        let result = wire::records_encode(&[record], &mut bytes)
            .and_then(|len| self.dispatch.event(self.token, &bytes[..len], env));
        self.status(result)
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
        let result = wire::records_encode(&[record], &mut bytes)
            .and_then(|len| self.dispatch.input(self.token, &bytes[..len], env));
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
        let result = self
            .dispatch
            .draw(self.token, &frame, env, &mut canvas.clipped(pane));
        self.status(result);
    }
}

impl<D: Dispatch> Screen for NativeScreen<D> {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        // The native contract sends neither draw nor input below the top screen.
        // Compare instance keys, since several instances may have the same id.
        if self
            .key
            .is_some_and(|key| env.stack().last().is_some_and(|entry| entry.key == key))
        {
            self.draw_visible(canvas, env);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.key = cx.key();
        self.shown(cx.env());
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        if self.shown(cx.env()) {
            Handled::Yes
        } else {
            Handled::No
        }
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
