//! What the plugin does with the radio's events (`wifi_scanned`,
//! `wifi_done`, `network_wanted`, `network_idle`, 92-95, and the radio's own
//! state changes, the screen shown, the power mode and its two timers), as
//! decisions without the host: the handlers of the interface-6 plugin
//! (`lib.rs`) on facts the native screen reads fallibly first
//! ([`Facts`]), answering what is to be done ([`Action`]), with the
//! [`Policy`] as the state. A refused allocation ends in [`Error::NoMemory`]
//! with nothing done (the policy as it was).
//!
//! Differences from interface 6: a join that another request superseded
//! ([`Failure::Superseded`]) changes and says nothing (the replacement
//! decides), and the clock is the host's milliseconds.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::native_model;
use crate::native_policy::{Failure, Goal, Known, Net, Password, Policy, Radio, Step};

#[cfg(test)]
mod tests;

/// How long after a round that joined nothing it is tried again.
pub const RETRY_MS: u64 = 10_000;

/// What the host says now, read before an event is decided.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub radio: Radio,
    /// The radio is busy (joining, scanning).
    pub busy: bool,
    /// Work waits for the network (`dev_wifi.flags` bit 1).
    pub wanted: bool,
    /// The networks in reach, strongest first.
    pub networks: &'a [Net],
    /// The networks joined before, with their passwords.
    pub known: &'a [Known],
    /// The host's milliseconds.
    pub now_ms: u64,
    /// Charging keeps the radio on, and the user wants that.
    pub charging_keeps: bool,
    /// Home or a book shows (nothing there uses the network).
    pub at_rest: bool,
    /// The setting that the radio comes on when a plugin wants the network.
    pub on_demand: bool,
    /// The idle time before the radio goes off, in milliseconds.
    pub idle_ms: u64,
    /// The network the user picked last, to join first when in reach.
    pub join_first: Option<&'a str>,
    /// The network joined now, when online.
    pub joined: Option<&'a str>,
}

/// A timer to set: the policy's generation it was set at, so that it does
/// nothing once the network was wanted again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    /// Switch the radio off if nothing keeps it on.
    IdleOff { due_ms: u64, generation: u32 },
    /// Try the networks joined before again.
    Retry { due_ms: u64, generation: u32 },
}

/// What is said to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// No network joined before is in reach.
    NoneInReach,
    /// Joining failed (said when the user waits for it).
    Failed { ssid: String, failure: Failure },
}

/// What is to be done.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Tell the radio.
    Step(Step),
    /// Switch the radio off.
    Off,
    /// Set a timer.
    Timer(Timer),
    /// Say it.
    Notify(Notice),
    /// No network is coming for what waits: it fails now.
    NoNetwork,
    /// A network the user picked was joined: first next time.
    Picked(String),
    /// A network chosen with a new password was joined: keep the password.
    KeepSecret { ssid: String, password: Password },
}

fn one(action: Action) -> Result<Vec<Action>, Error> {
    native_model::one(action)
}

fn steps(found: Vec<Step>, mut out: Vec<Action>) -> Result<Vec<Action>, Error> {
    out.try_reserve_exact(found.len())
        .map_err(|_| Error::NoMemory)?;
    for step in found {
        out.push(Action::Step(step));
    }
    Ok(out)
}

/// The radio goes off after its idle time, if nothing keeps it on.
fn schedule_off(policy: &Policy, f: &Facts<'_>) -> Option<Action> {
    (f.radio.is_on() && policy.may_go_off(f.charging_keeps)).then_some(Action::Timer(
        Timer::IdleOff {
            due_ms: f.now_ms.saturating_add(f.idle_ms),
            generation: policy.generation,
        },
    ))
}

/// After a round that joined nothing: another round soon, if the network is
/// still wanted (or charging keeps the radio on), else the radio off later.
fn schedule_retry(policy: &Policy, f: &Facts<'_>) -> Option<Action> {
    if policy.still_wanted(f.wanted, f.now_ms) || f.charging_keeps {
        Some(Action::Timer(Timer::Retry {
            due_ms: f.now_ms.saturating_add(RETRY_MS),
            generation: policy.generation,
        }))
    } else if !f.busy {
        schedule_off(policy, f)
    } else {
        None
    }
}

fn some(action: Option<Action>, out: &mut Vec<Action>) -> Result<(), Error> {
    if let Some(action) = action {
        push(out, action)?;
    }
    Ok(())
}

impl Policy {
    fn set_first(&mut self, first: Option<&str>) -> Result<Option<String>, Error> {
        let new = first.map(copy).transpose()?;
        Ok(core::mem::replace(&mut self.first, new))
    }

