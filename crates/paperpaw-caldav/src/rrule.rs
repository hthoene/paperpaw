//! Recurrence rules (`RRULE`, RFC 5545) for the common cases: daily, weekly,
//! monthly and yearly with an interval, `COUNT` or `UNTIL`, `BYDAY` (also
//! `1MO` and `-1FR`), `BYMONTHDAY` (also negative), `BYMONTH` and `BYSETPOS`.
//! Days are counted from 1970-01-01, as everywhere in this crate; a rule says
//! which days an event falls on and the caller keeps the time of day.
//!
//! A rule with parts that are not read (`BYYEARDAY`, `BYWEEKNO`, a frequency
//! finer than a day) is refused by [`Rule::parse`]: the event is then shown
//! once, at its start, which is wrong less often than a guess would be.
use alloc::vec::Vec;

use crate::date::DateTime;
use crate::date::{civil_from_days, days_from_civil, days_in_month, weekday};

/// The most periods (days, weeks, months, years) looked at for one rule.
const PERIODS_MAX: u32 = 40_000;
/// The most days a period names (every weekday of a year).
const DAYS_MAX: usize = 400;

/// How often a rule repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// A recurrence rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub freq: Freq,
    /// Every this many days, weeks, months or years (at least 1).
    pub interval: u16,
    /// The most occurrences, counted from the start; 0 for no limit.
    pub count: u32,
    /// The last day an occurrence may fall on.
    pub until: Option<i32>,
    /// `BYDAY`: `ordinal * 8 + weekday + 1` (weekday 0 is Sunday, ordinal 0
    /// every such day), ended by a 0.
    by_day: [i8; 8],
    /// `BYMONTHDAY`: bit `n - 1` is day `n`; the second mask counts from the
    /// month's end (bit `n - 1` is day `-n`).
    month_days: u32,
    month_days_back: u32,
    /// `BYMONTH`: bit `n - 1` is month `n`.
    months: u16,
    /// `BYSETPOS`, ended by a 0.
    set_pos: [i8; 4],
    /// The day a week starts on (0 is Sunday).
    wkst: u8,
}

fn day_code(name: &str) -> Option<u8> {
    Some(match name {
        "SU" => 0,
        "MO" => 1,
        "TU" => 2,
        "WE" => 3,
        "TH" => 4,
        "FR" => 5,
        "SA" => 6,
        _ => return None,
    })
}

