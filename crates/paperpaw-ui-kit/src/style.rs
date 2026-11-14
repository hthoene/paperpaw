//! Stock UI geometry read through typed native settings and font calls.

use crate::widget::Frame;
use alloc::string::String;
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        fonts::Font,
        settings::{self, SettingValue},
        snapshot::Snapshot,
    },
};

const UI_SIZE: &str = "core.ui-size";
const UI_ROW: &str = "core.ui-row";
const UI_TITLE: &str = "core.ui-title";
const UI_MARGIN: &str = "core.ui-margin";
const UI_LINES: &str = "core.ui-lines";
const UI_HEADER: &str = "core.ui-header";
const UI_SEPARATORS: &str = "core.ui-separators";
const UI_PALETTE: &str = "core.ui-palette";
const UI_SHADOW: &str = "core.ui-shadow";
const UI_FRAME: &str = "core.ui-frame";
const UI_DENSITY: &str = "core.ui-density";
const UI_BUTTON: &str = "core.ui-button";

/// The roles a theme may colour beyond the sheet of palette names the SDK
/// knows (they sit in the same setting, `core.ui-palette`, and an older reader
/// of it skips them): `None` where the theme did not set one, so the look is
/// what it was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Roles {
    /// The plate of sheets, dialogs and chips (`panel`).
    pub panel: Option<Color>,
    /// The bar the chosen row is on (`focus`).
    pub focus: Option<Color>,
    /// The fill of buttons that are filled (`control`).
    pub control: Option<Color>,
    /// The lines round plates and chips (`outline`).
    pub outline: Option<Color>,
    /// The chosen chip (`chip`).
    pub chip: Option<Color>,
    /// The dark bar of a header (`bar`).
    pub bar: Option<Color>,
}

impl Roles {
    /// The role names, in the order of the fields.
    pub const NAMES: [&'static str; 6] = ["panel", "focus", "control", "outline", "chip", "bar"];

    /// The roles on a panel of two grays, where a fill is black or white: a
    /// fill lighter than mid-gray would read as paper, so a role that is one
    /// (`focus`, `control`, `chip`) is left out and the part keeps its look.
    #[must_use]
    pub fn for_two_grays(self) -> Self {
        let dark = |role: Option<Color>| role.filter(|color| color.luma() < 128);
        Self {
            focus: dark(self.focus),
            control: dark(self.control),
            chip: dark(self.chip),
            ..self
        }
    }

    fn read(palette: &str) -> Self {
        let get = |name| palette_find(palette, name);
        Self {
            panel: get("panel"),
            focus: get("focus"),
            control: get("control"),
            outline: get("outline"),
            chip: get("chip"),
            bar: get("bar"),
        }
    }
}

/// Of `a` and `b` the ink that reads better on `plate`.
#[must_use]
pub fn readable(plate: Color, a: Color, b: Color) -> Color {
    if plate.luma().abs_diff(a.luma()) >= plate.luma().abs_diff(b.luma()) {
        a
    } else {
        b
    }
}

/// What a sheet, a dialog or a chip stands on, by its frame: the colour of the
/// plate, whether a line runs round it, and the inks its content is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plate {
    /// The plate; none where the content stands on the page itself.
    pub fill: Option<Color>,
    /// A line round it, in the content's ink.
    pub outline: bool,
    /// The colour of that line.
    pub line: Color,
    /// The ink of the content.
    pub ink: Color,
    /// The paper of the content (what the chosen part's text is drawn in).
    pub paper: Color,
}

/// How buttons that are not chosen look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonLook {
    /// A line round the label, as they always were.
    #[default]
    Outlined,
    /// A card's colour behind the label, no line.
    Filled,
    /// The label alone.
    Flat,
}

impl ButtonLook {
    fn of(value: &str) -> Self {
        match value {
            "core.button.filled" => Self::Filled,
            "core.button.flat" => Self::Flat,
            _ => Self::Outlined,
        }
    }
}

/// How much room the interface leaves between and round its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Density {
    Compact,
    #[default]
    Regular,
    Roomy,
}

impl Density {
    fn of(value: &str) -> Self {
        match value {
            "core.density.compact" => Self::Compact,
            "core.density.roomy" => Self::Roomy,
            _ => Self::Regular,
        }
    }

    /// `pixels` a part keeps: three quarters, as it was, five quarters.
    fn scale(self, pixels: u32) -> u32 {
        match self {
            Self::Compact => pixels * 3 / 4,
            Self::Regular => pixels,
            Self::Roomy => pixels * 5 / 4,
        }
    }
}

fn integer(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut bytes = [0; 32];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => 0,
    })
}

/// The bytes a text setting is read into: the palette (`core.ui-palette`) must
/// fit, or every screen fails (#114, I2).
pub(crate) const TEXT_BYTES: usize = 512;

/// The palette text as read: a palette that does not fit the buffer it is read
/// into ([`TEXT_BYTES`]; the host answers `NoSpace`) is read as none, so the
/// stock grays are drawn, not nothing at all (every screen failed on it).
fn palette_or_none(read: Result<String, Error>) -> Result<String, Error> {
    match read {
        Err(Error::NoSpace) => Ok(String::new()),
        other => other,
    }
}

