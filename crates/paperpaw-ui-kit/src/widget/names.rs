//! Names of weekdays and months, and dates written as a widget's date format
//! says; all from the catalog, nothing kept.
use super::cx::Cx;
use super::settings::Stack;
use alloc::string::String;
use paperpaw_sdk::native::{Error, formatting::Arg};

const WEEKDAYS: [&str; 7] = [
    "core.weekday.0",
    "core.weekday.1",
    "core.weekday.2",
    "core.weekday.3",
    "core.weekday.4",
    "core.weekday.5",
    "core.weekday.6",
];
const WEEKDAYS_SHORT: [&str; 7] = [
    "core.weekday.short.0",
    "core.weekday.short.1",
    "core.weekday.short.2",
    "core.weekday.short.3",
    "core.weekday.short.4",
    "core.weekday.short.5",
    "core.weekday.short.6",
];
const MONTHS: [&str; 12] = [
    "core.month.1",
    "core.month.2",
    "core.month.3",
    "core.month.4",
    "core.month.5",
    "core.month.6",
    "core.month.7",
    "core.month.8",
    "core.month.9",
    "core.month.10",
    "core.month.11",
    "core.month.12",
];
const MONTHS_SHORT: [&str; 12] = [
    "core.month.short.1",
    "core.month.short.2",
    "core.month.short.3",
    "core.month.short.4",
    "core.month.short.5",
    "core.month.short.6",
    "core.month.short.7",
    "core.month.short.8",
    "core.month.short.9",
    "core.month.short.10",
    "core.month.short.11",
    "core.month.short.12",
];

fn key(keys: &'static [&'static str], at: usize) -> &'static str {
    keys.get(at % keys.len().max(1)).copied().unwrap_or("")
}

/// The name of weekday `n` (0: Sunday).
pub fn weekday_name(cx: &mut Cx<'_>, n: u8) -> Result<String, Error> {
    cx.tr(key(&WEEKDAYS, usize::from(n)))
}

/// The short name of weekday `n` (0: Sunday): "Wed".
pub fn weekday_short(cx: &mut Cx<'_>, n: u8) -> Result<String, Error> {
    cx.tr(key(&WEEKDAYS_SHORT, usize::from(n)))
}

/// The name of month `n` (1 to 12).
pub fn month_name(cx: &mut Cx<'_>, n: u8) -> Result<String, Error> {
    cx.tr(key(&MONTHS, usize::from(n.clamp(1, 12) - 1)))
}

/// The short name of month `n` (1 to 12): "Oct".
pub fn month_short(cx: &mut Cx<'_>, n: u8) -> Result<String, Error> {
    cx.tr(key(&MONTHS_SHORT, usize::from(n.clamp(1, 12) - 1)))
}

/// The part of the choice `<widget>.<key>` after the last dot (`circle` of
/// `widgets.mark.circle`), as an owned short string.
pub fn chosen_name(cx: &Cx<'_>, key: &str) -> Result<Stack<48>, Error> {
    cx.settings.with_text(key, |value| {
        let mut out = Stack::<48>::new();
        let name = value.rsplit('.').next().unwrap_or(value);
        // A name that does not fit is no name of ours: empty reads as the default.
        if core::fmt::Write::write_str(&mut out, name).is_err() {
            return Stack::<48>::new();
        }
        out
    })
}

/// `year`-`month`-`day` written as the widget's date format says; `year`
/// left out where `with_year` is not set (but in numbers).
pub fn date_text(
    cx: &mut Cx<'_>,
    year: i32,
    month: u8,
    day: u8,
    with_year: bool,
) -> Result<String, Error> {
    let month_text = month_name(cx, month)?;
    let weekday = weekday_name(cx, paperpaw_sdk::time::weekday(year, month, day))?;
    let format = chosen_name(cx, "date-format")?;
    match format.as_str() {
        "iso" => {
            let mut text = Stack::<24>::new();
            core::fmt::Write::write_fmt(&mut text, format_args!("{year:04}-{month:02}-{day:02}"))
                .map_err(|_| Error::NoSpace)?;
            cx.messages.digits(cx.host, text.as_str())
        }
        "numeric" => {
            let mut day_text = Stack::<8>::new();
            core::fmt::Write::write_fmt(&mut day_text, format_args!("{day:02}"))
                .map_err(|_| Error::NoSpace)?;
            let mut month_digits = Stack::<8>::new();
            core::fmt::Write::write_fmt(&mut month_digits, format_args!("{month:02}"))
                .map_err(|_| Error::NoSpace)?;
            let day_digits = cx.messages.digits(cx.host, day_text.as_str())?;
            let month_digits = cx.messages.digits(cx.host, month_digits.as_str())?;
            cx.format(
                "widgets.date.numeric",
                &[
                    Arg::Text {
                        name: "day",
                        value: &day_digits,
                    },
                    Arg::Text {
                        name: "month",
                        value: &month_digits,
                    },
                    Arg::Int {
                        name: "year",
                        value: i64::from(year),
                    },
                ],
            )
        }
        other => {
            let args = [
                Arg::Int {
                    name: "day",
                    value: i64::from(day),
                },
                Arg::Text {
                    name: "month",
                    value: &month_text,
                },
                Arg::Int {
                    name: "year",
                    value: i64::from(year),
                },
                Arg::Text {
                    name: "weekday",
                    value: &weekday,
                },
            ];
            let key = if other == "weekday" {
                "core.date"
            } else if with_year {
                "widgets.date.long"
            } else {
                "widgets.date.short"
            };
            cx.messages.format(cx.host, key, &args)
        }
    }
}
