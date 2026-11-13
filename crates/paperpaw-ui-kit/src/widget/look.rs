//! The look widgets share: a frame (none, a line, a light card, a dark
//! card), how far the content keeps from it, the corners, the line, a hard
//! shadow, the paper and the ink; each a setting of the widget named
//! `<widget>.<key>`, or one setting for all widgets until the reader chose a
//! value for the widget itself.
use super::settings::{Settings, base};
use crate::{Paint, Style};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    decorations::{self, Tone},
    native::Error,
};

/// What the reader chose for all widgets.
pub const ALL_FRAME: &str = "core.widgets.frame";
pub const ALL_PAPER: &str = "core.widgets.paper";
pub const ALL_INK: &str = "core.widgets.ink";
pub const ALL_CORNERS: &str = "core.widgets.corners";
pub const ALL_LINE: &str = "core.widgets.frame-line";
pub const ALL_SHADOW: &str = "core.widgets.shadow";
pub const ALL_PADDING: &str = "core.widgets.padding";
pub const ALL_GAP: &str = "core.widgets.gap";

/// The value of the frame for all widgets that leaves each its own.
pub const OWN_FRAME: &str = "core.frame.own";
/// The frames a widget offers.
pub const FRAMES: &[&str] = &[
    "core.frame.none",
    "core.frame.line",
    "core.frame.card",
    "core.frame.dark",
];
/// The text sizes a widget offers; the last, "as large as fits", only where
/// the widget works it out.
pub const SIZES: &[&str] = &[
    "core.size.small",
    "core.size.medium",
    "core.size.large",
    "core.size.fit",
];
/// The paddings a widget offers.
pub const PADDINGS: &[&str] = &[
    "core.padding.auto",
    "core.padding.none",
    "core.padding.small",
    "core.padding.normal",
    "core.padding.large",
];
/// The alignments a widget offers.
pub const ALIGNMENTS: &[&str] = &["core.align.left", "core.align.center", "core.align.right"];
/// The papers a widget offers (a tone or a colour code is kept too).
pub const PAPERS: &[&str] = &[
    PAPER_NORMAL,
    PAPER_INVERTED,
    PAPER_LIGHT_GRAY,
    PAPER_DARK_GRAY,
    PAPER_NONE,
];
pub const PAPER_NORMAL: &str = "core.plate.normal";
pub const PAPER_INVERTED: &str = "core.plate.inverted";
pub const PAPER_LIGHT_GRAY: &str = "core.plate.light-gray";
pub const PAPER_DARK_GRAY: &str = "core.plate.dark-gray";
pub const PAPER_NONE: &str = "core.plate.none";
/// The ink that follows the paper.
pub const INK_AUTO: &str = "core.ink.auto";
pub const CORNERS_AUTO: &str = "core.corners.auto";
pub const LINE_AUTO: &str = "core.lines.auto";
pub const SHADOW_NONE: &str = "core.shadow.none";
pub const SHADOW_HARD: &str = "core.shadow.hard";

/// What a widget is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Frame {
    /// Nothing: the widget on the page itself.
    None,
    /// A line round it.
    Line,
    /// A light card behind it.
    Card,
    /// A dark card, the widget's text light on it.
    Dark,
}

impl Frame {
    /// The frame a value of [`FRAMES`] names; none for others.
    #[must_use]
    pub fn of(value: &str) -> Self {
        match FRAMES.iter().position(|f| *f == value) {
            Some(1) => Self::Line,
            Some(2) => Self::Card,
            Some(3) => Self::Dark,
            _ => Self::None,
        }
    }
}

/// How large a widget's text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextSize {
    Small,
    Medium,
    Large,
    /// As large as its place allows: the widget works it out.
    Fit,
}

impl TextSize {
    /// The size a value of [`SIZES`] names; medium for others.
    #[must_use]
    pub fn of(value: &str) -> Self {
        match SIZES.iter().position(|s| *s == value) {
            Some(0) => Self::Small,
            Some(2) => Self::Large,
            Some(3) => Self::Fit,
            _ => Self::Medium,
        }
    }

    /// The text size in pixels: the body's, a title's, or larger.
    #[must_use]
    pub fn text(self, style: &Style) -> f32 {
        match self {
            Self::Small => style.text,
            Self::Medium => style.title,
            Self::Large | Self::Fit => style.title * 1.6,
        }
    }
}

