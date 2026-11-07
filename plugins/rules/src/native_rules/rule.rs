//! Rules on the card, without the host: what a rule is, how a line of
//! `rules.txt` reads and is written, and the battery levels a rule waits for.
//! Every growth is reserved first: a refused allocation is `Error::NoMemory`.
//!
//! One rule a line: `trigger<TAB>effect<TAB>target<TAB>on|off<TAB>condition`
//! (no condition: always). The format is the one interface 6 wrote.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Id, IdError, native::Error};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy, decimal, decimal_u64},
};

/// Days of the week as bits, Sunday the lowest.
pub const EVERY_DAY: u8 = 0b111_1111;
/// Monday to Friday.
pub const WEEKDAYS: u8 = 0b011_1110;
/// Saturday and Sunday.
pub const WEEKENDS: u8 = 0b100_0001;
/// The most the file may hold: what is read is what may be written.
pub const MAX_FILE: usize = 64 * 1024;

/// When a rule applies. Setting ids are kept as text, checked with the SDK's
/// own id check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// PaperPaw has started.
    Start,
    /// At this minute of the day, on the days set.
    Time {
        minute: u16,
        days: u8,
    },
    /// Every so many minutes, from the start.
    Every(u16),
    /// A book was opened, closed, read to its last page.
    BookOpened,
    BookClosed,
    BookFinished,
    /// The device woke up, or goes to sleep.
    Resume,
    Sleep,
    /// The battery's charge fell below this percentage.
    BatteryBelow(u8),
    /// The battery's charge rose to this percentage or above.
    BatteryAtLeast(u8),
    /// Charging began, or ended.
    Charging,
    Unplugged,
    /// A switch (an on/off setting) was turned on, or off.
    SwitchOn(String),
    SwitchOff(String),
}

/// What a rule does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Runs an action.
    Action(String),
    /// Turns a switch on or off.
    Switch(String, bool),
    /// Sets a number.
    Number(String, i64),
    /// Shows a message.
    Message(String),
}

/// When a rule may apply at all, besides its trigger.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Condition {
    #[default]
    Always,
    /// Between two minutes of the day (over midnight when `from` is later).
    Between {
        from: u16,
        to: u16,
    },
    /// While a book is open, or while none is.
    Reading,
    NotReading,
    /// While the device runs on its battery, or while it charges.
    OnBattery,
    Charging,
    /// While a switch is on, or off.
    SwitchOn(String),
    SwitchOff(String),
}

/// One rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub trigger: Trigger,
    pub effect: Effect,
    /// Rules can be paused without being removed.
    pub on: bool,
    pub condition: Condition,
}

impl Rule {
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            trigger: self.trigger.try_clone()?,
            effect: self.effect.try_clone()?,
            on: self.on,
            condition: self.condition.try_clone()?,
        })
    }
}

/// A copy of `rules` made with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy_rules(rules: &[Rule]) -> Result<Vec<Rule>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(rules.len())
        .map_err(|_| Error::NoMemory)?;
    for rule in rules {
        out.push(rule.try_clone()?);
    }
    Ok(out)
}

impl Trigger {
    fn try_clone(&self) -> Result<Self, Error> {
        Ok(match self {
            Self::Start => Self::Start,
            Self::Time { minute, days } => Self::Time {
                minute: *minute,
                days: *days,
            },
            Self::Every(m) => Self::Every(*m),
            Self::BookOpened => Self::BookOpened,
            Self::BookClosed => Self::BookClosed,
            Self::BookFinished => Self::BookFinished,
            Self::Resume => Self::Resume,
            Self::Sleep => Self::Sleep,
            Self::BatteryBelow(p) => Self::BatteryBelow(*p),
            Self::BatteryAtLeast(p) => Self::BatteryAtLeast(*p),
            Self::Charging => Self::Charging,
            Self::Unplugged => Self::Unplugged,
            Self::SwitchOn(id) => Self::SwitchOn(copy(id)?),
            Self::SwitchOff(id) => Self::SwitchOff(copy(id)?),
        })
    }
}

impl Effect {
    fn try_clone(&self) -> Result<Self, Error> {
        Ok(match self {
            Self::Action(id) => Self::Action(copy(id)?),
            Self::Switch(id, on) => Self::Switch(copy(id)?, *on),
            Self::Number(id, v) => Self::Number(copy(id)?, *v),
            Self::Message(text) => Self::Message(copy(text)?),
        })
    }
}

