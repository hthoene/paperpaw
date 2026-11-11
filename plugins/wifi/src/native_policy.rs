//! When the radio is on and which network is joined, without the host: the
//! decisions of the interface-6 `Policy` (`policy.rs`, whose module
//! documentation says what they are) on inputs of the plugin's own, with
//! fallible allocation, so that the native screen can feed it what
//! `dev_wifi`, `dev_network` and the events 71, 92 and 93 tell. A refused
//! allocation ends in [`Error::NoMemory`] with the policy as it was.
//!
//! Differences from interface 6, all from `docs/abi/device.md`: the clock is
//! the host's milliseconds, a join that another request superseded
//! ([`Failure::Superseded`], `wifi_done` reason 4) changes nothing (its
//! replacement decides), and a password is wiped when it is dropped.
use alloc::{string::String, vec::Vec};
use core::fmt;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::{push, wipe_text},
    text::copy,
};

/// Scans in a row that find no network joined before, until none counts as
/// in reach: the first after the radio comes on often finds none.
pub const SCANS: u8 = 3;
/// How long work waits for the network (`docs/abi/network.md` §3.2).
pub const NETWORK_WAIT_MS: u64 = 30_000;

/// A password, wiped from memory when it is dropped; never printed.
#[derive(PartialEq, Eq, Default)]
pub struct Password(String);

impl Password {
    /// A copy of `text` made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(text: &str) -> Result<Self, Error> {
        Ok(Self(copy(text)?))
    }

    /// The password.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether there is none (an open network).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Self::new(&self.0)
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(..)")
    }
}

impl Drop for Password {
    fn drop(&mut self) {
        wipe_text(core::mem::take(&mut self.0));
    }
}

/// Where the radio is (`dev_wifi.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Radio {
    #[default]
    Off,
    /// On, in no network.
    On,
    Joining,
    Online,
    /// The device's own network is open.
    Hosting,
}

impl Radio {
    /// Anything but off.
    #[must_use]
    pub fn is_on(self) -> bool {
        self != Self::Off
    }
}

/// A network in reach (`dev_network`), strongest first in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    pub ssid: String,
    /// It asks for a password.
    pub secured: bool,
}

/// A network joined before, with its password (empty for an open one).
#[derive(Debug, PartialEq, Eq)]
pub struct Known {
    pub ssid: String,
    pub password: Password,
}

/// What the radio is told to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Switch it on (not joining anything yet).
    On,
    /// Look for networks.
    Scan,
    /// Join a network, with its password if it has one.
    Join { ssid: String, password: Password },
}

/// Why joining a network failed (`wifi_done.reason`, `device.md` §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// 1: not in reach.
    NotFound,
    /// 2: the password was not accepted.
    Password,
    /// 3: no answer in time.
    Timeout,
    /// 4: another join or host replaced the request.
    Superseded,
    /// 0 and the rest: refused, needs the user, unsupported, unknown.
    Other,
}

impl Failure {
    /// The failure a `wifi_done` reason stands for.
    #[must_use]
    pub fn from_reason(reason: u32) -> Self {
        match reason {
            1 => Self::NotFound,
            2 => Self::Password,
            3 => Self::Timeout,
            4 => Self::Superseded,
            _ => Self::Other,
        }
    }
}

impl From<paperpaw_sdk::native::events::WifiFailure> for Failure {
    fn from(failure: paperpaw_sdk::native::events::WifiFailure) -> Self {
        use paperpaw_sdk::native::events::WifiFailure as Reason;
        match failure {
            Reason::NotFound => Self::NotFound,
            Reason::Password => Self::Password,
            Reason::Timeout => Self::Timeout,
            Reason::Superseded => Self::Superseded,
            _ => Self::Other,
        }
    }
}

/// What the radio is for now.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum Goal {
    /// Nothing to join.
    #[default]
    Nothing,
    /// The strongest network joined before, once networks are found.
    Known,
    /// The network the user chose, with the password given.
    Chosen { ssid: String, password: Password },
}