pub fn setting_text(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut bytes = [0; TEXT_BYTES];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Text(value) => crate::text::copy(value)?,
        _ => String::new(),
    })
}

#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the stock look is a set of switches"
)]
pub struct Style {
    pub corner: u32,
    pub pill: bool,
    /// The chosen row is a frame, not a bar (focus `core.focus.ring`, or the
    /// soft one on a panel of two grays).
    pub ring: bool,
    /// The chosen row is a light bar with the text's ink on it (focus
    /// `core.focus.soft` on a panel of more than two grays).
    pub soft: bool,
    /// Pixels of the hard shadow of sheets, dialogs and chips; 0 for none
    /// (`core.ui-shadow`).
    pub shadow: u16,
    /// The frame of sheets, dialogs and chips where the theme chose one for
    /// all (`core.ui-frame`); `None` leaves each its own.
    pub frame: Option<Frame>,
    /// How buttons that are not chosen look (`core.ui-button`).
    pub button: ButtonLook,
    /// The panel shows black and white alone: a card is a sparser pattern.
    pub mono: bool,
    /// The colours a theme gave the parts beyond the palette's grays.
    pub roles: Roles,
    pub separator_ink: Color,
    /// Dither separator tints for the monochrome UI of grayscale panels.
    pub separator_dither: bool,
    pub text: f32,
    pub small: f32,
    pub title: f32,
    pub head: f32,
    pub row: u16,
    pub header: u16,
    pub margin: u16,
    pub gap: u16,
    pub line: u16,
    pub dark: bool,
    pub plain: bool,
    pub separators: String,
    pub ink: Color,
    pub paper: Color,
    pub disabled: Color,
    pub rule: Color,
    /// Secondary text.
    pub muted: Color,
    /// Notes and footers.
    pub note: Color,
    /// Height of a section heading line.
    pub heading: u16,
    /// Cards, e.g. the frames of widgets.
    pub card: Color,
    /// Lesser text on a dark card.
    pub on_dark_muted: Color,
    /// Gray text is drawn black (setting `core.ui-contrast`).
    pub strong: bool,
    /// The interface runs right to left (its language is written so, see
    /// [`language_is_rtl`]): screens lay themselves out once from the left and
    /// mirror each part with [`Style::flip`] and [`Style::flip_x`].
    pub rtl: bool,
    /// Somebody asks what the screen shows (`canvas.describe`): the parts
    /// told with [`crate::paint::describe`] are queued only then. False until a
    /// draw callback has asked ([`crate::paint::asking`]): a screen that does
    /// not ask queues nothing.
    pub describing: bool,
}

impl Style {
    pub fn read(host: &Host<'_>, display: &Snapshot, font: &Font<'_>) -> Result<Self, Error> {
        let dpi = u32::from(display.display_dpi());
        let tenth = |value: u32| tenth_mm(dpi, value);
        let pt = |points: f32| points * dpi as f32 / 72.0;
        let base = integer(host, UI_SIZE)?.clamp(6, 40) as f32;
        let text = pt(base);
        let small = pt(base * 0.75);
        let line_height = crate::geometry::line_height(font, text)?;
        let rows = integer(host, UI_ROW)?.clamp(120, 300) as f32 / 100.0;
        let control_row = line_height * rows;
        let least_line = crate::geometry::line_height(font, small * 0.7)?;
        let row = if 2.0 * least_line > control_row {
            if base < 10.0 {
                control_row.max(2.0 * least_line + tenth(6) as f32)
            } else {
                control_row.max(2.0 * crate::geometry::line_height(font, small)? + tenth(20) as f32)
            }
        } else {
            control_row
        };
        let finger = finger_px(display);
        let lift = finger.saturating_sub(row as u32);
        let header = setting_text(host, UI_HEADER)?;
        let palette = palette_or_none(setting_text(host, UI_PALETTE))?;
        let color = |name, default| palette_color(&palette, name, default);
        let strong = setting_text(host, "core.ui-contrast")? == "core.contrast.strong";
        let faint = |name, default| {
            if strong {
                color("text", Color::BLACK)
            } else {
                color(name, default)
            }
        };
        let looks = Looks::read(host, display, tenth(10))?;
        let corners = setting_text(host, "core.ui-corners")?;
        let corner = match corners.as_str() {
            "core.corners.small" => tenth(8),
            "core.corners.round" => tenth(20),
            "core.corners.soft" => tenth(35),
            _ => 0,
        };
        let gap = narrow(looks.density.scale(tenth(20)));
        Ok(Self {
            corner,
            pill: matches!(corners.as_str(), "core.corners.round" | "core.corners.soft"),
            ring: looks.ring,
            soft: looks.soft,
            shadow: looks.shadow,
            frame: looks.frame,
            button: looks.button,
            mono: display.display_gray_levels() == Some(2),
            roles: if display.display_gray_levels() == Some(2) {
                Roles::read(&palette).for_two_grays()
            } else {
                Roles::read(&palette)
            },
            separator_dither: !display.display_is_colour(),
            separator_ink: if setting_text(host, UI_SEPARATORS)? == "core.separators.inset" {
                color("line-faint", Color::gray(210))
            } else {
                color("line", Color::gray(190))
            },
            text,
            small,
            title: pt(base * 1.15),
            head: pt(base * integer(host, UI_TITLE)?.clamp(100, 200) as f32 / 100.0),
            row: narrow((row as u32).max(finger)),
            header: narrow((line_height * (rows + 0.1)) as u32 + lift),
            margin: narrow(
                looks
                    .density
                    .scale(tenth(integer(host, UI_MARGIN)?.clamp(0, 15) as u32 * 10)),
            ),
            gap,
            line: narrow(tenth(integer(host, UI_LINES)?.clamp(1, 10) as u32).max(1)),
            dark: header == "core.header.dark",
            plain: header == "core.header.plain",
            separators: setting_text(host, UI_SEPARATORS)?,
            ink: color("text", Color::BLACK),
            paper: color("paper", Color::WHITE),
            disabled: color("disabled", Color::gray(150)),
            rule: color("rule", Color::gray(150)),
            muted: faint("muted", Color::gray(90)),
            note: faint("note", Color::gray(60)),
            heading: narrow(
                (crate::geometry::line_height(font, small)? * 1.25) as u32 + u32::from(gap),
            ),
            card: color("card", Color::gray(225)),
            on_dark_muted: color("on-dark-muted", Color::gray(200)),
            strong,
            rtl: ui_language_is_rtl(host),
            describing: false,
        })
    }