impl Condition {
    fn try_clone(&self) -> Result<Self, Error> {
        Ok(match self {
            Self::Always => Self::Always,
            Self::Between { from, to } => Self::Between {
                from: *from,
                to: *to,
            },
            Self::Reading => Self::Reading,
            Self::NotReading => Self::NotReading,
            Self::OnBattery => Self::OnBattery,
            Self::Charging => Self::Charging,
            Self::SwitchOn(id) => Self::SwitchOn(copy(id)?),
            Self::SwitchOff(id) => Self::SwitchOff(copy(id)?),
        })
    }
}

/// Whether `id` is a setting or action id, by the SDK's own check.
///
/// # Errors
/// [`Error::NoMemory`] where the check could not copy the id.
pub fn is_id(id: &str) -> Result<bool, Error> {
    match Id::try_parse(id) {
        Ok(_) => Ok(true),
        Err(IdError::Invalid) => Ok(false),
        Err(_) => Err(Error::NoMemory),
    }
}

/// `22:00` for minute 1320 of the day, in `buf`.
#[must_use]
pub fn clock(minute: u16, buf: &mut [u8; 5]) -> &str {
    let minute = minute % (24 * 60);
    let (h, m) = (minute / 60, minute % 60);
    buf.copy_from_slice(&[
        b'0' + (h / 10) as u8,
        b'0' + (h % 10) as u8,
        b':',
        b'0' + (m / 10) as u8,
        b'0' + (m % 10) as u8,
    ]);
    // Five ASCII bytes.
    core::str::from_utf8(buf).unwrap_or("00:00")
}

/// The minute of the day for `22:00` (also `22.00`, `7:5`, `22`).
#[must_use]
pub fn parse_time(text: &str) -> Option<u16> {
    let text = text.trim();
    let (hours, minutes) = text.split_once([':', '.', 'h']).unwrap_or((text, "0"));
    let hours: u16 = hours.trim().parse().ok()?;
    let minutes: u16 = minutes.trim().parse().ok()?;
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// A battery level from 1 to 100 %, as in `80` or `80 %`.
#[must_use]
pub fn parse_percent(text: &str) -> Option<u8> {
    text.trim()
        .trim_end_matches('%')
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|p| (1..=100).contains(p))
}

/// Whether `minute` of the day lies between `from` and `to` (`to` not
/// included; over midnight when `from` is later).
#[must_use]
pub fn between(minute: u16, from: u16, to: u16) -> bool {
    if from <= to {
        (from..to).contains(&minute)
    } else {
        minute >= from || minute < to
    }
}

/// Minutes from `now` (minute of the day) until `at`: never 0.
#[must_use]
pub fn minutes_until(now: u16, at: u16) -> u16 {
    let day = 24 * 60;
    let ahead = (at % day + day - now % day) % day;
    if ahead == 0 { day } else { ahead }
}

/// Words: the first, then a space and the rest.
fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    let (word, rest) = text.split_once(' ').unwrap_or((text, ""));
    (word, rest.trim())
}

impl Trigger {
    /// As written in the file.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn text(&self) -> Result<String, Error> {
        let mut digits = [0; 20];
        let mut clock_buf = [0; 5];
        match self {
            Self::Start => copy("start"),
            Self::Time { minute, days } if *days == EVERY_DAY => {
                concat(&["time ", clock(*minute, &mut clock_buf)])
            }
            Self::Time { minute, days } => concat(&[
                "time ",
                clock(*minute, &mut clock_buf),
                " ",
                decimal(usize::from(*days), &mut digits),
            ]),
            Self::Every(m) => concat(&["every ", decimal(usize::from(*m), &mut digits)]),
            Self::BookOpened => copy("book"),
            Self::BookClosed => copy("book-closed"),
            Self::BookFinished => copy("book-finished"),
            Self::Resume => copy("resume"),
            Self::Sleep => copy("sleep"),
            Self::BatteryBelow(p) => {
                concat(&["battery-below ", decimal(usize::from(*p), &mut digits)])
            }
            Self::BatteryAtLeast(p) => {
                concat(&["battery-at ", decimal(usize::from(*p), &mut digits)])
            }
            Self::Charging => copy("charging"),
            Self::Unplugged => copy("unplugged"),
            Self::SwitchOn(id) => concat(&["switch-on ", id]),
            Self::SwitchOff(id) => concat(&["switch-off ", id]),
        }
    }

    /// The trigger a file's text describes; `None` where this version does
    /// not read it.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn parse(text: &str) -> Result<Option<Self>, Error> {
        let (word, rest) = split_word(text);
        Ok(Some(match word {
            "start" => Self::Start,
            "book" => Self::BookOpened,
            "book-closed" => Self::BookClosed,
            "book-finished" => Self::BookFinished,
            "resume" => Self::Resume,
            "sleep" => Self::Sleep,
            "charging" => Self::Charging,
            "unplugged" => Self::Unplugged,
            "time" => {
                let (time, days) = rest.split_once(' ').unwrap_or((rest, ""));
                let days = match days.trim() {
                    "" => EVERY_DAY,
                    days => match days.parse::<u8>() {
                        Ok(d) if d & EVERY_DAY != 0 => d & EVERY_DAY,
                        _ => return Ok(None),
                    },
                };
                let Some(minute) = parse_time(time) else {
                    return Ok(None);
                };
                Self::Time { minute, days }
            }
            "every" => match rest.parse::<u16>() {
                Ok(m) if m > 0 => Self::Every(m),
                _ => return Ok(None),
            },
            "battery-below" => match parse_percent(rest) {
                Some(p) => Self::BatteryBelow(p),
                None => return Ok(None),
            },
            "battery-at" => match parse_percent(rest) {
                Some(p) => Self::BatteryAtLeast(p),
                None => return Ok(None),
            },
            "switch-on" if is_id(rest)? => Self::SwitchOn(copy(rest)?),
            "switch-off" if is_id(rest)? => Self::SwitchOff(copy(rest)?),
            _ => return Ok(None),
        }))
    }
}

