//! Declared work: what a plugin wants done, with what it needs and by
//! when, run by the kernel at the moment that costs least (ADR 0025).
//!
//! A plugin does not reach the network on its own account in the
//! background. It declares the work ([`Command::Work`]), and when it is
//! time the kernel emits the work's event with its parameters, as a
//! timer would; the plugin does the work then. The kernel runs together
//! everything that needs the network, so a reading session costs one
//! short connection when it starts and one when it ends:
//!
//! ```
//! use core::time::Duration;
//! use paperpaw_sdk::work::{Moments, Work};
//! use paperpaw_sdk::{Command, Id};
//!
//! const PUSH: Id = Id::new_static("kosync.push");
//! // The position: at the next natural moment, at the latest in 30 min.
//! let push = Work::network(PUSH)
//!     .expected()
//!     .within(Duration::from_secs(30 * 60))
//!     .at_moments(Moments::BOOK_CLOSED | Moments::SLEEP | Moments::ONLINE);
//! let command = Command::work(push);
//! # let _ = command;
//! ```
//!
//! - **The id** is an event of the plugin, and a **key** tells apart
//!   several works of one event ([`Work::key`], e.g. a timer for each
//!   rule). Declaring the same id and key again replaces the work;
//!   [`Command::CancelWork`] takes it back.
//! - **What it needs**: nothing, the network ([`Work::network`]), the
//!   device charging ([`Work::charging`]).
//! - **How much the reader counts on it**: [`expected`](Work::expected)
//!   work is done, at the latest by its deadline;
//!   [`optional`](Work::optional) work is done when it is cheap and left
//!   out when it is not.
//! - **When**: not before ([`after`](Work::after)), at the latest
//!   ([`within`](Work::within); without one, only when it is cheap), or
//!   exactly ([`Work::at`], a timer); [`every`](Work::every) declares it
//!   again after each run.
//! - **At which moments** it may run ([`Moments`]): waking, a book opened
//!   or closed, going to sleep, the radio on anyway, charging, idle.
//!
//! **Its event is background work**, whatever brought it due: what the
//! handler says becomes the plugin's note, and its requests are not the
//! reader's. Its requests keep the radio on until they are answered.
//!
//! **Jobs** are work in steps: long work (an install, a catalog synced or
//! walked, a book's pages counted) registers a step function for its id
//! ([`WorkSpec`], [`Registrar::add_work`]). When it runs, the kernel calls
//! the step instead of emitting the event, again while it answers
//! [`Step::Continue`], each time after the frame is drawn and only when no
//! input waits, so the device answers at once and sleeps between steps.
//! A step should take a few milliseconds.
//!
//! [`Registrar::add_work`]: crate::Registrar::add_work
//!
//! **Nothing is refused silently, nothing nags.** Work held back by the
//! power mode stays queued and runs when allowed; work that can never run
//! (the reader switched the plugin's network off, the manifest does not
//! declare it) is taken out with [`events::WORK_REFUSED`] and a
//! [`Refusal`] saying why and when to try again. A request that does not
//! go out is answered with one too ([`HttpReply::refused`]). None of this
//! is ever shown over reading: background work cannot raise notices, its
//! [`Command::Notify`] becomes the plugin's note, shown on its own page,
//! the Power page and the battery view.
//!
//! [`Command::Work`]: crate::Command::Work
//! [`Command::CancelWork`]: crate::Command::CancelWork
//! [`Command::Notify`]: crate::Command::Notify
//! [`events::WORK_REFUSED`]: crate::builtin::events::WORK_REFUSED
//! [`HttpReply::refused`]: crate::HttpReply::refused

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::ops::BitOr;
use core::time::Duration;

use paperpaw_platform::Instant;

use crate::env::Env;
use crate::event::EventData;
use crate::id::Id;
use crate::params::Params;

/// Natural moments at which work may run, cheaply: the radio is on
/// anyway, or about to be for the reader.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Moments(u32);

impl Moments {
    pub const NONE: Moments = Moments(0);
    /// The device started or woke up.
    pub const WAKE: Moments = Moments(1);
    /// A book was opened.
    pub const BOOK_OPENED: Moments = Moments(1 << 1);
    /// A book was closed.
    pub const BOOK_CLOSED: Moments = Moments(1 << 2);
    /// The device goes to sleep.
    pub const SLEEP: Moments = Moments(1 << 3);
    /// The radio is on anyway, for the reader or for other work.
    pub const ONLINE: Moments = Moments(1 << 4);
    /// The device was plugged in.
    pub const CHARGING: Moments = Moments(1 << 5);
    /// No input for a while, the screen still on.
    pub const IDLE: Moments = Moments(1 << 6);
    /// Bluetooth is on anyway (something else uses it).
    pub const BLUETOOTH_ON: Moments = Moments(1 << 7);
    pub const ALL: Moments = Moments(0xff);

