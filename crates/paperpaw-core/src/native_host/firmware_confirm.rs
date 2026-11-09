//! The firmware confirmation (`docs/abi/device.md` 5.0, 5.3): the host's own
//! words and screen for an install or a restart, which only the user answers.
//!
//! * **Words.** [`texts`] builds title, body and buttons from the machine's
//!   [`Ask`]: the asking plugin, the image's project and version as read from
//!   the image, its signer (a PaperPaw key by name, another key by
//!   fingerprint, or none), "older" where the plugin allowed it and, for a
//!   foreign image, the four points (the first two again for a restart).
//! * **Screen.** [`HostConfirm`] is a kernel-owned screen on `ui::Dialog`. It
//!   guards the device (a host row that guards is the input target, see
//!   `ui_guard::input_index`), starts with the focus on *No*, drops what
//!   arrives within [`GUARD_MS`] of being shown, ends as *dismissed* on Back
//!   or after [`TIMEOUT_MS`], and tells the kernel its answer with an event
//!   that carries the random nonce only the host and this screen know.
//! * **Kernel.** [`Kernel::firmware_confirm_pump`] opens the screen when a
//!   question waits and nothing guards (never over the lock), takes the
//!   answer ([`Kernel::firmware_answer`]), dismisses at sleep and does the
//!   restart after a yes.
use super::firmware_install::{Answer, Ask};
use crate::Kernel;
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use paperpaw_i18n::Arg;
use paperpaw_platform::{Instant, firmware::Signer};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{Dialog, Ui, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, HintVisibility, Id, Params,
    Rect, RefreshMode, Screen,
};

/// The screen's id: a host screen (`core.*`), opened only by the kernel.
pub(crate) const SCREEN: Id = Id::new_static("core.firmware-confirm");
/// The event the screen answers with; the kernel's alone (see
/// [`Kernel::firmware_answer`]).
pub(crate) const ANSWERED: Id = Id::new_static("core.firmware-answered");
const NONCE: &str = "nonce";
const TITLE: &str = "title";
const TEXT: &str = "text";
const YES_WORD: &str = "yes";
const NO_WORD: &str = "no";
const ANSWER: &str = "answer";
/// Input within this long of being shown is dropped (`device.md` 5.0).
const GUARD_MS: u64 = 500;
/// Ten minutes without an answer end it as dismissed (decision 132).
const TIMEOUT_MS: u64 = 600_000;

/// What the screen says.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Texts {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) yes: String,
    pub(crate) no: String,
}

