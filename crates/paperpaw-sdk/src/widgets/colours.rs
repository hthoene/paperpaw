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

use super::look::{Frame, choice_setting};
use crate::builtin::settings::{WIDGET_INK, WIDGET_PAPER, parse_rgb};
use crate::decorations::{self, Tone};
use crate::env::Env;
use crate::id::Id;
use crate::settings::{SettingExtra, SettingSpec};
use crate::ui::{self, UiStyle};

/// What lies under the widgets of a grid
/// ([`GridView::draw_on`](super::grid::GridView::draw_on)); a widget's
/// own paper is painted by [`draw_frame`](super::draw_frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Plate {
    /// White, as on PaperPaw's own home ([`GridView::draw`](super::grid::GridView::draw)).
    Paper,
    /// Nothing: the page shows (a picture). The screen draws what lies
    /// under an item again before the item is drawn anew.
    None,
    /// A colour, e.g. that of a dark page.
    Colour(Color),
}

/// The paper as the widget's frame makes it: white, black on a dark card.
pub const PAPER_NORMAL: &str = "core.plate.normal";
/// Black, with light text.
pub const PAPER_INVERTED: &str = "core.plate.inverted";
pub const PAPER_LIGHT_GRAY: &str = "core.plate.light-gray";
pub const PAPER_DARK_GRAY: &str = "core.plate.dark-gray";
/// No paper: the page shows (taken to be light for the automatic ink).
pub const PAPER_NONE: &str = "core.plate.none";
/// The ink that reads on the paper: white on a dark one, black on a
/// light one.
pub const INK_AUTO: &str = "core.ink.auto";

/// The papers every panel shows, in the order settings offer them.
pub const PAPERS: &[&str] = &[
    PAPER_NORMAL,
    PAPER_INVERTED,
    PAPER_LIGHT_GRAY,
    PAPER_DARK_GRAY,
    PAPER_NONE,
];

/// The inks every panel shows, in the order settings offer them.
pub const INKS: &[&str] = &[INK_AUTO, Tone::Black.id(), Tone::White.id()];

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

/// `widget`'s paper, by its [`paper_setting`], else by
/// [`WIDGET_PAPER`](crate::builtin::settings::WIDGET_PAPER).
pub fn paper(env: &Env<'_>, widget: &str) -> ColourSpec {
    chosen(env, widget, "paper", WIDGET_PAPER.static_str())
}

/// `widget`'s ink, by its [`ink_setting`], else by
/// [`WIDGET_INK`](crate::builtin::settings::WIDGET_INK).
pub fn ink(env: &Env<'_>, widget: &str) -> ColourSpec {
    chosen(env, widget, "ink", WIDGET_INK.static_str())
}

/// The widget's own where the reader chose it, else that of all where
/// that is not automatic, as [`frame`](super::frame) is.
fn chosen(env: &Env<'_>, widget: &str, key: &str, all: &str) -> ColourSpec {
    let own = format!("{widget}.{key}");
    let settings = env.settings();
    if !settings.is_chosen(&own) {
        let every = colour_spec(settings.text(all));
        if every != ColourSpec::Auto {
            return every;
        }
    }
    colour_spec(settings.text(&own))
}

/// Keeps a tone or a colour code a colour picker set, which the choice's
/// own values lack.
static KEEPS_COLOURS: SettingExtra = SettingExtra::new().keeps_unknown();

/// The setting of what `widget` sits on, `<widget>.paper`, in the
/// widget's settings group; [`paper`] reads it.
pub fn paper_setting(widget: &Id) -> SettingSpec {
    choice_setting(widget, "paper", "core.setting.paper", PAPERS, PAPER_NORMAL)
        .with_extra(&KEEPS_COLOURS)
        .description("core.setting.paper.help")
}

/// [`paper_setting`] kept in the program (a `static` list): `id` must be
/// `<widget>.paper`.
pub const fn paper_spec(id: &'static str, widget: Id) -> SettingSpec {
    SettingSpec::choice(id, "core.setting.paper", PAPERS, PAPER_NORMAL, widget)
        .with_extra(&KEEPS_COLOURS)
        .description("core.setting.paper.help")
}

