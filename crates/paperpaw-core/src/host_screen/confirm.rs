//! The firmware confirmation's screen (`docs/abi/device.md` 5.0, 5.3; the
//! words and the kernel's side in `native_host::firmware_confirm`): a box in
//! the middle of the screen with the title, the text and the buttons yes,
//! [yes with data,] no. It guards the device, starts with the focus on *No*,
//! drops what arrives within [`GUARD_MS`] of being shown, ends as
//! *dismissed* on Back or after [`TIMEOUT_MS`], and tells the kernel its
//! answer with an event that carries the nonce only the host and this screen
//! know. It is drawn on the host's [`Board`].
use crate::{Place, Turn};
use alloc::string::String;
use alloc::vec::Vec;

use super::board::Board;
use crate::Ask;
use crate::native_host::firmware_confirm::{self as words, ANSWERED};
use crate::native_host::firmware_install::Answer;
use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Canvas, Gesture, Handled, Hint, Id, Params, Rect};

/// Input within this long of being shown is dropped (`device.md` 5.0).
const GUARD_MS: u64 = 500;
/// Ten minutes without an answer end it as dismissed (decision 132).
const TIMEOUT_MS: u64 = 600_000;

/// The confirmation; see the [module documentation](self).
pub(crate) struct Confirm {
    /// The buttons in their order: yes, [yes with data,] no.
    board: Board,
    nonce: String,
    /// When it was shown: the input guard and the timeout count from here.
    shown: Option<Instant>,
    /// It has answered (or ended as dismissed): nothing is told twice.
    done: bool,
}

impl Confirm {
    /// The screen the kernel opened with `params` (`firmware_confirm::open_params`).
    pub(crate) fn new(env: &Place<'_>, params: &Params) -> Self {
        let get = |key| params.get(key).unwrap_or("");
        let mut labels = Vec::from([String::from(get(words::YES_WORD))]);
        if let Some(data) = params.get(words::DATA_WORD) {
            labels.push(String::from(data));
        }
        labels.push(String::from(get(words::NO_WORD)));
        let no = labels.len() - 1;
        Self {
            board: Board::new(env, (get(words::TITLE), get(words::TEXT)), labels, no),
            nonce: String::from(get(words::NONCE)),
            shown: None,
            done: false,
        }
    }

    /// The box on the screen.
    pub(super) fn region(&self) -> Rect {
        self.board.region()
    }

    pub(super) fn hint(&self, action: &Id) -> Hint {
        self.board.hint(action)
    }

    pub(super) fn draw(&self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
        self.board.draw(canvas, env);
    }

    /// Whether input counts yet (not within [`GUARD_MS`] of being shown).
    fn counts(&self, now: Instant) -> bool {
        self.shown
            .is_some_and(|at| now.0.saturating_sub(at.0) >= GUARD_MS)
    }

    /// What pressing the button `at` answers: the first is a yes, a middle
    /// one a yes with the data, the last a no.
    fn answer_of(&self, at: usize) -> Answer {
        if at == 0 {
            Answer::Yes
        } else if at + 1 == self.board.buttons() {
            Answer::No
        } else {
            Answer::YesAndData
        }
    }

    /// Tells the kernel the answer, once, and closes.
    fn finish(&mut self, answer: Answer, cx: &mut Turn<'_, '_>) {
        if core::mem::replace(&mut self.done, true) {
            return;
        }
        let code = match answer {
            Answer::Yes => "yes",
            Answer::No => "no",
            Answer::YesAndData => "yes-data",
            Answer::Dismissed => "dismissed",
        };
        cx.command(Ask::Emit(
            ANSWERED,
            Params::new()
                .with(words::NONCE, self.nonce.clone())
                .with(words::ANSWER, code),
        ));
        cx.close();
    }

    pub(super) fn on_show(&mut self, cx: &mut Turn<'_, '_>) {
        if self.shown.is_none() {
            let now = cx.env().now();
            self.shown = Some(now);
            cx.wake_at(Instant(now.0.saturating_add(TIMEOUT_MS)));
        }
    }

    pub(super) fn on_timer(&mut self, cx: &mut Turn<'_, '_>) {
        let now = cx.env().now();
        if self
            .shown
            .is_some_and(|at| now.0.saturating_sub(at.0) >= TIMEOUT_MS)
        {
            self.finish(Answer::Dismissed, cx);
        }
    }

    /// Back (and a tap outside) dismiss; nothing else closes it.
    pub(super) fn on_close_request(&mut self, cx: &mut Turn<'_, '_>) -> Handled {
        if self.counts(cx.env().now()) {
            self.finish(Answer::Dismissed, cx);
        }
        Handled::Yes
    }

    pub(super) fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        if !self.counts(cx.env().now()) {
            return Handled::Yes;
        }
        if *action == actions::CONFIRM {
            self.finish(self.answer_of(self.board.focus()), cx);
            return Handled::Yes;
        }
        if *action == actions::BACK {
            self.finish(Answer::Dismissed, cx);
            return Handled::Yes;
        }
        // Scrolls or moves the focus; every other action does nothing: only
        // the user's answer counts.
        self.board.step(action, cx);
        Handled::Yes
    }

    /// A swipe over the box scrolls; a tap counts only on its own buttons.
    pub(super) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Turn<'_, '_>) -> Handled {
        if !self.counts(cx.env().now()) {
            return Handled::Yes;
        }
        if let Some(at) = self.board.on_gesture(gesture, cx) {
            self.finish(self.answer_of(at), cx);
        }
        Handled::Yes
    }
}