/// How far a widget's content keeps from its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Padding {
    /// None without a frame, a gap inside one.
    Auto,
    None,
    Small,
    Normal,
    Large,
}

impl Padding {
    /// The padding a value of [`PADDINGS`] names; automatic for others.
    #[must_use]
    pub fn of(value: &str) -> Self {
        match PADDINGS.iter().position(|p| *p == value) {
            Some(1) => Self::None,
            Some(2) => Self::Small,
            Some(3) => Self::Normal,
            Some(4) => Self::Large,
            _ => Self::Auto,
        }
    }

    /// The pixels it keeps from the edges of `frame` drawn in `style`.
    #[must_use]
    pub fn pixels(self, style: &Style, frame: Frame) -> u16 {
        let gap = u32::from(style.gap);
        let step = match self {
            Self::Auto if frame == Frame::None => 0,
            Self::Auto | Self::Normal => gap,
            Self::None => 0,
            Self::Small => gap / 2,
            Self::Large => gap * 2,
        };
        let line = match frame {
            Frame::None => 0,
            _ => u32::from(style.line) * 2,
        };
        crate::geometry::clamp_u16(i64::from(step + line))
    }
}

/// Where a line of text sits in its room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    /// The alignment a value of [`ALIGNMENTS`] names; left for others.
    #[must_use]
    pub fn of(value: &str) -> Self {
        match ALIGNMENTS.iter().position(|a| *a == value) {
            Some(1) => Self::Center,
            Some(2) => Self::Right,
            _ => Self::Left,
        }
    }

    /// Where text `width` wide starts in `area`.
    #[must_use]
    pub fn x(self, area: Rect, width: f32) -> f32 {
        let left = f32::from(area.x);
        let room = (f32::from(area.width) - width).max(0.0);
        match self {
            Self::Center => left + room / 2.0,
            Self::Right => left + room,
            Self::Left => left,
        }
    }
}

/// A shadow below and right of a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Shadow {
    None,
    Hard,
}

/// A widget's place inside its frame, and the colours to draw in there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Look {
    /// Where its content goes.
    pub area: Rect,
    /// Text and lines.
    pub ink: Color,
    /// Text that matters less.
    pub faint: Color,
    /// What lies behind the content.
    pub paper: Color,
    pub frame: Frame,
}

/// How a colour setting reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Colour {
    Auto,
    None,
    /// The interface's own text colour (an inverted plate).
    Ink,
    Is(Color),
}

fn colour(value: &str) -> Colour {
    match value {
        PAPER_INVERTED => Colour::Ink,
        PAPER_LIGHT_GRAY => Colour::Is(Tone::LightGray.color()),
        PAPER_DARK_GRAY => Colour::Is(Tone::DarkGray.color()),
        PAPER_NONE | decorations::NONE => Colour::None,
        _ => {
            if let Some(tone) = Tone::from_id(value) {
                Colour::Is(tone.color())
            } else if value.starts_with('#')
                && let Some([r, g, b]) = paperpaw_sdk::builtin::settings::parse_rgb(value)
            {
                Colour::Is(Color::rgb(r, g, b))
            } else {
                Colour::Auto
            }
        }
    }
}

/// The widget's settings made into its look: reads settings and holds the
/// style the widget draws in (the interface's, with the widget's corners and
/// line).
#[derive(Debug)]
pub struct Shape {
    /// The style with this widget's corners and frame line.
    pub style: Style,
    frame: Frame,
    shadow: Shadow,
    paper: Colour,
    ink: Colour,
    padding: Padding,
    corner: Option<u32>,
}

fn px(style: &Style, tenths: u32) -> u32 {
    (u32::from(style.gap) * tenths / 20).max(1)
}

