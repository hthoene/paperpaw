//! Days and times as words, shared by the screens, the widgets and the sleep
//! screen: *Today*, *Tomorrow*, a short date, a time of day on the reader's
//! clock, and what an event says of when it is. All from the catalog.
use alloc::string::String;
use paperpaw_caldav::date::{civil_from_days, weekday};
use paperpaw_sdk::native::{Error, Host, formatting::Arg};
use paperpaw_ui_kit::{
    Messages,
    text::{concat, decimal},
    widget::settings::text_of,
};

use crate::model::{NO_TIME, Occ};

/// Whether times are shown on a twelve-hour clock.
///
/// # Errors
/// The host's refusal.
pub fn twelve_hours(host: &Host<'_>) -> Result<bool, Error> {
    text_of(host, "core.clock-format", |value| {
        value == paperpaw_sdk::time::CLOCK_12H
    })
}

fn number(host: &Host<'_>, say: &mut Messages, value: u32) -> Result<String, Error> {
    let mut digits = [0u8; 20];
    let text = decimal(usize::try_from(value).unwrap_or(0), &mut digits);
    say.digits(host, text)
}

/// A time of day (seconds into the day) as the reader wants to see it.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn time_text(
    host: &Host<'_>,
    say: &mut Messages,
    twelve: bool,
    secs: u32,
) -> Result<String, Error> {
    let (hour, minute) = ((secs / 3600) % 24, (secs / 60) % 60);
    let mut text = String::new();
    let shown = if twelve {
        match hour % 12 {
            0 => 12,
            h => h,
        }
    } else {
        hour
    };
    if !twelve && shown < 10 {
        text.push('0');
    }
    let mut digits = [0u8; 20];
    text.push_str(decimal(shown as usize, &mut digits));
    text.push(':');
    if minute < 10 {
        text.push('0');
    }
    text.push_str(decimal(minute as usize, &mut digits));
    let text = say.digits(host, &text)?;
    if !twelve {
        return Ok(text);
    }
    say.format(
        host,
        if hour < 12 {
            "core.clock.am"
        } else {
            "core.clock.pm"
        },
        &[Arg::Text {
            name: "time",
            value: &text,
        }],
    )
}

/// The name of weekday `n` (0 is Sunday), short or long.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn weekday_name(
    host: &Host<'_>,
    say: &mut Messages,
    n: u8,
    short: bool,
) -> Result<String, Error> {
    let mut digits = [0u8; 20];
    let key = concat(&[
        if short {
            "core.weekday.short."
        } else {
            "core.weekday."
        },
        decimal(usize::from(n % 7), &mut digits),
    ])?;
    say.copy(host, &key)
}

/// The name of month `n` (1 to 12), short or long.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn month_name(
    host: &Host<'_>,
    say: &mut Messages,
    n: u8,
    short: bool,
) -> Result<String, Error> {
    let mut digits = [0u8; 20];
    let key = concat(&[
        if short {
            "core.month.short."
        } else {
            "core.month."
        },
        decimal(usize::from(n.clamp(1, 12)), &mut digits),
    ])?;
    say.copy(host, &key)
}

/// A day with its weekday and date: "Wed 18 Nov".
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn short_date(host: &Host<'_>, say: &mut Messages, day: i32) -> Result<String, Error> {
    let (_, month, date) = civil_from_days(i64::from(day));
    let weekday = weekday_name(host, say, weekday(i64::from(day)), true)?;
    let month = month_name(host, say, month, true)?;
    say.format(
        host,
        "calendar.date.short",
        &[
            Arg::Text {
                name: "weekday",
                value: &weekday,
            },
            Arg::Int {
                name: "day",
                value: i64::from(date),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
        ],
    )
}

/// A day in words: *Today*, *Tomorrow*, *Yesterday*, else its short date.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn day_text(
    host: &Host<'_>,
    say: &mut Messages,
    day: i32,
    today: i32,
) -> Result<String, Error> {
    match i64::from(day) - i64::from(today) {
        0 => say.copy(host, "calendar.day.today"),
        1 => say.copy(host, "calendar.day.tomorrow"),
        -1 => say.copy(host, "calendar.day.yesterday"),
        _ => short_date(host, say, day),
    }
}

/// A day with its long date: "Wednesday, 18 November".
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn long_date(host: &Host<'_>, say: &mut Messages, day: i32) -> Result<String, Error> {
    let (_, month, date) = civil_from_days(i64::from(day));
    let weekday = weekday_name(host, say, weekday(i64::from(day)), false)?;
    let month = month_name(host, say, month, false)?;
    say.format(
        host,
        "core.date",
        &[
            Arg::Int {
                name: "day",
                value: i64::from(date),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
            Arg::Text {
                name: "weekday",
                value: &weekday,
            },
        ],
    )
}

/// When an occurrence is within its day: its time, *All day*, or where it
/// goes on.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn when_text(
    host: &Host<'_>,
    say: &mut Messages,
    twelve: bool,
    occ: &Occ,
    long: bool,
) -> Result<String, Error> {
    if occ.goes_on && occ.end != NO_TIME && long {
        let end = time_text(host, say, twelve, occ.end)?;
        return say.format(
            host,
            "calendar.until",
            &[Arg::Text {
                name: "time",
                value: &end,
            }],
        );
    }
    if occ.whole_day() {
        return say.copy(host, "calendar.all-day");
    }
    let start = time_text(host, say, twelve, occ.secs)?;
    if !long || occ.end == NO_TIME {
        return Ok(start);
    }
    let end = time_text(host, say, twelve, occ.end)?;
    concat(&[&start, "–", &end])
}

/// A birthday's title with the age it makes: "Jane (41)".
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn aged(host: &Host<'_>, say: &mut Messages, name: &str, age: u16) -> Result<String, Error> {
    if age == 0 {
        return paperpaw_ui_kit::text::copy(name);
    }
    let years = number(host, say, u32::from(age))?;
    concat(&[name, " (", &years, ")"])
}