/// What the plugin keeps between events.
#[derive(Debug, Default)]
pub struct Policy {
    pub goal: Goal,
    /// Networks tried this round (to try the next one).
    pub tried: Vec<String>,
    /// Scans this round that found no network joined before.
    pub scans: u8,
    /// The user switched the radio on (on the page): it stays on.
    pub manual: bool,
    /// A network chosen with a password not kept yet: kept once joined, and
    /// offered again when the network is chosen after a failure (let go only
    /// when the password was wrong).
    pub pending: Option<(String, Password)>,
    /// Counts the times the radio was wanted; a timer to switch it off
    /// carries the count it was set at and does nothing once it changed.
    pub generation: u32,
    /// Charging kept the radio on, as last seen (to act on changes only).
    pub charging_kept: bool,
    /// When (host milliseconds) a plugin asked for the network last; `None`
    /// once the plugin said none is coming, which fails what waited.
    pub asked_at: Option<u64>,
    /// The setup with a phone is under way: it says what the radio does,
    /// nothing is joined by itself meanwhile.
    pub phone: bool,
    /// The network to join first when in reach: the one the user picked
    /// last; none for the strongest.
    pub first: Option<String>,
    /// `first` failed once this round and was tried again.
    pub first_again: bool,
}

fn steps(step: Step) -> Result<Vec<Step>, Error> {
    crate::native_model::one(step)
}

fn join(ssid: &str, password: &Password) -> Result<Step, Error> {
    Ok(Step::Join {
        ssid: copy(ssid)?,
        password: password.try_clone()?,
    })
}

/// Of `networks` in reach and not tried yet, the one joined before to join
/// without asking: `first` if it is among them, else the strongest. Only
/// secured networks with their password: anyone can set up an open network
/// by any name, so an open one (or an open twin of a secured one) is joined
/// only when chosen on the page.
#[must_use]
pub fn best_known<'a>(
    networks: &[Net],
    known: &'a [Known],
    tried: &[String],
    first: Option<&str>,
) -> Option<&'a Known> {
    let joinable = |n: &&Net| n.secured && !tried.contains(&n.ssid);
    let known_as = |n: &Net| {
        known
            .iter()
            .find(|k| k.ssid == n.ssid && !k.password.is_empty())
    };
    networks
        .iter()
        .filter(joinable)
        .filter(|n| Some(n.ssid.as_str()) == first)
        .find_map(known_as)
        .or_else(|| networks.iter().filter(joinable).find_map(known_as))
}

impl Policy {
    /// Whether work still waits for the network at `now_ms`: the host says so
    /// (`dev_wifi.flags` bit 1), and it can still be waiting, asked for less
    /// than [`NETWORK_WAIT_MS`] ago and not failed by the plugin.
    #[must_use]
    pub fn still_wanted(&self, wanted: bool, now_ms: u64) -> bool {
        wanted
            && self
                .asked_at
                .is_some_and(|at| now_ms < at.saturating_add(NETWORK_WAIT_MS))
    }