    const NAMES: [(Moments, &'static str); 8] = [
        (Moments::WAKE, "wake"),
        (Moments::BOOK_OPENED, "book-opened"),
        (Moments::BOOK_CLOSED, "book-closed"),
        (Moments::SLEEP, "sleep"),
        (Moments::ONLINE, "online"),
        (Moments::CHARGING, "charging"),
        (Moments::IDLE, "idle"),
        (Moments::BLUETOOTH_ON, "bluetooth-on"),
    ];

    /// Whether every moment of `other` is among these.
    pub const fn contains(self, other: Moments) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any moment of `other` is among these.
    pub const fn intersects(self, other: Moments) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn from_bits(bits: u32) -> Moments {
        Moments(bits & Moments::ALL.0)
    }

    /// Their names, `,` between them, e.g. `book-closed,sleep`.
    pub fn names(self) -> String {
        let mut text = String::new();
        for (moment, name) in Self::NAMES {
            if self.contains(moment) {
                if !text.is_empty() {
                    text.push(',');
                }
                text.push_str(name);
            }
        }
        text
    }

    /// The moments named in `text` ([`names`](Self::names)); unknown
    /// names are passed over.
    pub fn parse(text: &str) -> Moments {
        text.split(',')
            .filter_map(|name| {
                Self::NAMES
                    .iter()
                    .find(|(_, n)| *n == name.trim())
                    .map(|(m, _)| *m)
            })
            .fold(Moments::NONE, |all, m| all | m)
    }
}

impl BitOr for Moments {
    type Output = Moments;

    fn bitor(self, other: Moments) -> Moments {
        Moments(self.0 | other.0)
    }
}

/// How much the reader counts on a work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Importance {
    /// The reader counts on it (a position, an export they sent): it is
    /// done, at the latest by its deadline (made longer by the mode).
    #[default]
    Expected,
    /// Nice to have (weather, headlines, a catalog refreshed): done when
    /// it is cheap; in Saving only with the radio on anyway or charging,
    /// in Offline not at all.
    Optional,
}

/// Something a plugin wants done (see the [module](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Work {
    /// The event emitted when it runs (or the step function registered
    /// for it); the same id and key replace it.
    pub id: Id,
    /// Tells apart several works of one id; empty for one.
    pub key: String,
    /// Passed with the event.
    pub params: Params,
    /// What it needs: the network, Bluetooth, the device charging.
    pub needs: Needs,
    pub importance: Importance,
    /// An exact time: a timer, run then whatever else holds.
    pub at: Option<Instant>,
    /// Not before this long from now.
    pub after: Option<Duration>,
    /// At the latest this long from now (made longer by the mode); none:
    /// only when it is cheap, never on its own account.
    pub within: Option<Duration>,
    /// Declared again this long after each run.
    pub every: Option<Duration>,
    /// The moments at which it may run.
    pub moments: Moments,
}

impl Work {
    /// Work needing nothing, at any moment (e.g. counting pages while
    /// idle: add [`at_moments`](Self::at_moments)).
    pub fn new(id: impl Into<Id>) -> Self {
        Self {
            id: id.into(),
            key: String::new(),
            params: Params::new(),
            needs: Needs::NOTHING,
            importance: Importance::Expected,
            at: None,
            after: None,
            within: None,
            every: None,
            moments: Moments::ALL,
        }
    }

    /// Work needing the network.
    pub fn network(id: impl Into<Id>) -> Self {
        Self {
            needs: Needs::NETWORK,
            ..Self::new(id)
        }
    }

    /// Work needing Bluetooth (reading a sensor now and then): it runs
    /// at the moments the network's would, and its commands may switch
    /// the radio on.
    pub fn bluetooth(id: impl Into<Id>) -> Self {
        Self {
            needs: Needs::BLUETOOTH,
            ..Self::new(id)
        }
    }

