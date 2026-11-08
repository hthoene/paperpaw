//! What *Reading goal* decides without a host: the cat's mood, the goal read
//! from the statistics' setting, which seconds count as today's, the text of
//! `today.txt`, and when what the statistics told is written to the card.
//!
//! Nothing here allocates or can panic: texts go into caller-owned buffers,
//! numbers are clamped, bad input gives `None` or the safe default.

/// The minutes the cat goes by without a goal.
pub const DEFAULT_GOAL: u32 = 20;
/// The longest `today.txt` read (`<day> <seconds>`, a signed and an unsigned
/// 32-bit number).
pub const TODAY_FILE_MAX: usize = 64;

/// How the cat feels about today's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// Nothing read yet (less than a minute).
    Sad,
    /// Some, less than the *content* share of the goal.
    Meh,
    /// From that share on, below the *happy* share.
    Content,
    /// The goal reached.
    Happy,
}

impl Mood {
    pub const ALL: [Mood; 4] = [Mood::Sad, Mood::Meh, Mood::Content, Mood::Happy];

    /// After `seconds` read of a goal of `goal` minutes, content from
    /// `content` percent of it, happy from `happy` percent.
    #[must_use]
    pub fn of(seconds: u32, goal: u32, content: u32, happy: u32) -> Mood {
        if seconds < 60 {
            return Mood::Sad;
        }
        let goal = u64::from(goal.max(1)) * 60;
        let percent = u64::from(seconds) * 100 / goal;
        if percent >= u64::from(happy) {
            Mood::Happy
        } else if percent >= u64::from(content) {
            Mood::Content
        } else {
            Mood::Meh
        }
    }

    /// Its name: of its picture file and its translations.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Mood::Sad => "sad",
            Mood::Meh => "meh",
            Mood::Content => "content",
            Mood::Happy => "happy",
        }
    }
}

/// Whether the reading statistics count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statistics {
    Counting,
    /// There, switched off.
    Off,
    /// Not there (not installed).
    Missing,
}

impl Statistics {
    /// From what is known of the `stats` plugin: `None` if it is not
    /// installed, else whether it is enabled.
    #[must_use]
    pub const fn of(enabled: Option<bool>) -> Self {
        match enabled {
            Some(true) => Self::Counting,
            Some(false) => Self::Off,
            None => Self::Missing,
        }
    }
}

/// The goal in minutes from the statistics' setting, `stats.goal.30`: 30;
/// `stats.goal.off` (or anything else, or 0): none.
#[must_use]
pub fn goal_minutes(setting: &str) -> Option<u32> {
    setting
        .rsplit('.')
        .next()
        .and_then(|minutes| minutes.parse::<u32>().ok())
        .filter(|&minutes| minutes > 0)
}

/// The day and the seconds the statistics told (`today.txt`: `<day> <seconds>`).
/// Anything that is not exactly that is nothing.
#[must_use]
pub fn parse_told(text: &[u8]) -> Option<(i32, u32)> {
    let text = core::str::from_utf8(text).ok()?;
    let (day, seconds) = text.trim().split_once(' ')?;
    Some((day.parse().ok()?, seconds.parse().ok()?))
}

/// `<day> <seconds>\n`, written into `out`; the text, or `None` if `out` is
/// too small (32 bytes always suffice).
#[must_use]
pub fn told_text(day: i32, seconds: u32, out: &mut [u8]) -> Option<&str> {
    let mut at = 0usize;
    let mut put = |bytes: &[u8]| -> Option<()> {
        let end = at.checked_add(bytes.len())?;
        out.get_mut(at..end)?.copy_from_slice(bytes);
        at = end;
        Some(())
    };
    let mut digits = [0u8; 20];
    if day < 0 {
        put(b"-")?;
    }
    put(decimal(u64::from(day.unsigned_abs()), &mut digits))?;
    put(b" ")?;
    put(decimal(u64::from(seconds), &mut digits))?;
    put(b"\n")?;
    core::str::from_utf8(out.get(..at)?).ok()
}

fn decimal(mut value: u64, digits: &mut [u8; 20]) -> &[u8] {
    let mut at = digits.len();
    loop {
        at -= 1;
        if let Some(slot) = digits.get_mut(at) {
            *slot = b'0' + (value % 10) as u8;
        }
        value /= 10;
        if value == 0 {
            break;
        }
    }
    digits.get(at..).unwrap_or(&[])
}

/// The statistics' figure of today as the reply of the stage `reading-goal.read-today` carries it
/// (schema 1): the day as an `i32` and the seconds read on it as a `u32`, both
/// little-endian. Anything but exactly eight bytes is nothing.
#[must_use]
pub fn parse_stage(payload: &[u8]) -> Option<(i32, u32)> {
    let bytes: [u8; 8] = payload.try_into().ok()?;
    let (day, seconds) = bytes.split_at(4);
    Some((
        i32::from_le_bytes(day.try_into().ok()?),
        u32::from_le_bytes(seconds.try_into().ok()?),
    ))
}

/// What is known of today's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Today {
    /// The goal in minutes, if one is set.
    pub goal: Option<u32>,
    /// Minutes read today.
    pub minutes: u32,
    pub mood: Mood,
    pub statistics: Statistics,
}

impl Today {
    /// Today from the goal setting, what the statistics told last (`told`),
    /// today's number (`today`, days since 1970 by the local time, if the
    /// device knows it), the two thresholds in percent (clamped as the
    /// settings are) and the statistics' state. What was told on another day
    /// is not today's.
    #[must_use]
    pub fn of(
        goal_setting: &str,
        told: Option<(i32, u32)>,
        today: Option<i32>,
        content: i64,
        happy: i64,
        statistics: Statistics,
    ) -> Self {
        let goal = goal_minutes(goal_setting);
        let seconds = match (told, today) {
            (Some((day, seconds)), Some(today)) if day == today => seconds,
            _ => 0,
        };
        let content = content.clamp(1, 99) as u32;
        let happy = happy.clamp(1, 1000) as u32;
        Self {
            goal,
            minutes: seconds / 60,
            mood: Mood::of(seconds, goal.unwrap_or(DEFAULT_GOAL), content, happy),
            statistics,
        }
    }
}

/// What the statistics told, and when it goes to `today.txt`: not at every
/// page turned (the card is slow), but when a book closes, the device sleeps
/// or stops, or the statistics tell while no book is open — and only if the
/// file does not already hold it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    told: Option<(i32, u32)>,
    reading: bool,
}

impl Kept {
    /// Nothing told yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            told: None,
            reading: false,
        }
    }

    /// What was told since the start, else `file` (the parsed `today.txt`).
    #[must_use]
    pub fn told(&self, file: Option<(i32, u32)>) -> Option<(i32, u32)> {
        self.told.or(file)
    }

    /// The statistics told today's seconds. `true`: write it now.
    pub fn read_today(&mut self, day: i32, seconds: u32) -> bool {
        self.told = Some((day, seconds));
        !self.reading
    }

    /// A book opened: writing waits.
    pub fn book_opened(&mut self) {
        self.reading = true;
    }

    /// A book closed. `true`: write what was told.
    pub fn book_closed(&mut self) -> bool {
        self.reading = false;
        true
    }

    /// The device sleeps or stops. `true`: write what was told.
    #[must_use]
    pub const fn must_flush(&self) -> bool {
        true
    }

    /// What to write, if it differs from what the file (`file`, parsed) holds.
    #[must_use]
    pub fn to_write(&self, file: Option<(i32, u32)>) -> Option<(i32, u32)> {
        self.told.filter(|&told| file != Some(told))
    }
}

#[cfg(test)]
mod tests;
