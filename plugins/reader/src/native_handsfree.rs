//! Pages that turn by themselves (view slice R8c): the action
//! `reader.auto-turn` starts and stops it, the host's work service
//! (`core.work`, `docs/abi/work.md`) brings the time, and the page turns every
//! `reader.auto-turn-seconds` seconds, as Beta 3's `auto_turn` did.
//!
//! The timer is one work with the key [`KEY`], declared once and again after
//! each turn (a key declared again replaces the work, its time counted from
//! now), so at most one is waiting and a stopped timer leaves nothing behind.
//! What is decided without a host (the interval, what an input does to the
//! timer, the model's states) is [`AutoTurn`] and [`classify`], tested
//! without one; [`Hands`] is the glue. Nothing is allocated at a tick: the
//! buffers of the calls are on the stack (at most 512 bytes).
use crate::{
    declarations::{ACTIONS, FIRST_ACTION, WORK_DUE},
    native_pages::{self, Turn},
};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::Input,
        commands::CommandSender,
        formatting::{self, Arg, FormatBuffers},
        settings::{self, SettingValue},
        ui::{Instance, Notify},
        work::{Work, WorkSpec},
    },
};

/// The key of the work that turns the page.
pub const KEY: &str = "auto-turn";
/// The action that starts and stops the timer (the full id).
pub const ACTION: &str = "reader.auto-turn";
/// The setting with the seconds between two pages.
pub const SETTING: &str = "reader.auto-turn-seconds";
/// The action a due timer asks of the reading screen.
pub const NEXT_PAGE: &str = "core.next-page";
/// Beta 3's bounds of the interval, in seconds (the setting's own).
pub const MIN_SECONDS: i64 = 5;
pub const MAX_SECONDS: i64 = 600;
/// The setting's default.
pub const DEFAULT_SECONDS: i64 = 30;

const STARTED: &str = "reader.auto-turn.started";
const STOPPED: &str = "reader.auto-turn.stopped";
const UNAVAILABLE: &str = "reader.auto-turn.unavailable";

/// The interval in seconds, within Beta 3's bounds (integers only).
#[must_use]
pub const fn clamp_seconds(seconds: i64) -> i64 {
    if seconds < MIN_SECONDS {
        MIN_SECONDS
    } else if seconds > MAX_SECONDS {
        MAX_SECONDS
    } else {
        seconds
    }
}

/// The interval in milliseconds for a setting's value.
#[must_use]
pub const fn interval_ms(seconds: i64) -> u32 {
    // At most 600,000: well inside `u32` and the work service's seven days.
    (clamp_seconds(seconds) * 1000) as u32
}

/// The callback token of the action `reader.auto-turn` (`FIRST_ACTION` + its
/// place in `ACTIONS`); `u32::MAX` if the list lacks it (a test holds it).
#[must_use]
pub const fn action_token() -> u32 {
    token_of(b"auto-turn")
}

/// The callback token of the reader's action `name` (`FIRST_ACTION` + its place
/// in `ACTIONS`); `u32::MAX` if the list lacks it.
#[must_use]
pub const fn token_of(want: &[u8]) -> u32 {
    let mut at = 0;
    while at < ACTIONS.len() {
        let name = ACTIONS[at].0.as_bytes();
        let mut same = name.len() == want.len();
        let mut i = 0;
        while same && i < want.len() {
            same = name[i] == want[i];
            i += 1;
        }
        if same {
            return FIRST_ACTION + at as u32;
        }
        at += 1;
    }
    u32::MAX
}

/// Whether `token` is the token of one of the reader's own actions.
#[must_use]
pub const fn is_reader_action(token: u32) -> bool {
    token >= FIRST_ACTION && (token - FIRST_ACTION) < ACTIONS.len() as u32
}

/// What an input means to the timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The action that starts and stops the timer.
    Toggle,
    /// A page turn by hand: the count starts anew (Beta 3).
    Turn,
    /// The menu or the confirm button (a tap in the middle): stops it and is
    /// taken (Beta 3).
    Stop,
    /// Any other action, gesture or widget input: stops it, and goes on.
    Other,
    /// A raw key or contact (the host resolves it to an action or gesture) and
    /// what a later host adds.
    Ignored,
}

