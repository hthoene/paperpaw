//! The lock screen's steps without a host: what a key, a dot, a code or an
//! action does, as a list of effects for the plugin to carry out. It is the
//! interface-6 `LockScreen` made free of the kernel: the same steps (unlock,
//! check the code in use, choose the kind, a new code, the same again), the
//! same messages and the same wrong-tries rules, with every allocation
//! fallible and the codes wiped as soon as they are done with.
use alloc::string::String;

use paperpaw_sdk::Point;
use paperpaw_sdk::native::Error;

use crate::native_model::{self as model, Attempts, Pin, Pressed, TRIES};

/// The most effects one input has.
const EFFECTS_MAX: usize = 8;

/// Where the screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The code to unlock.
    Unlock,
    /// The code in use, before it is changed.
    Verify,
    /// Which kind of lock, listed.
    Choose,
    /// A new code.
    New,
    /// The new code again.
    Confirm,
}

/// What the screen says under the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    None,
    /// `lock.wrong`
    Wrong,
    /// `lock.too-short`, with the shortest length.
    TooShort,
    /// `lock.mismatch`
    Mismatch,
}

/// What the plugin carries out, in order.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// Draw the screen again (a fast refresh).
    Redraw,
    /// Close the screen.
    Back,
    /// The device is unlocked: close the screen.
    Unlocked,
    /// Ask for the password on the keyboard.
    AskPassword,
    /// Show the notice with this translation key.
    Notify(&'static str),
    /// Remove the code kept.
    Forget,
    /// Set the kind setting to this value (`lock.kind.pin`).
    SetKind(&'static str),
    /// Keep this code for this kind, with a salt of its own.
    Keep(&'static str, String),
    /// The code opened the lock but was kept with a salt made from it: keep it
    /// anew, at random.
    Renew(&'static str, String),
    /// Wake the screen at this time (the monotonic clock) to end a wait.
    WakeAt(u64),
    /// Keep this many wrong tries on the card (0: none).
    KeepTries(u8),
}

/// The effects of one input.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Effects {
    list: [Option<Effect>; EFFECTS_MAX],
    len: usize,
}

impl Effects {
    fn push(&mut self, effect: Effect) {
        if let Some(slot) = self.list.get_mut(self.len) {
            *slot = Some(effect);
            self.len += 1;
        }
    }

    /// The effects, in order.
    pub fn iter(&self) -> impl Iterator<Item = &Effect> {
        self.list.iter().flatten()
    }

    /// Takes the effects out, in order.
    pub fn take(&mut self) -> impl Iterator<Item = Effect> + '_ {
        self.len = 0;
        self.list.iter_mut().filter_map(Option::take)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Whether the device is locked and how unlocking went so far: shared by the
/// screens, so that a screen made anew (turned, reopened) goes on where the
/// last one was.
#[derive(Debug, Default)]
pub struct Lockstate {
    pub locked: bool,
    pub attempts: Attempts,
}

/// What the screen needs to know of the device at an input.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The code kept (`kind:salt:hash`), if any.
    pub stored: Option<&'a str>,
    /// The kind of the lock in use, if there is one.
    pub active: Option<&'static str>,
    /// The monotonic clock.
    pub now_ms: u64,
    /// Whether the device has a touch screen (patterns are offered, and the
    /// pattern has no *OK* button).
    pub touch: bool,
}

/// The screen's state.
#[derive(Debug)]
pub struct Flow {
    step: Step,
    /// What is entered: `pin`, `pattern` or `password`.
    kind: &'static str,
    pin: Pin,
    /// The dots of a pattern (`0` to `8`).
    dots: String,
    /// The first of two new codes, while the same is asked again.
    first: Option<String>,
    message: Message,
    /// The key or dot chosen with the buttons (the pattern's *OK* is 9).
    pub focus: usize,
    /// Whether the focus is shown (the buttons were used).
    pub shown: bool,
    /// The finger drawing a pattern: its id, and where it is.
    finger: Option<(u32, Point)>,
    /// Whether the keyboard was opened already.
    asked: bool,
}

impl Drop for Flow {
    fn drop(&mut self) {
        model::wipe_string(&mut self.dots);
        if let Some(first) = &mut self.first {
            model::wipe_string(first);
        }
    }
}

/// The pattern's *OK* where there is no touch.
pub const PATTERN_OK: usize = 9;
/// The keypad's key that deletes the last digit.
const DELETE_KEY: usize = 9;

impl Flow {
    /// A screen to unlock (`unlock`), else one to set up: it checks the code in
    /// use first where there is one, else lists the kinds.
    #[must_use]
    pub fn new(unlock: bool, active: Option<&'static str>, focus_shown: bool) -> Self {
        let step = if unlock {
            Step::Unlock
        } else if active.is_some() {
            Step::Verify
        } else {
            Step::Choose
        };
        Self {
            step,
            kind: active.unwrap_or("pin"),
            pin: Pin::default(),
            dots: String::new(),
            first: None,
            message: Message::None,
            focus: 0,
            shown: focus_shown,
            finger: None,
            asked: false,
        }
    }

    #[must_use]
    pub const fn step(&self) -> Step {
        self.step
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }

    #[must_use]
    pub const fn message(&self) -> Message {
        self.message
    }

    /// Whether the keyboard was opened for the password already.
    #[must_use]
    pub const fn asked(&self) -> bool {
        self.asked
    }

    /// The digits of a PIN entered (as many dots to draw).
    #[must_use]
    pub fn pin_len(&self) -> usize {
        self.pin.len()
    }

    /// The pattern's dots drawn so far, in order.
    pub fn pattern(&self) -> impl Iterator<Item = usize> + '_ {
        self.dots
            .bytes()
            .map(|b| usize::from(b.saturating_sub(b'0')).min(model::DOTS - 1))
    }

    /// Where the finger drawing a pattern is.
    #[must_use]
    pub fn finger(&self) -> Option<Point> {
        self.finger.map(|(_, at)| at)
    }

    /// What the screen asks for (a translation key).
    #[must_use]
    pub fn title_key(&self) -> &'static str {
        match (self.step, self.kind) {
            (Step::Choose, _) => "lock.setting.kind",
            (Step::Confirm, _) => "lock.again",
            (Step::New, "pattern") => "lock.new-pattern",
            (Step::New, "password") => "lock.new-password",
            (Step::New, _) => "lock.new-pin",
            (Step::Verify, "pattern") => "lock.current-pattern",
            (Step::Verify, "password") => "lock.current-password",
            (Step::Verify, _) => "lock.current-pin",
            (_, "pattern") => "lock.draw-pattern",
            (_, "password") => "lock.enter-password",
            _ => "lock.enter-pin",
        }
    }

    /// The kinds offered; patterns only where fingers draw them.
    pub fn kinds(touch: bool) -> impl Iterator<Item = &'static str> {
        model::KINDS
            .iter()
            .copied()
            .filter(move |k| *k != "pattern" || touch)
    }

    /// The things chosen with the buttons, in order.
    #[must_use]
    pub fn targets(&self, touch: bool) -> usize {
        match self.kind {
            "pattern" if touch => model::DOTS,
            "pattern" => model::DOTS + 1,
            "password" => 1,
            _ => model::KEYS.len(),
        }
    }

    fn clear_entered(&mut self) {
        self.pin.clear();
        model::wipe_string(&mut self.dots);
        self.dots.clear();
        self.finger = None;
    }

    /// Unlocking with no code kept any more (it was removed elsewhere): there
    /// is nothing to ask for, so the device can be used. Whether it was so.
    pub fn released(&mut self, lock: &mut Lockstate, room: &Context<'_>, fx: &mut Effects) -> bool {
        if self.step != Step::Unlock || !lock.locked || room.active.is_some() {
            return false;
        }
        self.unlock(lock, fx);
        true
    }

    /// Unlocked: the tries forgotten and the screen closes.
    fn unlock(&mut self, lock: &mut Lockstate, fx: &mut Effects) {
        lock.locked = false;
        if lock.attempts.succeeded() {
            fx.push(Effect::KeepTries(0));
        }
        fx.push(Effect::Unlocked);
    }

    /// A wrong code: counted, and after [`TRIES`] the next one waits.
    fn failed(&mut self, lock: &mut Lockstate, room: &Context<'_>, fx: &mut Effects) {
        let failed = lock.attempts.failed(room.now_ms);
        if let Some(until) = failed.wake_at {
            fx.push(Effect::WakeAt(until));
        }
        fx.push(Effect::KeepTries(failed.tries));
        self.message = Message::Wrong;
    }

    /// Whether the next try has to wait.
    #[must_use]
    pub fn waiting(lock: &Lockstate, now_ms: u64) -> bool {
        lock.attempts.waiting(now_ms)
    }

    /// The wait is over (the timer woke): the tries are forgotten.
    pub fn wait_over(&mut self, lock: &mut Lockstate, room: &Context<'_>, fx: &mut Effects) {
        if lock.attempts.expire(room.now_ms) {
            fx.push(Effect::KeepTries(0));
            self.message = Message::None;
            fx.push(Effect::Redraw);
        }
    }

    /// Kind `kind` was chosen: off at once, else the new code.
    pub fn choose_kind(&mut self, kind: &'static str, fx: &mut Effects) {
        if kind == "off" {
            fx.push(Effect::Forget);
            fx.push(Effect::SetKind("lock.kind.off"));
            fx.push(Effect::Notify("lock.removed"));
            fx.push(Effect::Back);
            return;
        }
        self.kind = kind;
        self.step = Step::New;
        self.clear_entered();
        self.focus = 0;
        self.asked = false;
        if kind == "password" {
            self.asked = true;
            fx.push(Effect::AskPassword);
        }
        fx.push(Effect::Redraw);
    }

    /// The password was typed.
    ///
    /// # Errors
    /// `NoMemory` as for [`Flow::submit`].
    pub fn typed(
        &mut self,
        password: String,
        lock: &mut Lockstate,
        room: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        if matches!(self.step, Step::Choose) || self.kind != "password" {
            let mut password = password;
            model::wipe_string(&mut password);
            return Ok(());
        }
        self.submit(password, lock, room, fx)
    }

    /// A code was entered: the digits, the dots or the password.
    ///
    /// # Errors
    /// `NoMemory` when the code cannot be checked; the screen is as before the
    /// code was entered, which is gone.
    pub fn submit(
        &mut self,
        mut entered: String,
        lock: &mut Lockstate,
        room: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        self.clear_entered();
        match self.step {
            Step::Unlock | Step::Verify => {
                if self.released(lock, room, fx) {
                    model::wipe_string(&mut entered);
                    return Ok(());
                }
                let right = match model::check(room.stored, &entered) {
                    Ok(right) => right,
                    Err(error) => {
                        model::wipe_string(&mut entered);
                        return Err(error);
                    }
                };
                if !right {
                    self.failed(lock, room, fx);
                } else {
                    // Kept with a salt made from it: kept anew, at random.
                    let renewal = model::renewal(room.stored, &entered);
                    match renewal {
                        Ok(Some(kind)) => {
                            if let Some(kind) = model::KINDS.iter().find(|k| **k == kind) {
                                fx.push(Effect::Renew(kind, core::mem::take(&mut entered)));
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            model::wipe_string(&mut entered);
                            return Err(error);
                        }
                    }
                    if self.step == Step::Unlock {
                        model::wipe_string(&mut entered);
                        self.unlock(lock, fx);
                        return Ok(());
                    }
                    if lock.attempts.succeeded() {
                        fx.push(Effect::KeepTries(0));
                    }
                    self.step = Step::Choose;
                    self.focus = 0;
                    self.message = Message::None;
                }
                model::wipe_string(&mut entered);
            }
            Step::New => {
                if model::too_short(&entered) {
                    self.message = Message::TooShort;
                    model::wipe_string(&mut entered);
                } else {
                    self.first = Some(entered);
                    self.step = Step::Confirm;
                    self.message = Message::None;
                    self.asked = false;
                }
            }
            Step::Confirm => {
                let first = self.first.take().unwrap_or_default();
                let equal = first == entered;
                let mut first = first;
                if equal {
                    model::wipe_string(&mut first);
                    fx.push(Effect::Keep(self.kind, entered));
                    fx.push(Effect::SetKind(kind_value(self.kind)));
                    fx.push(Effect::Notify("lock.done"));
                    fx.push(Effect::Back);
                    return Ok(());
                }
                model::wipe_string(&mut first);
                model::wipe_string(&mut entered);
                self.step = Step::New;
                self.asked = false;
                self.message = Message::Mismatch;
            }
            Step::Choose => model::wipe_string(&mut entered),
        }
        if self.kind == "password" && !self.asked && self.step != Step::Choose {
            self.asked = true;
            fx.push(Effect::AskPassword);
        }
        fx.push(Effect::Redraw);
        Ok(())
    }

    /// A key of the PIN's keypad was pressed.
    ///
    /// # Errors
    /// As [`Flow::submit`]; a digit that cannot be kept leaves the digits as
    /// they were.
    pub fn press(
        &mut self,
        key: usize,
        lock: &mut Lockstate,
        room: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        match self.pin.press(key)? {
            Pressed::Submit(digits) => return self.submit(digits, lock, room, fx),
            Pressed::Changed => {}
        }
        self.message = Message::None;
        fx.push(Effect::Redraw);
        Ok(())
    }

    /// Adds dot `dot` to the pattern, once.
    ///
    /// # Errors
    /// `NoMemory` when the dot cannot be kept.
    pub fn add_dot(&mut self, dot: usize) -> Result<bool, Error> {
        model::add_dot(&mut self.dots, dot)
    }

    /// The thing the buttons chose was confirmed.
    ///
    /// # Errors
    /// As [`Flow::submit`].
    pub fn choose(
        &mut self,
        lock: &mut Lockstate,
        room: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        match self.kind {
            "pattern" if self.focus == PATTERN_OK => {
                let entered = core::mem::take(&mut self.dots);
                self.submit(entered, lock, room, fx)
            }
            "pattern" => {
                self.add_dot(self.focus)?;
                fx.push(Effect::Redraw);
                Ok(())
            }
            "password" => {
                self.asked = true;
                fx.push(Effect::AskPassword);
                Ok(())
            }
            _ => self.press(self.focus, lock, room, fx),
        }
    }

    /// *Back*: closes the screen, or while unlocking takes the last digit or
    /// dot away.
    pub fn back(&mut self, fx: &mut Effects) {
        if self.step != Step::Unlock {
            fx.push(Effect::Back);
            return;
        }
        let was = self.pin.len() + self.dots.len();
        self.pin.press(DELETE_KEY).ok();
        self.dots.pop();
        if self.pin.len() + self.dots.len() != was {
            fx.push(Effect::Redraw);
        }
    }

    /// The password's button, or the keypad's key, was tapped.
    ///
    /// # Errors
    /// As [`Flow::submit`].
    pub fn tapped_key(
        &mut self,
        key: usize,
        lock: &mut Lockstate,
        room: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        self.shown = false;
        self.press(key, lock, room, fx)
    }

    /// The password's button was tapped.
    pub fn tapped_password(&mut self, fx: &mut Effects) {
        self.shown = false;
        self.asked = true;
        fx.push(Effect::AskPassword);
    }

    /// The screen is shown: asks for the password where one is due.
    pub fn shown_screen(&mut self, fx: &mut Effects) {
        if self.step != Step::Choose && self.kind == "password" && !self.asked {
            self.asked = true;
            fx.push(Effect::AskPassword);
        }
    }

    /// A finger went down on dot `dot` (if on one).
    ///
    /// # Errors
    /// `NoMemory` when the dot cannot be kept.
    pub fn finger_down(
        &mut self,
        id: u32,
        at: Point,
        dot: Option<usize>,
        fx: &mut Effects,
    ) -> Result<bool, Error> {
        let Some(dot) = dot else {
            return Ok(false);
        };
        self.clear_entered();
        self.message = Message::None;
        self.shown = false;
        self.add_dot(dot)?;
        self.finger = Some((id, at));
        fx.push(Effect::Redraw);
        Ok(true)
    }

    /// The finger moved, to dot `dot` if on one.
    ///
    /// # Errors
    /// `NoMemory` when the dot cannot be kept.
    pub fn finger_moved(
        &mut self,
        id: u32,
        at: Point,
        dot: Option<usize>,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        if let Some(dot) = dot {
            self.add_dot(dot)?;
        }
        let moved = self
            .finger
            .is_some_and(|(_, was)| (was.x - at.x).abs() + (was.y - at.y).abs() > 8);
        if moved {
            self.finger = Some((id, at));
            fx.push(Effect::Redraw);
        }
        Ok(())
    }

    /// The finger was lifted: the pattern is the code.
    ///
    /// # Errors
    /// As [`Flow::submit`].
    pub fn finger_up(
        &mut self,
        lock: &mut Lockstate,
        room: &Context<'_>,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        let entered = core::mem::take(&mut self.dots);
        self.submit(entered, lock, room, fx)
    }

    /// The contact was cancelled: the pattern is dropped.
    pub fn finger_cancelled(&mut self, fx: &mut Effects) {
        self.clear_entered();
        fx.push(Effect::Redraw);
    }
}

/// The setting's value for a kind.
fn kind_value(kind: &str) -> &'static str {
    model::KIND_VALUES
        .iter()
        .copied()
        .find(|v| v.strip_prefix(model::KIND_PREFIX) == Some(kind))
        .unwrap_or(model::KIND_VALUES[0])
}

/// The wrong tries before the next one waits (for the screen's own use).
pub const WRONG_TRIES: u8 = TRIES;