    /// Network work no reader waits for, e.g. an action a rule ran: at
    /// the next moment the radio is on anyway, on waking or charging, at
    /// the latest `within` from now (longer while saving), held in
    /// Offline. For the reader's own doing, act at once instead
    /// ([`Env::reader`](crate::Env)).
    pub fn when_cheap(id: impl Into<Id>, within: Duration) -> Self {
        Self::network(id)
            .within(within)
            .at_moments(Moments::WAKE | Moments::ONLINE | Moments::CHARGING)
    }

    /// A timer: the event at `at`, exactly, whatever screen is open.
    pub fn at(id: impl Into<Id>, at: Instant) -> Self {
        Self {
            at: Some(at),
            moments: Moments::NONE,
            ..Self::new(id)
        }
    }

    pub fn with(mut self, params: Params) -> Self {
        self.params = params;
        self
    }

    /// One of several works of the same id, e.g. a rule's timer.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }

    pub fn expected(mut self) -> Self {
        self.importance = Importance::Expected;
        self
    }

    pub fn optional(mut self) -> Self {
        self.importance = Importance::Optional;
        self
    }

    /// Only while the device charges (e.g. a large download).
    pub fn charging(mut self) -> Self {
        self.needs = self.needs | Needs::CHARGING;
        self
    }

    pub fn after(mut self, delay: Duration) -> Self {
        self.after = Some(delay);
        self
    }

    pub fn within(mut self, deadline: Duration) -> Self {
        self.within = Some(deadline);
        self
    }

    pub fn every(mut self, period: Duration) -> Self {
        self.every = Some(period);
        self
    }

    pub fn at_moments(mut self, moments: Moments) -> Self {
        self.moments = moments;
        self
    }

    /// Again after a failed attempt (the first is 1): not before 30 s,
    /// doubling up to 6 h, with up to a tenth more so that devices do not
    /// all return at once (`jitter`, 0 to 255, e.g. a random byte).
    pub fn retry(self, attempt: u32, jitter: u8) -> Self {
        self.after(backoff(attempt, jitter))
    }
}

/// The first wait after a failure.
pub const RETRY_FIRST: Duration = Duration::from_secs(30);
/// The longest wait between attempts.
pub const RETRY_MOST: Duration = Duration::from_secs(6 * 3600);

/// How long to wait before `attempt` (the first is 1): [`RETRY_FIRST`]
/// doubling up to [`RETRY_MOST`], plus up to a tenth by `jitter`.
pub fn backoff(attempt: u32, jitter: u8) -> Duration {
    let first = RETRY_FIRST.as_millis() as u64;
    let most = RETRY_MOST.as_millis() as u64;
    let doubled = first.saturating_mul(1u64 << attempt.saturating_sub(1).min(20));
    let base = doubled.min(most);
    let extra = base / 10 * u64::from(jitter) / 255;
    Duration::from_millis(base + extra)
}

/// Why a work or a request did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Reason {
    /// The mode is Offline: network work waits for the reader's next
    /// connection.
    Offline,
    /// The reader switched this plugin's network off.
    NetworkOff,
    /// The plugin's manifest does not say it uses the network
    /// ([`Uses::NETWORK`](crate::Uses::NETWORK)).
    NotDeclared,
    /// The mode defers it to a cheaper moment.
    Saving,
    /// No known network, or joining it failed.
    NoNetwork,
    /// The reader switched Bluetooth off, or there is none.
    BluetoothOff,
    /// The device switched off before it was done.
    Sleeping,
    /// The plugin asked again before it was told it might.
    TooOften,
    /// The plugin took it back ([`Command::CancelWork`],
    /// [`Command::CancelHttp`]).
    ///
    /// [`Command::CancelWork`]: crate::Command::CancelWork
    /// [`Command::CancelHttp`]: crate::Command::CancelHttp
    Cancelled,
}

impl Reason {
    const NAMES: [(Reason, &'static str); 9] = [
        (Reason::Offline, "offline"),
        (Reason::NetworkOff, "network-off"),
        (Reason::NotDeclared, "not-declared"),
        (Reason::Saving, "saving"),
        (Reason::NoNetwork, "no-network"),
        (Reason::BluetoothOff, "bluetooth-off"),
        (Reason::Sleeping, "sleeping"),
        (Reason::TooOften, "too-often"),
        (Reason::Cancelled, "cancelled"),
    ];

    pub fn as_str(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(r, _)| *r == self)
            .map_or("", |(_, n)| n)
    }

    pub fn parse(text: &str) -> Option<Reason> {
        Self::NAMES
            .iter()
            .find(|(_, n)| *n == text)
            .map(|(r, _)| *r)
    }
}