impl Condition {
    /// As written in the file; empty for always.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn text(&self) -> Result<String, Error> {
        let mut a = [0; 5];
        let mut b = [0; 5];
        match self {
            Self::Always => Ok(String::new()),
            Self::Between { from, to } => {
                concat(&["between ", clock(*from, &mut a), " ", clock(*to, &mut b)])
            }
            Self::Reading => copy("reading"),
            Self::NotReading => copy("not-reading"),
            Self::OnBattery => copy("battery"),
            Self::Charging => copy("charging"),
            Self::SwitchOn(id) => concat(&["switch-on ", id]),
            Self::SwitchOff(id) => concat(&["switch-off ", id]),
        }
    }

    /// The condition a file's text describes; `None` where this version
    /// does not read it.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn parse(text: &str) -> Result<Option<Self>, Error> {
        let (word, rest) = split_word(text);
        Ok(Some(match word {
            "" | "always" => Self::Always,
            "between" => {
                let Some((from, to)) = rest.split_once(' ') else {
                    return Ok(None);
                };
                match (parse_time(from), parse_time(to)) {
                    (Some(from), Some(to)) => Self::Between { from, to },
                    _ => return Ok(None),
                }
            }
            "reading" => Self::Reading,
            "not-reading" => Self::NotReading,
            "battery" => Self::OnBattery,
            "charging" => Self::Charging,
            "switch-on" if is_id(rest)? => Self::SwitchOn(copy(rest)?),
            "switch-off" if is_id(rest)? => Self::SwitchOff(copy(rest)?),
            _ => return Ok(None),
        }))
    }
}

/// The rule a line of the file describes; `None` where this version does not
/// read it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn parse_line(line: &str) -> Result<Option<Rule>, Error> {
    let mut fields = line.split('\t');
    let (Some(trigger), Some(kind), Some(target)) = (fields.next(), fields.next(), fields.next())
    else {
        return Ok(None);
    };
    let Some(trigger) = Trigger::parse(trigger)? else {
        return Ok(None);
    };
    let target_id = target.trim();
    let effect = match kind {
        "action" if is_id(target_id)? => Effect::Action(copy(target_id)?),
        "switch-on" if is_id(target_id)? => Effect::Switch(copy(target_id)?, true),
        "switch-off" if is_id(target_id)? => Effect::Switch(copy(target_id)?, false),
        "set" => {
            let Some((id, value)) = target.split_once('=') else {
                return Ok(None);
            };
            let (id, value) = (id.trim(), value.trim());
            match value.parse::<i64>() {
                Ok(value) if is_id(id)? => Effect::Number(copy(id)?, value),
                _ => return Ok(None),
            }
        }
        "message" => Effect::Message(copy(target)?),
        _ => return Ok(None),
    };
    let on = fields.next().is_none_or(|f| f.trim() != "off");
    let Some(condition) = Condition::parse(fields.next().unwrap_or_default())? else {
        return Ok(None);
    };
    Ok(Some(Rule {
        trigger,
        effect,
        on,
        condition,
    }))
}

/// A line of the file as text, without its line ending; `None` where it is
/// not UTF-8.
fn line_text(line: &[u8]) -> Option<&str> {
    core::str::from_utf8(line.strip_suffix(b"\r").unwrap_or(line)).ok()
}

