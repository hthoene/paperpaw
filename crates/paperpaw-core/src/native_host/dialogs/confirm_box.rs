//! The box of a yes/no question: the one dialog path of the host's two
//! confirmations, the question of a request ([`Confirm`](super::Confirm), kind
//! 2) and the firmware confirmation (`firmware_confirm`). It holds the
//! `ui::Dialog`, the focus and the drawing, scrolls a text that does not fit,
//! moves the focus between the buttons and tells which button a tap hit. What
//! the buttons *mean* (an answer to the store, an answer event with a nonce,
//! the input guard, the timeout) stays with each screen.
use alloc::vec::Vec;
use paperpaw_sdk::ui::{Dialog, ListChange, Ui, UiStyle};
use paperpaw_sdk::{Canvas, Cx, Env, Gesture, GestureKind, Id, Rect, RefreshMode};

use paperpaw_sdk::builtin::actions;

/// What a gesture did to the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Touch {
    /// The text scrolled, or the tap was not for the box's buttons.
    Taken,
    /// A tap on the button with this index.
    Button(usize),
}

/// The box; see the [module documentation](self).
pub(crate) struct ConfirmBox {
    style: UiStyle,
    dialog: Dialog,
    focus: usize,
}

impl ConfirmBox {
    /// A box with `title`, the lines of `text` (split at newlines, none for an
    /// empty text) and `buttons`, in the middle of the screen (at the top when
    /// it is taller), the focus on button `focus`.
    pub(crate) fn new(
        env: &Env<'_>,
        title: &str,
        text: &str,
        buttons: &[&str],
        focus: usize,
    ) -> Self {
        let style = UiStyle::new(env);
        let lines: Vec<&str> = if text.is_empty() {
            Vec::new()
        } else {
            text.split('\n').collect()
        };
        let mut dialog = Dialog::new(env, &style, title, &lines, buttons);
        let room = env.device().display.height.saturating_sub(dialog.height());
        dialog.place(room / 2);
        Self {
            style,
            dialog,
            focus,
        }
    }

    /// The button with the focus.
    pub(crate) fn focus(&self) -> usize {
        self.focus
    }

    /// The panel on the screen.
    pub(crate) fn region(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let whole = Rect::new(0, 0, display.width, display.height);
        self.dialog.panel().intersection(&whole).unwrap_or(whole)
    }

    pub(crate) fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let mut ui = Ui::new(canvas, env, &self.style);
        self.dialog.draw(&mut ui, Some(self.focus));
    }

    /// An action the box uses: a page button that scrolls a text that does not
    /// fit, or a step of the focus along the buttons (up and down, and the page
    /// buttons beside the screen on the X4, as left and right). `false` for any
    /// other: the screen decides (*Back*, *Confirm*).
    pub(crate) fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> bool {
        let scrolled = self.dialog.on_action(action);
        if scrolled.changed() {
            self.redraw(cx);
        }
        if scrolled.taken() {
            return true;
        }
        let along = if [actions::UP, actions::PREV_PAGE].contains(action) {
            &actions::LEFT
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(action) {
            &actions::RIGHT
        } else {
            action
        };
        let Some(focus) = self.dialog.step(self.focus, along) else {
            return false;
        };
        if focus != self.focus {
            let reach = |i| {
                self.dialog
                    .button(i)
                    .map(|b| paperpaw_sdk::ui::button_reach(&self.style, b))
            };
            match (reach(self.focus), reach(focus)) {
                (Some(left), Some(reached)) => ListChange::focus(left, reached).request(cx),
                _ => self.redraw(cx),
            }
            self.focus = focus;
        }
        true
    }

    /// A gesture: scrolls the text, or a tap that hits a button.
    pub(crate) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Touch {
        let scrolled = self.dialog.on_gesture(&gesture);
        if scrolled.changed() {
            self.redraw(cx);
        }
        if scrolled.taken() || gesture.kind != GestureKind::Tap {
            return Touch::Taken;
        }
        self.dialog
            .hit(gesture.start)
            .map_or(Touch::Taken, Touch::Button)
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw_area(self.dialog.panel());
        cx.request_refresh(RefreshMode::Fast);
    }
}