/// When a refused work or request may be tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Retry {
    /// Not before then.
    After(Instant),
    /// At the next of these moments.
    At(Moments),
    /// When the mode or the reader allows it, told by
    /// [`events::WORK_ALLOWED`](crate::builtin::events::WORK_ALLOWED).
    WhenAllowed,
    /// Not without the reader changing something.
    Never,
}

impl Retry {
    fn write(self) -> String {
        match self {
            Retry::After(at) => alloc::format!("after:{}", at.millis()),
            Retry::At(moments) => alloc::format!("at:{}", moments.names()),
            Retry::WhenAllowed => String::from("allowed"),
            Retry::Never => String::from("never"),
        }
    }

    fn parse(text: &str) -> Option<Retry> {
        if let Some(millis) = text.strip_prefix("after:") {
            return millis.parse().ok().map(|m| Retry::After(Instant(m)));
        }
        if let Some(names) = text.strip_prefix("at:") {
            return Some(Retry::At(Moments::parse(names)));
        }
        match text {
            "allowed" => Some(Retry::WhenAllowed),
            "never" => Some(Retry::Never),
            _ => None,
        }
    }
}

/// What declared work needs ([`Work::needs`]): a set, so that what is
/// needed later (another radio) is a new constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Needs(u8);

impl Needs {
    pub const NOTHING: Needs = Needs(0);
    /// The network: it runs when the radio is on anyway, or switches it
    /// on when due.
    pub const NETWORK: Needs = Needs(1);
    /// Bluetooth ([`bluetooth`](crate::bluetooth)).
    pub const BLUETOOTH: Needs = Needs(2);
    /// The device charging: it runs only then.
    pub const CHARGING: Needs = Needs(4);

    pub const fn contains(self, other: Needs) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl core::ops::BitOr for Needs {
    type Output = Needs;

    fn bitor(self, other: Needs) -> Needs {
        Needs(self.0 | other.0)
    }
}

/// Why a work or a request did not run, and when to try again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Refusal {
    pub reason: Reason,
    pub retry: Retry,
}

impl Refusal {
    pub const fn new(reason: Reason, retry: Retry) -> Self {
        Self { reason, retry }
    }

    /// Writes it into `params` ([`params::REFUSED`], [`params::RETRY`]).
    pub fn write(&self, params: &mut Params) {
        params.set(params::REFUSED, self.reason.as_str().to_string());
        params.set(params::RETRY, self.retry.write());
    }

    /// The refusal in `params`, if there is one.
    pub fn read(params: &Params) -> Option<Refusal> {
        let reason = Reason::parse(params.get(params::REFUSED)?)?;
        let retry = params
            .get(params::RETRY)
            .and_then(Retry::parse)
            .unwrap_or(Retry::Never);
        Some(Refusal { reason, retry })
    }
}

/// Parameters the kernel adds to work's events and refused replies.
pub mod params {
    /// Why a work runs now: `time`, `deadline` or a moment's name
    /// ([`Moments::names`](super::Moments::names)).
    pub const WHY: &str = "work-why";
    /// The work a refusal is about ([`WorkRefused`](super::WorkRefused)).
    pub const WORK: &str = "work";
    /// A refusal's reason ([`Reason::as_str`](super::Reason::as_str)).
    pub const REFUSED: &str = "refused";
    /// When to try again.
    pub const RETRY: &str = "retry";
}

/// Why a work runs now, from its event's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// Its exact time ([`Work::at`]).
    Time,
    /// Its deadline came.
    Deadline,
    /// One of its moments.
    Moment(Moments),
}

impl Why {
    /// Writes it into a work's event parameters.
    pub fn write(self, params: &mut Params) {
        let text = match self {
            Why::Time => String::from("time"),
            Why::Deadline => String::from("deadline"),
            Why::Moment(moments) => moments.names(),
        };
        params.set(params::WHY, text);
    }

    /// Why the work whose event brought `params` runs.
    pub fn read(params: &Params) -> Option<Why> {
        match params.get(params::WHY)? {
            "time" => Some(Why::Time),
            "deadline" => Some(Why::Deadline),
            names => Some(Why::Moment(Moments::parse(names))).filter(|w| match w {
                Why::Moment(m) => !m.is_empty(),
                _ => true,
            }),
        }
    }
}

/// A declared work taken out because it can never run
/// ([`events::WORK_REFUSED`](crate::builtin::events::WORK_REFUSED)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorkRefused {
    pub work: Id,
    pub refusal: Refusal,
}