/// The rules of the file's bytes. A line this version does not read is skipped
/// (see [`unread_lines`]).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn decode(bytes: &[u8]) -> Result<Vec<Rule>, Error> {
    let mut rules = Vec::new();
    for line in bytes.split(|&b| b == b'\n') {
        if let Some(rule) = line_text(line).map(parse_line).transpose()?.flatten() {
            push(&mut rules, rule)?;
        }
    }
    Ok(rules)
}

/// The lines of the file this version does not read (a newer version's, or
/// damaged), as they are, each ending in a newline.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn unread_lines(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut unread = Vec::new();
    for line in bytes.split(|&b| b == b'\n') {
        let text = line_text(line);
        let read = match text {
            Some(t) => t.trim().is_empty() || parse_line(t)?.is_some(),
            None => false,
        };
        if !read {
            unread
                .try_reserve(line.len() + 1)
                .map_err(|_| Error::NoMemory)?;
            unread.extend_from_slice(line);
            unread.push(b'\n');
        }
    }
    // The split after the last newline is an empty "line" that was read.
    Ok(unread)
}

/// The file's bytes for `rules` followed by the `unread` lines, refused when
/// longer than `limit`: what is written must be what [`decode`]'s reader takes.
///
/// # Errors
/// [`Error::NoSpace`] beyond `limit`, [`Error::NoMemory`].
pub fn encode_within(rules: &[Rule], unread: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    let mut lines: Vec<String> = Vec::new();
    lines
        .try_reserve_exact(rules.len())
        .map_err(|_| Error::NoMemory)?;
    let mut len = unread.len();
    for rule in rules {
        let line = encode_line(rule)?;
        len = len.checked_add(line.len()).ok_or(Error::NoSpace)?;
        if len > limit {
            return Err(Error::NoSpace);
        }
        lines.push(line);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    for line in &lines {
        out.extend_from_slice(line.as_bytes());
    }
    out.extend_from_slice(unread);
    Ok(out)
}

/// One rule as a line of the file, ending in a newline.
fn encode_line(rule: &Rule) -> Result<String, Error> {
    let mut digits = [0; 20];
    let trigger = rule.trigger.text()?;
    let condition = rule.condition.text()?;
    let (kind, target) = match &rule.effect {
        Effect::Action(id) => ("action", copy(id)?),
        Effect::Switch(id, true) => ("switch-on", copy(id)?),
        Effect::Switch(id, false) => ("switch-off", copy(id)?),
        Effect::Number(id, value) => {
            let number = if *value < 0 {
                let mut d = [0; 20];
                let digits = decimal_u64(value.unsigned_abs(), &mut d);
                concat(&["-", digits])?
            } else {
                copy(decimal_u64(*value as u64, &mut digits))?
            };
            ("set", concat(&[id, "=", &number])?)
        }
        // One line, no fields within it.
        Effect::Message(text) => ("message", single_line(text)?),
    };
    let on = if rule.on { "on" } else { "off" };
    if rule.condition == Condition::Always {
        concat(&[&trigger, "\t", kind, "\t", &target, "\t", on, "\n"])
    } else {
        concat(&[
            &trigger, "\t", kind, "\t", &target, "\t", on, "\t", &condition, "\n",
        ])
    }
}

/// `text` with tabs and line breaks turned into spaces.
fn single_line(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.extend(text.chars().map(|c| {
        if matches!(c, '\t' | '\n' | '\r') {
            ' '
        } else {
            c
        }
    }));
    Ok(out)
}

/// How far the battery has to fall below a level reached before reaching it
/// counts again, in points: the X4 estimates its level from the battery's
/// voltage, which wavers by a point or two between readings.
pub const REARM: u8 = 2;

/// Battery levels (bit `n` for `n` %): those reached and not fallen clearly
/// below since, or those reached just now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Levels(u128);

impl Levels {
    /// The levels up to `percent`, as reached by a battery at `percent`.
    #[must_use]
    pub fn up_to(percent: u8) -> Self {
        Self(u128::MAX >> (127 - u32::from(percent.min(127))))
    }

    /// Whether the level `percent` is among them.
    #[must_use]
    pub fn contains(self, percent: u8) -> bool {
        percent < 128 && self.0 & (1 << percent) != 0
    }

    /// The battery is at `percent` now: the levels it reached just now. Those
    /// it fell more than [`REARM`] points below count as not reached any more.
    pub fn reach(&mut self, percent: u8) -> Self {
        let now = Self::up_to(percent).0;
        let new = now & !self.0;
        self.0 = (self.0 | now) & Self::up_to(percent.saturating_add(REARM)).0;
        Self(new)
    }
}
