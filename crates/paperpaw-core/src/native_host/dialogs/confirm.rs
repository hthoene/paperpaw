//! The host's confirm dialog (slot `core.confirm`): a yes or no question
//! (`docs/abi/ui.md` 3.1, kind 2) in a box in the middle of the screen,
//! answered through the request store (outcome 0, `choice` 1 for yes and 0 for
//! no). Made with no request (an interface 6 caller, until its cleanup) it
//! replies with an event as the SDK screen did. *Back*, a tap outside the box
//! or a close with no answer cancels.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::ui::{Dialog, Ui, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Params, Rect, RefreshMode,
    Screen,
};

/// The word on the yes button, e.g. *Delete*.
pub(super) const YES_LABEL: &str = "yes-label";
/// The word on the no button, e.g. *Keep*.
pub(super) const NO_LABEL: &str = "no-label";
/// Set: the focus starts on yes, where yes is the answer to expect and
/// harmless.
pub(super) const FOCUS_YES: &str = "focus-yes";
/// Set: one button only, yes (by default *OK*): a text shown to be read
/// whole, which the page buttons scroll where it is long.
pub(super) const ONLY_YES: &str = "only-yes";

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
    /// Where the answer goes; none for a dialog made with no request.
    answer: Option<super::Answerer>,
    /// With no request (an interface 6 caller, until its cleanup): the
    /// question's parameters and the event the answer is replied with.
    params: Params,
    reply: Option<Id>,
}

impl Confirm {
    pub(crate) fn new(env: &Env<'_>, params: &Params, answer: Option<super::Answerer>) -> Self {
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
            answer,
            params: params.clone(),
            reply: params
                .get(params::REPLY)
                .and_then(|reply| Id::parse(reply).ok()),
        }
    }

    /// Closes, and answers `yes`.
    fn answer(&self, yes: bool, cx: &mut Cx<'_, '_>) {
        if let Some(to) = &self.answer {
            to.answer(0, u32::from(yes));
        }
        cx.command(Command::Back);
        if self.answer.is_none()
            && let Some(reply) = &self.reply
        {
            cx.command(Command::Emit(
                reply.clone(),
                paperpaw_sdk::replies::Confirmed::new(yes, self.params.clone()).to_params(),
            ));
        }
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