impl WorkRefused {
    /// Work `work` taken out, for `refusal`.
    pub fn new(work: Id, refusal: Refusal) -> Self {
        Self { work, refusal }
    }
}

impl EventData for WorkRefused {
    fn to_params(&self) -> Params {
        let mut params = Params::new().with(params::WORK, self.work.as_str().to_string());
        self.refusal.write(&mut params);
        params
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(Self {
            work: Id::parse(params.get(params::WORK)?).ok()?,
            refusal: Refusal::read(params)?,
        })
    }
}

/// Where a declared work stands, for a plugin's page ("Waiting for the
/// next connection", "Held: Offline mode").
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkState {
    /// Queued; at the latest at `due` (none: only when it is cheap).
    Waiting { due: Option<Instant> },
    /// Queued, but held back, e.g. by the mode.
    Held(Refusal),
    /// A job whose steps go on; its progress, if it says. (Work without
    /// steps runs at once and is gone, or waits again for its period.)
    Running { progress: Option<Progress> },
}

/// How far a job is: `done` of `of` (files, entries, pages).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Progress {
    pub done: u32,
    pub of: u32,
}

impl Progress {
    /// `done` of `of`.
    pub const fn new(done: u32, of: u32) -> Progress {
        Progress { done, of }
    }

    /// In percent, 0 to 100.
    pub fn percent(self) -> u8 {
        if self.of == 0 {
            return 0;
        }
        (u64::from(self.done.min(self.of)) * 100 / u64::from(self.of)) as u8
    }
}

/// What a job's step says ([`WorkSpec`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// More to do: the next step after the frame, when no input waits.
    Continue,
    /// More to do, this far.
    Progress(Progress),
    /// Nothing to do before then.
    Wait(Instant),
    /// Nothing to do before this event, e.g. the reply to its request.
    WaitFor(Id),
    /// Done; declared again after `every`, if it says.
    Done,
    /// Failed: declared again after the backoff ([`backoff`]), with a
    /// line for the plugin's note.
    Failed(String),
}

/// One step of a job: the work's parameters, commands to send.
pub type StepFn = Box<dyn Fn(&Env<'_>, &Params, &mut crate::Commands) -> Step>;

/// A job: a work done in steps (see the [module](self)), registered with
/// [`Registrar::add_work`](crate::Registrar::add_work) and run when a
/// [`Work`] of its id is due.
#[non_exhaustive]
pub struct WorkSpec {
    pub id: Id,
    /// Translation key of its name, shown on the Power page and in the
    /// battery view ("Syncing catalogs").
    pub title: alloc::borrow::Cow<'static, str>,
    pub step: StepFn,
}

impl WorkSpec {
    pub fn new(
        id: impl Into<Id>,
        title: impl Into<alloc::borrow::Cow<'static, str>>,
        step: impl Fn(&Env<'_>, &Params, &mut crate::Commands) -> Step + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            step: Box::new(step),
        }
    }
}

impl fmt::Debug for WorkSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// What a plugin's background work last said, kept by the kernel in
/// place of a notice: background work never shows one over reading.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Note {
    pub at: Instant,
    pub text: String,
}

impl Note {
    /// What a work said at `at`.
    pub fn new(at: Instant, text: impl Into<String>) -> Self {
        Self {
            at,
            text: text.into(),
        }
    }
}

/// Where every declared work stands, and the plugins' notes, kept by the
/// kernel ([`Env::work`](crate::Env)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkStates {
    works: Vec<(Id, String, WorkState)>,
    notes: Vec<(Id, Note)>,
}

/// [`WorkStates::none`].
static NO_WORK: WorkStates = WorkStates {
    works: Vec::new(),
    notes: Vec::new(),
};

