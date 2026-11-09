//! The host's confirm dialog (slot `core.confirm`): a yes or no question
//! (`docs/abi/ui.md` 3.1, kind 2) in a box in the middle of the screen,
//! answered through the request store (outcome 0, `choice` 1 for yes and 0 for
//! no). *Back*, a tap outside the box or a close with no answer cancels. No
//! event is sent and no interface 6 screen is inside.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{Dialog, Ui, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Rect, RefreshMode, Screen,
};

/// `ask` flag bit 8 (`only_yes`): one button, by default *OK*.
const ONLY_YES: u32 = 1 << 8;
/// `ask` flag bit 9 (`focus_yes`): the focus starts on yes.
const FOCUS_YES: u32 = 1 << 9;

/// The buttons, in their order.
const YES: usize = 0;
const NO: usize = 1;

/// The question; see the [module documentation](self).
pub(crate) struct Confirm {
    style: UiStyle,
    dialog: Dialog,
    focus: usize,
    /// The buttons' words, for the hint below the confirming button.
    labels: [String; 2],
    answer: super::Answerer,
}

impl Confirm {
    /// The question as the asker formatted it: `yes` and `no` are the buttons'
    /// words, empty for the dialog's own; `flags` the `ask` flags.
    pub(crate) fn new(
        env: &Env<'_>,
        title: &str,
        text: &str,
        (yes, no): (&str, &str),
        flags: u32,
        answer: super::Answerer,
    ) -> Self {
        let style = UiStyle::new(env);
        let label = |word: &str, fallback: &str| -> String {
            if word.is_empty() {
                String::from(env.i18n().tr(fallback))
            } else {
                String::from(word)
            }
        };
        let only_yes = flags & ONLY_YES != 0;
        let (yes, no) = (
            label(
                yes,
                if only_yes {
                    "core.confirm.ok"
                } else {
                    "core.confirm.yes"
                },
            ),
            label(no, "core.confirm.no"),
        );
        let text: Vec<&str> = if text.is_empty() {
            Vec::new()
        } else {
            text.split('\n').collect()
        };
        let buttons: &[&str] = if only_yes {
            &[yes.as_str()]
        } else {
            &[yes.as_str(), no.as_str()]
        };
        let mut dialog = Dialog::new(env, &style, title, &text, buttons);
        // In the middle of the screen; at the top when it is taller.
        let room = env.device().display.height.saturating_sub(dialog.height());
        dialog.place(room / 2);
        Self {
            style,
            dialog,
            focus: if only_yes || flags & FOCUS_YES != 0 {
                YES
            } else {
                NO
            },
            labels: [yes, no],
            answer,
        }
    }

    /// Closes, and answers `yes`.
    fn answer(&self, yes: bool, cx: &mut Cx<'_, '_>) {
        self.answer.answer(0, u32::from(yes));
        cx.command(Command::Back);
    }
}

impl Screen for Confirm {
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        let display = env.device().display;
        let whole = Rect::new(0, 0, display.width, display.height);
        Some(self.dialog.panel().intersection(&whole).unwrap_or(whole))
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn hint(&self, action: &Id, _: &Env<'_>) -> Hint {
        if *action == actions::CONFIRM {
            Hint::Text(self.labels[self.focus].clone())
        } else if [actions::BACK, actions::LEFT, actions::RIGHT].contains(action) {
            Hint::Default
        } else {
            Hint::Hidden
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let mut ui = Ui::new(canvas, env, &self.style);
        self.dialog.draw(&mut ui, Some(self.focus));
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            self.answer(self.focus == YES, cx);
            return Handled::Yes;
        }
        // Where question and text do not fit, the page buttons scroll them.
        let scrolled = self.dialog.on_action(action);
        if scrolled.changed() {
            cx.request_redraw_area(self.dialog.panel());
            cx.request_refresh(RefreshMode::Fast);
        }
        if scrolled.taken() {
            return Handled::Yes;
        }
        // Up and down, and the page buttons (beside the screen on the X4),
        // move along the two buttons too, as left and right.
        let along = if [actions::UP, actions::PREV_PAGE].contains(action) {
            &actions::LEFT
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(action) {
            &actions::RIGHT
        } else {
            action
        };
        match self.dialog.step(self.focus, along) {
            Some(focus) => {
                if focus != self.focus {
                    let reach = |i| {
                        self.dialog
                            .button(i)
                            .map(|b| paperpaw_sdk::ui::button_reach(&self.style, b))
                    };
                    match (reach(self.focus), reach(focus)) {
                        (Some(left), Some(reached)) => {
                            paperpaw_sdk::ui::ListChange::focus(left, reached).request(cx);
                        }
                        _ => {
                            cx.request_redraw_area(self.dialog.panel());
                            cx.request_refresh(RefreshMode::Fast);
                        }
                    }
                    self.focus = focus;
                }
                Handled::Yes
            }
            // Back closes, without an answer; the others do as they do.
            None => Handled::No,
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let scrolled = self.dialog.on_gesture(&gesture);
        if scrolled.changed() {
            cx.request_redraw_area(self.dialog.panel());
            cx.request_refresh(RefreshMode::Fast);
        }
        if scrolled.taken() {
            return Handled::Yes;
        }
        if gesture.kind == GestureKind::Tap {
            match self.dialog.hit(gesture.start) {
                Some(YES) => self.answer(true, cx),
                Some(_) => self.answer(false, cx),
                None => {}
            }
        }
        Handled::Yes
    }
}
