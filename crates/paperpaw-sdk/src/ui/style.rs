//! The sizes and looks every part of the UI is drawn with ([`UiStyle`]),
//! the focus, separators and rounded fills.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// Sizes of the standard UI elements, in pixels: made for the device and
/// the user's settings with [`UiStyle::new`], or `UiStyle::PLAIN` where
/// there is no environment (tests, pictures made beforehand).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct UiStyle {
    /// Body text size.
    pub(crate) text: f32,
    /// Secondary text (status lines, hints).
    pub(crate) small: f32,
    /// Titles (e.g. of dialogs and widgets).
    pub(crate) title: f32,
    /// The titles of headers (core.ui-title), larger than [`title`] in
    /// some themes.
    ///
    /// [`title`]: UiStyle::title()
    pub(crate) head: f32,
    /// Height of a list row.
    pub(crate) row: u32,
    /// The row height controls are sized from (buttons, switches, ticks,
    /// pickers): [`UiStyle::row()`], except where rows grow to hold a label
    /// on two lines at the largest sizes while controls keep their size.
    pub(crate) control_row: u32,
    /// Height of a header bar.
    pub(crate) header: u32,
    /// Horizontal margin of content.
    pub(crate) margin: u32,
    /// Standard gap between elements.
    pub(crate) gap: u32,
    /// Width of lines and borders.
    pub(crate) line: u32,
    /// Headers on a dark bar, else on a light one with a line below.
    pub(crate) dark_header: bool,
    /// Radius of the corners of highlights, buttons and boxes (0: square).
    pub(crate) corner: u32,
    /// Gray text is drawn black (see [`UiStyle::faint`]).
    pub(crate) strong: bool,
    /// Buttons, switches, the focus and other small shapes as pills,
    /// their ends round (see [`UiStyle::radius`]), with round or soft
    /// corners; else with [`corner`].
    ///
    /// [`corner`]: UiStyle::corner()
    pub(crate) pill: bool,
    /// How the focus of rows and items is drawn.
    pub(crate) focus: Focus,
    /// The lines between rows, below light headers and above notes.
    pub(crate) separators: Separators,
    /// Headers without a bar or a line: the title on the page itself.
    pub(crate) plain_header: bool,
    /// The entries of a list's sections on rounded cards.
    pub(crate) cards: bool,
    /// The grays the panel shows (2 for black and white alone).
    pub(crate) grays: u16,
    /// The least size of what a finger sets or presses, about 7 mm, on
    /// touch screens; 0 with buttons alone (see [`UiStyle::touch_row`]).
    pub(crate) finger: u32,
    /// The look a plugin gave the parts (see [`look`](super::look)).
    pub(crate) look: super::look::Look,
    /// The grays of the interface (see [`UiStyle::ink`]).
    pub(crate) palette: Palette,
    /// 1 where the interface runs right to left (see
    /// [`UiStyle::is_rtl`]), else 0.
    pub(crate) direction: u8,
}

/// What the style is: its sizes in pixels and its looks. A style is made
/// with [`UiStyle::new`]; the `with_` methods change one part of it, e.g.
/// for a screen of its own.
impl UiStyle {
    /// Body text size.
    pub fn text(&self) -> f32 {
        self.text
    }

    /// Secondary text (status lines, hints).
    pub fn small(&self) -> f32 {
        self.small
    }

    /// Titles (e.g. of dialogs and widgets).
    pub fn title(&self) -> f32 {
        self.title
    }

    /// Horizontal margin of content.
    pub fn margin(&self) -> u16 {
        px(self.margin)
    }

    /// Standard gap between elements.
    pub fn gap(&self) -> u16 {
        px(self.gap)
    }

    /// Width of lines and borders.
    pub fn line(&self) -> u16 {
        px(self.line)
    }

    /// The look a plugin gave the parts (see [`look`](super::look)).
    pub fn look(&self) -> super::look::Look {
        self.look
    }

}