    /// What the page is painted over under a header: `area` (where the rows
    /// go) and the gap above it, so the strip between the header and the rows
    /// takes the theme's paper and not what the host cleared the screen to.
    #[must_use]
    pub fn page(&self, area: Rect) -> Rect {
        let gap = self.gap.min(area.y);
        Rect::new(
            area.x,
            area.y - gap,
            area.width,
            area.height.saturating_add(gap),
        )
    }

    /// The colour of the bar the chosen row is on (the theme's `focus`; else
    /// the text's ink, a card's colour for the light bar); of the frame it is
    /// drawn in as a ring (the theme's `outline`, else the ink).
    #[must_use]
    pub fn bar(&self) -> Color {
        if self.ring {
            return self.roles.outline.unwrap_or(self.ink);
        }
        self.roles
            .focus
            .unwrap_or(if self.soft { self.soft_bar() } else { self.ink })
    }

    /// The light bar where the theme gave no `focus`: the paper, three tenths of
    /// the way to the ink, so that it still stands out from the paper on a panel
    /// of four grays (a card's colour, gray 225, would be paper there).
    #[must_use]
    pub fn soft_bar(&self) -> Color {
        let mix = |paper: u8, ink: u8| {
            let (p, i) = (u16::from(paper), u16::from(ink));
            let value = if i >= p {
                p + (i - p) * 3 / 10
            } else {
                p - (p - i) * 3 / 10
            };
            value as u8
        };
        Color::rgb(
            mix(self.paper.r, self.ink.r),
            mix(self.paper.g, self.ink.g),
            mix(self.paper.b, self.ink.b),
        )
    }

    /// The ink of text on the chosen row: the paper's on a dark bar, the
    /// text's own on a light one or in a frame; on the theme's `focus` bar,
    /// whichever of the two reads better.
    #[must_use]
    pub fn on_bar(&self) -> Color {
        if self.ring {
            self.ink
        } else if let Some(bar) = self.roles.focus {
            readable(bar, self.ink, self.paper)
        } else if self.soft {
            self.ink
        } else {
            self.paper
        }
    }

    /// The plate of a part whose own frame is `own`: the frame the theme
    /// chose for all where it chose one (`core.ui-frame`). A dark plate is
    /// the darker of the inks with the lighter one on it; where that is the
    /// page's own paper (a dark theme) it is a card, or the theme's panel. A
    /// panel the theme coloured is every plate's colour, and the content takes
    /// whichever ink reads better on it.
    #[must_use]
    pub fn plate(&self, own: Frame) -> Plate {
        let kind = self.frame.unwrap_or(own);
        let (fill, outline) = match kind {
            Frame::None => (None, false),
            Frame::Line => (Some(self.paper), true),
            Frame::Card if self.mono => (Some(sparser(self.card)), true),
            // A theme that coloured the outline wants the edge of its cards seen.
            Frame::Card => (Some(self.card), self.roles.outline.is_some()),
            Frame::Dark if self.darker() == self.paper => (Some(self.card), false),
            Frame::Dark => (Some(self.darker()), false),
        };
        let fill = match (fill, self.roles.panel) {
            (Some(_), Some(panel)) => Some(panel),
            (fill, _) => fill,
        };
        // The content takes the ink that reads on the plate, whatever the plate
        // is: a card in a dark theme, a dark plate, a panel the theme coloured.
        let (ink, paper) = match fill {
            Some(plate) => {
                let ink = readable(plate, self.ink, self.paper);
                (
                    ink,
                    if ink == self.ink {
                        self.paper
                    } else {
                        self.ink
                    },
                )
            }
            None => (self.ink, self.paper),
        };
        Plate {
            fill,
            outline,
            line: self.roles.outline.unwrap_or(ink),
            ink,
            paper,
        }
    }

