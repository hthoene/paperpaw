//! The radio's events on the native plugin: a plugin wants the network
//! (`network_wanted`), the network is idle (`network_idle`), networks were
//! found (`wifi_scanned`), a join ended (`wifi_done`), the radio changed, the
//! power mode, the screen shown, charging, and the two timers (idle off,
//! retry) as work through `core.work`. The decisions are
//! [`native_radio`](crate::native_radio); here the facts are read from the host
//! and what comes out is done.
//!
//! Beta 3 policy is kept: nothing here decides.
use alloc::string::String;
use alloc::vec::Vec;

use super::DEVICE_INPUT;
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    events::WifiDone,
    snapshot,
    storage::DataStorage,
    ui::UiCursor,
    work::{Work, WorkDue, WorkSpec},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::text::copy;

use super::views::Info;
use super::{NativeWifi, ON_DEMAND, WHILE_CHARGING, native_known, native_model};
use crate::native_policy::{Failure, Goal, Known, Net, Password, Radio};
use crate::native_radio::{Action, Facts, Notice, Timer};

/// The work that switches the radio off, and the one that tries again.
const IDLE_KEY: &str = "wifi.idle-off";
const RETRY_KEY: &str = "wifi.retry";
/// The tokens the two works come back with.
const IDLE_WORK: u32 = 1;
const RETRY_WORK: u32 = 2;
/// The file that keeps the network the user picked last.
const PICKED: &str = "picked.txt";
/// The *Join first* setting and its two choices.
const JOIN_FIRST: &str = "wifi.join-first";
const FIRST_STRONGEST: &str = "wifi.join-first.strongest";
/// The idle choices and how long each is, in seconds.
const IDLE_SECONDS: [(&str, u64); 5] = [
    ("wifi.idle-off.30s", 30),
    ("wifi.idle-off.1m", 60),
    ("wifi.idle-off.2m", 120),
    ("wifi.idle-off.5m", 300),
    ("wifi.idle-off.10m", 600),
];

/// How much of the networks joined before a decision reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    /// None of it.
    No,
    /// Whether there is any: the names are listed, no password read.
    Any,
    /// Every network with its password.
    All,
}

/// What a decision needs read from the host besides the radio (always read):
/// each fact only where the decision looks at it (`native_radio`).
#[derive(Clone, Copy)]
struct Need {
    /// The networks in reach.
    networks: bool,
    known: Level,
    /// `known` is read only where charging keeps the radio on.
    known_if_charging: bool,
    /// Charging keeps the radio on (the setting, the snapshot, the power).
    charging: bool,
    /// Home or a book shows (the stack).
    at_rest: bool,
    on_demand: bool,
    /// The idle time.
    idle: bool,
    /// The network picked last (the setting, the file).
    join_first: bool,
    /// The name of the network joined (a copy).
    joined: bool,
}

const NOTHING: Need = Need {
    networks: false,
    known: Level::No,
    known_if_charging: false,
    charging: false,
    at_rest: false,
    on_demand: false,
    idle: false,
    join_first: false,
    joined: false,
};

/// What the decisions that look at the networks in reach and the ones joined
/// before (the next to join, the failure) read.
const SCAN: Need = Need {
    networks: true,
    known: Level::All,
    charging: true,
    idle: true,
    join_first: true,
    ..NOTHING
};

/// The facts of one event, owned: read before it is decided.
struct Seen {
    info: Info,
    /// The name of the network joined, where online and asked for.
    joined: String,
    networks: Vec<Net>,
    level: Level,
    known: Vec<Known>,
    /// Stands for the networks joined before where only whether there are any
    /// is read: a name, no password.
    some: Known,
    now_ms: u64,
    charging_keeps: bool,
    at_rest: bool,
    on_demand: bool,
    idle_ms: u64,
    join_first: Option<String>,
}

impl Seen {
    fn facts(&self) -> Facts<'_> {
        Facts {
            radio: self.info.radio,
            busy: self.info.busy,
            wanted: self.info.wanted,
            networks: &self.networks,
            known: match self.level {
                Level::No => &[],
                Level::Any => core::slice::from_ref(&self.some),
                Level::All => &self.known,
            },
            now_ms: self.now_ms,
            charging_keeps: self.charging_keeps,
            at_rest: self.at_rest,
            on_demand: self.on_demand,
            idle_ms: self.idle_ms,
            join_first: self.join_first.as_deref(),
            joined: (!self.joined.is_empty()).then_some(self.joined.as_str()),
        }
    }
}