impl Shape {
    /// Reads the settings of the widget behind `settings`.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn read(settings: &Settings<'_, '_>, mut style: Style) -> Result<Self, Error> {
        let frame = settings.chosen_frame()?;
        let corner =
            settings.chosen("corners", ALL_CORNERS, CORNERS_AUTO, |value| match value {
                "core.corners.square" => Some(0),
                "core.corners.small" => Some(px(&style, 8)),
                "core.corners.round" => Some(px(&style, 20)),
                "core.corners.soft" => Some(px(&style, 35)),
                _ => None,
            })?;
        if let Some(radius) = corner {
            style.corner = radius;
        }
        let line = settings.chosen("frame-line", ALL_LINE, LINE_AUTO, |value| match value {
            "core.lines.thin" => Some(px(&style, 2)),
            "core.lines.medium" => Some(px(&style, 4)),
            "core.lines.thick" => Some(px(&style, 8)),
            _ => None,
        })?;
        if let Some(width) = line {
            style.line = crate::geometry::clamp_u16(i64::from(width));
        }
        let shadow = settings.chosen("drop-shadow", ALL_SHADOW, SHADOW_NONE, |value| {
            if value == SHADOW_HARD {
                Shadow::Hard
            } else {
                Shadow::None
            }
        })?;
        let paper = settings.chosen("paper", ALL_PAPER, PAPER_NORMAL, colour)?;
        let ink = settings.chosen("ink", ALL_INK, INK_AUTO, colour)?;
        let padding = settings.chosen("padding", ALL_PADDING, "core.padding.auto", Padding::of)?;
        Ok(Self {
            style,
            frame,
            shadow,
            paper,
            ink,
            padding,
            corner,
        })
    }

    /// The frame it is drawn in.
    #[must_use]
    pub const fn frame(&self) -> Frame {
        self.frame
    }

    fn shadow_offset(&self) -> u16 {
        let opaque =
            matches!(self.frame, Frame::Card | Frame::Dark) || matches!(self.paper, Colour::Is(_));
        if self.shadow == Shadow::Hard && opaque {
            crate::geometry::clamp_u16(i64::from(px(&self.style, 10)))
        } else {
            0
        }
    }

    /// The plate: `area` without the room a shadow takes.
    fn plate(&self, area: Rect) -> Rect {
        let offset = self.shadow_offset();
        Rect::new(
            area.x,
            area.y,
            area.width.saturating_sub(offset),
            area.height.saturating_sub(offset),
        )
    }

    fn plate_corner(&self) -> u32 {
        match self.frame {
            Frame::None => self.corner.unwrap_or(0),
            _ => self.style.corner,
        }
    }

    /// Where the content goes in `area` and in which colours, without
    /// drawing.
    #[must_use]
    pub fn look(&self, area: Rect) -> Look {
        let area = self.plate(area);
        let pad = self.padding.pixels(&self.style, self.frame);
        let pad_x = pad.min(area.width / 4);
        let pad_y = pad.min(area.height / 4);
        let inner = Rect::new(
            area.x.saturating_add(pad_x),
            area.y.saturating_add(pad_y),
            area.width.saturating_sub(2 * pad_x),
            area.height.saturating_sub(2 * pad_y),
        );
        let paper = match (self.paper, self.frame) {
            (Colour::Is(colour), _) => colour,
            (Colour::Ink, _) => self.style.ink,
            (Colour::None, _) | (Colour::Auto, Frame::None | Frame::Line | Frame::Card) => {
                self.style.paper
            }
            (Colour::Auto, Frame::Dark) => self.style.darker(),
        };
        let (ink, faint) = match self.ink {
            Colour::Is(colour) => (colour, colour),
            _ => self.inks_on(paper),
        };
        Look {
            area: inner,
            ink,
            faint,
            paper,
            frame: self.frame,
        }
    }

    /// The text and the lesser text on `paper`: the light of the interface's
    /// two inks on a dark plate, the dark one on a light plate.
    fn inks_on(&self, paper: Color) -> (Color, Color) {
        if paper.luma() < 128 {
            let light = self.style.lighter();
            let faint = if self.style.strong {
                light
            } else {
                self.style.on_dark_muted
            };
            (light, faint)
        } else {
            (self.style.darker(), self.style.muted)
        }
    }

    /// The height the frame takes above and below the content.
    #[must_use]
    pub fn frame_height(&self) -> u16 {
        let probe = Rect::new(0, 0, 1000, 1000);
        1000 - self.look(probe).area.height
    }

    /// Paints the frame in `area`: the shadow, the paper, the card or the
    /// line. `grays` is how many grays the panel shows (2: black and white).
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    pub fn paint(&self, out: &mut Vec<Paint>, area: Rect, grays: u16) -> Result<Look, Error> {
        let look = self.look(area);
        let plate = self.plate(area);
        let offset = self.shadow_offset();
        let corner = self.plate_corner();
        if offset > 0 {
            let below = Rect::new(
                plate.x.saturating_add(offset),
                plate.y.saturating_add(offset),
                plate.width,
                plate.height,
            );
            let dark_gray = Tone::DarkGray.color();
            let shade = if look.paper.luma() < dark_gray.luma() / 2 {
                dark_gray
            } else {
                self.style.darker()
            };
            crate::paint::push(out, Paint::Focus(below, corner, None, shade))?;
        }
        let border = u32::from(self.style.line).max(2);
        match self.paper {
            Colour::Is(_) | Colour::Ink => {
                let colour = look.paper;
                crate::paint::push(out, Paint::Focus(plate, corner, None, colour))?;
                if look.frame == Frame::Line {
                    crate::paint::push(out, Paint::Focus(plate, corner, Some(border), look.ink))?;
                }
            }
            Colour::None => {
                if look.frame == Frame::Line {
                    crate::paint::push(out, Paint::Focus(plate, corner, Some(border), look.ink))?;
                }
            }
            Colour::Auto => match look.frame {
                Frame::None => {}
                Frame::Line => crate::paint::push(
                    out,
                    Paint::Focus(plate, self.style.corner, Some(border), self.style.darker()),
                )?,
                Frame::Card if grays <= 2 => {
                    let card = self.style.card;
                    let sparser = |c: u8| c + (255 - c) / 2;
                    let pattern = Color::rgb(sparser(card.r), sparser(card.g), sparser(card.b));
                    crate::paint::push(out, Paint::Focus(plate, self.style.corner, None, pattern))?;
                    let line = u32::from((self.style.line / 2).max(1));
                    crate::paint::push(
                        out,
                        Paint::Focus(plate, self.style.corner, Some(line), self.style.darker()),
                    )?;
                }
                Frame::Card => crate::paint::push(
                    out,
                    Paint::Focus(plate, self.style.corner, None, self.style.card),
                )?,
                Frame::Dark => crate::paint::push(
                    out,
                    Paint::Focus(plate, self.style.corner, None, self.style.darker()),
                )?,
            },
        }
        Ok(look)
    }
}