impl WorkStates {
    /// No work and no notes, e.g. for tests.
    pub fn none() -> &'static WorkStates {
        &NO_WORK
    }

    /// Where the work with this id stands (its first key, if several).
    pub fn get(&self, id: &Id) -> Option<&WorkState> {
        self.works.iter().find(|(i, ..)| i == id).map(|(.., s)| s)
    }

    /// Where the work with this id and key stands.
    pub fn keyed(&self, id: &Id, key: &str) -> Option<&WorkState> {
        self.works
            .iter()
            .find(|(i, k, _)| i == id && k == key)
            .map(|(.., s)| s)
    }

    /// Every declared work: its id, key and state.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &str, &WorkState)> {
        self.works.iter().map(|(i, k, s)| (i, k.as_str(), s))
    }

    /// What the plugin's background work last said.
    pub fn note(&self, plugin: &Id) -> Option<&Note> {
        self.notes.iter().find(|(p, _)| p == plugin).map(|(_, n)| n)
    }

    /// Sets every work's state at once (for the kernel).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_works(&mut self, works: Vec<(Id, String, WorkState)>) {
        self.works = works;
    }

    /// Keeps a plugin's note in place of the one before (for the kernel).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_note(&mut self, plugin: Id, note: Note) {
        match self.notes.iter_mut().find(|(p, _)| *p == plugin) {
            Some((_, old)) => *old = note,
            None => self.notes.push((plugin, note)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moments_are_named_both_ways() {
        let m = Moments::BOOK_CLOSED | Moments::SLEEP | Moments::ONLINE;
        assert_eq!(m.names(), "book-closed,sleep,online");
        assert_eq!(Moments::parse(&m.names()), m);
        assert_eq!(Moments::parse("sleep, nonsense"), Moments::SLEEP);
        assert!(m.contains(Moments::SLEEP));
        assert!(!m.contains(Moments::SLEEP | Moments::WAKE));
        assert!(m.intersects(Moments::SLEEP | Moments::WAKE));
        assert_eq!(Moments::from_bits(0xff), Moments::ALL);
    }

    #[test]
    fn builders_say_what_they_set() {
        let w = Work::network(Id::new_static("kosync.push"))
            .optional()
            .within(Duration::from_secs(60))
            .every(Duration::from_secs(3600))
            .at_moments(Moments::SLEEP);
        assert!(w.needs.contains(Needs::NETWORK) && !w.needs.contains(Needs::CHARGING));
        assert_eq!(w.importance, Importance::Optional);
        assert_eq!(w.within, Some(Duration::from_secs(60)));
        assert_eq!(w.every, Some(Duration::from_secs(3600)));
        assert_eq!(w.moments, Moments::SLEEP);
        let timer = Work::at(Id::new_static("rules.time"), Instant(5000)).key("rule-3");
        assert_eq!(timer.at, Some(Instant(5000)));
        assert_eq!(timer.key, "rule-3");
        assert!(!timer.needs.contains(Needs::NETWORK));
        assert_eq!(timer.moments, Moments::NONE);
    }

    #[test]
    fn backoff_doubles_up_to_six_hours() {
        assert_eq!(backoff(1, 0), Duration::from_secs(30));
        assert_eq!(backoff(2, 0), Duration::from_secs(60));
        assert_eq!(backoff(3, 0), Duration::from_secs(120));
        assert_eq!(backoff(40, 0), RETRY_MOST);
        assert_eq!(backoff(1, 255), Duration::from_secs(33));
        assert_eq!(backoff(0, 0), Duration::from_secs(30));
    }

    #[test]
    fn refusals_survive_parameters() {
        for retry in [
            Retry::After(Instant(1234)),
            Retry::At(Moments::WAKE | Moments::ONLINE),
            Retry::WhenAllowed,
            Retry::Never,
        ] {
            let refusal = Refusal::new(Reason::Saving, retry);
            let mut params = Params::new();
            refusal.write(&mut params);
            assert_eq!(Refusal::read(&params), Some(refusal));
        }
        assert_eq!(Refusal::read(&Params::new()), None);
        for (reason, _) in Reason::NAMES {
            assert_eq!(Reason::parse(reason.as_str()), Some(reason));
        }
    }

    #[test]
    fn refused_work_is_an_event() {
        let refused = WorkRefused {
            work: Id::new_static("stats.sync"),
            refusal: Refusal::new(Reason::NetworkOff, Retry::Never),
        };
        assert_eq!(
            WorkRefused::from_params(&refused.to_params()),
            Some(refused)
        );
    }

    #[test]
    fn progress_in_percent() {
        assert_eq!(Progress { done: 1, of: 4 }.percent(), 25);
        assert_eq!(Progress { done: 9, of: 4 }.percent(), 100);
        assert_eq!(Progress { done: 3, of: 0 }.percent(), 0);
    }

    #[test]
    fn why_a_work_runs() {
        for why in [Why::Time, Why::Deadline, Why::Moment(Moments::SLEEP)] {
            let mut params = Params::new();
            why.write(&mut params);
            assert_eq!(Why::read(&params), Some(why));
        }
        assert_eq!(Why::read(&Params::new().with(params::WHY, "x")), None);
    }
}
