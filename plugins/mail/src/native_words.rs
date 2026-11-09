//! The words of the screens from the host: the catalog's messages, and the
//! time a mail came as the reader wants to see it.
use crate::native_model::browser::{When, Words, when_of};
use crate::native_model::mem::Error as MailError;
use crate::native_model::texts::Say;
use alloc::string::String;
use paperpaw_sdk::native::{Error, Host, formatting::Arg, snapshot};
use paperpaw_ui_kit::{Messages, text, widget::settings::text_of};

/// Words from the catalog and the clock, for one reading.
pub(crate) struct HostWords<'h> {
    host: &'h Host<'h>,
    messages: Messages,
    /// Now, in Unix seconds, if the device knows.
    unix: Option<i64>,
    /// Seconds the reader's time zone is ahead of UTC.
    offset: i64,
}

impl<'h> HostWords<'h> {
    /// The words, with the time as it is now.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusal.
    pub(crate) fn new(host: &'h Host<'h>) -> Result<Self, Error> {
        let snapshot = snapshot::capture_snapshot(host)?;
        Ok(Self {
            host,
            messages: Messages::new()?,
            unix: snapshot.unix_seconds(),
            offset: snapshot.utc_offset_minutes().map_or(0, i64::from) * 60,
        })
    }

    /// A message of the catalog with what it takes.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusal.
    pub(crate) fn say(&mut self, say: Say<'_>) -> Result<String, Error> {
        match say {
            Say::Key(key) => self.messages.copy(self.host, key),
            Say::One { key, name, text } => {
                self.messages
                    .format(self.host, key, &[Arg::Text { name, value: text }])
            }
            Say::Two { key, first, second } => self.messages.format(
                self.host,
                key,
                &[
                    Arg::Text {
                        name: first.0,
                        value: first.1,
                    },
                    Arg::Text {
                        name: second.0,
                        value: second.1,
                    },
                ],
            ),
        }
    }

    /// A time of day: `14:05`, or `2:05 PM` where the reader keeps a
    /// twelve-hour clock, in the digits of the language shown.
    fn clock(&mut self, hour: u8, minute: u8) -> Result<String, Error> {
        let twelve = text_of(self.host, "core.clock-format", |value| {
            value == paperpaw_sdk::time::CLOCK_12H
        })?;
        if !twelve {
            let plain = text::format(format_args!("{hour:02}:{minute:02}"))?;
            return self.messages.digits(self.host, &plain);
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
        let plain = text::format(format_args!("{shown}:{minute:02}"))?;
        let time = self.messages.digits(self.host, &plain)?;
        self.messages.format(
            self.host,
            key,
            &[Arg::Text {
                name: "time",
                value: &time,
            }],
        )
    }
}

impl Words for HostWords<'_> {
    fn tr(&mut self, key: &str) -> Result<String, MailError> {
        self.messages.copy(self.host, key)
    }

    fn when(&mut self, date: i64) -> Result<String, MailError> {
        match when_of(date, self.unix, self.offset) {
            When::Never => Ok(String::new()),
            When::Today { hour, minute } => self.clock(hour, minute),
            When::Day { month, day } => self.messages.format(
                self.host,
                "mail.date",
                &[
                    Arg::Int {
                        name: "day",
                        value: i64::from(day),
                    },
                    Arg::Int {
                        name: "month",
                        value: i64::from(month),
                    },
                ],
            ),
        }
    }
}
