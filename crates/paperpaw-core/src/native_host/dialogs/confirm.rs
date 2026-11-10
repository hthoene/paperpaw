//! The host's confirm dialog (slot `core.confirm`): a yes or no question
//! (`docs/abi/ui.md` 3.1, kind 2) in a box in the middle of the screen,
//! answered through the request store (outcome 0, `choice` 1 for yes and 0 for
//! no). *Back*, a tap outside the box or a close with no answer cancels. No
//! event is sent and no interface 6 screen is inside.
use alloc::string::String;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Canvas, Command, Cx, Env, Gesture, Handled, Hint, Id, Rect, Screen};

/// `ask` flag bit 8 (`only_yes`): one button, by default *OK*.
const ONLY_YES: u32 = 1 << 8;
/// `ask` flag bit 9 (`focus_yes`): the focus starts on yes.
const FOCUS_YES: u32 = 1 << 9;

/// The buttons, in their order.
const YES: usize = 0;
const NO: usize = 1;

/// The question; see the [module documentation](self).
pub(crate) struct Confirm {
    board: super::ConfirmBox,
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
        let buttons: &[&str] = if only_yes {
            &[yes.as_str()]
        } else {
            &[yes.as_str(), no.as_str()]
        };
        let focus = if only_yes || flags & FOCUS_YES != 0 {
            YES
        } else {
            NO
        };
        Self {
            board: super::ConfirmBox::new(env, title, text, buttons, focus),
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
        Some(self.board.region(env))
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn hint(&self, action: &Id, _: &Env<'_>) -> Hint {
        if *action == actions::CONFIRM {
            Hint::Text(self.labels[self.board.focus()].clone())
        } else if [actions::BACK, actions::LEFT, actions::RIGHT].contains(action) {
            Hint::Default
        } else {
            Hint::Hidden
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.board.draw(canvas, env);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            self.answer(self.board.focus() == YES, cx);
            return Handled::Yes;
        }
        // Back closes, without an answer; the others do as they do.
        if self.board.on_action(action, cx) {
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        match self.board.on_gesture(gesture, cx) {
            super::Touch::Button(YES) => self.answer(true, cx),
            super::Touch::Button(_) => self.answer(false, cx),
            super::Touch::Taken => {}
        }
        Handled::Yes
    }
}
