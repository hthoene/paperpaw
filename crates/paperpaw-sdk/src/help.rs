//! First-time help: what a screen's controls do, shown over it the first
//! time it opens (by the `help` plugin), calmly, once.
//!
//! A plugin names, for a screen of its own (or another's), a few hints:
//! a text each (a translation key), for touch screens, for devices with
//! buttons alone, or for both, and where it points, if anywhere: a zone of
//! the screen (in thousandths of its width and height, e.g. the reader's
//! tap zones), drawn as a frame with the text in it; hints without a zone
//! are listed in a box in the middle.
//!
//! ```
//! use paperpaw_sdk::help::{self, HelpHint, Zone};
//! # use paperpaw_sdk::{Id, RegisterError};
//! # fn register(
//! #     mut contribute: impl FnMut(Id, String) -> Result<(), RegisterError>,
//! # ) -> Result<(), RegisterError> {
//! help::add(&mut contribute, "my.screen", &[
//!     HelpHint::touch("my.help.next").zone(Zone::new(667, 0, 333, 1000)),
//!     HelpHint::buttons("my.help.select"),
//! ])?;
//! # Ok(())
//! # }
//! ```
//!
//! The hints go to the point [`POINT`] as text, a line each:
//! `<screen> <input> <zone> <text key>`, `input` one of `any`, `touch`,
//! `buttons`, `zone` `x,y,w,h` or `-` (so data plugins and native ones can
//! add them alike). Readers switch them off (`help.show`) or have them
//! shown again (*Show hints again*, in the system settings).
//!
//! They show as the screen opens. A screen that first prepares what it
//! shows (the reader, opening a book) has them wait instead ([`wait`]),
//! and says when it is ready (`ready`): screens below another get no
//! timers, so it would not get ready under them.

use alloc::format;
use alloc::string::String;

use crate::{Id, RegisterError};

/// The point hints go to.
pub(crate) const POINT: &str = "help.hints";

/// What a hint is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Input {
    /// Every device.
    Any,
    /// Touch screens.
    Touch,
    /// Devices with buttons alone.
    Buttons,
}

impl Input {
    fn id(self) -> &'static str {
        match self {
            Input::Any => "any",
            Input::Touch => "touch",
            Input::Buttons => "buttons",
        }
    }
}

/// A part of the screen, in thousandths of its width and height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Zone {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// A hint: its text (a translation key), for which devices, where.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HelpHint {
    pub text: String,
    pub input: Input,
    pub zone: Option<Zone>,
}

impl HelpHint {
    /// Its line for [`POINT`], for `screen`.
    pub fn line(&self, screen: &str) -> String {
        let zone = match self.zone {
            Some(z) => format!("{},{},{},{}", z.x, z.y, z.w, z.h),
            None => String::from("-"),
        };
        format!("{screen} {} {zone} {}", self.input.id(), self.text)
    }
}

/// Adds `hints` for `screen` through `contribute` (the kernel's
/// `Items::contribute`).
pub fn add(
    contribute: &mut dyn FnMut(Id, String) -> Result<(), RegisterError>,
    screen: &str,
    hints: &[HelpHint],
) -> Result<(), RegisterError> {
    let mut text = String::new();
    for hint in hints {
        text.push_str(&hint.line(screen));
        text.push('\n');
    }
    contribute(Id::new_static(POINT), text)
}