/// A size in pixels as the getters give it.
fn px(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// What a colour is for ([`UiStyle::ink`]): a colour role of the
/// interface, each set by the palette (setting `core.ui-palette`), so a
/// theme changes them all at once and plugins draw in the same colours.
///
/// An `Ink` is a colour role; how text is inked (its tone) is a
/// [`TextInk`]. The roles are constants of one type, so more can come
/// without a new plugin interface: a role this release does not know is
/// drawn in the colour of its [`fallback`](Ink::fallback), the role a
/// theme made for older releases already sets. Roles 0 to 13 have a
/// default of their own, 14 to 18 are named, 19 to 31 are free (the
/// palette has a slot for each), and what is past them still reads a
/// fallback.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ink(u8);

/// The names of the named [`Ink`]s in the palette, by their numbers.
const INKS: [&str; 19] = [
    "text",
    "paper",
    "note",
    "detail",
    "muted",
    "faint",
    "disabled",
    "rule",
    "line",
    "line-faint",
    "on-focus-muted",
    "on-dark-muted",
    "card",
    "section",
    "accent",
    "on-accent",
    "link",
    "selection",
    "warning",
];

impl Ink {
    /// The role of `value`; one this release has no name for is kept, and
    /// reads its [`fallback`](Ink::fallback).
    pub const fn from_value(value: u8) -> Ink {
        Ink(value)
    }

    /// Text and what is drawn in its colour.
    pub const TEXT: Ink = Ink(0);
    /// What the interface is drawn on.
    pub const PAPER: Ink = Ink(1);
    /// Notes below the titles of rows, what an empty screen explains.
    pub const NOTE: Ink = Ink(2);
    /// What is told beside or below a row's title: details, the notes of
    /// lists, hints on a page.
    pub const DETAIL: Ink = Ink(3);
    /// Headings of sections, a widget's lesser text.
    pub const MUTED: Ink = Ink(4);
    /// The least of text, e.g. what an empty screen says.
    pub const FAINT: Ink = Ink(5);
    /// What cannot be used now, e.g. an arrow at the last page.
    pub const DISABLED: Ink = Ink(6);
    /// The line below a light header or above the row of button hints.
    pub const RULE: Ink = Ink(7);
    /// The lines between rows.
    pub const LINE: Ink = Ink(8);
    /// The fainter lines between rows that start at the text.
    pub const LINE_FAINT: Ink = Ink(9);
    /// Gray text on a dark focus.
    pub const ON_FOCUS_MUTED: Ink = Ink(10);
    /// Lesser text on a dark card.
    pub const ON_DARK_MUTED: Ink = Ink(11);
    /// Cards, e.g. the frames of widgets (on panels of black and white
    /// alone, a sparser pattern of it).
    pub const CARD: Ink = Ink(12);
    /// The cards a list's sections are on.
    pub const SECTION: Ink = Ink(13);
    /// What stands out on a colour panel, e.g. a selected tab (text where
    /// a theme sets none).
    pub const ACCENT: Ink = Ink(14);
    /// What is drawn on an [`ACCENT`](Ink::ACCENT) (paper where a theme
    /// sets none).
    pub const ON_ACCENT: Ink = Ink(15);
    /// A link in a text (text where a theme sets none).
    pub const LINK: Ink = Ink(16);
    /// What a selection is marked with (a card's colour where a theme
    /// sets none).
    pub const SELECTION: Ink = Ink(17);
    /// A warning or an error (text where a theme sets none).
    pub const WARNING: Ink = Ink(18);

    /// Every named role, in the order of their numbers.
    pub const ALL: &'static [Ink] = &[
        Ink::TEXT,
        Ink::PAPER,
        Ink::NOTE,
        Ink::DETAIL,
        Ink::MUTED,
        Ink::FAINT,
        Ink::DISABLED,
        Ink::RULE,
        Ink::LINE,
        Ink::LINE_FAINT,
        Ink::ON_FOCUS_MUTED,
        Ink::ON_DARK_MUTED,
        Ink::CARD,
        Ink::SECTION,
        Ink::ACCENT,
        Ink::ON_ACCENT,
        Ink::LINK,
        Ink::SELECTION,
        Ink::WARNING,
    ];

    /// The roles that have a default colour of their own; the ones after
    /// them read their [`fallback`](Ink::fallback) until a theme sets them.
    const OWN: u8 = 14;

    /// The number the role goes by in data.
    pub const fn value(self) -> u8 {
        self.0
    }

    /// Its name in the palette, e.g. `muted` (empty for a role without
    /// one).
    pub const fn name(self) -> &'static str {
        if (self.0 as usize) < INKS.len() {
            INKS[self.0 as usize]
        } else {
            ""
        }
    }

    /// The role whose colour this one has where a theme does not set it:
    /// one of the first 14 for every role, itself for those; text for a
    /// role this release does not know.
    pub const fn fallback(self) -> Ink {
        match self.0 {
            // The roles with a default of their own.
            0..=13 => self,
            15 => Ink::PAPER,
            17 => Ink::CARD,
            // ACCENT, LINK, WARNING and the roles of later releases.
            _ => Ink::TEXT,
        }
    }

    /// Text, which strong contrast draws black (a role this release does
    /// not know is text where its fallback is).
    const fn is_text(self) -> bool {
        // (Every fallback past the first 14 is text, paper or a card.)
        matches!(self.0, 2..=5)
    }
}

