//! A yes or no question in a box in the middle of the screen, e.g.
//! whether to delete a file: the screen of slot
//! [`CONFIRM`](paperpaw_sdk::builtin::slots::CONFIRM), which
//! [`ask::confirm`](paperpaw_sdk::ask::confirm) asks. The answer comes back as
//! the plugin's reply event, a [`Confirmed`] with the context it was
//! asked with. *Back*, or a tap outside the box, answers nothing. On a
//! screen wider than the readable column ([`ui::readable`](paperpaw_sdk::ui::readable))
//! the box is as wide as the column at most, and what lies beside it is
//! outside.
//!
//! A screen of one's own bound to the slot reads the question from
//! [`params::TITLE`], [`params::TEXT`] (a line of text below it, may be
//! missing), [`YES_LABEL`] and [`NO_LABEL`] (the buttons' words, the
//! kernel's *Yes* and *No* where missing), [`FOCUS_YES`] (set: the focus
//! starts on yes, else on no) and [`params::REPLY`], and answers with
//! [`Confirmed::new`] and all of the parameters it was opened with.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::ui::confirm::{FOCUS_YES, NO_LABEL, ONLY_YES, YES_LABEL};
use paperpaw_sdk::ui::{Dialog, Ui, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Params, Rect, RefreshMode,
};

/// The buttons, in their order.
const YES: usize = 0;
const NO: usize = 1;

/// The question; see the [module documentation](self).
#[derive(Debug)]
pub struct Confirm {
    style: UiStyle,
    dialog: Dialog,
    focus: usize,
    /// The buttons' words, for the hint below the confirming button.
    labels: [String; 2],
    /// The question's parameters: they come back with the answer.
    params: Params,
    reply: Option<Id>,
}

impl Confirm {
    pub fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let label = |key: &str, fallback: &str| -> String {
            match params.get(key) {
                Some(label) if !label.is_empty() => String::from(label),
                _ => String::from(env.i18n().tr(fallback)),
            }
        };
        let only_yes = params.get(ONLY_YES).is_some();
        let (yes, no) = (
            label(
                YES_LABEL,
                if only_yes {
                    "core.confirm.ok"
                } else {
                    "core.confirm.yes"
                },
            ),
            label(NO_LABEL, "core.confirm.no"),
        );
        let text: Vec<&str> = params
            .get(params::TEXT)
            .filter(|t| !t.is_empty())
            .map(|t| t.split('\n').collect())
            .unwrap_or_default();
        let title = params.get(params::TITLE).unwrap_or_default();
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
            focus: if only_yes || params.get(FOCUS_YES).is_some() {
                YES
            } else {
                NO
            },
            labels: [yes, no],
            params: params.clone(),
            reply: params
                .get(params::REPLY)
                .and_then(|reply| Id::parse(reply).ok()),
        }
    }

    /// Closes, and answers `yes`.
    fn answer(&self, yes: bool, cx: &mut Cx<'_, '_>) {
        cx.command(Command::Back);
        if let Some(reply) = &self.reply {
            cx.command(Command::Emit(
                reply.clone(),
                Confirmed::new(yes, self.params.clone()).to_params(),
            ));
        }
    }
}

impl crate::Layer for Confirm {
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
