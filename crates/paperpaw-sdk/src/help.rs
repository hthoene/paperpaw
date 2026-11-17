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
//! and says when it is ready ([`ready`]): screens below another get no
//! timers, so it would not get ready under them.

use alloc::format;
use alloc::string::{String, ToString};

use crate::builtin::events::ScreenShown;
use crate::event::Event;
use crate::{Id, Rect, RegisterError};

/// The point hints go to.
pub const POINT: &str = "help.hints";

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

    fn of(id: &str) -> Option<Input> {
        [Input::Any, Input::Touch, Input::Buttons]
            .into_iter()
            .find(|i| i.id() == id)
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

impl Zone {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    /// Where it is on `screen`.
    pub fn on(self, screen: Rect) -> Rect {
        let part =
            |value: u16, size: u16| (u32::from(value.min(1000)) * u32::from(size) / 1000) as u16;
        let x = part(self.x, screen.width);
        let y = part(self.y, screen.height);
        Rect::new(
            screen.x + x,
            screen.y + y,
            part(self.w, screen.width).min(screen.width - x),
            part(self.h, screen.height).min(screen.height - y),
        )
    }
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
    pub fn any(text: &str) -> Self {
        Self::of(text, Input::Any)
    }

    pub fn touch(text: &str) -> Self {
        Self::of(text, Input::Touch)
    }

    pub fn buttons(text: &str) -> Self {
        Self::of(text, Input::Buttons)
    }

    fn of(text: &str, input: Input) -> Self {
        Self {
            text: text.to_string(),
            input,
            zone: None,
        }
    }

    /// Pointing at `zone`.
    pub fn zone(mut self, zone: Zone) -> Self {
        self.zone = Some(zone);
        self
    }

    /// Its line for [`POINT`], for `screen`.
    pub fn line(&self, screen: &str) -> String {
        let zone = match self.zone {
            Some(z) => format!("{},{},{},{}", z.x, z.y, z.w, z.h),
            None => String::from("-"),
        };
        format!("{screen} {} {zone} {}", self.input.id(), self.text)
    }

    /// The hint of a line, and its screen.
    pub fn parse(line: &str) -> Option<(&str, HelpHint)> {
        let mut words = line.split_whitespace();
        let screen = words.next()?;
        let input = Input::of(words.next()?)?;
        let zone = match words.next()? {
            "-" => None,
            zone => {
                let mut n = zone.split(',').map(|n| n.parse::<u16>().ok());
                Some(Zone::new(n.next()??, n.next()??, n.next()??, n.next()??))
            }
        };
        let text = words.next()?.to_string();
        Some((screen, HelpHint { text, input, zone }))
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

/// Sent by a screen whose hints wait ([`wait`]) once it shows what it is
/// about ([`ready`]).
pub const READY: Event<ScreenShown> = Event::new("help.ready");

/// The hints of `screen` wait until it says it is ready ([`ready`]),
/// rather than show as it opens.
pub fn wait(
    contribute: &mut dyn FnMut(Id, String) -> Result<(), RegisterError>,
    screen: &str,
) -> Result<(), RegisterError> {
    contribute(
        Id::new_static(POINT),
        format!(
            "{screen} wait
"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_go_as_lines_and_come_back() {
        let hint = HelpHint::touch("reader.help.next").zone(Zone::new(667, 0, 333, 1000));
        let line = hint.line("reader.screen");
        assert_eq!(line, "reader.screen touch 667,0,333,1000 reader.help.next");
        assert_eq!(HelpHint::parse(&line), Some(("reader.screen", hint)));
        let plain = HelpHint::buttons("a.b");
        assert_eq!(HelpHint::parse(&plain.line("x.y")), Some(("x.y", plain)));
        assert_eq!(HelpHint::parse("x.y touch 1,2 a.b"), None);
        assert_eq!(HelpHint::parse("x.y sometimes - a.b"), None);
    }

    #[test]
    fn zones_lie_on_the_screen() {
        let screen = Rect::new(0, 0, 480, 800);
        assert_eq!(
            Zone::new(667, 0, 333, 1000).on(screen),
            Rect::new(320, 0, 159, 800)
        );
        assert_eq!(
            Zone::new(900, 900, 500, 500).on(screen),
            Rect::new(432, 720, 48, 80)
        );
    }
}
