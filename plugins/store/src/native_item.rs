//! The reading of a plugin of the library's JSON into an [`Entry`] (slice 2,
//! host-free): the keys of a plugin and of its `latest` version that make the
//! index line and say whether this device lists it. The interface-6 rules,
//! made fallible: a refused allocation is `NoMemory`, and every text of the
//! library (nobody reviewed it, community plugins too) is **cut** at the
//! limit of its field, at a character's end, never dropped with its plugin;
//! only a line of the index is refused at a cap (see [`Entry::parse`]).
//! Nothing indexes text in a way that can panic.
//!
//! What a build, the files, the key rotations and the screenshots add (the
//! keys below `builds`, `files`, `rotations`, `screenshots`) and a plugin's
//! page stay with the fetch (`PORTING-7.md`).
use crate::native_model::{Entry, MAX_NAME, MAX_SUMMARY, group};
use alloc::string::String;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::copy;

/// `text` cut to at most `max` bytes, at a character's end.
///
/// # Errors
/// `NoMemory` when the cut text cannot be kept.
pub fn cut(text: &str, max: usize) -> Result<String, Error> {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    copy(text.get(..end).unwrap_or_default())
}

/// A date of the library (`2026-03-14`, or a year) as `20260314` (a year as
/// its January 1st); 0 for any other.
#[must_use]
pub fn date(text: &str) -> u32 {
    let (mut n, mut value) = (0_u32, 0_u32);
    for digit in text.chars().filter_map(|c| c.to_digit(10)).take(8) {
        n += 1;
        value = value * 10 + digit;
    }
    match n {
        8 => value,
        4 => value * 10_000 + 101,
        _ => 0,
    }
}

/// What is known of the plugin being read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub entry: Entry,
    /// `data` or `code` (anything else is not offered).
    pub kind: String,
    pub built_in: bool,
    /// The library says it is PaperPaw's own.
    pub official: bool,
    /// `devices`, where the library names them: any, and this one.
    pub devices: bool,
    pub device: bool,
    /// The format of the plugin's data the library says it is in.
    pub formats: u32,
    /// What the library says it waits for: `author` or `update`.
    pub waits: String,
}

impl Item {
    /// A plugin of which nothing is read yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            formats: 1,
            ..Self::default()
        }
    }

    /// Takes what `key` (the path below the plugin) says; `device` is the
    /// name of the device that asks.
    ///
    /// # Errors
    /// `NoMemory` when a text cannot be kept; what was read stays.
    pub fn read<S: AsRef<str>>(
        &mut self,
        key: &[S],
        value: &str,
        device: &str,
    ) -> Result<(), Error> {
        let entry = &mut self.entry;
        match key {
            [key] => match key.as_ref() {
                "id" => entry.id = cut(value, MAX_NAME)?,
                "name" => entry.name = cut(value, MAX_NAME)?,
                "summary" => entry.summary = cut(value, MAX_SUMMARY)?,
                "author" => entry.author = cut(value, MAX_NAME)?,
                "licence" => entry.licence = cut(value, MAX_NAME)?,
                "group" => entry.group = group(value),
                "kind" => self.kind = cut(value, MAX_NAME)?,
                "waits" => self.waits = cut(value, MAX_NAME)?,
                "built_in" => self.built_in = value == "true",
                "official" => self.official = value == "true",
                "unreviewed" => entry.unreviewed = value == "true",
                "added" => entry.added = date(value),
                _ => {}
            },
            [devices, at] if devices.as_ref() == "devices" && at.as_ref() == "[]" => {
                self.devices = true;
                self.device |= value == device;
            }
            [latest, key] if latest.as_ref() == "latest" => match key.as_ref() {
                "version" => entry.version = cut(value, MAX_NAME)?,
                "sdk" => entry.sdk = cut(value, MAX_NAME)?,
                "formats" => self.formats = value.parse().unwrap_or(u32::MAX),
                "size" => entry.size = value.parse().unwrap_or(0),
                "date" => entry.updated = date(value),
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }
}
