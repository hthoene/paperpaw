//! An event (`VEVENT`) read from a calendar text, and the time it starts and
//! ends at moved to the reader's own clock.
use alloc::{string::String, vec::Vec};

use crate::{
    Error,
    date::DateTime,
    ical::{Property, UID_MAX, UpperName, unescape_into},
    tz::{self, Zone},
};

/// The most bytes kept of an `RRULE`.
pub const RULE_MAX: usize = 160;
/// The most days of `EXDATE` kept.
pub const EXCEPTIONS_MAX: usize = 16;

/// A date or time with the zone it is meant in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct When {
    pub at: DateTime,
    /// A zone of the table, if the time names one it knows (`TZID`).
    pub zone: Option<Zone>,
}

impl When {
    /// The day and the second of the day on the reader's clock; `offset`
    /// says how far it is east of UTC at a time in UTC. A date alone has no
    /// second. A time without a zone is the reader's own already.
    #[must_use]
    pub fn local(&self, offset: &mut dyn FnMut(i64) -> i32) -> (i32, Option<u32>) {
        let Some(seconds) = self.at.seconds else {
            return (self.at.day, None);
        };
        let wall = self.at.unix();
        let utc = if self.at.utc {
            wall
        } else if let Some(zone) = self.zone {
            wall - i64::from(zone.offset_at_local(wall))
        } else {
            return (self.at.day, Some(seconds));
        };
        let local = utc + i64::from(offset(utc));
        (
            i32::try_from(local.div_euclid(86_400)).unwrap_or(self.at.day),
            u32::try_from(local.rem_euclid(86_400)).ok(),
        )
    }
}

/// What is read of an event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub start: Option<When>,
    pub end: Option<When>,
    /// `DURATION`, in seconds.
    pub duration: Option<u32>,
    /// The value of its `RRULE`, empty for none (or one too long).
    pub rule: String,
    /// Occurrences left out (`EXDATE`).
    pub exceptions: Vec<When>,
    /// The start this one stands in for, where it is a changed time of an
    /// event that repeats.
    pub instead_of: Option<When>,
    /// `STATUS:CANCELLED`.
    pub cancelled: bool,
    /// `TRANSP:TRANSPARENT` (free time, not busy).
    pub free: bool,
}

/// How long `PT1H30M`, `P1D` or `P2W` is, in seconds.
#[must_use]
pub fn duration(value: &str) -> Option<u32> {
    let rest = value.trim().strip_prefix('P')?;
    let (mut total, mut number, mut time) = (0u32, 0u32, false);
    for c in rest.chars() {
        match c {
            '0'..='9' => number = number.checked_mul(10)?.checked_add(c.to_digit(10)?)?,
            'T' => time = true,
            'W' => total = total.checked_add(number.checked_mul(604_800)?)?,
            'D' => total = total.checked_add(number.checked_mul(86_400)?)?,
            'H' if time => total = total.checked_add(number.checked_mul(3600)?)?,
            'M' if time => total = total.checked_add(number.checked_mul(60)?)?,
            'S' if time => total = total.checked_add(number)?,
            _ => return None,
        }
        if !c.is_ascii_digit() && c != 'T' {
            number = 0;
        }
    }
    Some(total)
}

fn when(property: &Property<'_>, value: &str) -> Option<When> {
    let at = DateTime::parse(value)?;
    let zone = property.param("TZID").and_then(tz::find);
    Some(When { at, zone })
}

/// Reads the events of a calendar text from its content lines. Only what an
/// event says itself counts: its alarms do not.
#[derive(Debug)]
pub struct EventReader {
    depth: u16,
    event: Option<u16>,
    current: Event,
    text_max: usize,
}

impl EventReader {
    /// A reader that keeps `text_max` bytes of a summary and of a place.
    #[must_use]
    pub const fn new(text_max: usize) -> Self {
        Self {
            depth: 0,
            event: None,
            current: Event {
                uid: String::new(),
                summary: String::new(),
                location: String::new(),
                start: None,
                end: None,
                duration: None,
                rule: String::new(),
                exceptions: Vec::new(),
                instead_of: None,
                cancelled: false,
                free: false,
            },
            text_max,
        }
    }

    /// Back to the start of a text.
    pub fn reset(&mut self) {
        *self = Self::new(self.text_max);
    }