/// What `input` is to the timer.
#[must_use]
pub fn classify(input: &Input<'_>) -> Kind {
    match *input {
        Input::Action { action, .. } => {
            if action == ACTION {
                Kind::Toggle
            } else if native_pages::turn(input) != Turn::Ignored {
                Kind::Turn
            } else if matches!(action, "core.menu" | "core.confirm") {
                Kind::Stop
            } else {
                Kind::Other
            }
        }
        Input::Gesture { .. } | Input::Widget { .. } => Kind::Other,
        _ => Kind::Ignored,
    }
}

/// What the timer's model did on a toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Started,
    Stopped,
}

/// What an input did to the timer, and what is left to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// Nothing: the input is the screen's.
    Pass,
    /// The input is a toggle.
    Toggle,
    /// A page turn: the timer is set again, the screen turns the page.
    Rearm,
    /// The timer stopped; `said` asks to tell the user, `taken` that the input
    /// is used up.
    Stopped { said: bool, taken: bool },
}

/// The timer: running for one screen instance, or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoTurn<I> {
    running: Option<I>,
}

impl<I> Default for AutoTurn<I> {
    fn default() -> Self {
        Self { running: None }
    }
}

impl<I: Copy + PartialEq> AutoTurn<I> {
    /// The screen the timer runs for.
    #[must_use]
    pub const fn running(&self) -> Option<I> {
        self.running
    }

    /// Starts it for `on`, or stops it wherever it runs (Beta 3's toggle).
    pub fn toggle(&mut self, on: I) -> Change {
        if self.running.take().is_some() {
            Change::Stopped
        } else {
            self.running = Some(on);
            Change::Started
        }
    }

    /// Stops it; whether it was running.
    pub fn stop(&mut self) -> bool {
        self.running.take().is_some()
    }

    /// Stops it if it runs for `instance` (the screen closed); whether it did.
    pub fn left(&mut self, instance: I) -> bool {
        if self.running == Some(instance) {
            self.running = None;
            return true;
        }
        false
    }

    /// What an input of kind `kind` on the screen `on` does to the timer.
    pub fn react(&mut self, on: I, kind: Kind) -> Reaction {
        match (kind, self.running) {
            (Kind::Toggle, _) => Reaction::Toggle,
            (Kind::Ignored, _) | (_, None) => Reaction::Pass,
            // An input of another screen than the one turning: it stops.
            (_, Some(at)) if at != on => {
                self.running = None;
                Reaction::Stopped {
                    said: false,
                    taken: false,
                }
            }
            (Kind::Turn, Some(_)) => Reaction::Rearm,
            (Kind::Stop, Some(_)) => {
                self.running = None;
                Reaction::Stopped {
                    said: true,
                    taken: true,
                }
            }
            (_, Some(_)) => {
                self.running = None;
                Reaction::Stopped {
                    said: false,
                    taken: false,
                }
            }
        }
    }
}

/// The interval the setting asks for, in seconds (the default where it cannot
/// be read).
fn seconds(host: &Host<'_>) -> i64 {
    let mut bytes = [0; 32];
    match settings::read_setting(host, SETTING, &mut bytes) {
        Ok(SettingValue::Int(value)) => clamp_seconds(value),
        _ => DEFAULT_SECONDS,
    }
}

/// Declares the work again: due in `seconds`.
fn arm(host: &Host<'_>, work: &Work, seconds: i64) -> Result<(), Error> {
    let spec = WorkSpec::new(KEY, WORK_DUE)?.after_ms(interval_ms(seconds));
    // The host asks for room for the op's answer (`max_reply` 64).
    work.declare(host, &spec, &mut [0; 256], &mut [0; 64])
}

/// Takes the work back; a refusal leaves a due call the model ignores.
fn disarm(host: &Host<'_>, work: Option<&Work>) {
    if let Some(work) = work {
        work.cancel(host, KEY, &mut [0; 256], &mut [0; 64]).ok();
    }
}

