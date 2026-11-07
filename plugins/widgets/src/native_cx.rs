//! What a widget draws with in one callback: the host, its settings, the
//! interface font, the look its settings make, the clock and the catalog.
//! Nothing here outlives the callback.
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
use paperpaw_ui_kit::{
    Messages, Style,
    widget::{Settings, Shape, settings::text_of},
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
        let font = Font::acquire_role(host, FontRole::Ui)?;
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

    /// The seconds the chosen time zone is ahead of UTC at `unix`; 0 where
    /// none is chosen (the system's zone is not known to a plugin).
    ///
    /// # Errors
    /// The host's refusal.
    pub fn zone_offset(&self, unix: i64) -> Result<i32, Error> {
        let rule = text_of(self.host, ZONE_RULE, |rule| {
            TimeZone::parse(rule.trim()).map(|zone| zone.offset_at(unix))
        })?;
        if let Some(offset) = rule {
            return Ok(offset);
        }
        text_of(self.host, ZONE, |id| {
            TIME_ZONES
                .iter()
                .find(|(name, _)| *name == id)
                .and_then(|(_, rule)| TimeZone::parse(rule))
                .map_or(0, |zone| zone.offset_at(unix))
        })
    }

    /// The local date and time now, in the chosen zone, if the device
    /// knows the time.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn local_time(&self) -> Result<Option<LocalTime>, Error> {
        let Some(unix) = self.snapshot.unix_seconds() else {
            return Ok(None);
        };
        let offset = self.zone_offset(unix)?;
        Ok(Some(UnixTime::from_secs(unix).local(offset)))
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
        let mut text = paperpaw_ui_kit::widget::settings::Stack::<16>::new();
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

    /// `value` written in the language's digits.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    pub fn number(&mut self, value: i64) -> Result<String, Error> {
        let mut digits = paperpaw_ui_kit::widget::settings::Stack::<24>::new();
        core::fmt::Write::write_fmt(&mut digits, format_args!("{value}"))
            .map_err(|_| Error::NoSpace)?;
        self.messages.digits(self.host, digits.as_str())
    }
}