/// How long the idle choice `key` is, in milliseconds (a minute where it is
/// not one of the choices).
fn idle_ms(key: &str) -> u64 {
    IDLE_SECONDS
        .iter()
        .find(|(choice, _)| *choice == key)
        .map_or(60, |(_, seconds)| *seconds)
        * 1000
}

impl NativeWifi {
    /// Reads what the decision of an event needs.
    fn seen(&mut self, host: &Host<'_>, now_ms: u64, need: Need) -> Result<Seen, Error> {
        self.ensure(host)?;
        if need.joined {
            let named = self.read_named(host)?;
            let joined = if named.radio == Radio::Online {
                named.ssid
            } else {
                String::new()
            };
            return self.seen_with(host, now_ms, named.info, joined, need);
        }
        let info = self.read_info(host)?;
        self.seen_with(host, now_ms, info, String::new(), need)
    }

    /// As [`Self::seen`], the radio read already (and the services open).
    fn seen_with(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        info: Info,
        joined: String,
        need: Need,
    ) -> Result<Seen, Error> {
        // The stack first: where nothing shows at rest, charging is not asked.
        let at_rest = need.at_rest && info.radio.is_on() && self.at_rest(host)?;
        let charging_keeps =
            need.charging && (!need.at_rest || at_rest) && self.charging_keeps(host)?;
        let mut networks = Vec::new();
        if need.networks && info.radio.is_on() {
            self.walk_networks(host, &mut networks)?;
        }
        let mut level = need.known;
        if need.known_if_charging && !charging_keeps {
            level = Level::No;
        }
        let known = if level == Level::All {
            self.read_known(host)?
        } else {
            Vec::new()
        };
        if level == Level::Any && !self.read_kept_any(host)? {
            level = Level::No;
        }
        let join_first = if need.join_first {
            let strongest = matches!(
                Self::text(host, JOIN_FIRST)?.as_deref(),
                Some(FIRST_STRONGEST)
            );
            if strongest { None } else { Self::picked(host)? }
        } else {
            None
        };
        Ok(Seen {
            info,
            joined,
            networks,
            level,
            known,
            some: Known {
                ssid: String::new(),
                password: Password::default(),
            },
            now_ms,
            charging_keeps,
            at_rest,
            on_demand: need.on_demand && Self::flag(host, ON_DEMAND)?,
            idle_ms: if need.idle {
                idle_ms(&Self::idle(host)?)
            } else {
                0
            },
            join_first,
        })
    }