impl core::fmt::Debug for Ink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write as _;
        match INKS.get(usize::from(self.0)) {
            Some(name) => {
                f.write_str("Ink::")?;
                for c in name.chars() {
                    f.write_char(if c == '-' {
                        '_'
                    } else {
                        c.to_ascii_uppercase()
                    })?;
                }
                Ok(())
            }
            None => write!(f, "Ink({})", self.0),
        }
    }
}

/// The colour of each [`Ink`], read from the setting `core.ui-palette`:
/// names and colours separated by spaces, e.g. `muted=60 line=170
/// link=#2a6f97`; a colour is a gray (0 black, 255 white) or `#rrggbb`
/// (a gray panel uses its brightness). What it leaves out reads the
/// default of the role, or the colour of its
/// [`fallback`](Ink::fallback), which follows the theme; a name given
/// twice takes the last; a name or a colour that is not understood
/// (older releases do not read `#rrggbb`) is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    colors: [Color; Palette::SLOTS],
    /// Bit `n` is set where the theme set the role with number `n`.
    set: u32,
}

impl Palette {
    /// The number of roles the palette holds a colour for.
    pub(crate) const SLOTS: usize = 32;

    /// The interface's own grays.
    pub(crate) const DEFAULT: Palette = {
        const GRAYS: [u8; Ink::OWN as usize] = [
            0, 255, 60, 80, 90, 120, 150, 150, 190, 210, 215, 200, 225, 238,
        ];
        let mut colors = [Color::BLACK; Palette::SLOTS];
        let mut at = 0;
        while at < GRAYS.len() {
            colors[at] = Color::gray(GRAYS[at]);
            at += 1;
        }
        Palette { colors, set: 0 }
    };

    /// The default with what `text` sets; names it does not know and bad
    /// values are left out.
    pub fn parse(text: &str) -> Palette {
        let mut palette = Palette::DEFAULT;
        for pair in text.split(|c: char| c.is_ascii_whitespace()) {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let (Some(at), Some(color)) = (
                INKS.iter().position(|known| *known == name),
                parse_color(value),
            ) else {
                continue;
            };
            palette.colors[at] = color;
            palette.set |= 1 << at;
        }
        palette
    }

    /// The colour of `ink`: the theme's where it set it, else the default
    /// of the first 14 roles, else the colour of its fallback.
    pub fn color(&self, ink: Ink) -> Color {
        let at = usize::from(ink.value());
        let own = at < usize::from(Ink::OWN) || (at < Self::SLOTS && self.set >> at & 1 != 0);
        // Every fallback is one of the first 14, which have their own.
        let at = if own {
            at
        } else {
            usize::from(ink.fallback().value())
        };
        self.colors.get(at).copied().unwrap_or(Color::BLACK)
    }
}

