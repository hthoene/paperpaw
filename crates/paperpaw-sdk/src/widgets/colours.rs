//! The colours of widgets (#206): what a widget sits on, its paper
//! (`<widget>.paper`), and the colour of its text, its ink
//! (`<widget>.ink`), each also for every widget whose own the reader did
//! not choose ([`WIDGET_PAPER`], [`WIDGET_INK`]). [`widget_look`]
//! gives a widget's colours as they say and [`draw_frame`] paints its
//! paper, so a widget drawing in `look.ink`, `look.faint` and on
//! `look.paper` follows them as it is.
//!
//! A paper is one of [`PAPERS`] (normal, inverted, light gray, dark
//! gray, transparent: what every panel shows), a tone of
//! [`decorations`](crate::decorations) (`core.tone.blue`), or a colour
//! code (`#3b6fd4`) for colour panels; panels without colour show a
//! colour by its lightness. The ink is automatic ([`INK_AUTO`]: white on
//! a dark paper, black on a light one), a tone or a colour code.
//! Colours are those of the page as drawn: night mode inverts them with
//! the rest of the screen.
//!
//! A screen showing widgets on a page of its own (a picture, a dark
//! page) says what lies under them with a [`Plate`]
//! ([`GridView::draw_on`](super::grid::GridView::draw_on)).
//!
//! [`WIDGET_PAPER`]: crate::builtin::settings::WIDGET_PAPER
//! [`WIDGET_INK`]: crate::builtin::settings::WIDGET_INK
//! [`widget_look`]: super::widget_look
//! [`draw_frame`]: super::draw_frame

use alloc::format;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use super::look::Frame;
use crate::builtin::settings::{WIDGET_INK, WIDGET_PAPER, parse_rgb};
use crate::decorations::{self, Tone};
use crate::env::Env;
use crate::id::Id;
use crate::settings::{SettingExtra, SettingSpec};
use crate::ui::{self, UiStyle};


/// The paper as the widget's frame makes it: white, black on a dark card.
pub const PAPER_NORMAL: &str = "core.plate.normal";
/// Black, with light text.
pub const PAPER_INVERTED: &str = "core.plate.inverted";
pub const PAPER_LIGHT_GRAY: &str = "core.plate.light-gray";
pub const PAPER_DARK_GRAY: &str = "core.plate.dark-gray";
/// No paper: the page shows (taken to be light for the automatic ink).
pub const PAPER_NONE: &str = "core.plate.none";



/// A colour a widget's paper or ink setting names ([`colour_spec`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColourSpec {
    /// As the frame makes it (paper), or what reads on the paper (ink).
    Auto,
    /// None: the page shows (paper only).
    None,
    /// This colour.
    Is(Color),
}

/// The colour `value` names: one of [`PAPERS`] or [`INK_AUTO`], a tone's
/// id ([`Tone::id`]), [`decorations::NONE`] or a colour code `#rrggbb`
/// (`#rgb`); automatic for any other.
pub fn colour_spec(value: &str) -> ColourSpec {
    match value {
        PAPER_INVERTED => ColourSpec::Is(Color::BLACK),
        PAPER_LIGHT_GRAY => ColourSpec::Is(Tone::LightGray.color()),
        PAPER_DARK_GRAY => ColourSpec::Is(Tone::DarkGray.color()),
        PAPER_NONE | decorations::NONE => ColourSpec::None,
        _ => {
            if let Some(tone) = Tone::from_id(value) {
                ColourSpec::Is(tone.color())
            } else if value.starts_with('#')
                && let Some([r, g, b]) = parse_rgb(value)
            {
                ColourSpec::Is(Color::rgb(r, g, b))
            } else {
                ColourSpec::Auto
            }
        }
    }
}














