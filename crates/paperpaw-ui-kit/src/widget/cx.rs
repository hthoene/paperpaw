//! What a widget draws with in one callback: the host, its settings, the
//! font chosen for it, the look its settings make, the clock and the catalog.
//! Nothing here outlives the callback.
use crate::{
    Messages, Style,
    widget::{Settings, Shape, settings::text_of},
};
use alloc::string::String;
use paperpaw_sdk::{
    native::{
        Error, Host,
        fonts::{Font, FontRole},
        formatting::Arg,
        snapshot::{self, Snapshot},
    },
    time::{LocalTime, TIME_ZONES, TimeZone, UnixTime},
};

/// The setting that names a time zone by a POSIX rule, and the one that picks
/// a zone from the list.
const ZONE_RULE: &str = "core.time-zone-rule";
const ZONE: &str = "core.time-zone";

/// One widget's drawing context.
#[derive(Debug)]
pub struct Cx<'a> {
    pub host: &'a Host<'a>,
    pub settings: Settings<'a, 'a>,
    pub font: Font<'a>,
    pub shape: Shape,
    pub snapshot: Snapshot,
    pub messages: Messages,
}

impl<'a> Cx<'a> {
    /// The context of `widget` (its id with the placement).
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn open(host: &'a Host<'a>, widget: &'a str) -> Result<Self, Error> {
        let settings = Settings::new(host, widget);
        let font = widget_font(host, widget)?;
        let snapshot = snapshot::capture_snapshot(host)?;
        let style = Style::read(host, &snapshot, &font)?;
        Ok(Self {
            host,
            settings,
            shape: Shape::read(&settings, style)?,
            font,
            snapshot,
            messages: Messages::new()?,
        })
    }

    /// The style the widget draws in.
    #[must_use]
    pub const fn style(&self) -> &Style {
        &self.shape.style
    }

    /// The seconds the chosen time zone is ahead of UTC at `unix`; where
    /// none is chosen, the system's offset from the callback's snapshot.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn zone_offset(&self, unix: i64) -> Result<i32, Error> {
        offset_or_system(self.host, unix, &self.snapshot)
    }

    /// The local date and time now, in the chosen zone, if the device
    /// knows the time.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn local_time(&self) -> Result<Option<LocalTime>, Error> {
        local_time_in(self.host, &self.snapshot)
    }

    /// The catalog's text for `key`.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    pub fn tr(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }

    /// The catalog's text for `key` with its arguments.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    pub fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }

    /// Whether times are shown on a twelve-hour clock (`core.clock-format`).
    ///
    /// # Errors
    /// The host's refusal.
    pub fn twelve_hours(&self) -> Result<bool, Error> {
        text_of(self.host, "core.clock-format", |value| {
            value == paperpaw_sdk::time::CLOCK_12H
        })
    }

    /// A time of day as the reader wants to see it: `14:05` or `2:05 PM`, in
    /// the digits of the language shown.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    #[allow(dead_code, reason = "the world clock and the word clock use it")]
    pub fn clock_of(&mut self, hour: u8, minute: u8) -> Result<String, Error> {
        let mut text = super::settings::Stack::<16>::new();
        if !self.twelve_hours()? {
            core::fmt::Write::write_fmt(&mut text, format_args!("{hour:02}:{minute:02}"))
                .map_err(|_| Error::NoSpace)?;
            return self.messages.digits(self.host, text.as_str());
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
        core::fmt::Write::write_fmt(&mut text, format_args!("{shown}:{minute:02}"))
            .map_err(|_| Error::NoSpace)?;
        let time = self.messages.digits(self.host, text.as_str())?;
        self.messages.format(
            self.host,
            key,
            &[Arg::Text {
                name: "time",
                value: &time,
            }],
        )
    }

    /// The catalog's text for `key` in the form `count` selects, with the
    /// arguments (`{count}` is bound too).
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    pub fn plural(&mut self, key: &str, count: i64, args: &[Arg<'_>]) -> Result<String, Error> {
        let mut all: alloc::vec::Vec<Arg<'_>> = alloc::vec::Vec::new();
        all.try_reserve(args.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        all.push(Arg::Count(count));
        all.extend_from_slice(args);
        self.messages.format(self.host, key, &all)
    }

    /// The first day of the week, 0 (Sunday) or 1 (Monday), as set for the
    /// device, or as the language's habit says.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    pub fn week_start(&mut self) -> Result<u8, Error> {
        let set = text_of(self.host, "core.week-start", |value| match value {
            "core.week.sunday" => Some(0),
            "core.week.monday" => Some(1),
            _ => None,
        })?;
        match set {
            Some(day) => Ok(day),
            None => Ok(u8::from(self.tr("core.week-start-day")? != "0")),
        }
    }

    /// `value` written in the language's digits.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    pub fn number(&mut self, value: i64) -> Result<String, Error> {
        let mut digits = super::settings::Stack::<24>::new();
        core::fmt::Write::write_fmt(&mut digits, format_args!("{value}"))
            .map_err(|_| Error::NoSpace)?;
        self.messages.digits(self.host, digits.as_str())
    }
}

