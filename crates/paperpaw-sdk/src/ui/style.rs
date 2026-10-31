//! The sizes and looks every part of the UI is drawn with ([`UiStyle`]),
//! the focus, separators and rounded fills.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// Sizes of the standard UI elements, in pixels: made for the device and
/// the user's settings with [`UiStyle::new`], or [`UiStyle::PLAIN`] where
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

/// How a header is drawn ([`UiStyle::header_kind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeaderKind {
    /// On a light bar with a line below.
    Light,
    /// On a dark bar.
    Dark,
    /// Without a bar or a line: the title on the page itself.
    Plain,
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

    /// The titles of headers, larger than [`title`](Self::title) in some
    /// themes.
    pub fn head(&self) -> f32 {
        self.head
    }

    /// Height of a list row.
    pub fn row(&self) -> u16 {
        px(self.row)
    }

    /// The row height controls are sized from (buttons, switches, ticks,
    /// pickers): [`row`](Self::row), except where rows grow to hold a
    /// label on two lines at the largest sizes while controls keep their
    /// size.
    pub fn control_row(&self) -> u16 {
        px(self.control_row)
    }

    /// Height of a header bar.
    pub fn header(&self) -> u16 {
        px(self.header)
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

    /// Radius of the corners of highlights, buttons and boxes (0: square).
    pub fn corner(&self) -> u16 {
        px(self.corner)
    }

    /// The least size of what a finger sets or presses, about 7 mm, on
    /// touch screens; 0 with buttons alone (see
    /// [`touch_row`](Self::touch_row)).
    pub fn finger(&self) -> u16 {
        px(self.finger)
    }

    /// Gray text is drawn black (see [`faint`](Self::faint)).
    pub fn strong(&self) -> bool {
        self.strong
    }

    /// Buttons, switches, the focus and other small shapes as pills, their
    /// ends round (see [`radius`](Self::radius)); else with
    /// [`corner`](Self::corner).
    pub fn pill(&self) -> bool {
        self.pill
    }

    /// How the focus of rows and items is drawn.
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The lines between rows, below light headers and above notes.
    pub fn separators(&self) -> Separators {
        self.separators
    }

    /// The entries of a list's sections on rounded cards.
    pub fn cards(&self) -> bool {
        self.cards
    }

    /// How headers are drawn.
    pub fn header_kind(&self) -> HeaderKind {
        if self.plain_header {
            HeaderKind::Plain
        } else if self.dark_header {
            HeaderKind::Dark
        } else {
            HeaderKind::Light
        }
    }

    /// The interface runs right to left (its language's catalog says so,
    /// see [`Env::direction`]): headers, rows, switches and sliders are
    /// mirrored, text drawn with this as its base direction.
    pub fn is_rtl(&self) -> bool {
        self.direction == 1
    }

    /// The same, right to left or not (see [`UiStyle::is_rtl`]).
    pub fn with_rtl(mut self, rtl: bool) -> Self {
        self.direction = u8::from(rtl);
        self
    }

    /// The panel shows black and white alone (no grays between).
    pub fn black_and_white(&self) -> bool {
        self.grays <= 2
    }

    /// The panel shows grays smoothly (16 or more), not a few steps.
    pub fn smooth_grays(&self) -> bool {
        self.grays >= 16
    }

    /// The look a plugin gave the parts (see [`look`](super::look)).
    pub fn look(&self) -> super::look::Look {
        self.look
    }

    /// The grays of the interface (see [`ink`](Self::ink)).
    pub fn palette(&self) -> Palette {
        self.palette
    }

    /// With body text `text` large.
    pub fn with_text(mut self, text: f32) -> Self {
        self.text = text;
        self
    }

    /// With the parts in `look`.
    pub fn with_look(mut self, look: super::look::Look) -> Self {
        self.look = look;
        self
    }

    /// With rows `row` high.
    pub fn with_row(mut self, row: u16) -> Self {
        self.row = u32::from(row);
        self
    }

    /// With a margin of `margin`.
    pub fn with_margin(mut self, margin: u16) -> Self {
        self.margin = u32::from(margin);
        self
    }

    /// With corners of radius `corner`.
    pub fn with_corner(mut self, corner: u16) -> Self {
        self.corner = u32::from(corner);
        self
    }

    /// With small shapes as pills, or not.
    pub fn with_pill(mut self, pill: bool) -> Self {
        self.pill = pill;
        self
    }

    /// With gray text drawn black, or not.
    pub fn with_strong(mut self, strong: bool) -> Self {
        self.strong = strong;
        self
    }

    /// With `separators` between rows.
    pub fn with_separators(mut self, separators: Separators) -> Self {
        self.separators = separators;
        self
    }

    /// With the grays of `palette`.
    pub fn with_palette(mut self, palette: Palette) -> Self {
        self.palette = palette;
        self
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

/// What is known of each named [`Ink`], by its number: the name in the
/// palette, the constant's name, and the role it reads where a theme
/// sets none.
const INKS: [(&str, &str, Ink); 19] = [
    ("text", "TEXT", Ink::TEXT),
    ("paper", "PAPER", Ink::PAPER),
    ("note", "NOTE", Ink::NOTE),
    ("detail", "DETAIL", Ink::DETAIL),
    ("muted", "MUTED", Ink::MUTED),
    ("faint", "FAINT", Ink::FAINT),
    ("disabled", "DISABLED", Ink::DISABLED),
    ("rule", "RULE", Ink::RULE),
    ("line", "LINE", Ink::LINE),
    ("line-faint", "LINE_FAINT", Ink::LINE_FAINT),
    ("on-focus-muted", "ON_FOCUS_MUTED", Ink::ON_FOCUS_MUTED),
    ("on-dark-muted", "ON_DARK_MUTED", Ink::ON_DARK_MUTED),
    ("card", "CARD", Ink::CARD),
    ("section", "SECTION", Ink::SECTION),
    ("accent", "ACCENT", Ink::TEXT),
    ("on-accent", "ON_ACCENT", Ink::PAPER),
    ("link", "LINK", Ink::TEXT),
    ("selection", "SELECTION", Ink::CARD),
    ("warning", "WARNING", Ink::TEXT),
];

impl Ink {
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

    /// The role of `value`; one this release has no name for is kept, and
    /// reads its [`fallback`](Ink::fallback).
    pub const fn from_value(value: u8) -> Ink {
        Ink(value)
    }

    /// Its name in the palette, e.g. `muted` (empty for a role without
    /// one).
    pub const fn name(self) -> &'static str {
        if (self.0 as usize) < INKS.len() {
            INKS[self.0 as usize].0
        } else {
            ""
        }
    }

    /// The role whose colour this one has where a theme does not set it:
    /// one of the first 14 for every role, itself for those; text for a
    /// role this release does not know.
    pub const fn fallback(self) -> Ink {
        if self.0 < Ink::OWN {
            self
        } else if (self.0 as usize) < INKS.len() {
            INKS[self.0 as usize].2
        } else {
            Ink::TEXT
        }
    }

    /// Text, which strong contrast draws black (a role this release does
    /// not know is text where its fallback is).
    const fn is_text(self) -> bool {
        matches!(
            self.fallback(),
            Ink::NOTE | Ink::DETAIL | Ink::MUTED | Ink::FAINT
        )
    }
}

impl core::fmt::Debug for Ink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match INKS.get(usize::from(self.0)) {
            Some((_, ident, _)) => write!(f, "Ink::{ident}"),
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
pub struct Palette {
    colors: [Color; Palette::SLOTS],
    /// Bit `n` is set where the theme set the role with number `n`.
    set: u32,
}

impl Palette {
    /// The number of roles the palette holds a colour for.
    pub const SLOTS: usize = 32;

    /// The interface's own grays.
    pub const DEFAULT: Palette = {
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
        for pair in text.split_whitespace() {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let (Some(at), Some(color)) = (
                INKS.iter().position(|(known, ..)| *known == name),
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
        if at < Self::SLOTS && self.set & (1 << at) != 0 || ink.value() < Ink::OWN {
            return self.colors[at];
        }
        // Every fallback is one of the first 14, which have their own.
        self.colors[usize::from(ink.fallback().value())]
    }
}

/// A gray (`60`) or a colour (`#2a6f97`).
fn parse_color(value: &str) -> Option<Color> {
    let Some(hex) = value.strip_prefix('#') else {
        return value.parse::<u8>().ok().map(Color::gray);
    };
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let part = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some(Color::rgb(part(0)?, part(2)?, part(4)?))
}

impl Default for Palette {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How the interface's text is inked: the tone curve of the setting
/// `core.ui-text-curve` (see
/// [`ToneCurve::parse_for`](paperpaw_text::ToneCurve::parse_for)), for the
/// script of the interface's language; the stock ink where it is empty or
/// says nothing usable.
pub fn text_ink(env: &Env<'_>) -> TextInk {
    let value = env.settings().text(settings::UI_TEXT_CURVE.as_str());
    if value.is_empty() {
        return TextInk::DEFAULT;
    }
    ToneCurve::parse_for(value, env.lang().likely_script()).map_or(TextInk::DEFAULT, TextInk::new)
}

/// What a text is (see [`UiStyle::size`]): its size follows from the
/// style's, so text of one kind is as large on every screen.
///
/// The roles are constants of one type, so more can come without a new
/// plugin interface: a role this release does not know is sized as
/// [`TextRole::BODY`], and a look's text painter
/// ([`Painters::text`](crate::ui::look::Painters::text)) gets the role of
/// every run it is asked about.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRole(u8);

impl TextRole {
    /// Smaller than small, e.g. under an icon.
    pub const CAPTION: Self = Self(0);
    /// Hints, notes, status lines.
    pub const SMALL: Self = Self(1);
    /// Most text.
    pub const BODY: Self = Self(2);
    /// Titles of dialogs and widgets.
    pub const TITLE: Self = Self(3);
    /// The titles of headers.
    pub const HEAD: Self = Self(4);
    /// Large, e.g. a widget's main line.
    pub const HEADLINE: Self = Self(5);
    /// Larger, e.g. a clock.
    pub const DISPLAY: Self = Self(6);
    /// Large numbers standing alone, e.g. a page count.
    pub const NUMERAL: Self = Self(7);
    /// A hint under a control (sized as [`TextRole::SMALL`]).
    pub const HINT: Self = Self(8);
    /// The label of a button or a list row (sized as [`TextRole::BODY`]).
    pub const BUTTON: Self = Self(9);
    /// A glyph of the icon font (sized as [`TextRole::BODY`]; the caller
    /// sets an icon's own size).
    pub const ICON: Self = Self(10);

    /// The role's number, as the constants above have it: 0 to 10 are
    /// named, the others are free.
    pub const fn value(self) -> u8 {
        self.0
    }

    /// The role of a number; one this release has no name for is a role
    /// all the same.
    pub const fn from_value(value: u8) -> Self {
        Self(value)
    }

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
pub enum Focus {
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
pub enum Separators {
    /// Light lines within the margins.
    Line,
    /// Fainter lines, from where the text starts.
    Inset,
    /// None: room alone parts the rows.
    None,
}

impl UiStyle {
    /// Fixed sizes for text 20 pixels high, square corners, lines between
    /// rows, the focus a dark bar, two grays: what no device or setting
    /// changes. Its fields can be changed as any style's.
    pub const PLAIN: UiStyle = UiStyle {
        text: 20.0,
        small: 15.0,
        title: 23.0,
        head: 23.0,
        row: 60,
        control_row: 60,
        header: 64,
        margin: 12,
        gap: 8,
        line: 2,
        dark_header: false,
        corner: 0,
        strong: false,
        pill: false,
        focus: Focus::Fill,
        separators: Separators::Line,
        plain_header: false,
        cards: false,
        grays: 2,
        finger: 0,
        look: super::look::Look::none(),
        palette: Palette::DEFAULT,
        direction: 0,
    };

    pub fn new(env: &Env<'_>) -> Self {
        let m = env.metrics();
        let base = env.settings().read(&settings::UI_SIZE).clamp(6, 40) as f32;
        let text = m.pt(base);
        let font = env.fonts().role(fonts::UI.as_str());
        let line_height = font
            .as_ref()
            .map_or(text * 1.25, |f| f.line_metrics(text).line_height);
        let s = env.settings();
        let rows = s.read(&settings::UI_ROW).clamp(120, 300) as f32 / 100.0;
        let small = m.pt(base * 0.75);
        // A row holds a label on two lines in the smallest size rows set
        // it in (see `draw_value_row`): where even that does not fit (at
        // the largest sizes, whose fonts come in steps), rows are as high
        // as two lines of small text; at other sizes they stay as they are.
        let control_row = line_height * rows;
        let row = match &font {
            Some(f) if 2.0 * f.line_metrics(small * 0.7).line_height > control_row => {
                control_row.max(2.0 * f.line_metrics(small).line_height + m.mm(2) as f32)
            }
            _ => control_row,
        };
        Self {
            text,
            small,
            title: m.pt(base * 1.15),
            head: m.pt(base * s.read(&settings::UI_TITLE).clamp(100, 200) as f32 / 100.0),
            row: row as u32,
            control_row: control_row as u32,
            header: (line_height * (rows + 0.1)) as u32,
            margin: m.mm(s.read(&settings::UI_MARGIN).clamp(0, 15) as u32),
            gap: m.mm(2),
            line: m
                .tenth_mm(s.read(&settings::UI_LINES).clamp(1, 10) as u32)
                .max(1),
            dark_header: s.text(settings::UI_HEADER.as_str()) == settings::HEADER_DARK,
            corner: match settings::CORNERS
                .iter()
                .position(|c| *c == s.text(settings::UI_CORNERS.as_str()))
            {
                Some(1) => m.tenth_mm(8),
                Some(2) => m.mm(2),
                Some(3) => m.tenth_mm(35),
                _ => 0,
            },
            strong: s.text(settings::UI_CONTRAST.as_str()) == settings::CONTRAST_STRONG,
            pill: settings::CORNERS[2..].contains(&s.text(settings::UI_CORNERS.as_str())),
            focus: match s.text(settings::UI_FOCUS.as_str()) {
                settings::FOCUS_RING => Focus::Ring,
                settings::FOCUS_SOFT => Focus::Soft,
                _ => Focus::Fill,
            },
            separators: match s.text(settings::UI_SEPARATORS.as_str()) {
                settings::SEPARATORS_INSET => Separators::Inset,
                settings::SEPARATORS_NONE => Separators::None,
                _ => Separators::Line,
            },
            plain_header: s.text(settings::UI_HEADER.as_str()) == settings::HEADER_PLAIN,
            cards: s.text(settings::UI_GROUPS.as_str()) == settings::GROUPS_CARDS,
            grays: env.device().display.format.gray_levels().unwrap_or(256),
            finger: if env.device().input.touch { m.mm(7) } else { 0 },
            look: env.look(),
            palette: Palette::parse(s.text(settings::UI_PALETTE.as_str())),
            direction: u8::from(env.direction() == paperpaw_lang::Direction::Rtl),
        }
    }

    /// The radius of the corners of a shape `height` pixels high: half
    /// its height for pills, else the style's corners.
    /// Taller shapes (a row with a note) keep the corners, at most half a
    /// margin round: the room between their edge and their text, so the
    /// curve never comes near what is in them.
    pub fn radius(&self, height: u32) -> u32 {
        if self.pill && height <= self.row * 5 / 4 {
            height / 2
        } else if height > self.row * 5 / 4 {
            self.corner.min(self.margin / 2)
        } else {
            self.corner
        }
    }

    /// Whether the focus is drawn dark, so what is on it is drawn white.
    pub fn dark_focus(&self) -> bool {
        self.focus == Focus::Fill
    }

    /// The ink of what is drawn on a row or item, `focused` or not:
    /// white on a dark focus, else black.
    pub fn focus_ink(&self, focused: bool) -> Color {
        if focused && self.dark_focus() {
            self.ink(Ink::PAPER)
        } else {
            self.ink(Ink::TEXT)
        }
    }

    /// The paper of a row or item, `focused` or not: what its content may
    /// be cleared to (black on a dark focus).
    pub fn focus_paper(&self, focused: bool) -> Color {
        if focused && self.dark_focus() {
            self.ink(Ink::TEXT)
        } else {
            self.ink(Ink::PAPER)
        }
    }

    /// The colour of gray text (notes) on a row or item, `focused` or not
    /// (light on a dark focus).
    pub fn focus_faint(&self, focused: bool, ink: Ink) -> Color {
        if focused && self.dark_focus() {
            self.ink(Ink::ON_FOCUS_MUTED)
        } else {
            self.faint(ink)
        }
    }

    /// A row or bar of `height` for buttons: on touch screens as much
    /// higher as its buttons ([`button_height`]) grow to be as large as a
    /// fingertip ([`UiStyle::finger`]), so that the room above and below
    /// them stays what it is in a row of `height`; as it is with buttons
    /// alone. Every row or bar that holds buttons is made with it.
    pub fn touch_row(&self, height: u32) -> u32 {
        height.max(self.finger + (height - height * 7 / 10))
    }

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
    /// sized as [`TextRole::BODY`]).
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

    /// The colour of gray text such as hints and notes, of `ink`'s role
    /// in the palette; black for strong contrast.
    pub fn faint(&self, ink: Ink) -> Color {
        if self.strong {
            self.ink(Ink::TEXT)
        } else {
            self.palette.color(ink)
        }
    }

    /// A header's background and text: the palette's text and paper, the
    /// other way round for light headers.
    pub fn header_colors(&self) -> (Color, Color) {
        let (text, paper) = (self.ink(Ink::TEXT), self.ink(Ink::PAPER));
        if self.dark_header {
            (text, paper)
        } else {
            (paper, text)
        }
    }

    /// Draws a header's background in `area`: dark, or light with a thin
    /// line below (as the separators are; none for plain headers).
    pub(super) fn header_background(&self, canvas: &mut Canvas<'_>, area: Rect) {
        let (paper, ink) = self.header_colors();
        canvas.fill(area, paper);
        if self.dark_header || self.plain_header {
            return;
        }
        let line = self.line.max(1) as u16;
        let y = (area.y + area.height).saturating_sub(line);
        match self.separators {
            Separators::Line => canvas.fill(Rect::new(area.x, y, area.width, line), ink),
            Separators::Inset => {
                let margin = self.margin as u16;
                let width = area.width.saturating_sub(2 * margin);
                canvas.fill_tint(
                    Rect::new(area.x + margin, y, width, line),
                    self.ink(Ink::RULE),
                );
            }
            Separators::None => {}
        }
    }

    /// The part of a header's `area` its title and signs are centred in:
    /// all of it, but the line below a light header.
    pub(super) fn header_band(&self, area: Rect) -> Rect {
        let lined = !self.dark_header && !self.plain_header && self.separators != Separators::None;
        let line = if lined { self.line.max(1) as u16 } else { 0 };
        Rect::new(area.x, area.y, area.width, area.height.saturating_sub(line))
    }

    /// The UI font, inked as the setting `core.ui-text-curve` says
    /// ([`text_ink`]).
    pub fn font<'a>(&self, env: &Env<'a>) -> Option<FontHandle<'a>> {
        env.fonts()
            .role(fonts::UI.as_str())
            .map(|font| font.inked(text_ink(env)))
    }

    /// Height of a row with a note: the title line, then up to
    /// [`NOTE_LINES`] lines of small text (see [`Row::value`](super::Row::value)).
    pub fn noted_row(&self) -> u32 {
        self.noted_row_of(NOTE_LINES)
    }

    /// Height of a row with a note of `lines` lines (e.g. 1 for denser
    /// lists of many entries): a title line, then the note.
    pub fn noted_row_of(&self, lines: usize) -> u32 {
        let notes = (lines as f32 * self.small * NOTE_LINE_HEIGHT) as u32;
        self.title_band() + notes + self.gap * 3 / 2
    }

    /// Height of the rows of a list `width` wide whose notes are `notes`:
    /// a noted row ([`UiStyle::noted_row`]), higher by the lines more the
    /// longest note takes (the largest text on a small screen), up to
    /// [`NOTE_LINES_MOST`] lines; a longer note is shortened with "…".
    pub fn noted_row_for<'n>(
        &self,
        env: &Env<'_>,
        width: u16,
        notes: impl IntoIterator<Item = &'n str>,
    ) -> u32 {
        let Some(font) = self.font(env) else {
            return self.noted_row();
        };
        let room = (f32::from(width) - 2.0 * self.margin as f32).max(1.0);
        let lines = notes
            .into_iter()
            .map(|note| font.wrap(self.small, note, room).len())
            .max()
            .unwrap_or(0);
        self.noted_row_of(lines.clamp(NOTE_LINES, NOTE_LINES_MOST))
    }

    /// Height of the title line of a noted row.
    pub(super) fn title_band(&self) -> u32 {
        self.row * 3 / 4
    }
}

/// Where the focus of a row or an item in `rect` is drawn: a little in
/// from the sides, so it sits within the screen's margins as a rounded
/// bar, and off the lines between rows.
pub fn focus_rect(style: &UiStyle, rect: Rect) -> Rect {
    let side = (style.margin / 2) as u16;
    let edge = (style.line as u16).min(rect.height / 8);
    rect.inset(side.min(rect.width / 8), edge)
}

/// Draws the focus of the row or item in `rect` (see [`focus_rect`]):
/// black, for content drawn white on it. Every list draws its focus this
/// way, so it looks the same everywhere.
pub(crate) fn draw_focus(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect) {
    let r = focus_rect(style, rect);
    if style
        .look
        .focus(canvas, style, super::look::FocusPart { rect: r })
    {
        return;
    }
    // Items taller than wide (a cover) keep the corners.
    let radius = if r.width < r.height {
        style.corner
    } else {
        style.radius(u32::from(r.height))
    };
    let ring = (style.line * 2).max(3);
    match style.focus {
        Focus::Fill => canvas.fill_round(r, radius, Color::BLACK),
        Focus::Ring => canvas.stroke_round(r, ring, radius, Color::BLACK),
        Focus::Soft if style.grays > 2 => {
            // As light as the panel shows with black text reading well.
            let gray = if style.grays >= 16 { 215 } else { 170 };
            canvas.fill_round(r, radius, Color::gray(gray));
        }
        Focus::Soft => canvas.stroke_round(r, ring, radius, Color::BLACK),
    }
}

/// Draws the line below the row in `rect` as the style's separators
/// are: `indent` is where the row's text starts after the margin (e.g.
/// right of an icon), where faint lines start.
pub(crate) fn draw_separator(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect, indent: u16) {
    let part = super::look::SeparatorPart { row: rect, indent };
    if style.look.separator(canvas, style, part) {
        return;
    }
    let margin = style.margin as u16;
    let line = style.line as u16;
    let y = rect.y + rect.height - line;
    match style.separators {
        Separators::Line => canvas.fill_tint(
            Rect::new(
                rect.x + margin,
                y,
                rect.width.saturating_sub(2 * margin),
                line,
            ),
            style.ink(Ink::LINE),
        ),
        Separators::Inset => {
            let start = margin + indent;
            let thin = (line / 2).max(1);
            canvas.fill_tint(
                Rect::new(
                    rect.x + start,
                    y,
                    rect.width.saturating_sub(start + margin),
                    thin,
                ),
                style.ink(Ink::LINE_FAINT),
            );
        }
        Separators::None => {}
    }
}

/// Fills a rectangle with the style's corners.
pub fn fill_round(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect, color: Color) {
    canvas.fill_round(area, style.corner, color);
}

/// Draws a box's frame, `thickness` wide, with the style's corners.
pub fn stroke_round(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    area: Rect,
    thickness: u32,
    color: Color,
) {
    canvas.stroke_round(area, thickness, style.corner, color);
}