    /// The ink of text on `fill`: `fallback` where the theme coloured no part
    /// (`coloured` false), else whichever of the two inks reads better.
    #[must_use]
    pub fn on(&self, fill: Color, coloured: bool, fallback: Color) -> Color {
        if coloured {
            readable(fill, self.ink, self.paper)
        } else {
            fallback
        }
    }

    /// The bar a header is drawn dark on (`bar`; the text's ink).
    #[must_use]
    pub fn header_bar(&self) -> Color {
        self.roles.bar.unwrap_or(self.ink)
    }

    /// The fill of a button that is filled and not chosen (`control`; a card).
    #[must_use]
    pub fn control(&self) -> Color {
        self.roles.control.unwrap_or(self.card)
    }

    /// The colour of a hard shadow: dark gray under a plate that is darker
    /// than that, else the darker of the inks.
    #[must_use]
    pub fn shade(&self, plate: Option<Color>) -> Color {
        let dark_gray = Color::gray(90);
        if plate.is_some_and(|fill| fill.luma() < dark_gray.luma() / 2) {
            dark_gray
        } else {
            self.darker()
        }
    }

    /// The darker of the interface's two inks (text and paper): the ink on
    /// light plates and the plate of a dark frame. Black in the default theme.
    #[must_use]
    pub fn darker(&self) -> Color {
        if self.ink.luma() <= self.paper.luma() {
            self.ink
        } else {
            self.paper
        }
    }

    /// The lighter of the interface's two inks: the ink on dark plates.
    /// White in the default theme.
    #[must_use]
    pub fn lighter(&self) -> Color {
        if self.ink.luma() <= self.paper.luma() {
            self.paper
        } else {
            self.ink
        }
    }

    /// `rect`, laid out left to right in `outer`, as it stands where the
    /// interface runs right to left: its top and size the same, its left edge
    /// as far from `outer`'s right edge as its left edge was from `outer`'s
    /// left one, never left of 0. Not moved otherwise. Mirrored twice, a
    /// rectangle inside `outer` is where it was.
    #[must_use]
    pub fn flip(&self, outer: Rect, rect: Rect) -> Rect {
        flip_rect(self.rtl, outer, rect)
    }

    /// Where a span `width` wide, laid out left to right from `x` in `outer`,
    /// starts where the interface runs right to left (as far from the right
    /// edge of `outer` as it was from its left one); `x` otherwise. For text
    /// and other parts placed in fractions of a pixel; [`Style::flip`] for
    /// rectangles.
    #[must_use]
    pub fn flip_x(&self, outer: Rect, x: f32, width: f32) -> f32 {
        flip_span(self.rtl, outer, x, width)
    }
}

/// A [`Style`] a screen keeps between its draws, read from the settings once
/// and kept until the screen clears it. A read is a dozen host calls and a
/// screen drawn in bands (the X4) reads once per band: a screen that holds one
/// reads once per look. The screen must clear it when `setting_changed`
/// arrives and when it is shown or opened, or it draws a look the reader has
/// left. Nothing in the kit holds one for a screen yet: a plugin that does not
/// ask reads as before. **Whoever first holds one adds the test** that it is
/// cleared on `setting_changed` and on show.
#[derive(Debug, Default)]
pub struct StyleCache {
    style: Option<Style>,
}

impl StyleCache {
    /// An empty cache.
    #[must_use]
    pub const fn new() -> Self {
        Self { style: None }
    }

    /// The kept look, read first if there is none.
    ///
    /// # Errors
    /// What [`Style::read`] fails with; nothing is kept then.
    pub fn get(
        &mut self,
        host: &Host<'_>,
        display: &Snapshot,
        font: &Font<'_>,
    ) -> Result<&mut Style, Error> {
        self.get_with(|| Style::read(host, display, font))
    }

    /// [`StyleCache::get`] with the reading given.
    ///
    /// # Errors
    /// What `read` fails with; nothing is kept then.
    pub fn get_with(
        &mut self,
        read: impl FnOnce() -> Result<Style, Error>,
    ) -> Result<&mut Style, Error> {
        let style = match self.style.take() {
            Some(style) => style,
            None => read()?,
        };
        Ok(self.style.insert(style))
    }

    /// Forgets the look: the next [`StyleCache::get`] reads it again.
    pub fn clear(&mut self) {
        self.style = None;
    }

    /// Whether a look is kept.
    #[must_use]
    pub const fn is_kept(&self) -> bool {
        self.style.is_some()
    }
}