/// The first eight bytes of a key, in groups of four hex digits.
fn fingerprint(key: &[u8; 32]) -> String {
    let mut text = String::new();
    for (at, byte) in key.iter().take(8).enumerate() {
        if at > 0 && at % 2 == 0 {
            text.push(' ');
        }
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// The confirmation's words for `ask`, asked by the plugin `asker`.
pub(crate) fn texts(i18n: &paperpaw_i18n::Translator, asker: &str, ask: &Ask) -> Texts {
    let mut lines: Vec<String> = Vec::new();
    lines.push(i18n.format("core.firmware.asked-by", &[("name", Arg::Str(asker))]));
    lines.push(i18n.format(
        "core.firmware.image",
        &[
            ("project", Arg::Str(ask.project.as_str())),
            ("version", Arg::Str(ask.version.as_str())),
        ],
    ));
    match &ask.signer {
        Signer::PaperPaw { name, .. } => {
            lines.push(i18n.format("core.firmware.signed", &[("key", Arg::Str(name.as_str()))]))
        }
        Signer::Other { key } => lines.push(i18n.format(
            "core.firmware.other-key",
            &[("fingerprint", Arg::Str(&fingerprint(key)))],
        )),
        _ => lines.push(String::from(i18n.tr("core.firmware.unsigned"))),
    }
    if ask.older {
        lines.push(String::from(i18n.tr("core.firmware.older")));
    }
    if ask.foreign {
        // The restart says the first two points again; the install all four.
        let points = if ask.restart { 2 } else { 4 };
        for point in 1..=points {
            let key = format!("core.firmware.foreign.{point}");
            lines.push(String::from(i18n.tr(&key)));
        }
    }
    let (title, yes) = if ask.restart {
        ("core.firmware.title.restart", "core.firmware.yes.restart")
    } else {
        ("core.firmware.title", "core.firmware.yes")
    };
    Texts {
        title: String::from(i18n.tr(title)),
        body: lines.join("\n"),
        yes: String::from(i18n.tr(yes)),
        no: String::from(i18n.tr("core.firmware.no")),
    }
}

/// The buttons, in their order.
const YES: usize = 0;
const NO: usize = 1;

/// The confirmation screen; see the [module documentation](self).
pub(crate) struct HostConfirm {
    style: UiStyle,
    dialog: Dialog,
    focus: usize,
    nonce: String,
    labels: [String; 2],
    /// When it was shown: the input guard and the timeout count from here.
    shown: Option<Instant>,
    /// It has answered (or ended as dismissed): nothing is told twice.
    done: bool,
}

impl HostConfirm {
    /// The screen the kernel opened with `params` (see [`open_params`]).
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let get = |key| params.get(key).unwrap_or("");
        let text: Vec<&str> = get(TEXT).split('\n').collect();
        let labels = [String::from(get(YES_WORD)), String::from(get(NO_WORD))];
        let buttons = [labels[YES].as_str(), labels[NO].as_str()];
        let mut dialog = Dialog::new(env, &style, get(TITLE), &text, &buttons);
        let room = env.device().display.height.saturating_sub(dialog.height());
        dialog.place(room / 2);
        Self {
            style,
            dialog,
            // Focus starts on No.
            focus: NO,
            nonce: String::from(get(NONCE)),
            labels,
            shown: None,
            done: false,
        }
    }

    /// Whether input counts yet (not within [`GUARD_MS`] of being shown).
    fn counts(&self, env: &Env<'_>) -> bool {
        self.shown
            .is_some_and(|at| env.now().0.saturating_sub(at.0) >= GUARD_MS)
    }

    /// Tells the kernel the answer, once, and closes.
    fn finish(&mut self, answer: Answer, cx: &mut Cx<'_, '_>) {
        if self.done {
            return;
        }
        self.done = true;
        let code = match answer {
            Answer::Yes => "yes",
            Answer::No => "no",
            Answer::Dismissed => "dismissed",
        };
        cx.command(Command::Emit(
            ANSWERED,
            Params::new()
                .with(NONCE, self.nonce.clone())
                .with(ANSWER, code),
        ));
        cx.close();
    }
}

impl Screen for HostConfirm {
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        let display = env.device().display;
        let whole = Rect::new(0, 0, display.width, display.height);
        Some(self.dialog.panel().intersection(&whole).unwrap_or(whole))
    }

    fn dims_below(&self) -> bool {
        true
    }

    // Nothing shows above it, and no plugin's input reaches it.
    fn guards(&self) -> bool {
        true
    }

    fn holds_edges(&self) -> bool {
        true
    }

    fn hints(&self) -> Option<HintVisibility> {
        Some(HintVisibility::Hidden)
    }

    fn hint(&self, action: &Id, _: &Env<'_>) -> Hint {
        if *action == actions::CONFIRM {
            Hint::Text(self.labels[self.focus].clone())
        } else {
            Hint::Hidden
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.shown.is_none() {
            let now = cx.env().now();
            self.shown = Some(now);
            cx.wake_at(Instant(now.0.saturating_add(TIMEOUT_MS)));
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let due = self
            .shown
            .is_some_and(|at| cx.env().now().0.saturating_sub(at.0) >= TIMEOUT_MS);
        if due {
            self.finish(Answer::Dismissed, cx);
        }
    }

    // Back (and a tap outside) dismiss; nothing else closes it.
    fn on_close_request(&mut self, _: paperpaw_sdk::CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        if self.counts(cx.env()) {
            self.finish(Answer::Dismissed, cx);
        }
        Handled::Yes
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let mut ui = Ui::new(canvas, env, &self.style);
        self.dialog.draw(&mut ui, Some(self.focus));
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.counts(cx.env()) {
            return Handled::Yes;
        }
        if *action == actions::CONFIRM {
            let answer = if self.focus == YES {
                Answer::Yes
            } else {
                Answer::No
            };
            self.finish(answer, cx);
            return Handled::Yes;
        }
        if *action == actions::BACK {
            self.finish(Answer::Dismissed, cx);
            return Handled::Yes;
        }
        let scrolled = self.dialog.on_action(action);
        if scrolled.changed() {
            cx.request_redraw_area(self.dialog.panel());
            cx.request_refresh(RefreshMode::Fast);
        }
        if scrolled.taken() {
            return Handled::Yes;
        }
        let along = if [actions::UP, actions::PREV_PAGE].contains(action) {
            &actions::LEFT
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(action) {
            &actions::RIGHT
        } else {
            action
        };
        if let Some(focus) = self.dialog.step(self.focus, along)
            && focus != self.focus
        {
            self.focus = focus;
            cx.request_redraw_area(self.dialog.panel());
            cx.request_refresh(RefreshMode::Fast);
        }
        // Every other action does nothing: only the user's answer counts.
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.counts(cx.env()) {
            return Handled::Yes;
        }
        let scrolled = self.dialog.on_gesture(&gesture);
        if scrolled.changed() {
            cx.request_redraw_area(self.dialog.panel());
            cx.request_refresh(RefreshMode::Fast);
        }
        if scrolled.taken() {
            return Handled::Yes;
        }
        // A touch counts only inside its own buttons.
        if gesture.kind == GestureKind::Tap {
            match self.dialog.hit(gesture.start) {
                Some(YES) => self.finish(Answer::Yes, cx),
                Some(_) => self.finish(Answer::No, cx),
                None => {}
            }
        }
        Handled::Yes
    }
}

/// The params the kernel opens the screen with.
pub(crate) fn open_params(texts: &Texts, nonce: u64) -> Params {
    Params::new()
        .with(NONCE, nonce.to_string())
        .with(TITLE, texts.title.clone())
        .with(TEXT, texts.body.clone())
        .with(YES_WORD, texts.yes.clone())
        .with(NO_WORD, texts.no.clone())
}

/// What an answer event says: the nonce and the answer.
pub(crate) fn read_answer(event: &Id, params: &Params) -> Option<(u64, Answer)> {
    if *event != ANSWERED {
        return None;
    }
    let nonce = params.get(NONCE)?.parse().ok()?;
    let answer = match params.get(ANSWER)? {
        "yes" => Answer::Yes,
        "no" => Answer::No,
        "dismissed" => Answer::Dismissed,
        _ => return None,
    };
    Some((nonce, answer))
}

impl Kernel {
    /// Opens the confirmation of a question that waits, when nothing guards
    /// the device (it is never drawn over the lock), and does the restart
    /// after a yes. `true` while another pass has something to do.
    pub(crate) fn firmware_confirm_pump(
        &mut self,
        driver: &alloc::rc::Rc<core::cell::RefCell<super::firmware_driver::Driver>>,
    ) -> bool {
        self.firmware_restart_pump(driver);
        let Some(ask) = driver.try_borrow().ok().and_then(|d| d.peek_ask()) else {
            return false;
        };
        if self.guard().is_some() {
            // Waits for the guard to end; the pump runs again with the next tick.
            return false;
        }
        let mut bytes = [0u8; 8];
        self.shared.random.fill(&mut bytes);
        let nonce = u64::from_le_bytes(bytes) ^ self.now.0;
        let asker = self
            .shared
            .native_instances
            .id(ask.owner)
            .map_or_else(String::new, |id| String::from(id.as_str()));
        let said = texts(&self.shared.translator, &asker, &ask);
        let params = open_params(&said, nonce);
        let before = self.stack.len();
        self.open(SCREEN, None, &params);
        let shown = self.stack.len() > before;
        if let Ok(mut driver) = driver.try_borrow_mut() {
            if shown {
                driver.ask_shown(nonce, ask);
            } else {
                // It cannot be shown (the stack is full, no memory): ended as
                // dismissed, so nothing waits for a screen that never comes.
                driver.ask_dismissed(ask);
            }
        }
        true
    }

    /// The user's answer, where `event` is the confirmation's: taken only for
    /// the question shown, once (`true` where it was the confirmation's).
    pub(crate) fn firmware_answer(&mut self, event: &Id, params: &Params) -> bool {
        let Some((nonce, answer)) = read_answer(event, params) else {
            return false;
        };
        if let Some(driver) = self.firmware.clone()
            && let Ok(mut driver) = driver.try_borrow_mut()
        {
            let _ = driver.answer_shown(nonce, answer);
        }
        true
    }

    /// The device goes to sleep: a confirmation that shows is dismissed, and
    /// the jobs that run or wait end with reason 20.
    pub(crate) fn firmware_suspend(&mut self) {
        let Some(driver) = self.firmware.clone() else {
            return;
        };
        let nonce = driver
            .try_borrow_mut()
            .ok()
            .and_then(|mut driver| driver.dismiss_shown());
        if let Some(nonce) = nonce {
            self.firmware_close_screen(nonce);
        }
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.abort_all();
        }
    }

    /// The plugin `key` stopped: its requests and its confirmation go.
    pub(crate) fn firmware_sender_stopped(&mut self, key: u64) {
        let Some(driver) = self.firmware.clone() else {
            return;
        };
        let nonce = driver
            .try_borrow_mut()
            .ok()
            .and_then(|mut driver| driver.sender_stopped(key));
        if let Some(nonce) = nonce {
            self.firmware_close_screen(nonce);
        }
    }

    /// Closes the confirmation opened with `nonce`.
    fn firmware_close_screen(&mut self, nonce: u64) {
        let nonce = nonce.to_string();
        let open = self
            .stack
            .iter()
            .find(|s| s.params.get(NONCE) == Some(nonce.as_str()))
            .map(|s| s.serial);
        if let Some(serial) = open {
            self.output
                .commands
                .push(Command::Close(paperpaw_sdk::ScreenKey::new(serial)));
        }
    }

    /// A yes to the restart: the plugins hear that PaperPaw stops, and on the
    /// next pass what they saved is written and the device restarts. A
    /// restart that cannot start ends the request with its reason.
    fn firmware_restart_pump(
        &mut self,
        driver: &alloc::rc::Rc<core::cell::RefCell<super::firmware_driver::Driver>>,
    ) {
        let wanted = driver
            .try_borrow()
            .is_ok_and(|driver| driver.restart_wanted());
        if !wanted {
            return;
        }
        if !self.firmware_stopping {
            self.firmware_stopping = true;
            self.output
                .commands
                .push(paperpaw_sdk::builtin::events::STOPPING.emit(()));
            return;
        }
        self.firmware_stopping = false;
        let _ = self.shared.storage.sync();
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.restart_now();
        }
    }
}

#[cfg(test)]
#[path = "firmware_confirm_tests.rs"]
mod tests;