    /// The radio stays on now: charging, and the user wants that, and the
    /// device is online-capable (offline keeps it off also while charging).
    fn charging_keeps(&self, host: &Host<'_>) -> Result<bool, Error> {
        if !Self::flag(host, WHILE_CHARGING)? {
            return Ok(false);
        }
        if !snapshot::capture_snapshot(host)?.is_charging() {
            return Ok(false);
        }
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(DEVICE_INPUT)?, buffer(SERVICE_BUFFER)?);
        match device.power(host, 0, &mut input, &mut out) {
            Ok(power) => Ok(power.online()),
            Err(Error::NotFound | Error::Unsupported) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Home or a book shows (the screen on top is the home's or the reader's):
    /// nothing there uses the network.
    fn at_rest(&self, host: &Host<'_>) -> Result<bool, Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let page = ui.stack(host, UiCursor::start(), &mut input, &mut out)?;
        // The stack is read top first.
        Ok(match page.into_iter().next() {
            Some(row) => {
                let slot = row?.slot();
                slot == "core.home" || slot == "core.reader"
            }
            None => false,
        })
    }

    /// The networks joined before, each with its password; none where the
    /// device keeps none.
    fn read_known(&mut self, host: &Host<'_>) -> Result<Vec<Known>, Error> {
        let names = self.read_kept(host)?;
        let Some(secrets) = self.secrets(host)? else {
            return Ok(Vec::new());
        };
        native_known::known_with(&names, |name| {
            match native_known::password_of(secrets, host, name) {
                Err(Error::NotFound) => Ok(None),
                other => other,
            }
        })
    }

    /// A setting that is text, `None` where it is something else.
    pub(super) fn text(host: &Host<'_>, key: &str) -> Result<Option<String>, Error> {
        use paperpaw_sdk::native::settings::{SettingValue, read_setting};
        let mut scratch = [0; 64];
        match read_setting(host, key, &mut scratch)? {
            SettingValue::Text(text) => Ok(Some(copy(text)?)),
            _ => Ok(None),
        }
    }

    /// The network the user picked last on the page, if one was. A missing or
    /// unreadable file is no pick; a refused allocation is `NoMemory`.
    fn picked(host: &Host<'_>) -> Result<Option<String>, Error> {
        let storage = DataStorage::new(host);
        let Ok(bytes) = paperpaw_sdk::read_file(&storage, PICKED, 256) else {
            return Ok(None);
        };
        let Ok(name) = core::str::from_utf8(&bytes) else {
            return Ok(None);
        };
        let name = name.trim_end_matches(['\r', '\n']);
        if name.is_empty() {
            return Ok(None);
        }
        Ok(Some(copy(name)?))
    }

    /// Does what the decision says.
    fn act(&mut self, host: &Host<'_>, actions: Vec<Action>) -> Result<(), Error> {
        for action in actions {
            match action {
                Action::Step(step) => {
                    self.run(host, native_model::one(step)?)?;
                }
                Action::Off => CommandSender::new(host, &mut [0; 64]).wifi_off()?,
                // Best effort, as the other keeps: a timer that cannot be set
                // must not drop what else the event stages.
                Action::Timer(timer) => native_model::best_effort(self.set_timer(host, timer)),
                Action::Notify(notice) => native_model::best_effort(Self::say(host, &notice)),
                // No network is coming for what waits: it ends now, not after
                // the host's 30 s.
                Action::NoNetwork => CommandSender::new(host, &mut [0; 64]).network_none()?,
                Action::Picked(ssid) => native_model::best_effort(Self::keep_picked(host, &ssid)),
                Action::KeepSecret { ssid, password } => {
                    self.keep_secret(host, &ssid, password.as_str())?;
                }
            }
        }
        Ok(())
    }

    fn say(host: &Host<'_>, notice: &Notice) -> Result<(), Error> {
        match notice {
            Notice::NoneInReach => Self::notice(host, "wifi.none-in-reach"),
            Notice::Failed { ssid, failure } => {
                let key = match failure {
                    Failure::Password => "wifi.failed.password",
                    Failure::NotFound => "wifi.failed.not-found",
                    Failure::Timeout => "wifi.failed.timeout",
                    _ => "wifi.failed.other",
                };
                Self::tell(host, key, ssid)
            }
        }
    }

    /// The network joined on the user's pick is joined first next time.
    fn keep_picked(host: &Host<'_>, ssid: &str) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        let line = paperpaw_ui_kit::text::concat(&[ssid, "\n"])?;
        storage
            .write(PICKED, line.as_bytes())
            .map_err(|_| Error::Io)
    }

    /// Sets a timer as work due at the time it says, replacing the one with
    /// the same key (a timer set again at a later generation is the only one).
    /// Where the host has no work service the timer is not set: the radio is
    /// then switched off when home or a book shows, not after its idle time.
    fn set_timer(&mut self, host: &Host<'_>, timer: Timer) -> Result<(), Error> {
        let (key, token, due_ms, generation) = match timer {
            Timer::IdleOff { due_ms, generation } => (IDLE_KEY, IDLE_WORK, due_ms, generation),
            Timer::Retry { due_ms, generation } => (RETRY_KEY, RETRY_WORK, due_ms, generation),
        };
        let Some(work) = self.work(host)? else {
            return Ok(());
        };
        let spec = WorkSpec::at(key, token, due_ms.max(1))?;
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        work.declare(host, &spec, &mut input, &mut out)?;
        match token {
            IDLE_WORK => self.idle_generation = Some(generation),
            _ => self.retry_generation = Some(generation),
        }
        Ok(())
    }

    /// The work service, `None` where the device has none.
    pub(super) fn work(&mut self, host: &Host<'_>) -> Result<Option<&Work>, Error> {
        if self.work.is_none() {
            match Work::open(host) {
                Ok(work) => self.work = Some(work),
                Err(Error::Unsupported | Error::NotFound | Error::Denied) => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        Ok(self.work.as_ref())
    }

    /// A plugin wants the network: the radio on, the network joined before.
    pub(super) fn network_wanted(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        let need = Need {
            on_demand: true,
            known: Level::Any,
            ..NOTHING
        };
        let seen = self.seen(host, now_ms, need)?;
        let actions = self.policy.on_wanted(&seen.facts())?;
        self.act(host, actions)
    }

    /// Nothing uses the network: off a while later.
    pub(super) fn network_idle(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        let need = Need {
            charging: true,
            idle: true,
            ..NOTHING
        };
        let seen = self.seen(host, now_ms, need)?;
        let actions = self.policy.on_idle(&seen.facts())?;
        self.act(host, actions)
    }

    /// Networks were found: the next network joined before, another scan, or
    /// the failure told.
    pub(super) fn wifi_scanned(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        let seen = self.seen(host, now_ms, SCAN)?;
        let actions = self.policy.on_scanned(&seen.facts())?;
        self.act(host, actions)?;
        self.phone_scanned(host)?;
        self.reread(host)
    }

    /// A join ended. A success is the radio's change (the radio changed event
    /// says it); a failure goes to the policy, so that a wrong password stops
    /// being pending and the next network is tried.
    pub(super) fn wifi_done(&mut self, host: &Host<'_>, done: &WifiDone<'_>) -> Result<(), Error> {
        let Some(failure) = done.failure() else {
            return Ok(());
        };
        let seen = self.seen(host, done.now_ms(), SCAN)?;
        let actions = self
            .policy
            .on_failed(&seen.facts(), done.ssid(), Failure::from(failure))?;
        self.act(host, actions)?;
        self.phone_failed(host, done.now_ms(), done.ssid(), failure)?;
        self.reread(host)
    }

    /// The radio changed.
    pub(super) fn radio_changed(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        let need = Need {
            // Whether any network is joined before, only where charging could
            // join one.
            known: Level::Any,
            known_if_charging: true,
            charging: true,
            idle: true,
            // The network picked is compared with the one just joined, which
            // the user chose.
            join_first: matches!(self.policy.goal, Goal::Chosen { .. }),
            joined: true,
            ..NOTHING
        };
        let seen = self.seen(host, now_ms, need)?;
        let actions = self.policy.on_changed(&seen.facts())?;
        self.act(host, actions)?;
        self.phone_changed(host, now_ms)?;
        self.reread(host)
    }

    /// Another power mode: a radio only charging kept on may go off.
    pub(super) fn power_changed(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        let need = Need {
            charging: true,
            idle: true,
            ..NOTHING
        };
        let seen = self.seen(host, now_ms, need)?;
        let actions = self.policy.on_power(&seen.facts())?;
        self.act(host, actions)
    }

    /// The stack of screens changed: back home or in a book, the radio goes
    /// off now. The radio is read first, and the stack only while it is on:
    /// nothing else is read while it is off.
    pub(super) fn stack_changed(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        self.ensure(host)?;
        let info = self.read_info(host)?;
        if !info.radio.is_on() {
            return Ok(());
        }
        let need = Need {
            at_rest: true,
            charging: true,
            ..NOTHING
        };
        let seen = self.seen_with(host, now_ms, info, String::new(), need)?;
        let actions = self.policy.on_screen_shown(&seen.facts())?;
        self.act(host, actions)
    }

    /// The status changed, or the device started: charging may keep the radio
    /// on, or no longer.
    pub(super) fn charging(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        started: bool,
    ) -> Result<(), Error> {
        let need = Need {
            known: Level::Any,
            known_if_charging: true,
            charging: true,
            idle: true,
            ..NOTHING
        };
        let seen = self.seen(host, now_ms, need)?;
        let actions = self.policy.on_charging(&seen.facts(), started)?;
        self.act(host, actions)
    }

    /// A timer came due: the radio goes off, or another round is tried, if
    /// nothing was wanted since it was set.
    pub(super) fn work_due(&mut self, host: &Host<'_>, due: &WorkDue<'_>) -> Result<(), Error> {
        if due.key() == super::phone::WAKE_KEY {
            return self.phone_wake(host, due.now_ms());
        }
        let (idle, generation) = match due.key() {
            IDLE_KEY => (true, self.idle_generation.take()),
            RETRY_KEY => (false, self.retry_generation.take()),
            _ => return Ok(()),
        };
        let Some(generation) = generation else {
            return Ok(());
        };
        let need = Need {
            charging: true,
            // Another round looks at whether a network was joined before, and
            // may set the next timer.
            known: if idle { Level::No } else { Level::Any },
            idle: !idle,
            ..NOTHING
        };
        let seen = self.seen(host, due.now_ms(), need)?;
        let actions = if idle {
            self.policy.on_idle_timer(&seen.facts(), generation)?
        } else {
            self.policy.on_retry_timer(&seen.facts(), generation)?
        };
        self.act(host, actions)
    }
}