    /// A plugin wants the network.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_wanted(&mut self, f: &Facts<'_>) -> Result<Vec<Action>, Error> {
        // Not during the setup with a phone, nor while the own network is
        // open (a transfer without a router).
        if !f.on_demand || self.phone || f.radio == Radio::Hosting {
            return Ok(Vec::new());
        }
        let out = if f.known.is_empty() {
            // Nothing to join: what waits fails now.
            one(Action::NoNetwork)?
        } else {
            Vec::new()
        };
        let out = steps(self.wanted(f.radio, f.known)?, out)?;
        self.asked_at = if f.known.is_empty() {
            None
        } else {
            Some(f.now_ms)
        };
        Ok(out)
    }

    /// The radio's state changed.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; a password to keep stays pending.
    pub fn on_changed(&mut self, f: &Facts<'_>) -> Result<Vec<Action>, Error> {
        let mut out = Vec::new();
        let mut keep = false;
        if let Some(ssid) = f.joined {
            // A network the user picked on the page and joined: first next
            // time.
            if matches!(&self.goal, Goal::Chosen { ssid: chosen, .. } if chosen == ssid)
                && f.join_first != Some(ssid)
            {
                push(&mut out, Action::Picked(copy(ssid)?))?;
            }
            if let Some((pending, password)) = &self.pending
                && pending == ssid
            {
                let action = Action::KeepSecret {
                    ssid: copy(ssid)?,
                    password: password.try_clone()?,
                };
                push(&mut out, action)?;
                keep = true;
            }
        }
        // The setup with a phone says what the radio does, as does the
        // plugin that opened the own network.
        if self.phone || f.radio == Radio::Hosting {
            if keep {
                self.pending = None;
            }
            return Ok(out);
        }
        for step in self.changed(f.radio)? {
            push(&mut out, Action::Step(step))?;
        }
        // On and in no network while charging keeps it on (switched on
        // again, or a network left): join one.
        if f.radio == Radio::On
            && self.goal == Goal::Nothing
            && f.charging_keeps
            && !f.known.is_empty()
        {
            for step in self.retry()? {
                push(&mut out, Action::Step(step))?;
            }
        }
        // On, with nothing to do (joined, nothing waits).
        if !f.busy && !self.still_wanted(f.wanted, f.now_ms) {
            some(schedule_off(self, f), &mut out)?;
        }
        if keep {
            self.pending = None;
        }
        Ok(out)
    }

    /// Networks were found.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_scanned(&mut self, f: &Facts<'_>) -> Result<Vec<Action>, Error> {
        if self.phone {
            return Ok(Vec::new());
        }
        let old = self.set_first(f.join_first)?;
        let result = self.scanned_inner(f);
        if result.is_err() {
            self.first = old;
        }
        result
    }

    fn scanned_inner(&mut self, f: &Facts<'_>) -> Result<Vec<Action>, Error> {
        if let Some(found) = self.scanned(f.networks, f.known)? {
            return steps(found, Vec::new());
        }
        self.scans += 1;
        if self.still_wanted(f.wanted, f.now_ms) && self.scans < crate::native_policy::SCANS {
            // The first scans after the radio came on may find nothing.
            return one(Action::Step(Step::Scan));
        }
        let mut out = Vec::new();
        if self.still_wanted(f.wanted, f.now_ms) {
            push(&mut out, Action::Notify(Notice::NoneInReach))?;
            push(&mut out, Action::NoNetwork)?;
            self.asked_at = None;
        }
        self.goal = Goal::Nothing;
        some(schedule_retry(self, f), &mut out)?;
        Ok(out)
    }

    /// A request to join `ssid` ended with `failure` (`wifi_done`).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_failed(
        &mut self,
        f: &Facts<'_>,
        ssid: &str,
        failure: Failure,
    ) -> Result<Vec<Action>, Error> {
        if failure == Failure::Superseded {
            return Ok(Vec::new());
        }
        // The setup with a phone says it, and opens its network again.
        if self.phone {
            self.goal = Goal::Nothing;
            if self.pending.as_ref().is_some_and(|(p, _)| p == ssid) {
                self.pending = None;
            }
            return Ok(Vec::new());
        }
        let chosen = matches!(self.goal, Goal::Chosen { .. });
        let old = self.set_first(f.join_first)?;
        let result = self.failed_inner(f, ssid, failure, chosen);
        if result.is_err() {
            self.first = old;
        }
        result
    }

    fn failed_inner(
        &mut self,
        f: &Facts<'_>,
        ssid: &str,
        failure: Failure,
        chosen: bool,
    ) -> Result<Vec<Action>, Error> {
        match self.failed(ssid, failure, f.networks, f.known)? {
            Some(found) if !found.is_empty() => steps(found, Vec::new()),
            // The last try (or the user's choice): say why.
            result => {
                let mut out = Vec::new();
                if result.is_none() {
                    self.goal = Goal::Nothing;
                    some(schedule_retry(self, f), &mut out)?;
                }
                // Said when the user waits for it, not for tries in the
                // background.
                if chosen || self.still_wanted(f.wanted, f.now_ms) {
                    let notice = Notice::Failed {
                        ssid: copy(ssid)?,
                        failure,
                    };
                    push(&mut out, Action::Notify(notice))?;
                }
                Ok(out)
            }
        }
    }

    /// The password typed for network `ssid`: it is kept once joined.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; nothing changed.
    pub fn on_password(
        &mut self,
        f: &Facts<'_>,
        ssid: &str,
        password: &str,
    ) -> Result<Vec<Action>, Error> {
        let pending = (copy(ssid)?, Password::new(password)?);
        let out = steps(self.choose(f.radio, ssid, password)?, Vec::new())?;
        self.pending = Some(pending);
        Ok(out)
    }

    /// Nothing uses the network (`network_idle`), or another power mode: off
    /// a while later.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_idle(&mut self, f: &Facts<'_>) -> Result<Vec<Action>, Error> {
        let mut out = Vec::new();
        some(schedule_off(self, f), &mut out)?;
        Ok(out)
    }

    /// Another power mode: a radio only charging kept on may go off.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_power(&mut self, f: &Facts<'_>) -> Result<Vec<Action>, Error> {
        if f.busy {
            return Ok(Vec::new());
        }
        self.on_idle(f)
    }

    /// Back home or in a book: the radio goes off now, not after its idle
    /// time, so that its memory is free before a book opens.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_screen_shown(&mut self, f: &Facts<'_>) -> Result<Vec<Action>, Error> {
        if f.at_rest
            && f.radio.is_on()
            && f.radio != Radio::Hosting
            && !f.busy
            && !self.phone
            && !self.still_wanted(f.wanted, f.now_ms)
            && self.may_go_off(f.charging_keeps)
        {
            return one(Action::Off);
        }
        Ok(Vec::new())
    }

    /// The idle timer set at `generation` ran.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_idle_timer(&mut self, f: &Facts<'_>, generation: u32) -> Result<Vec<Action>, Error> {
        if generation == self.generation
            && f.radio.is_on()
            && !f.busy
            && !self.still_wanted(f.wanted, f.now_ms)
            && self.may_go_off(f.charging_keeps)
        {
            return one(Action::Off);
        }
        Ok(Vec::new())
    }

    /// The retry timer set at `generation` ran: another round, if the network
    /// is still wanted.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn on_retry_timer(&mut self, f: &Facts<'_>, generation: u32) -> Result<Vec<Action>, Error> {
        if generation != self.generation
            || f.radio != Radio::On
            || self.goal != Goal::Nothing
            || self.phone
        {
            return Ok(Vec::new());
        }
        if (self.still_wanted(f.wanted, f.now_ms) || f.charging_keeps) && !f.known.is_empty() {
            steps(self.retry()?, Vec::new())
        } else if !f.busy {
            let mut out = Vec::new();
            some(schedule_off(self, f), &mut out)?;
            Ok(out)
        } else {
            Ok(Vec::new())
        }
    }

    /// The status changed or the device started: charging may keep the radio
    /// on, or no longer. The status changes with every percent of the
    /// battery, so only a change of charging counts (`started` always does).
    ///
    /// # Errors
    /// [`Error::NoMemory`]; nothing changed.
    pub fn on_charging(&mut self, f: &Facts<'_>, started: bool) -> Result<Vec<Action>, Error> {
        let keeps = f.charging_keeps;
        if (keeps == self.charging_kept && !started) || self.phone || f.radio == Radio::Hosting {
            return Ok(Vec::new());
        }
        let out = if keeps {
            if f.radio.is_on() {
                Vec::new()
            } else {
                steps(self.wanted(f.radio, f.known)?, Vec::new())?
            }
        } else if !f.busy && !self.still_wanted(f.wanted, f.now_ms) {
            let mut out = Vec::new();
            some(schedule_off(self, f), &mut out)?;
            out
        } else {
            Vec::new()
        };
        self.charging_kept = keeps;
        Ok(out)
    }
}