/// [`Style::flip`] for a widget that keeps only whether the interface runs
/// right to left.
#[must_use]
pub fn flip_rect(rtl: bool, outer: Rect, rect: Rect) -> Rect {
    if !rtl {
        return rect;
    }
    let x =
        2 * i32::from(outer.x) + i32::from(outer.width) - i32::from(rect.x) - i32::from(rect.width);
    Rect::new(
        x.clamp(0, i32::from(u16::MAX)) as u16,
        rect.y,
        rect.width,
        rect.height,
    )
}

/// [`Style::flip_x`] for a widget that keeps only whether the interface runs
/// right to left.
#[must_use]
pub fn flip_span(rtl: bool, outer: Rect, x: f32, width: f32) -> f32 {
    if rtl {
        2.0 * f32::from(outer.x) + f32::from(outer.width) - x - width
    } else {
        x
    }
}

/// Whether text in the language `tag` (`ar`, `he`, `fa-IR`, ...) is written
/// right to left: by its script where the tag names one, else by its language
/// ([`Lang::direction`](paperpaw_sdk::lang::Lang::direction)).
#[must_use]
pub fn language_is_rtl(tag: &str) -> bool {
    paperpaw_sdk::lang::Lang::parse(tag).direction() == paperpaw_sdk::lang::Direction::Rtl
}

/// Whether the interface language of the host (`core.language`) is written
/// right to left. A host that does not say leaves the interface left to right.
fn ui_language_is_rtl(host: &Host<'_>) -> bool {
    let mut scratch = [0u8; 64];
    paperpaw_sdk::native::i18n::language(host, &mut scratch).is_ok_and(language_is_rtl)
}

/// The settings of the look beyond the sizes and colours, read together.
struct Looks {
    ring: bool,
    soft: bool,
    shadow: u16,
    frame: Option<Frame>,
    button: ButtonLook,
    density: Density,
}

impl Looks {
    /// `shadow` is the length of the hard shadow in pixels.
    fn read(host: &Host<'_>, display: &Snapshot, shadow: u32) -> Result<Self, Error> {
        let (ring, soft) = focus_look(
            &setting_text(host, "core.ui-focus")?,
            display.display_gray_levels(),
        );
        let hard = setting_text(host, UI_SHADOW)? == "core.shadow.hard";
        let frame = setting_text(host, UI_FRAME)?;
        Ok(Self {
            ring,
            soft,
            shadow: if hard { narrow(shadow) } else { 0 },
            frame: crate::widget::look::FRAMES
                .contains(&frame.as_str())
                .then(|| Frame::of(&frame)),
            button: ButtonLook::of(&setting_text(host, UI_BUTTON)?),
            density: Density::of(&setting_text(host, UI_DENSITY)?),
        })
    }
}

/// How the chosen row is shown, as (a frame, a light bar): a dark bar when
/// neither. The light bar needs more than two grays (a panel that knows no
/// number of grays, such as a colour one, gets the frame); on two it is a
/// frame (#114).
fn focus_look(focus: &str, grays: Option<u16>) -> (bool, bool) {
    match focus {
        "core.focus.ring" => (true, false),
        "core.focus.soft" if grays.is_some_and(|levels| levels > 2) => (false, true),
        "core.focus.soft" => (true, false),
        _ => (false, false),
    }
}

/// A card on a panel of two grays: half way to the paper, as a pattern.
fn sparser(card: Color) -> Color {
    let half = |c: u8| c + (255 - c) / 2;
    Color::rgb(half(card.r), half(card.g), half(card.b))
}

/// `value` tenths of a millimetre in pixels at `dpi`, at least one pixel for
/// any length.
fn tenth_mm(dpi: u32, value: u32) -> u32 {
    let pixels = (value * dpi + 127) / 254;
    if pixels == 0 && value > 0 { 1 } else { pixels }
}

/// How many pixels high a fingertip is: 7 mm on a touch screen, `0` on one
/// that is not. The least a row or a chip on a touch screen is high.
#[must_use]
pub fn finger_px(display: &Snapshot) -> u32 {
    if display.has_touch() {
        tenth_mm(u32::from(display.display_dpi()), 70)
    } else {
        0
    }
}

/// A length in pixels, capped at [`crate::geometry::MAX_PX`] so sums of a few never wrap.
fn narrow(value: u32) -> u16 {
    value.min(crate::geometry::MAX_PX as u32) as u16
}

fn palette_color(palette: &str, name: &str, default: Color) -> Color {
    palette_find(palette, name).unwrap_or(default)
}

/// The colour the palette gives `name` (the last of a name given twice).
fn palette_find(palette: &str, name: &str) -> Option<Color> {
    let mut color = None;
    for pair in palette.split_ascii_whitespace() {
        if let Some((key, value)) = pair.split_once('=')
            && key == name
            && let Some(value) = parse_color(value)
        {
            color = Some(value);
        }
    }
    color
}

fn parse_color(value: &str) -> Option<Color> {
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        // `get`, not a slice: a str slice that fails links the panic that prints the
        // character it fell inside (Debug and Unicode tables, 4 KB per plugin).
        let byte = |from: usize, to: usize| {
            hex.get(from..to)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        };
        Some(Color::rgb(byte(0, 2)?, byte(2, 4)?, byte(4, 6)?))
    } else if !value.is_empty()
        && value.len() <= 3
        && value.bytes().all(|byte| byte.is_ascii_digit())
    {
        value.parse().ok().map(Color::gray)
    } else {
        None
    }
}