    /// Takes a content line; gives an event when the line ends one that has a
    /// start and a `UID` of at most [`UID_MAX`] bytes.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn line(&mut self, line: &[u8]) -> Result<Option<Event>, Error> {
        let Some(property) = Property::parse(line) else {
            return Ok(None);
        };
        if property.is("BEGIN") {
            self.depth = self.depth.saturating_add(1);
            if self.event.is_none() && property.value.trim().eq_ignore_ascii_case("VEVENT") {
                self.event = Some(self.depth);
                self.current = Event::default();
            }
            return Ok(None);
        }
        if property.is("END") {
            let ended = self.event == Some(self.depth);
            self.depth = self.depth.saturating_sub(1);
            if !ended {
                return Ok(None);
            }
            self.event = None;
            let event = core::mem::take(&mut self.current);
            let told = event.start.is_some() && event.uid.len() <= UID_MAX;
            return Ok(told.then_some(event));
        }
        if self.event == Some(self.depth) {
            self.property(&property)?;
        }
        Ok(None)
    }

    fn property(&mut self, property: &Property<'_>) -> Result<(), Error> {
        let event = &mut self.current;
        let value = property.value.trim();
        match property.name.to_ascii_uppercase_buf().as_str() {
            "UID" => {
                event.uid.clear();
                unescape_into(&mut event.uid, value, UID_MAX + 1, true)?;
            }
            "SUMMARY" => {
                event.summary.clear();
                unescape_into(&mut event.summary, property.value, self.text_max, true)?;
            }
            "LOCATION" => {
                event.location.clear();
                unescape_into(&mut event.location, property.value, self.text_max, true)?;
            }
            "DTSTART" => event.start = when(property, value),
            "DTEND" => event.end = when(property, value),
            "DURATION" => event.duration = duration(value),
            "RRULE" if value.len() <= RULE_MAX => {
                event.rule.clear();
                event
                    .rule
                    .try_reserve(value.len())
                    .map_err(|_| Error::NoMemory)?;
                event.rule.push_str(value);
            }
            "EXDATE" => {
                for part in value.split(',') {
                    if event.exceptions.len() >= EXCEPTIONS_MAX {
                        break;
                    }
                    if let Some(at) = when(property, part.trim()) {
                        event
                            .exceptions
                            .try_reserve(1)
                            .map_err(|_| Error::NoMemory)?;
                        event.exceptions.push(at);
                    }
                }
            }
            "RECURRENCE-ID" => event.instead_of = when(property, value),
            "STATUS" => event.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            "TRANSP" => event.free = value.eq_ignore_ascii_case("TRANSPARENT"),
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use crate::date::days_from_civil;
    use crate::ical::Lines;

    fn read(text: &str) -> Vec<Event> {
        let mut lines = Lines::new();
        let mut reader = EventReader::new(80);
        let mut found = Vec::new();
        let mut visit = |line: &[u8]| -> Result<(), Error> {
            if let Some(event) = reader.line(line)? {
                found.push(event);
            }
            Ok(())
        };
        lines.feed(text.as_bytes(), &mut visit).unwrap();
        lines.finish(&mut visit).unwrap();
        found
    }

    #[test]
    fn an_event_with_a_zone_a_rule_and_a_changed_time() {
        let events = read(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a@x\r\nSUMMARY:Team\\, weekly\r\n\
DTSTART;TZID=Europe/Berlin:20261102T100000\r\nDTEND;TZID=Europe/Berlin:20261102T110000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO\r\nEXDATE;TZID=Europe/Berlin:20261109T100000\r\n\
BEGIN:VALARM\r\nSUMMARY:no\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a@x\r\nRECURRENCE-ID;TZID=Europe/Berlin:20261116T100000\r\n\
DTSTART;TZID=Europe/Berlin:20261116T140000\r\nSUMMARY:Team moved\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n",
        );
        assert_eq!(events.len(), 2);
        let first = &events[0];
        assert_eq!(first.summary, "Team, weekly");
        assert_eq!(first.rule, "FREQ=WEEKLY;BYDAY=MO");
        assert_eq!(first.exceptions.len(), 1);
        assert!(first.instead_of.is_none());
        // 10:00 in Berlin in November is 09:00 UTC; a reader in New York
        // (UTC-5) has 04:00.
        let (day, seconds) = first.start.unwrap().local(&mut |_| -18_000);
        assert_eq!(day, days_from_civil(2026, 11, 2) as i32);
        assert_eq!(seconds, Some(4 * 3600));
        assert!(events[1].instead_of.is_some());
    }

    #[test]
    fn all_day_and_utc_and_floating_times() {
        let events = read(
            "BEGIN:VEVENT\nUID:b\nDTSTART;VALUE=DATE:20261224\nDTEND;VALUE=DATE:20261226\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:c\nDTSTART:20261224T230000Z\nDURATION:PT1H30M\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:d\nDTSTART:20261224T090000\nSTATUS:CANCELLED\nEND:VEVENT\n",
        );
        assert_eq!(events.len(), 3);
        let day = days_from_civil(2026, 12, 24) as i32;
        assert_eq!(events[0].start.unwrap().local(&mut |_| 0), (day, None));
        // 23:00 UTC with the reader one hour east is 00:00 the next day.
        assert_eq!(
            events[1].start.unwrap().local(&mut |_| 3600),
            (day + 1, Some(0))
        );
        assert_eq!(events[1].duration, Some(5400));
        assert_eq!(
            events[2].start.unwrap().local(&mut |_| 3600),
            (day, Some(9 * 3600))
        );
        assert!(events[2].cancelled);
    }

    #[test]
    fn durations() {
        assert_eq!(duration("P1D"), Some(86_400));
        assert_eq!(duration("P2W"), Some(1_209_600));
        assert_eq!(duration("PT45M"), Some(2700));
        assert_eq!(duration("P1DT2H"), Some(93_600));
        assert_eq!(duration("1D"), None);
    }
}