/// The setting of the colour of `widget`'s text, `<widget>.ink`, in the
/// widget's settings group; [`ink`] reads it.
pub fn ink_setting(widget: &Id) -> SettingSpec {
    choice_setting(widget, "ink", "core.setting.ink", INKS, INK_AUTO)
        .with_extra(&KEEPS_COLOURS)
        .description("core.setting.ink.help")
}

/// [`ink_setting`] kept in the program (a `static` list): `id` must be
/// `<widget>.ink`.
pub const fn ink_spec(id: &'static str, widget: Id) -> SettingSpec {
    SettingSpec::choice(id, "core.setting.ink", INKS, INK_AUTO, widget)
        .with_extra(&KEEPS_COLOURS)
        .description("core.setting.ink.help")
}

/// [`WIDGET_PAPER`](crate::builtin::settings::WIDGET_PAPER), the paper of
/// every widget whose own the reader did not choose, in `group` (the
/// kernel's).
pub const fn all_paper_spec(group: Id) -> SettingSpec {
    SettingSpec::choice(
        WIDGET_PAPER.static_str(),
        "core.setting.widget-paper",
        PAPERS,
        PAPER_NORMAL,
        group,
    )
    .with_extra(&KEEPS_COLOURS)
    .advanced()
    .description("core.setting.widget-paper.help")
}

/// [`WIDGET_INK`](crate::builtin::settings::WIDGET_INK), the ink of
/// every widget whose own the reader did not choose, in `group` (the
/// kernel's).
pub const fn all_ink_spec(group: Id) -> SettingSpec {
    SettingSpec::choice(
        WIDGET_INK.static_str(),
        "core.setting.widget-ink",
        INKS,
        INK_AUTO,
        group,
    )
    .with_extra(&KEEPS_COLOURS)
    .advanced()
    .description("core.setting.widget-ink.help")
}

/// Whether text on `paper` is better light than dark.
pub(crate) fn is_dark(paper: Color) -> bool {
    paper.luma() < 128
}

/// The ink and the faint ink that read on `paper`, as the style draws
/// text on light and on dark.
pub(crate) fn inks_on(style: &UiStyle, paper: Color) -> (Color, Color) {
    if is_dark(paper) {
        let faint = if style.strong {
            Color::WHITE
        } else {
            style.ink(ui::Ink::ON_DARK_MUTED)
        };
        (Color::WHITE, faint)
    } else {
        (Color::BLACK, style.ink(ui::Ink::MUTED))
    }
}

/// `widget`'s paper, ink and faint ink: as `frame` makes them, then as its
/// settings say.
pub(crate) fn colours(env: &Env<'_>, style: &UiStyle, widget: &str, frame: Frame) -> [Color; 3] {
    let paper = match (paper(env, widget), frame) {
        (ColourSpec::Is(colour), _) => colour,
        // The page shows, taken to be light.
        (ColourSpec::None, _) => Color::WHITE,
        (ColourSpec::Auto, Frame::Dark) => Color::BLACK,
        (ColourSpec::Auto, _) => Color::WHITE,
    };
    let (ink, faint) = match ink(env, widget) {
        ColourSpec::Is(colour) => (colour, colour),
        _ => inks_on(style, paper),
    };
    [paper, ink, faint]
}

/// Paints `widget`'s frame in `area` where its paper is set: the frame's
/// shape filled with the paper (none: a plain one), a line in the ink.
/// `false` where its paper is automatic, which the frame alone paints.
pub(crate) fn paint_paper(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    widget: &str,
    area: Rect,
    look: &super::WidgetLook,
) -> bool {
    let corner = match look.frame {
        Frame::None => 0,
        _ => style.corner,
    };
    match paper(env, widget) {
        ColourSpec::Auto => return false,
        ColourSpec::Is(colour) => canvas.fill_round_shaded(area, corner, colour),
        ColourSpec::None => {}
    }
    if look.frame == Frame::Line {
        let border = ui::button_border(style);
        canvas.stroke_round(area, border, style.corner, look.ink);
    }
    true
}
