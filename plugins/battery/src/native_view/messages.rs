//! Catalog and number formatting with reusable, fallibly allocated buffers.

use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    formatting::{self, Arg, FormatBuffers},
};

pub(super) struct Messages {
    out: Vec<u8>,
    arguments: Vec<u8>,
    decimal: String,
}

fn buffer(size: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    bytes.resize(size, 0);
    Ok(bytes)
}

impl Messages {
    pub(super) fn new(host: &Host<'_>) -> Result<Self, Error> {
        let mut result = Self {
            out: buffer(formatting::MAX_MESSAGE_BYTES)?,
            arguments: buffer(formatting::MAX_ARGUMENT_BYTES)?,
            decimal: String::new(),
        };
        result.decimal = match result.copy(host, paperpaw_sdk::numbers::DECIMAL_MARK) {
            Ok(mark) => mark,
            Err(Error::NotFound) => String::from("."),
            Err(error) => return Err(error),
        };
        Ok(result)
    }

    pub(super) fn copy(&mut self, host: &Host<'_>, key: &str) -> Result<String, Error> {
        Ok(String::from(formatting::copy_message(
            host,
            key,
            &mut self.out,
        )?))
    }

    pub(super) fn format(
        &mut self,
        host: &Host<'_>,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
        let mut buffers = FormatBuffers::new(&mut self.arguments, &mut self.out);
        let needed = formatting::format_message(host, key, args, &mut buffers)?;
        self.complete(needed)
    }

    fn complete(&self, length: usize) -> Result<String, Error> {
        let bytes = self.out.get(..length).ok_or(Error::NoSpace)?;
        let text = core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?;
        Ok(String::from(text))
    }

    pub(super) fn digits(&mut self, host: &Host<'_>, text: &str) -> Result<String, Error> {
        let needed = formatting::digits(host, text, &mut self.out)?;
        self.complete(needed)
    }

    pub(super) fn integer(&mut self, host: &Host<'_>, value: i64) -> Result<String, Error> {
        self.digits(host, &format!("{value}"))
    }

    pub(super) fn duration(&mut self, host: &Host<'_>, ms: u64) -> Result<String, Error> {
        let minutes = ms / 60_000;
        let (h, m) = ((minutes / 60) as i64, (minutes % 60) as i64);
        if minutes == 0 {
            self.copy(host, "battery.under-a-minute")
        } else if h == 0 {
            self.format(
                host,
                "battery.minutes",
                &[Arg::Int {
                    name: "m",
                    value: m,
                }],
            )
        } else {
            self.format(
                host,
                "battery.hours",
                &[
                    Arg::Int {
                        name: "h",
                        value: h,
                    },
                    Arg::Int {
                        name: "m",
                        value: m,
                    },
                ],
            )
        }
    }

    pub(super) fn percent(&mut self, host: &Host<'_>, value: f32) -> Result<String, Error> {
        let tenths = (value * 10.0 + 0.5) as u32;
        let number = self.digits(
            host,
            &format!("{}{}{}", tenths / 10, self.decimal, tenths % 10),
        )?;
        self.format(
            host,
            "battery.percent",
            &[Arg::Text {
                name: "value",
                value: &number,
            }],
        )
    }

    pub(super) fn volts(&mut self, host: &Host<'_>, millivolts: u16) -> Result<String, Error> {
        let hundredths = (u32::from(millivolts) + 5) / 10;
        self.digits(
            host,
            &format!(
                "{}{}{:02} V",
                hundredths / 100,
                self.decimal,
                hundredths % 100
            ),
        )
    }

    pub(super) fn seconds(&mut self, host: &Host<'_>, micros: u64) -> Result<String, Error> {
        let tenths = micros / 100_000;
        self.digits(
            host,
            &format!("{}{}{} s", tenths / 10, self.decimal, tenths % 10),
        )
    }
}