/// A gray (`60`) or a colour (`#2a6f97`).
fn parse_color(value: &str) -> Option<Color> {
    let digit = |b: u8, base: u8| -> Option<u8> {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        (d < base).then_some(d)
    };
    let bytes = value.as_bytes();
    if let [b'#', hex @ ..] = bytes {
        let [r1, r2, g1, g2, b1, b2] = *hex else {
            return None;
        };
        let pair = |hi: u8, lo: u8| Some(digit(hi, 16)? << 4 | digit(lo, 16)?);
        return Some(Color::rgb(pair(r1, r2)?, pair(g1, g2)?, pair(b1, b2)?));
    }
    // 1 to 3 digits, 0 to 255.
    if bytes.is_empty() || bytes.len() > 3 {
        return None;
    }
    let mut gray = 0u16;
    for b in bytes {
        gray = gray * 10 + u16::from(digit(*b, 10)?);
    }
    u8::try_from(gray).ok().map(Color::gray)
}

impl Default for Palette {
    fn default() -> Self {
        Self::DEFAULT
    }
}


/// What a text is (see [`UiStyle::size`]): its size follows from the
/// style's, so text of one kind is as large on every screen.
///
/// The roles are constants of one type, so more can come without a new
/// plugin interface: a role this release does not know is sized as
/// `TextRole::BODY`, and a look's text painter
/// (`Painters::text`) gets the role of
/// every run it is asked about.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRole(u8);

impl TextRole {
    /// Smaller than small, e.g. under an icon.
    pub(crate) const CAPTION: Self = Self(0);
    /// Hints, notes, status lines.
    pub(crate) const SMALL: Self = Self(1);
    /// Most text.
    pub(crate) const BODY: Self = Self(2);
    /// Titles of dialogs and widgets.
    pub(crate) const TITLE: Self = Self(3);
    /// The titles of headers.
    pub(crate) const HEAD: Self = Self(4);
    /// Large, e.g. a widget's main line.
    pub(crate) const HEADLINE: Self = Self(5);
    /// Larger, e.g. a clock.
    pub(crate) const DISPLAY: Self = Self(6);
    /// Large numbers standing alone, e.g. a page count.
    pub(crate) const NUMERAL: Self = Self(7);
    /// A hint under a control (sized as [`TextRole::SMALL`]).
    pub(crate) const HINT: Self = Self(8);

    const fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            0 => "CAPTION",
            1 => "SMALL",
            2 => "BODY",
            3 => "TITLE",
            4 => "HEAD",
            5 => "HEADLINE",
            6 => "DISPLAY",
            7 => "NUMERAL",
            8 => "HINT",
            9 => "BUTTON",
            10 => "ICON",
            _ => return None,
        })
    }
}

impl core::fmt::Debug for TextRole {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.name() {
            Some(name) => write!(f, "TextRole::{name}"),
            None => write!(f, "TextRole({})", self.0),
        }
    }
}

/// How the focus of rows and items is drawn ([`Ui::focus`](super::Ui::focus)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum Focus {
    /// A black bar, what is on it white.
    Fill,
    /// A thick round frame, what is in it black.
    Ring,
    /// A light gray bar, what is on it black; a frame on panels of black
    /// and white alone.
    Soft,
}

/// The lines between rows ([`Ui::separator`](super::Ui::separator)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum Separators {
    /// Light lines within the margins.
    Line,
    /// Fainter lines, from where the text starts.
    Inset,
    /// None: room alone parts the rows.
    None,
}

impl UiStyle {


    /// The colour of `ink` in the palette; gray text (notes, headings,
    /// what cannot be used) black for strong contrast.
    pub fn ink(&self, ink: Ink) -> Color {
        if self.strong && ink.is_text() {
            Color::BLACK
        } else {
            self.palette.color(ink)
        }
    }

    /// The size of text of `role` (one this release does not know is
    /// sized as `TextRole::BODY`).
    pub fn size(&self, role: TextRole) -> f32 {
        match role {
            TextRole::CAPTION => self.small * 0.8,
            TextRole::SMALL => self.small,
            TextRole::BODY => self.text,
            TextRole::TITLE => self.title,
            TextRole::HEAD => self.head,
            TextRole::HEADLINE => self.title * 1.6,
            TextRole::DISPLAY => self.title * 2.0,
            TextRole::NUMERAL => self.title * 3.0,
            TextRole::HINT => self.small,
            // BUTTON, ICON and the roles of later releases.
            _ => self.text,
        }
    }

}