/// The seconds the chosen time zone is ahead of UTC at `unix`; where none is
/// chosen, the system's offset now (as the host's snapshot says; 0 while the
/// device knows no time).
///
/// # Errors
/// The host's refusal.
pub fn zone_offset(host: &Host<'_>, unix: i64) -> Result<i32, Error> {
    match chosen_offset(host, unix)? {
        Some(offset) => Ok(offset),
        None => Ok(system_offset(&snapshot::capture_snapshot(host)?)),
    }
}

/// The local date and time of `snapshot`, in the chosen zone (else the
/// system's), if the device knows the time.
///
/// # Errors
/// The host's refusal.
pub fn local_time_in(host: &Host<'_>, snapshot: &Snapshot) -> Result<Option<LocalTime>, Error> {
    let Some(unix) = snapshot.unix_seconds() else {
        return Ok(None);
    };
    let offset = offset_or_system(host, unix, snapshot)?;
    Ok(Some(UnixTime::from_secs(unix).local(offset)))
}

/// The chosen zone's offset at `unix`, else the system's from `snapshot`.
fn offset_or_system(host: &Host<'_>, unix: i64, snapshot: &Snapshot) -> Result<i32, Error> {
    Ok(chosen_or_system(
        chosen_offset(host, unix)?,
        snapshot.utc_offset_minutes(),
    ))
}

/// The seconds the zone chosen in the settings (a POSIX rule, else one from
/// the list) is ahead of UTC at `unix`; `None` where none is chosen
/// ("automatic", empty or unknown), as Beta 3 then used the system's zone.
fn chosen_offset(host: &Host<'_>, unix: i64) -> Result<Option<i32>, Error> {
    let rule = text_of(host, ZONE_RULE, |rule| {
        TimeZone::parse(rule.trim()).map(|zone| zone.offset_at(unix))
    })?;
    if rule.is_some() {
        return Ok(rule);
    }
    text_of(host, ZONE, |id| {
        TIME_ZONES
            .iter()
            .find(|(name, _)| *name == id)
            .and_then(|(_, rule)| TimeZone::parse(rule))
            .map(|zone| zone.offset_at(unix))
    })
}

/// The system's offset in `snapshot`, in seconds; 0 where it gives none.
fn system_offset(snapshot: &Snapshot) -> i32 {
    chosen_or_system(None, snapshot.utc_offset_minutes())
}

/// The chosen zone's offset in seconds, else the system's (`system`, in
/// minutes, from the snapshot), else 0 (the device knows no time).
fn chosen_or_system(chosen: Option<i32>, system: Option<i16>) -> i32 {
    chosen.unwrap_or_else(|| system.map_or(0, |minutes| i32::from(minutes) * 60))
}

/// The local date and time now, without a drawing context (to tell whether a
/// widget changed, which needs neither font nor look).
///
/// # Errors
/// The host's refusal.
pub fn local_time_now(host: &Host<'_>) -> Result<Option<LocalTime>, Error> {
    local_time_in(host, &snapshot::capture_snapshot(host)?)
}

/// The key of the font the reader chose for a widget
/// (`docs/abi/screens-input.md` section 4).
const WIDGET_FONT: &str = "widget:";

/// The font the reader chose for `widget` (its id with the placement): its
/// own setting `<widget>.font`, else the one for all widgets, as the host
/// resolves the key `widget:<id>`. The menus' font where the host knows no
/// such key or the id is too long for one.
///
/// # Errors
/// The host's refusal.
pub fn widget_font<'a>(host: &'a Host<'a>, widget: &str) -> Result<Font<'a>, Error> {
    use core::fmt::Write;
    let mut key = super::settings::Stack::<128>::new();
    if key.write_str(WIDGET_FONT).is_ok()
        && key.write_str(widget).is_ok()
        && let Ok(font) = Font::acquire(host, key.as_str())
    {
        return Ok(font);
    }
    Font::acquire_role(host, FontRole::Ui)
}

/// The font of `face` (one of [`super::options::FACES`]): the menus' font, or
/// the reader's text font for the book's two (the native fonts have no italic
/// of their own, so the book's italic is the book's font).
///
/// # Errors
/// The host's refusal.
pub fn face_font<'a>(host: &'a Host<'a>, face: &str) -> Result<Font<'a>, Error> {
    let role = match face {
        "widgets.face.book" | "widgets.face.italic" => FontRole::Reading,
        _ => FontRole::Ui,
    };
    Font::acquire_role(host, role)
}

#[cfg(test)]
mod tests {
    use super::chosen_or_system;

    #[test]
    fn with_no_zone_chosen_the_systems_offset_counts() {
        // 08:41 UTC on a device two hours ahead shows 10:41 (#257), as in
        // Beta 3; before, the offset was 0 here.
        assert_eq!(chosen_or_system(None, Some(120)), 7_200);
        assert_eq!(chosen_or_system(None, Some(-330)), -19_800);
    }

    #[test]
    fn a_chosen_zone_wins_and_no_time_is_utc() {
        assert_eq!(chosen_or_system(Some(3_600), Some(120)), 3_600);
        assert_eq!(chosen_or_system(Some(0), Some(120)), 0);
        assert_eq!(chosen_or_system(None, None), 0);
    }
}
