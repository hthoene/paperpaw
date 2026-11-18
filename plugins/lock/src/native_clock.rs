//! The time and today in words for the lock screen, as the user wants them:
//! `14:05` or `2:05 PM` (`core.clock-format`), in the digits of the language,
//! and "Thursday, 15 October". Both are empty where the device does not know
//! the time.
use alloc::string::String;

use paperpaw_sdk::{
    native::{
        Error, Host,
        formatting::Arg,
        settings::{self, SettingValue},
        snapshot,
    },
    time::CLOCK_12H,
};
use paperpaw_ui_kit::{Messages, text, widget::cx::local_time_in};

use crate::native_screen::Clock;

/// Whether times are shown on a twelve-hour clock.
fn twelve_hours(host: &Host<'_>) -> Result<bool, Error> {
    let mut scratch = [0; 64];
    Ok(matches!(
        settings::read_setting(host, "core.clock-format", &mut scratch)?,
        SettingValue::Text(CLOCK_12H)
    ))
}

/// The time of day as it is shown.
fn clock_of(
    host: &Host<'_>,
    messages: &mut Messages,
    hour: u8,
    minute: u8,
) -> Result<String, Error> {
    if !twelve_hours(host)? {
        let digits = text::format(format_args!("{hour:02}:{minute:02}"))?;
        return messages.digits(host, &digits);
    }
    let key = if hour < 12 {
        "core.clock.am"
    } else {
        "core.clock.pm"
    };
    let shown = match hour % 12 {
        0 => 12,
        h => h,
    };
    let digits = text::format(format_args!("{shown}:{minute:02}"))?;
    let time = messages.digits(host, &digits)?;
    messages.format(
        host,
        key,
        &[Arg::Text {
            name: "time",
            value: &time,
        }],
    )
}

/// The clock now.
///
/// # Errors
/// The host's refusal and `NoMemory`.
pub fn now(host: &Host<'_>, messages: &mut Messages) -> Result<Clock, Error> {
    let display = snapshot::capture_snapshot(host)?;
    let Some(now) = local_time_in(host, &display)? else {
        return Ok(Clock::default());
    };
    let weekday = text::format(format_args!("core.weekday.{}", now.weekday))?;
    let month = text::format(format_args!("core.month.{}", now.month))?;
    let weekday = messages.copy(host, &weekday)?;
    let month = messages.copy(host, &month)?;
    let date = messages.format(
        host,
        "core.date",
        &[
            Arg::Text {
                name: "weekday",
                value: &weekday,
            },
            Arg::Int {
                name: "day",
                value: i64::from(now.day),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
        ],
    )?;
    Ok(Clock {
        time: clock_of(host, messages, now.hour, now.minute)?,
        date,
    })
}