impl Rule {
    /// Reads the value of an `RRULE` property; `None` for what is not read.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let mut rule = Self {
            freq: Freq::Daily,
            interval: 1,
            count: 0,
            until: None,
            by_day: [0; 8],
            month_days: 0,
            month_days_back: 0,
            months: 0,
            set_pos: [0; 4],
            wkst: 1,
        };
        let mut freq = false;
        for part in value.trim().split(';') {
            let (name, text) = part.split_once('=')?;
            let text = text.trim();
            match name.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    freq = true;
                    rule.freq = match text.to_ascii_uppercase().as_str() {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        _ => return None,
                    };
                }
                "INTERVAL" => rule.interval = text.parse::<u16>().ok()?.max(1),
                "COUNT" => rule.count = text.parse().ok()?,
                "UNTIL" => rule.until = Some(DateTime::parse(text)?.day),
                "WKST" => rule.wkst = day_code(&text.to_ascii_uppercase())?,
                "BYDAY" => rule.read_days(text)?,
                "BYMONTHDAY" => {
                    for item in text.split(',') {
                        let n: i32 = item.trim().parse().ok()?;
                        let bit = |n: i32| 1u32 << (n.unsigned_abs() - 1);
                        match n {
                            1..=31 => rule.month_days |= bit(n),
                            -31..=-1 => rule.month_days_back |= bit(n),
                            _ => return None,
                        }
                    }
                }
                "BYMONTH" => {
                    for item in text.split(',') {
                        let n: u8 = item.trim().parse().ok()?;
                        if !(1..=12).contains(&n) {
                            return None;
                        }
                        rule.months |= 1 << (n - 1);
                    }
                }
                "BYSETPOS" => {
                    for (slot, item) in rule.set_pos.iter_mut().zip(text.split(',')) {
                        let n: i8 = item.trim().parse().ok()?;
                        if n == 0 {
                            return None;
                        }
                        *slot = n;
                    }
                }
                // Parts that change what the rule means and are not read.
                "BYYEARDAY" | "BYWEEKNO" | "BYHOUR" | "BYMINUTE" | "BYSECOND" => return None,
                _ => {}
            }
        }
        freq.then_some(rule)
    }

    fn read_days(&mut self, text: &str) -> Option<()> {
        let mut at = 0;
        for item in text.split(',') {
            let item = item.trim().to_ascii_uppercase();
            let split = item.len().checked_sub(2)?;
            let (number, name) = (item.get(..split)?, item.get(split..)?);
            let ordinal: i8 = if number.is_empty() {
                0
            } else {
                number.parse().ok()?
            };
            if !(-5..=5).contains(&ordinal) {
                return None;
            }
            let code = i8::try_from(day_code(name)?).ok()? + 1;
            if let Some(slot) = self.by_day.get_mut(at) {
                *slot = ordinal * 8 + code;
                at += 1;
            }
        }
        Some(())
    }

    /// The weekdays `BYDAY` names with their ordinals.
    fn days(&self) -> impl Iterator<Item = (i8, u8)> + '_ {
        self.by_day
            .iter()
            .take_while(|code| **code != 0)
            .map(|code| {
                let code = i16::from(*code);
                let wd = (code - 1).rem_euclid(8);
                let ordinal = (code - 1).div_euclid(8);
                (ordinal as i8, wd as u8)
            })
    }

    fn has_weekday(&self, wd: u8) -> bool {
        self.days().any(|(_, day)| day == wd)
    }

    /// Whether the days of the rule are listed or fall by the start.
    fn month_day_ok(&self, y: i32, m: u8, d: u8) -> bool {
        if self.month_days == 0 && self.month_days_back == 0 {
            return true;
        }
        let back = days_in_month(y, m) - d + 1;
        self.month_days & (1 << (d - 1)) != 0 || self.month_days_back & (1 << (back - 1)) != 0
    }

    fn month_ok(&self, m: u8) -> bool {
        self.months == 0 || self.months & (1 << (m - 1)) != 0
    }

    /// The days of `first..=last` that are weekday `wd`, the `ordinal`th of
    /// them, or all with 0.
    fn weekdays_in(first: i32, last: i32, (ordinal, wd): (i8, u8), out: &mut Vec<i32>) {
        let lead = (i32::from(wd) - i32::from(weekday(i64::from(first))) + 7) % 7;
        let mut day = first + lead;
        let (mut all, mut n) = (Vec::new(), 0usize);
        while day <= last && n < DAYS_MAX {
            all.push(day);
            day += 7;
            n += 1;
        }
        match ordinal {
            0 => out.extend_from_slice(&all),
            1.. => out.extend(all.get(usize::from(ordinal.unsigned_abs()) - 1)),
            _ => out.extend(
                all.len()
                    .checked_sub(usize::from(ordinal.unsigned_abs()))
                    .and_then(|at| all.get(at)),
            ),
        }
    }

    /// The days of one month the rule names, `start_day` being the day of the
    /// month the event starts on.
    fn in_month(&self, (y, m): (i32, u8), start_day: u8, out: &mut Vec<i32>) {
        if !self.month_ok(m) {
            return;
        }
        let dim = days_in_month(y, m);
        let first = i32::try_from(days_from_civil(y, m, 1)).unwrap_or(0);
        let last = first + i32::from(dim) - 1;
        if self.by_day[0] != 0 {
            let mut found = Vec::new();
            for entry in self.days() {
                Self::weekdays_in(first, last, entry, &mut found);
            }
            for day in found {
                let (_, _, d) = civil_from_days(i64::from(day));
                if self.month_day_ok(y, m, d) {
                    out.push(day);
                }
            }
        } else if self.month_days != 0 || self.month_days_back != 0 {
            for d in 1..=dim {
                if self.month_day_ok(y, m, d) {
                    out.push(first + i32::from(d) - 1);
                }
            }
        } else if start_day <= dim {
            out.push(first + i32::from(start_day) - 1);
        }
    }

    /// The days of period `k` (counted from the start's own) sorted, and the
    /// first and last day the period covers.
    fn period(&self, start: i32, k: i64, out: &mut Vec<i32>) -> (i32, i32) {
        let step = i64::from(self.interval.max(1)) * k;
        let (sy, sm, sd) = civil_from_days(i64::from(start));
        let clamp = |n: i64| i32::try_from(n).unwrap_or(i32::MAX);
        let (first, last) = match self.freq {
            Freq::Daily => {
                let day = clamp(i64::from(start) + step);
                let (y, m, d) = civil_from_days(i64::from(day));
                let by_day = self.by_day[0] == 0 || self.has_weekday(weekday(i64::from(day)));
                if by_day && self.month_ok(m) && self.month_day_ok(y, m, d) {
                    out.push(day);
                }
                (day, day)
            }
            Freq::Weekly => {
                let lead = (i32::from(weekday(i64::from(start))) - i32::from(self.wkst) + 7) % 7;
                let begin = clamp(i64::from(start) - i64::from(lead) + step * 7);
                for day in begin..begin.saturating_add(7) {
                    let wd = weekday(i64::from(day));
                    let (y, m, d) = civil_from_days(i64::from(day));
                    let by_day = if self.by_day[0] == 0 {
                        wd == weekday(i64::from(start))
                    } else {
                        self.has_weekday(wd)
                    };
                    if by_day && self.month_ok(m) && self.month_day_ok(y, m, d) {
                        out.push(day);
                    }
                }
                (begin, begin.saturating_add(6))
            }
            Freq::Monthly => {
                let index = i64::from(sy) * 12 + i64::from(sm) - 1 + step;
                let (y, m) = (
                    clamp(index.div_euclid(12)),
                    (index.rem_euclid(12) + 1) as u8,
                );
                self.in_month((y, m), sd, out);
                let first = clamp(days_from_civil(y, m, 1));
                (first, first + i32::from(days_in_month(y, m)) - 1)
            }
            Freq::Yearly => {
                let y = clamp(i64::from(sy) + step);
                let first = clamp(days_from_civil(y, 1, 1));
                let last = clamp(days_from_civil(y, 12, 31));
                if self.months == 0 && self.by_day[0] != 0 && self.month_days == 0 {
                    // Weekdays of the year (`20MO` is the twentieth Monday).
                    for entry in self.days() {
                        Self::weekdays_in(first, last, entry, out);
                    }
                } else if self.months == 0 {
                    self.in_month((y, sm), sd, out);
                } else {
                    for m in 1..=12 {
                        self.in_month((y, m), sd, out);
                    }
                }
                (first, last)
            }
        };
        out.sort_unstable();
        out.dedup();
        if self.set_pos[0] != 0 {
            let all = core::mem::take(out);
            for pos in self.set_pos.iter().take_while(|pos| **pos != 0) {
                let at = if *pos > 0 {
                    usize::from(pos.unsigned_abs()).checked_sub(1)
                } else {
                    all.len().checked_sub(usize::from(pos.unsigned_abs()))
                };
                out.extend(at.and_then(|at| all.get(at)));
            }
            out.sort_unstable();
            out.dedup();
        }
        (first, last)
    }

    /// The period to begin at so that none before `from` is looked at (only
    /// where `COUNT` does not need them counted).
    fn skip_to(&self, start: i32, from: i32) -> i64 {
        let interval = i64::from(self.interval.max(1));
        let gap = i64::from(from) - i64::from(start);
        let (sy, sm, _) = civil_from_days(i64::from(start));
        let (fy, fm, _) = civil_from_days(i64::from(from));
        let periods = match self.freq {
            Freq::Daily => gap,
            Freq::Weekly => gap / 7,
            Freq::Monthly => {
                (i64::from(fy) * 12 + i64::from(fm)) - (i64::from(sy) * 12 + i64::from(sm))
            }
            Freq::Yearly => i64::from(fy) - i64::from(sy),
        };
        (periods / interval - 1).max(0)
    }

    /// Tells `visit` each day the rule names from `from` to `to`, in order;
    /// `visit` says whether to go on. The event starts on `start`.
    pub fn each(&self, start: i32, (from, to): (i32, i32), visit: &mut dyn FnMut(i32) -> bool) {
        let mut k = if self.count == 0 {
            self.skip_to(start, from)
        } else {
            0
        };
        let (mut seen, mut days) = (0u32, Vec::new());
        for _ in 0..PERIODS_MAX {
            days.clear();
            let (first, _) = self.period(start, k, &mut days);
            if first > to || self.until.is_some_and(|until| first > until) {
                return;
            }
            for day in &days {
                let day = *day;
                if day < start {
                    continue;
                }
                if self.until.is_some_and(|until| day > until) || day > to {
                    return;
                }
                seen += 1;
                if self.count > 0 && seen > self.count {
                    return;
                }
                if day >= from && !visit(day) {
                    return;
                }
            }
            k += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    fn day(y: i32, m: u8, d: u8) -> i32 {
        days_from_civil(y, m, d) as i32
    }

    fn list(
        rule: &str,
        start: (i32, u8, u8),
        from: (i32, u8, u8),
        to: (i32, u8, u8),
    ) -> Vec<(u8, u8)> {
        let rule = Rule::parse(rule).unwrap();
        let mut found = Vec::new();
        rule.each(
            day(start.0, start.1, start.2),
            (day(from.0, from.1, from.2), day(to.0, to.1, to.2)),
            &mut |d| {
                let (_, m, d) = civil_from_days(i64::from(d));
                found.push((m, d));
                true
            },
        );
        found
    }

    #[test]
    fn daily_with_interval_and_count() {
        let all = list(
            "FREQ=DAILY;INTERVAL=3;COUNT=4",
            (2026, 1, 1),
            (2026, 1, 1),
            (2026, 12, 31),
        );
        assert_eq!(all, [(1, 1), (1, 4), (1, 7), (1, 10)]);
        // The count runs from the start, not from the window.
        let late = list(
            "FREQ=DAILY;INTERVAL=3;COUNT=4",
            (2026, 1, 1),
            (2026, 1, 5),
            (2026, 12, 31),
        );
        assert_eq!(late, [(1, 7), (1, 10)]);
    }

    #[test]
    fn weekly_on_days_until() {
        // 2026-11-02 is a Monday.
        let found = list(
            "FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261118T000000Z",
            (2026, 11, 2),
            (2026, 11, 1),
            (2026, 12, 31),
        );
        assert_eq!(
            found,
            [(11, 2), (11, 4), (11, 9), (11, 11), (11, 16), (11, 18)]
        );
        let every_other = list(
            "FREQ=WEEKLY;INTERVAL=2",
            (2026, 11, 2),
            (2026, 11, 1),
            (2026, 12, 1),
        );
        assert_eq!(every_other, [(11, 2), (11, 16), (11, 30)]);
    }

    #[test]
    fn monthly_by_position_and_day() {
        // Last Friday of each month.
        let last = list(
            "FREQ=MONTHLY;BYDAY=-1FR",
            (2026, 1, 30),
            (2026, 1, 1),
            (2026, 4, 30),
        );
        assert_eq!(last, [(1, 30), (2, 27), (3, 27), (4, 24)]);
        // Second Tuesday.
        let second = list(
            "FREQ=MONTHLY;BYDAY=2TU",
            (2026, 1, 13),
            (2026, 1, 1),
            (2026, 3, 31),
        );
        assert_eq!(second, [(1, 13), (2, 10), (3, 10)]);
        // The 31st skips short months.
        let long = list("FREQ=MONTHLY", (2026, 1, 31), (2026, 1, 1), (2026, 5, 31));
        assert_eq!(long, [(1, 31), (3, 31), (5, 31)]);
        // The last day, and the last weekday by position.
        let end = list(
            "FREQ=MONTHLY;BYMONTHDAY=-1",
            (2026, 1, 31),
            (2026, 1, 1),
            (2026, 3, 31),
        );
        assert_eq!(end, [(1, 31), (2, 28), (3, 31)]);
        let work = list(
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            (2026, 1, 30),
            (2026, 1, 1),
            (2026, 3, 31),
        );
        assert_eq!(work, [(1, 30), (2, 27), (3, 31)]);
    }

    #[test]
    fn yearly_cases() {
        let birthday = list("FREQ=YEARLY", (2020, 2, 29), (2024, 1, 1), (2028, 12, 31));
        assert_eq!(birthday, [(2, 29), (2, 29)]);
        let thanks = list(
            "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH",
            (2026, 11, 26),
            (2026, 1, 1),
            (2027, 12, 31),
        );
        assert_eq!(thanks, [(11, 26), (11, 25)]);
    }

    #[test]
    fn what_is_not_read_is_refused() {
        assert!(Rule::parse("FREQ=YEARLY;BYWEEKNO=20").is_none());
        assert!(Rule::parse("FREQ=HOURLY").is_none());
        assert!(Rule::parse("COUNT=3").is_none());
    }

    #[test]
    fn skipping_ahead_changes_nothing() {
        for text in [
            "FREQ=DAILY;INTERVAL=5",
            "FREQ=WEEKLY;BYDAY=TU,TH;INTERVAL=3",
            "FREQ=MONTHLY;BYDAY=1MO;INTERVAL=2",
            "FREQ=YEARLY;INTERVAL=2",
        ] {
            let rule = Rule::parse(text).unwrap();
            let start = day(2020, 3, 3);
            let mut with_skip = Vec::new();
            rule.each(start, (day(2026, 1, 1), day(2026, 12, 31)), &mut |d| {
                with_skip.push(d);
                true
            });
            // The same rule with a count too high to matter counts from the start.
            let mut counted = Rule::parse(text).unwrap();
            counted.count = 100_000;
            let mut without = Vec::new();
            counted.each(start, (day(2026, 1, 1), day(2026, 12, 31)), &mut |d| {
                without.push(d);
                true
            });
            assert_eq!(with_skip, without, "{text}");
            assert!(!with_skip.is_empty(), "{text}");
        }
    }
}
