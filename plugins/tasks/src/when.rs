//! The day a reader types for a task: `2026-11-20`, `20.11.2026`, `20.11.`,
//! `11/20`, or a number of days from today.
use paperpaw_caldav::date::{civil_from_days, days_from_civil, days_in_month};

fn day_of(year: i32, month: u32, day: u32) -> Option<i32> {
    let (month, day) = (u8::try_from(month).ok()?, u8::try_from(day).ok()?);
    if !(1..=9999).contains(&year) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    i32::try_from(days_from_civil(year, month, day)).ok()
}

/// The year a typed one means: `26` is 2026.
fn year_of(text: &str) -> Option<i32> {
    let year: i32 = text.parse().ok()?;
    Some(if text.len() <= 2 { 2000 + year } else { year })
}

/// The day `text` means, seen from `today` (both in days since 1970). A date
/// without a year is the next such day.
#[must_use]
pub fn parse_day(text: &str, today: i32) -> Option<i32> {
    let text = text.trim().trim_end_matches('.');
    if text.is_empty() || !text.is_ascii() {
        return None;
    }
    if text.len() <= 3 && text.bytes().all(|b| b.is_ascii_digit()) {
        return today.checked_add(text.parse().ok()?);
    }
    let separator = ['-', '.', '/'].into_iter().find(|c| text.contains(*c))?;
    let mut parts = text.split(separator).map(str::trim);
    let (a, b, c) = (parts.next()?, parts.next()?, parts.next());
    if parts.next().is_some() {
        return None;
    }
    let number = |text: &str| text.parse::<u32>().ok();
    // Year first with `-`; day first with `.`; month first with `/`.
    let (year, month, day) = match (separator, c) {
        ('-', Some(day)) => (Some(a), number(b)?, number(day)?),
        ('-', None) => (None, number(a)?, number(b)?),
        ('.', year) => (year, number(b)?, number(a)?),
        (_, year) => (year, number(a)?, number(b)?),
    };
    let year = match year {
        Some(text) => Some(year_of(text)?),
        None => None,
    };
    if let Some(year) = year {
        return day_of(year, month, day);
    }
    let (this_year, _, _) = civil_from_days(i64::from(today));
    // The next year that has the day (the 29th of February: within four).
    (this_year..=this_year + 4)
        .filter_map(|year| day_of(year, month, day))
        .find(|found| *found >= today)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-18.
    const TODAY: i32 = 20_775;

    #[test]
    fn dates_are_read_as_they_are_typed() {
        for (text, day) in [
            ("2026-11-20", Some(20_777)),
            (" 2026-11-20 ", Some(20_777)),
            ("20.11.2026", Some(20_777)),
            ("20.11.26", Some(20_777)),
            ("20.11.", Some(20_777)),
            ("20.11", Some(20_777)),
            ("11/20", Some(20_777)),
            ("11/20/2026", Some(20_777)),
            ("11-20", Some(20_777)),
            ("0", Some(TODAY)),
            ("3", Some(TODAY + 3)),
            // A day already past this year is next year's.
            ("17.11.", Some(TODAY - 1 + 365)),
            ("18.11.", Some(TODAY)),
            ("29.2.", Some(21_243)),
            ("31.11.", None),
            ("2026-13-01", None),
            ("", None),
            ("tomorrow", None),
            ("1.2.3.4", None),
            ("१.२.", None),
        ] {
            assert_eq!(parse_day(text, TODAY), day, "{text}");
        }
    }
}
