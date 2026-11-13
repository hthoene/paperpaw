//! Stock UI geometry read through typed native settings and font calls.

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
    pub ring: bool,
    pub separator_ink: Color,
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
        let palette = setting_text(host, UI_PALETTE)?;
        let color = |name, default| palette_color(&palette, name, default);
        let strong = setting_text(host, "core.ui-contrast")? == "core.contrast.strong";
        let faint = |name, default| {
            if strong {
                color("text", Color::BLACK)
            } else {
                color(name, default)
            }
        };
        let focus = setting_text(host, "core.ui-focus")?;
        let ring = focus_is_ring(&focus);
        let corners = setting_text(host, "core.ui-corners")?;
        let corner = match corners.as_str() {
            "core.corners.small" => tenth(8),
            "core.corners.round" => tenth(20),
            "core.corners.soft" => tenth(35),
            _ => 0,
        };
        let gap = narrow(tenth(20));
        Ok(Self {
            corner,
            pill: matches!(corners.as_str(), "core.corners.round" | "core.corners.soft"),
            ring,
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
            margin: narrow(tenth(integer(host, UI_MARGIN)?.clamp(0, 15) as u32 * 10)),
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
/// arrives and when it is shown or opened, or it draws a look the reader
/// has left. Nothing in the kit holds one for a screen: a plugin that does not
/// ask reads as before.
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

/// Whether the chosen row is shown as a frame (a ring) and not as a filled bar.
/// The soft focus (a light bar) is a frame too until the kit draws it: it was
/// refused, and with it every screen, on a device whose theme chose it
/// (#114). The light bar itself is drawn from the focus settings of the
/// build-out (#2848, step 2); until then it is a frame.
fn focus_is_ring(focus: &str) -> bool {
    matches!(focus, "core.focus.ring" | "core.focus.soft")
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
    let mut color = default;
    for pair in palette.split_ascii_whitespace() {
        if let Some((key, value)) = pair.split_once('=')
            && key == name
            && let Some(value) = parse_color(value)
        {
            color = value;
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
    fn the_soft_focus_is_a_frame_until_the_kit_draws_a_light_bar() {
        // #114: it was refused, and with it every screen of a theme that
        // chose it (the Pebble theme does).
        assert!(focus_is_ring("core.focus.ring"));
        assert!(focus_is_ring("core.focus.soft"));
        assert!(!focus_is_ring("core.focus.fill"));
        assert!(!focus_is_ring(""));
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
}
