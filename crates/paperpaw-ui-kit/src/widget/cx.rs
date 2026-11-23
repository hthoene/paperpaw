//! What a widget draws with in one callback: the host, its settings, the
//! font chosen for it, the look its settings make, the clock and the catalog.
//! Nothing here outlives the callback.
use crate::{
    Messages, SharedInputs, Style,
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

/// The look a widget's context was made with, kept for the next one of the
/// same display ([`Cx::open_kept`], [`Cx::keep`]).
#[derive(Debug)]
pub struct Kept {
    display: (u16, u16),
    shape: Shape,
}

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

    /// As [`Cx::open`], with the look (the interface's style and the widget's
    /// own, some thirty settings) taken from `kept` where it is for this
    /// display, read and made again where it is not. The widget's owner keeps
    /// the look with [`Cx::keep`] and lets it go where a setting changes.
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn open_kept(
        host: &'a Host<'a>,
        widget: &'a str,
        kept: Option<Kept>,
    ) -> Result<Self, Error> {
        Self::open_kept_with(host, widget, kept, &SharedInputs::default())
    }

    /// As [`Cx::open_kept`], with the interface's settings taken from `shared`
    /// where a lay-out shares them ([`SharedInputs::share`]).
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn open_kept_with(
        host: &'a Host<'a>,
        widget: &'a str,
        kept: Option<Kept>,
        shared: &SharedInputs,
    ) -> Result<Self, Error> {
        let settings = Settings::new(host, widget);
        let font = widget_font(host, widget)?;
        let snapshot = snapshot::capture_snapshot(host)?;
        let display = (snapshot.display_width(), snapshot.display_height());
        let shape = match for_display(kept.map(|kept| (kept.display, kept.shape)), display) {
            Some(shape) => shape,
            None => Shape::read(&settings, shared.style(host, &snapshot, &font)?)?,
        };
        Ok(Self {
            host,
            settings,
            shape,
            font,
            snapshot,
            messages: Messages::new()?,
        })
    }

    /// The look of this context, to be kept for the next one ([`Cx::open_kept`]).
    #[must_use]
    pub fn keep(self) -> Kept {
        Kept {
            display: (
                self.snapshot.display_width(),
                self.snapshot.display_height(),
            ),
            shape: self.shape,
        }
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
    pub fn clock_of(&mut self, hour: u8, minute: u8) -> Result<String, Error> {
        let twelve = self.twelve_hours()?;
        clock_text(&mut self.messages, self.host, twelve, (hour, minute))
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

/// The time zone local days are counted in, read once: the zone chosen in
/// the settings, else the system's offset of one snapshot (0 while the device
/// knows no time). For turning many moments into local days without asking
/// the host for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalZone {
    chosen: Option<TimeZone>,
    /// The system's offset in seconds, where no zone is chosen.
    system: i32,
}

impl LocalZone {
    /// The zone as the settings and `snapshot` say it now.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn read(host: &Host<'_>, snapshot: &Snapshot) -> Result<Self, Error> {
        Ok(Self {
            chosen: chosen_zone(host)?,
            system: system_offset(snapshot),
        })
    }

    /// A zone always `offset` seconds ahead of UTC.
    #[must_use]
    pub const fn fixed(offset: i32) -> Self {
        Self {
            chosen: None,
            system: offset,
        }
    }

    /// The seconds the zone is ahead of UTC at `unix`.
    #[must_use]
    pub fn offset_at(&self, unix: i64) -> i32 {
        self.chosen.map_or(self.system, |zone| zone.offset_at(unix))
    }

    /// The local day of `unix`, in days since 1970-01-01.
    #[must_use]
    pub fn day_of(&self, unix: i64) -> i32 {
        let day = unix
            .saturating_add(i64::from(self.offset_at(unix)))
            .div_euclid(86_400);
        i32::try_from(day).unwrap_or(if day < 0 { i32::MIN } else { i32::MAX })
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

/// The zone chosen in the settings (a POSIX rule, else one from the list);
/// `None` where none is chosen ("automatic", empty or unknown), as Beta 3
/// then used the system's zone.
fn chosen_zone(host: &Host<'_>) -> Result<Option<TimeZone>, Error> {
    let rule = text_of(host, ZONE_RULE, |rule| TimeZone::parse(rule.trim()))?;
    if rule.is_some() {
        return Ok(rule);
    }
    text_of(host, ZONE, |id| {
        TIME_ZONES
            .iter()
            .find(|(name, _)| *name == id)
            .and_then(|(_, rule)| TimeZone::parse(rule))
    })
}

/// The seconds the zone chosen in the settings is ahead of UTC at `unix`;
/// `None` where none is chosen.
fn chosen_offset(host: &Host<'_>, unix: i64) -> Result<Option<i32>, Error> {
    Ok(chosen_zone(host)?.map(|zone| zone.offset_at(unix)))
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

/// What was kept for the display it was kept for, none for another one (the
/// look depends on the display's size).
fn for_display<S>(kept: Option<((u16, u16), S)>, display: (u16, u16)) -> Option<S> {
    kept.filter(|(was, _)| *was == display)
        .map(|(_, kept)| kept)
}

/// A time of day on a `twelve`-hour clock or not: `14:05` or `2:05 PM`, in
/// the digits of the language shown (the clock of a widget and of a page).
///
/// # Errors
/// The host's refusal and [`Error::NoMemory`].
pub(crate) fn clock_text(
    messages: &mut Messages,
    host: &Host<'_>,
    twelve: bool,
    (hour, minute): (u8, u8),
) -> Result<String, Error> {
    let mut text = super::settings::Stack::<16>::new();
    if !twelve {
        core::fmt::Write::write_fmt(&mut text, format_args!("{hour:02}:{minute:02}"))
            .map_err(|_| Error::NoSpace)?;
        return messages.digits(host, text.as_str());
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
    let time = messages.digits(host, text.as_str())?;
    messages.format(
        host,
        key,
        &[Arg::Text {
            name: "time",
            value: &time,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::{LocalZone, chosen_or_system, for_display};
    use paperpaw_sdk::time::TimeZone;

    #[test]
    fn a_kept_look_is_for_the_display_it_was_made_for() {
        assert_eq!(
            for_display(Some(((480, 800), "look")), (480, 800)),
            Some("look")
        );
        // Turned, or another panel: read again.
        assert_eq!(for_display(Some(((480, 800), "look")), (800, 480)), None);
        assert_eq!(for_display(Some(((480, 800), "look")), (480, 480)), None);
        assert_eq!(for_display::<&str>(None, (480, 800)), None);
    }

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

    #[test]
    fn a_local_day_starts_at_local_midnight() {
        // 2025-10-14 23:30 UTC is already the 15th one hour east, and still
        // the 14th one hour west.
        let unix = 1_760_484_600;
        assert_eq!(LocalZone::fixed(0).day_of(unix), 20_375);
        assert_eq!(LocalZone::fixed(3_600).day_of(unix), 20_376);
        assert_eq!(LocalZone::fixed(-3_600).day_of(unix), 20_375);
        // Before 1970 the day counts down, not towards 0.
        assert_eq!(LocalZone::fixed(0).day_of(-1), -1);
    }

    #[test]
    fn a_chosen_zone_follows_its_summer_time() {
        let zone = LocalZone {
            chosen: TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3"),
            system: 0,
        };
        // Past 22:00 UTC: midnight has passed in summer (+2), not in winter (+1).
        assert_eq!(zone.offset_at(1_751_408_200), 7_200);
        assert_eq!(zone.day_of(1_751_408_200), 20_271);
        assert_eq!(zone.offset_at(1_736_548_200), 3_600);
        assert_eq!(zone.day_of(1_736_548_200), 20_098);
    }
}