    /// The network is wanted: the radio on, or networks looked for.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn wanted(&mut self, radio: Radio, known: &[Known]) -> Result<Vec<Step>, Error> {
        self.generation = self.generation.wrapping_add(1);
        if known.is_empty() {
            return Ok(Vec::new());
        }
        let step = match radio {
            Radio::Off => Step::On,
            Radio::On => Step::Scan,
            Radio::Joining | Radio::Online | Radio::Hosting => return Ok(Vec::new()),
        };
        let found = steps(step)?;
        self.start_round(Goal::Known);
        Ok(found)
    }

    /// Tries the networks joined before again (after a round found none or
    /// failed): networks looked for anew.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn retry(&mut self) -> Result<Vec<Step>, Error> {
        let found = steps(Step::Scan)?;
        self.start_round(Goal::Known);
        Ok(found)
    }

    fn start_round(&mut self, goal: Goal) {
        self.goal = goal;
        self.first_again = false;
        self.tried.clear();
        self.scans = 0;
    }

    /// The user chose a network (with its password, or a kept one).
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the policy is as it was.
    pub fn choose(&mut self, radio: Radio, ssid: &str, password: &str) -> Result<Vec<Step>, Error> {
        let goal = Goal::Chosen {
            ssid: copy(ssid)?,
            password: Password::new(password)?,
        };
        let found = if radio.is_on() {
            steps(Step::Join {
                ssid: copy(ssid)?,
                password: Password::new(password)?,
            })?
        } else {
            steps(Step::On)?
        };
        self.start_round(goal);
        if !radio.is_on() {
            self.manual = true;
        }
        Ok(found)
    }

    /// The radio's state changed.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the policy is as it was.
    pub fn changed(&mut self, radio: Radio) -> Result<Vec<Step>, Error> {
        match radio {
            Radio::On => match &self.goal {
                Goal::Nothing => Ok(Vec::new()),
                Goal::Known => steps(Step::Scan),
                Goal::Chosen { ssid, password } => steps(join(ssid, password)?),
            },
            Radio::Online => {
                self.goal = Goal::Nothing;
                self.tried.clear();
                Ok(Vec::new())
            }
            Radio::Off => {
                self.goal = Goal::Nothing;
                self.manual = false;
                Ok(Vec::new())
            }
            Radio::Joining | Radio::Hosting => Ok(Vec::new()),
        }
    }

    /// Networks were found: the best one joined before, if one is wanted.
    /// `None` when there is none to try.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the policy is as it was.
    pub fn scanned(
        &mut self,
        networks: &[Net],
        known: &[Known],
    ) -> Result<Option<Vec<Step>>, Error> {
        if self.goal != Goal::Known {
            return Ok(Some(Vec::new()));
        }
        // The network to join first not heard: the first scans after the
        // radio came on often miss a weaker one, so it is looked for again
        // before another one is joined.
        if let Some(first) = &self.first
            && self.scans + 1 < SCANS
            && !self.tried.contains(first)
            && known
                .iter()
                .any(|k| k.ssid == *first && !k.password.is_empty())
            && !networks.iter().any(|n| n.ssid == *first)
        {
            let found = steps(Step::Scan)?;
            self.scans += 1;
            return Ok(Some(found));
        }
        let Some(best) = best_known(networks, known, &self.tried, self.first.as_deref()) else {
            return Ok(None);
        };
        let found = steps(join(&best.ssid, &best.password)?)?;
        let tried = copy(&best.ssid)?;
        push(&mut self.tried, tried)?;
        Ok(Some(found))
    }

    /// Joining `ssid` failed: the next network joined before, if any is
    /// wanted. `None` when there is none to try. A password typed for it is
    /// kept to offer again, unless it was wrong.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; nothing is changed but the tried networks.
    pub fn failed(
        &mut self,
        ssid: &str,
        failure: Failure,
        networks: &[Net],
        known: &[Known],
    ) -> Result<Option<Vec<Step>>, Error> {
        if failure == Failure::Superseded {
            return Ok(Some(Vec::new()));
        }
        if failure == Failure::Password
            && self
                .pending
                .as_ref()
                .is_some_and(|(pending, _)| pending == ssid)
        {
            self.pending = None;
        }
        match self.goal {
            Goal::Known => {
                // The network to join first gets a second try before the
                // next: one of its access points, weak or far, refuses a
                // join now and then, and the next network would then stay
                // joined.
                if !self.first_again
                    && self.first.as_deref() == Some(ssid)
                    && networks.iter().any(|n| n.ssid == ssid)
                    && let Some(k) = known
                        .iter()
                        .find(|k| k.ssid == ssid && !k.password.is_empty())
                {
                    let found = steps(join(&k.ssid, &k.password)?)?;
                    self.first_again = true;
                    return Ok(Some(found));
                }
                if !self.tried.iter().any(|t| t == ssid) {
                    let tried = copy(ssid)?;
                    push(&mut self.tried, tried)?;
                }
                self.scanned(networks, known)
            }
            Goal::Chosen { .. } => {
                self.goal = Goal::Nothing;
                Ok(Some(Vec::new()))
            }
            Goal::Nothing => Ok(Some(Vec::new())),
        }
    }

    /// Whether the radio may go off now that nothing uses the network.
    #[must_use]
    pub fn may_go_off(&self, charging_keeps_it: bool) -> bool {
        !self.manual && !charging_keeps_it && self.goal == Goal::Nothing
    }
}

#[cfg(test)]
mod tests;