/// Says `key` as a message of the device (not from a run: the host refuses a
/// message there); `seconds` fills `{seconds}`. A text that does not fit the
/// stack buffers, or a refusal, says nothing.
fn say(host: &Host<'_>, key: &str, seconds: Option<i64>) {
    let (mut scratch, mut out) = ([0; 128], [0; 256]);
    let length = match seconds {
        Some(value) => formatting::format_message(
            host,
            key,
            &[Arg::Int {
                name: "seconds",
                value,
            }],
            &mut FormatBuffers::new(&mut scratch, &mut out),
        ),
        None => formatting::copy_message(host, key, &mut out).map(str::len),
    };
    let Some(text) = length
        .ok()
        .and_then(|n| out.get(..n))
        .and_then(|bytes| core::str::from_utf8(bytes).ok())
    else {
        return;
    };
    if let Ok(note) = Notify::new(text) {
        let mut commands = [0; 512];
        CommandSender::new(host, &mut commands).notify(note).ok();
    }
}

/// What a due timer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// The timer is not running (a call that was due before it stopped).
    Idle,
    /// The page turned, the timer is set again: the quiet time starts.
    Turned,
    /// The page did not turn (the end of the book, or a refusal): stopped.
    Stopped,
}

/// The timer of the reading screens, with the host.
#[derive(Debug, Default)]
pub struct Hands {
    model: AutoTurn<Instance>,
}

impl Hands {
    /// Starts or stops the timer for `top`, the reading screen shown; where
    /// the host has no work (or refuses it) it says so and starts nothing.
    pub fn toggle(&mut self, host: &Host<'_>, top: Option<Instance>, work: Option<&Work>) {
        let Some(top) = top else {
            return;
        };
        if self.model.toggle(top) == Change::Stopped {
            disarm(host, work);
            say(host, STOPPED, None);
            return;
        }
        let seconds = seconds(host);
        if work.is_some_and(|work| arm(host, work, seconds).is_ok()) {
            say(host, STARTED, Some(seconds));
        } else {
            self.model.stop();
            say(host, UNAVAILABLE, None);
        }
    }

    /// An input of the reading screen `top`: `Some` where the timer used it up.
    /// A turn by hand sets the time again (the screen turns the page after).
    pub fn input(
        &mut self,
        host: &Host<'_>,
        top: Option<Instance>,
        input: &Input<'_>,
        work: Option<&Work>,
    ) -> Option<Handled> {
        let top = top?;
        match self.model.react(top, classify(input)) {
            Reaction::Pass => None,
            Reaction::Toggle => {
                self.toggle(host, Some(top), work);
                Some(Handled::Yes)
            }
            Reaction::Rearm => {
                let armed = work.is_some_and(|work| arm(host, work, seconds(host)).is_ok());
                if !armed {
                    self.model.stop();
                }
                None
            }
            Reaction::Stopped { said, taken } => {
                disarm(host, work);
                if said {
                    say(host, STOPPED, None);
                }
                taken.then_some(Handled::Yes)
            }
        }
    }

    /// The timer is due: `turn` asks the screen it runs for to turn the page
    /// (with the input given) and answers whether the page moved. Set again
    /// when it did; else (the end of the book, a refusal) stopped, with no
    /// message (a run is quiet).
    pub fn due(
        &mut self,
        host: &Host<'_>,
        (now_ms, work): (u64, Option<&Work>),
        turn: impl FnOnce(Instance, &Input<'_>) -> Result<bool, Error>,
    ) -> Due {
        let Some(instance) = self.model.running() else {
            return Due::Idle;
        };
        let input = Input::Action {
            now_ms,
            action: NEXT_PAGE,
        };
        let moved = turn(instance, &input).unwrap_or(false);
        if moved && work.is_some_and(|work| arm(host, work, seconds(host)).is_ok()) {
            Due::Turned
        } else {
            self.model.stop();
            Due::Stopped
        }
    }

    /// The reading screen `instance` closed: the timer for it stops.
    pub fn closed(&mut self, host: &Host<'_>, instance: Instance, work: Option<&Work>) {
        if self.model.left(instance) {
            disarm(host, work);
        }
    }

    /// The reader is left (another screen opens, the device sleeps or
    /// stops, an action of the reader runs): the timer stops, quietly.
    pub fn leave(&mut self, host: &Host<'_>, work: Option<&Work>) {
        if self.model.stop() {
            disarm(host, work);
        }
    }
}

#[cfg(test)]
mod tests;