/// A style for the tests of the parts that draw in it: its inks are told
/// apart from black, white and the grays a literal would give.
#[cfg(test)]
impl Style {
    pub(crate) fn themed(ink: Color, paper: Color) -> Self {
        Self {
            corner: 4,
            pill: false,
            ring: false,
            soft: false,
            shadow: 0,
            frame: None,
            button: ButtonLook::Outlined,
            mono: false,
            roles: Roles::default(),
            separator_dither: false,
            separator_ink: Color::rgb(3, 4, 5),
            text: 22.0,
            small: 17.0,
            title: 26.0,
            head: 28.0,
            row: 60,
            header: 56,
            margin: 16,
            gap: 8,
            line: 2,
            dark: false,
            plain: false,
            separators: String::new(),
            ink,
            paper,
            disabled: Color::rgb(6, 7, 8),
            rule: Color::rgb(9, 10, 11),
            muted: Color::rgb(60, 61, 62),
            note: Color::rgb(63, 64, 65),
            heading: 30,
            card: Color::rgb(200, 201, 202),
            on_dark_muted: Color::rgb(190, 191, 192),
            strong: false,
            rtl: false,
            describing: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_mirror_in_their_area_and_twice_is_the_same() {
        let outer = Rect::new(10, 0, 200, 50);
        let rect = Rect::new(30, 5, 40, 20);
        assert_eq!(flip_rect(false, outer, rect), rect);
        let flipped = flip_rect(true, outer, rect);
        // As far from the right edge as it was from the left one.
        assert_eq!(flipped, Rect::new(210 - 40 - 20, 5, 40, 20));
        assert_eq!(flip_rect(true, outer, flipped), rect);
    }

    #[test]
    fn a_part_wider_than_its_area_never_starts_left_of_zero() {
        let outer = Rect::new(0, 0, 100, 10);
        let flipped = flip_rect(true, outer, Rect::new(50, 0, 400, 10));
        assert_eq!(flipped.x, 0);
        // Spans in fractions of a pixel may start left of it.
        assert!(flip_span(true, outer, 50.0, 400.0) < 0.0);
        assert!((flip_span(true, outer, 10.0, 30.0) - 60.0).abs() < f32::EPSILON);
        assert!((flip_span(false, outer, 10.0, 30.0) - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn hostile_rectangles_do_not_wrap() {
        let outer = Rect::new(u16::MAX, 0, u16::MAX, 10);
        let rect = Rect::new(u16::MAX, 0, u16::MAX, 10);
        let _ = flip_rect(true, outer, rect);
        let _ = flip_rect(true, Rect::new(0, 0, 0, 0), rect);
    }

    #[test]
    fn the_scripts_written_right_to_left_are_found_by_their_tag() {
        for rtl in ["ar", "he", "fa", "ur", "ar-EG", "fa-IR", "yi", "sd-Arab"] {
            assert!(language_is_rtl(rtl), "{rtl}");
        }
        for ltr in [
            "en", "de", "de-CH", "zh-Hans", "", "es", "tr", "az-Latn", "uz-Cyrl",
        ] {
            assert!(!language_is_rtl(ltr), "{ltr}");
        }
        // A tag that names a left-to-right script is that, whatever the language.
        assert!(!language_is_rtl("ar-Latn"));
    }

    #[test]
    fn the_soft_focus_is_a_light_bar_on_more_than_two_grays_and_a_frame_on_two() {
        // #114: it was refused, and with it every screen of a theme that
        // chose it (the Pebble theme does).
        assert_eq!(focus_look("core.focus.soft", Some(4)), (false, true));
        assert_eq!(focus_look("core.focus.soft", Some(16)), (false, true));
        assert_eq!(focus_look("core.focus.soft", Some(2)), (true, false));
        assert_eq!(focus_look("core.focus.soft", None), (true, false));
        assert_eq!(focus_look("core.focus.ring", Some(16)), (true, false));
        assert_eq!(focus_look("core.focus.fill", Some(16)), (false, false));
        assert_eq!(focus_look("", Some(16)), (false, false));
    }

    #[test]
    fn a_full_palette_fits_the_buffer_it_is_read_into() {
        // The SDK's own role names, each set to a colour of the longest
        // form: names added past the buffer would fail every screen, and
        // need a second setting (#114, I2).
        let mut palette = String::new();
        for ink in paperpaw_sdk::ui::Ink::ALL {
            palette.push_str(ink.name());
            palette.push_str("=#rrggbb ");
        }
        assert!(palette.len() <= TEXT_BYTES / 4 * 3, "{}", palette.len());
    }

    #[test]
    fn a_cache_reads_once_until_it_is_cleared_and_keeps_nothing_on_failure() {
        let mut reads = 0;
        let mut cache = StyleCache::new();
        assert!(!cache.is_kept());
        let mut read = |ink| {
            reads += 1;
            Ok(Style::themed(ink, Color::WHITE))
        };
        let first = cache.get_with(|| read(Color::gray(1))).map(|s| s.ink);
        assert_eq!(first, Ok(Color::gray(1)));
        let again = cache.get_with(|| read(Color::gray(2))).map(|s| s.ink);
        assert_eq!(again, Ok(Color::gray(1)), "kept, not read again");
        cache.clear();
        assert!(!cache.is_kept());
        let after = cache.get_with(|| read(Color::gray(3))).map(|s| s.ink);
        assert_eq!(after, Ok(Color::gray(3)), "read again after clear");
        assert_eq!(reads, 2);
        cache.clear();
        assert!(cache.get_with(|| Err(Error::NoMemory)).is_err());
        assert!(!cache.is_kept());
    }

    #[test]
    fn a_theme_that_chose_a_frame_sets_the_plate_and_the_inks_on_it() {
        let mut style = Style::themed(Color::rgb(10, 20, 30), Color::rgb(240, 230, 220));
        // Its own: a line round paper, the inks as they are (the look until now).
        let own = style.plate(Frame::Line);
        assert_eq!((own.fill, own.outline), (Some(style.paper), true));
        assert_eq!((own.ink, own.paper), (style.ink, style.paper));
        style.frame = Some(Frame::None);
        assert_eq!(style.plate(Frame::Line).fill, None);
        style.frame = Some(Frame::Card);
        let card = style.plate(Frame::Line);
        assert_eq!((card.fill, card.outline), (Some(style.card), false));
        style.roles.outline = Some(Color::gray(40));
        assert!(
            style.plate(Frame::Line).outline,
            "an outline role shows the card's edge"
        );
        style.roles.outline = None;
        style.mono = true;
        let sparse = style.plate(Frame::Line);
        assert_eq!(sparse.fill, Some(sparser(style.card)));
        assert!(sparse.outline, "a pattern needs its line on two grays");
        style.mono = false;
        style.frame = Some(Frame::Dark);
        let dark = style.plate(Frame::Line);
        assert_eq!(dark.fill, Some(style.darker()));
        assert_eq!((dark.ink, dark.paper), (style.lighter(), style.darker()));
    }

    #[test]
    fn the_shadow_is_dark_gray_under_a_dark_plate_else_the_darker_ink() {
        let style = Style::themed(Color::BLACK, Color::WHITE);
        assert_eq!(style.shade(Some(Color::WHITE)), Color::BLACK);
        assert_eq!(style.shade(None), Color::BLACK);
        assert_eq!(style.shade(Some(Color::BLACK)), Color::gray(90));
    }

    #[test]
    fn density_scales_a_length_and_regular_keeps_it() {
        assert_eq!(Density::of("core.density.compact").scale(20), 15);
        assert_eq!(Density::of("core.density.roomy").scale(20), 25);
        assert_eq!(Density::of("core.density.regular").scale(20), 20);
        assert_eq!(Density::of("").scale(20), 20);
        assert_eq!(Density::of("core.density.compact").scale(0), 0);
    }

    #[test]
    fn a_button_look_is_named_by_its_value_and_outlined_is_the_default() {
        assert_eq!(ButtonLook::of("core.button.filled"), ButtonLook::Filled);
        assert_eq!(ButtonLook::of("core.button.flat"), ButtonLook::Flat);
        assert_eq!(ButtonLook::of("core.button.outlined"), ButtonLook::Outlined);
        assert_eq!(ButtonLook::of("anything else"), ButtonLook::Outlined);
    }

    #[test]
    fn the_roles_a_theme_sets_are_read_from_the_palette_and_others_stay_none() {
        let roles =
            Roles::read("text=10 panel=#112233 chip=200 bar=#fff outline=nonsense muted=60");
        assert_eq!(roles.panel, Some(Color::rgb(0x11, 0x22, 0x33)));
        assert_eq!(roles.chip, Some(Color::gray(200)));
        assert_eq!(roles.bar, None, "three hex digits are not a colour");
        assert_eq!(roles.outline, None);
        assert_eq!(roles.focus, None);
        assert_eq!(Roles::read(""), Roles::default());
        // The last of a name given twice wins, as for the grays.
        assert_eq!(Roles::read("panel=1 panel=2").panel, Some(Color::gray(2)));
    }

    #[test]
    fn a_palette_with_every_role_still_fits_the_buffer_it_is_read_into() {
        let mut palette = String::new();
        for ink in paperpaw_sdk::ui::Ink::ALL {
            palette.push_str(ink.name());
            palette.push_str("=#rrggbb ");
        }
        for name in Roles::NAMES {
            palette.push_str(name);
            palette.push_str("=#rrggbb ");
        }
        assert!(
            palette.len() < TEXT_BYTES,
            "{}: needs a second setting (I2)",
            palette.len()
        );
    }

    #[test]
    fn a_dark_plate_in_a_dark_theme_is_a_card_the_page_shows_apart() {
        // #1418 review: the plate was the page's own paper, so the dark card
        // was not to be seen on a dark page.
        let mut style = Style::themed(Color::rgb(235, 230, 220), Color::rgb(25, 30, 35));
        style.frame = Some(Frame::Dark);
        let plate = style.plate(Frame::Line);
        assert_eq!(plate.fill, Some(style.card));
        assert_ne!(plate.fill, Some(style.paper));
        // The card here is light: the content takes the dark ink on it, not
        // the theme's light text (#1420 review, remark 3).
        assert_eq!((plate.ink, plate.paper), (style.paper, style.ink));
        style.card = Color::gray(40);
        let dim = style.plate(Frame::Line);
        assert_eq!((dim.ink, dim.paper), (style.ink, style.paper));
    }

    #[test]
    fn a_panel_the_theme_coloured_is_the_plate_and_its_content_reads_on_it() {
        let mut style = Style::themed(Color::BLACK, Color::WHITE);
        style.roles.panel = Some(Color::gray(20));
        let plate = style.plate(Frame::Line);
        assert_eq!(plate.fill, Some(Color::gray(20)));
        assert_eq!((plate.ink, plate.paper), (Color::WHITE, Color::BLACK));
        style.roles.panel = Some(Color::gray(235));
        let plate = style.plate(Frame::Line);
        assert_eq!((plate.ink, plate.paper), (Color::BLACK, Color::WHITE));
        // No plate (frame none): the panel does not make one.
        style.frame = Some(Frame::None);
        assert_eq!(style.plate(Frame::Line).fill, None);
    }

    #[test]
    fn the_focus_bar_and_the_outline_and_the_header_bar_follow_their_roles() {
        let mut style = Style::themed(Color::BLACK, Color::WHITE);
        assert_eq!(style.bar(), Color::BLACK);
        assert_eq!(style.header_bar(), Color::BLACK);
        assert_eq!(style.control(), style.card);
        style.roles.focus = Some(Color::gray(200));
        style.roles.bar = Some(Color::gray(40));
        style.roles.control = Some(Color::gray(180));
        style.roles.outline = Some(Color::gray(120));
        assert_eq!(style.bar(), Color::gray(200));
        assert_eq!(style.on_bar(), Color::BLACK, "dark text on a light bar");
        assert_eq!(style.header_bar(), Color::gray(40));
        assert_eq!(style.control(), Color::gray(180));
        assert_eq!(style.plate(Frame::Line).line, Color::gray(120));
        style.roles.focus = Some(Color::gray(30));
        assert_eq!(style.on_bar(), Color::WHITE, "light text on a dark bar");
    }

    #[test]
    fn the_page_reaches_up_over_the_gap_under_the_header() {
        let style = Style::themed(Color::BLACK, Color::WHITE);
        let area = Rect::new(0, style.header + style.gap, 480, 700);
        let page = style.page(area);
        assert_eq!((page.y, page.height), (style.header, 700 + style.gap));
        // Never above the top of the screen.
        let high = style.page(Rect::new(0, 3, 480, 700));
        assert_eq!((high.y, high.height), (0, 703));
    }

    #[test]
    fn on_two_grays_a_light_fill_role_is_left_out_and_a_dark_one_kept() {
        let roles = Roles::read("focus=#b4bfc8 control=200 chip=#202020 panel=#ffffff bar=#fff0f0");
        let two = roles.for_two_grays();
        assert_eq!((two.focus, two.control), (None, None));
        assert_eq!(two.chip, Some(Color::rgb(0x20, 0x20, 0x20)));
        // The roles that are not fills of a part chosen stay.
        assert_eq!((two.panel, two.bar), (roles.panel, roles.bar));
    }

    #[test]
    fn a_ring_is_drawn_in_the_outline_not_the_light_bar_colour() {
        let mut style = Style::themed(Color::BLACK, Color::WHITE);
        style.roles.focus = Some(Color::gray(200));
        style.ring = true;
        assert_eq!(style.bar(), Color::BLACK);
        style.roles.outline = Some(Color::gray(40));
        assert_eq!(style.bar(), Color::gray(40));
    }

    /// What the host answers a read of a text longer than the buffer: `NoSpace`
    /// (`read_setting`'s contract), and nothing of the value.
    fn host_reads(value: &str) -> Result<String, Error> {
        if value.len() > TEXT_BYTES {
            Err(Error::NoSpace)
        } else {
            Ok(String::from(value))
        }
    }

    #[test]
    fn a_palette_longer_than_its_buffer_is_read_as_none_and_not_as_a_failure() {
        let long = "muted=60 ".repeat(67);
        assert!(long.len() >= 600);
        let palette = palette_or_none(host_reads(&long)).unwrap();
        assert_eq!(palette, "");
        // With none, every role has the colour it had without a theme.
        assert_eq!(palette_color(&palette, "text", Color::BLACK), Color::BLACK);
        assert_eq!(Roles::read(&palette), Roles::default());
        // A palette that fits is kept; any other failure is still one.
        assert_eq!(palette_or_none(host_reads("muted=60")).unwrap(), "muted=60");
        assert_eq!(palette_or_none(Err(Error::NoMemory)), Err(Error::NoMemory));
    }
}