impl Settings<'_, '_> {
    /// The frame: the reader's choice for all widgets, unless one is chosen
    /// for this widget.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn chosen_frame(&self) -> Result<Frame, Error> {
        self.chosen("frame", ALL_FRAME, OWN_FRAME, Frame::of)
    }

    /// The size of the widget's text: its `<widget>.size`; medium without one.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn size(&self) -> Result<TextSize, Error> {
        self.with_text("size", TextSize::of)
    }

    /// Where the widget's text sits: its `<widget>.align`.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn align(&self) -> Result<Align, Error> {
        self.with_text("align", Align::of)
    }

    /// Whether the widget shows `part` (`<widget>.<part>`).
    ///
    /// # Errors
    /// The host's refusal.
    pub fn shows(&self, part: &str) -> Result<bool, Error> {
        self.flag(part)
    }
}

/// The widget's id without its placement.
#[must_use]
pub fn base_id(widget: &str) -> &str {
    base(widget)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_name_their_choices_and_unknown_ones_fall_back() {
        assert_eq!(Frame::of("core.frame.dark"), Frame::Dark);
        assert_eq!(Frame::of("nonsense"), Frame::None);
        assert_eq!(TextSize::of("core.size.fit"), TextSize::Fit);
        assert_eq!(TextSize::of(""), TextSize::Medium);
        assert_eq!(Padding::of("core.padding.large"), Padding::Large);
        assert_eq!(Padding::of("x"), Padding::Auto);
        assert_eq!(Align::of("core.align.right"), Align::Right);
    }

    #[test]
    fn alignment_places_text_in_its_room() {
        let area = Rect::new(10, 0, 100, 20);
        assert!((Align::Left.x(area, 40.0) - 10.0).abs() < f32::EPSILON);
        assert!((Align::Center.x(area, 40.0) - 40.0).abs() < f32::EPSILON);
        assert!((Align::Right.x(area, 40.0) - 70.0).abs() < f32::EPSILON);
        // Wider than the room: at the left edge, never before it.
        assert!((Align::Right.x(area, 140.0) - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn colours_read_tones_codes_and_none() {
        assert_eq!(colour(PAPER_INVERTED), Colour::Ink);
        assert_eq!(colour(PAPER_NONE), Colour::None);
        assert_eq!(colour("#102030"), Colour::Is(Color::rgb(0x10, 0x20, 0x30)));
        assert_eq!(colour("core.ink.auto"), Colour::Auto);
        assert_eq!(colour("#12"), Colour::Auto);
    }
    fn themed(ink: Color, paper: Color) -> Style {
        Style::themed(ink, paper)
    }

    fn shape(style: Style, frame: Frame, paper: Colour, shadow: Shadow) -> Shape {
        Shape {
            style,
            frame,
            shadow,
            paper,
            ink: Colour::Auto,
            padding: Padding::Auto,
            corner: None,
        }
    }

    fn colours(out: &[Paint]) -> alloc::vec::Vec<Color> {
        out.iter()
            .filter_map(|op| match op {
                Paint::Fill(_, color) | Paint::Focus(_, _, _, color) => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The theme matrix of the widget frame (#114, step 0): in a theme whose
    /// inks are none of the literals, every colour a frame draws or gives its
    /// content is one of the theme's.
    #[test]
    fn a_widget_frame_is_drawn_in_the_themes_inks_only() {
        let themes = [
            themed(Color::rgb(20, 30, 40), Color::rgb(240, 235, 225)),
            // A dark theme: light text on dark paper.
            themed(Color::rgb(235, 230, 220), Color::rgb(25, 30, 35)),
        ];
        for style in themes {
            let sparser = |c: u8| c + (255 - c) / 2;
            let pattern = Color::rgb(
                sparser(style.card.r),
                sparser(style.card.g),
                sparser(style.card.b),
            );
            let allowed = [
                style.ink,
                style.paper,
                style.card,
                style.muted,
                style.on_dark_muted,
                pattern,
                Tone::DarkGray.color(),
            ];
            for frame in [Frame::None, Frame::Line, Frame::Card, Frame::Dark] {
                for paper in [Colour::Auto, Colour::None, Colour::Ink] {
                    for shadow in [Shadow::None, Shadow::Hard] {
                        for grays in [2, 16] {
                            let shape = shape(themed(style.ink, style.paper), frame, paper, shadow);
                            let mut out = alloc::vec::Vec::new();
                            let look = shape
                                .paint(&mut out, Rect::new(0, 0, 200, 100), grays)
                                .unwrap();
                            let mut used = colours(&out);
                            used.extend([look.ink, look.faint, look.paper]);
                            for color in used {
                                assert!(
                                    allowed.contains(&color),
                                    "{frame:?} {paper:?} {shadow:?} {grays}: {color:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// In the default theme nothing changes: black and white as before.
    #[test]
    fn the_default_theme_draws_the_widget_frames_as_before() {
        let style = || themed(Color::BLACK, Color::WHITE);
        let dark = shape(style(), Frame::Dark, Colour::Auto, Shadow::None);
        let mut out = alloc::vec::Vec::new();
        let look = dark.paint(&mut out, Rect::new(0, 0, 200, 100), 16).unwrap();
        assert_eq!(colours(&out), [Color::BLACK]);
        assert_eq!((look.ink, look.paper), (Color::WHITE, Color::BLACK));
        let line = shape(style(), Frame::Line, Colour::Auto, Shadow::None);
        let mut out = alloc::vec::Vec::new();
        let look = line.paint(&mut out, Rect::new(0, 0, 200, 100), 16).unwrap();
        assert_eq!(colours(&out), [Color::BLACK]);
        assert_eq!((look.ink, look.paper), (Color::BLACK, Color::WHITE));
        let inverted = shape(style(), Frame::Card, Colour::Ink, Shadow::Hard);
        let mut out = alloc::vec::Vec::new();
        let look = inverted
            .paint(&mut out, Rect::new(0, 0, 200, 100), 16)
            .unwrap();
        assert_eq!(look.paper, Color::BLACK);
        // The hard shadow of a plate this dark is a dark gray, as before.
        assert_eq!(colours(&out), [Tone::DarkGray.color(), Color::BLACK]);
    }
}
