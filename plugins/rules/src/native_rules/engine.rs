//! What the rules do, without the host: which rules an event sets off, what
//! they ask for, and the memory between events (the battery, the chain of
//! rules set off by each other's changes, the minutes). The host's answers
//! come in as [`Facts`]; what to do goes out as [`Act`]s.
use super::rule::{Condition, Effect, Levels, Rule, Trigger, between};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

/// How many rules may follow each other through the settings they change:
/// rules that undo each other stop there.
pub const MAX_CHAIN: u8 = 4;

/// What a rule asks the host to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Run an action.
    Run(String),
    /// Show a message.
    Announce(String),
    /// Set a setting.
    Set(String, Value),
}

/// A value a rule sets, or finds in a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Int(i64),
}

/// What the rules need to know of the device at the moment of an event.
pub struct Facts<'a> {
    /// A book is open.
    pub reading: bool,
    /// The battery charges; `None` without a battery.
    pub charging: Option<bool>,
    /// The minute of the day, local time; `None` while the clock is unknown.
    pub minute: Option<u16>,
    /// The value of a setting now; `None` where it has none (or not a switch
    /// or a number). The host call can fail.
    pub setting: &'a dyn Fn(&str) -> Result<Option<Value>, Error>,
}

impl core::fmt::Debug for Facts<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Facts")
            .field("reading", &self.reading)
            .field("charging", &self.charging)
            .field("minute", &self.minute)
            .finish_non_exhaustive()
    }
}

impl Facts<'_> {
    fn switch_on(&self, id: &str) -> Result<bool, Error> {
        Ok((self.setting)(id)? == Some(Value::Bool(true)))
    }
}

/// Whether `condition` holds now.
///
/// # Errors
/// The host's refusal when a setting is read.
pub fn holds(condition: &Condition, facts: &Facts<'_>) -> Result<bool, Error> {
    Ok(match condition {
        Condition::Always => true,
        Condition::Between { from, to } => facts
            .minute
            .is_some_and(|minute| between(minute, *from, *to)),
        Condition::Reading => facts.reading,
        Condition::NotReading => !facts.reading,
        Condition::OnBattery => facts.charging == Some(false),
        Condition::Charging => facts.charging == Some(true),
        Condition::SwitchOn(id) => facts.switch_on(id)?,
        Condition::SwitchOff(id) => !facts.switch_on(id)?,
    })
}

/// The settings rules changed, not yet heard of, each with how many rules in a
/// row led to the change.
#[derive(Debug, Default)]
pub struct Chain(Vec<(String, u8)>);

impl Chain {
    /// Setting `id` is changed by the `depth`-th rule in a row.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn caused(&mut self, id: &str, depth: u8) -> Result<(), Error> {
        let entry = (copy(id)?, depth);
        self.0.retain(|(other, _)| other != id);
        push(&mut self.0, entry)
    }

    /// How many rules in a row led to setting `id` changing: none where the
    /// change is not a rule's.
    pub fn depth_of(&mut self, id: &str) -> u8 {
        match self.0.iter().position(|(other, _)| other == id) {
            Some(at) => self.0.remove(at).1,
            None => 0,
        }
    }

    /// How many changes wait to be heard of.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether none waits.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Runs the rules whose trigger `applies` and whose condition holds, adding
/// what they ask for to `out`; `depth` rules in a row led here.
///
/// A switch or a number that already has the value is not set again.
///
/// # Errors
/// [`Error::NoMemory`], or the host's refusal when a setting is read.
pub fn run(
    rules: &[Rule],
    applies: impl Fn(&Trigger) -> bool,
    facts: &Facts<'_>,
    depth: u8,
    chain: &mut Chain,
    out: &mut Vec<Act>,
) -> Result<(), Error> {
    for rule in rules {
        if !rule.on || !applies(&rule.trigger) || !holds(&rule.condition, facts)? {
            continue;
        }
        let (id, value) = match &rule.effect {
            Effect::Action(action) => {
                push(out, Act::Run(copy(action)?))?;
                continue;
            }
            Effect::Message(text) => {
                push(out, Act::Announce(copy(text)?))?;
                continue;
            }
            Effect::Switch(id, on) => (id, Value::Bool(*on)),
            Effect::Number(id, value) => (id, Value::Int(*value)),
        };
        if (facts.setting)(id)? != Some(value) {
            chain.caused(id, depth.saturating_add(1))?;
            push(out, Act::Set(copy(id)?, value))?;
        }
    }
    Ok(())
}

/// The battery as it was last, and the levels it has reached: a charge
/// falling below a mark or reaching one, and charging beginning or ending,
/// compare with it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Battery {
    last: Option<(u8, bool, Levels)>,
}

/// What changed since the battery was last looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub was: u8,
    pub percent: u8,
    pub was_charging: bool,
    pub charging: bool,
    /// The levels reached just now.
    pub reached: Levels,
}

impl Battery {
    /// The battery is at `percent` now. The first look only remembers.
    pub fn look(&mut self, percent: u8, charging: bool) -> Option<Change> {
        let Some((was, was_charging, mut levels)) = self.last else {
            self.last = Some((percent, charging, Levels::up_to(percent)));
            return None;
        };
        let reached = levels.reach(percent);
        self.last = Some((percent, charging, levels));
        Some(Change {
            was,
            percent,
            was_charging,
            charging,
            reached,
        })
    }
}

impl Change {
    /// Whether `trigger` is set off by this change.
    #[must_use]
    pub fn sets_off(&self, trigger: &Trigger) -> bool {
        match trigger {
            Trigger::BatteryBelow(mark) => self.was >= *mark && self.percent < *mark,
            Trigger::BatteryAtLeast(mark) => self.reached.contains(*mark),
            Trigger::Charging => !self.was_charging && self.charging,
            Trigger::Unplugged => self.was_charging && !self.charging,
            _ => false,
        }
    }
}

/// Whether a time-of-day `trigger` is due at `minute` on `weekday` (Sunday 0).
#[must_use]
pub fn time_due(trigger: &Trigger, minute: u16, weekday: u8) -> bool {
    matches!(trigger, Trigger::Time { minute: at, days }
        if *at == minute && weekday < 7 && days & (1 << weekday) != 0)
}

/// Whether a repeating `trigger` is due after `minutes` since the start.
#[must_use]
pub fn every_due(trigger: &Trigger, minutes: u32) -> bool {
    matches!(trigger, Trigger::Every(every) if *every > 0 && minutes.is_multiple_of(u32::from(*every)))
}

/// Whether a switch `trigger` is set off by setting `id` becoming `on`.
#[must_use]
pub fn switch_due(trigger: &Trigger, id: &str, on: bool) -> bool {
    match trigger {
        Trigger::SwitchOn(s) => on && s == id,
        Trigger::SwitchOff(s) => !on && s == id,
        _ => false,
    }
}

/// The time of day the clock shows, as the minute of the day and the weekday
/// (Sunday 0), at `unix` seconds with the zone `offset` seconds ahead of UTC.
#[must_use]
pub fn local_minute(unix: i64, offset: i32) -> (u16, u8) {
    let local = unix.saturating_add(i64::from(offset));
    let day = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    // 1970-01-01 was a Thursday.
    let weekday = (day + 4).rem_euclid(7);
    ((secs / 60) as u16, weekday as u8)
}
