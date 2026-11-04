//! The shape of widgets (#206): how round their corners are
//! (`<widget>.corners`), how thick their frame's line is
//! (`<widget>.frame-line`) and whether their plate casts a shadow
//! (`<widget>.drop-shadow`), each also for every widget whose own the
//! reader did not choose ([`WIDGET_CORNERS`], [`WIDGET_FRAME_LINE`],
//! [`WIDGET_SHADOW`]); the spacing inside every widget
//! ([`WIDGET_PADDING`]) and the gap between them ([`gap`]).
//! [`widget_look`](super::widget_look) and
//! [`draw_frame`](super::draw_frame) follow them.
//!
//! [`WIDGET_CORNERS`]: crate::builtin::settings::WIDGET_CORNERS
//! [`WIDGET_FRAME_LINE`]: crate::builtin::settings::WIDGET_FRAME_LINE
//! [`WIDGET_SHADOW`]: crate::builtin::settings::WIDGET_SHADOW
//! [`WIDGET_PADDING`]: crate::builtin::settings::WIDGET_PADDING

use alloc::format;

use paperpaw_platform::{Color, Rect};

use super::colours::{self, ColourSpec};
use super::look::{Frame, choice_setting};
use crate::builtin::settings::{
    self, WIDGET_CORNERS, WIDGET_FRAME_LINE, WIDGET_GAP, WIDGET_PADDING, WIDGET_SHADOW,
};
use crate::decorations::Tone;
use crate::env::Env;
use crate::id::Id;
use crate::settings::SettingSpec;
use crate::ui::UiStyle;

/// Corners as the interface has them ([`settings::UI_CORNERS`]).
pub const CORNERS_AUTO: &str = "core.corners.auto";
/// The values of `<widget>.corners`: automatic, then the interface's
/// corners ([`settings::CORNERS`]: square, slightly rounded, round, soft).
pub const CORNER_CHOICES: &[&str] = &[
    CORNERS_AUTO,
    settings::CORNERS[0],
    settings::CORNERS[1],
    settings::CORNERS[2],
    settings::CORNERS[3],
];

/// A frame line as thick as the interface's lines ([`settings::UI_LINES`]).
pub const LINE_AUTO: &str = "core.lines.auto";
/// 0.2 mm.
pub const LINE_THIN: &str = "core.lines.thin";
/// 0.4 mm.
pub const LINE_MEDIUM: &str = "core.lines.medium";
/// 0.8 mm.
pub const LINE_THICK: &str = "core.lines.thick";
/// The values of `<widget>.frame-line`.
pub const LINE_CHOICES: &[&str] = &[LINE_AUTO, LINE_THIN, LINE_MEDIUM, LINE_THICK];

pub const SHADOW_NONE: &str = "core.shadow.none";
/// A solid shadow, 1 mm below and to the right of the plate.
pub const SHADOW_HARD: &str = "core.shadow.hard";
/// The values of `<widget>.drop-shadow`.
pub const SHADOWS: &[&str] = &[SHADOW_NONE, SHADOW_HARD];

pub const GAP_SMALL: &str = "core.gap.small";
pub const GAP_NORMAL: &str = "core.gap.normal";
pub const GAP_LARGE: &str = "core.gap.large";
/// The values of [`WIDGET_GAP`](crate::builtin::settings::WIDGET_GAP).
pub const GAPS: &[&str] = &[GAP_SMALL, GAP_NORMAL, GAP_LARGE];

/// The shadow a widget's plate casts ([`shadow`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Shadow {
    None,
    /// Solid, 1 mm below and to the right.
    Hard,
}

/// `widget`'s own value of `key` where the reader chose it, else that of
/// all widgets (`all`) where that is not `auto`, else its own (its
/// default): as [`frame`](super::frame) is.
fn chosen<'a>(env: &'a Env<'_>, widget: &str, key: &str, all: &str, auto: &str) -> &'a str {
    let own = format!("{widget}.{key}");
    let settings = env.settings();
    if !settings.is_chosen(&own) {
        let every = settings.text(all);
        if !every.is_empty() && every != auto {
            return every;
        }
    }
    settings.text(&own)
}

/// Tenths of a millimetre in pixels, by the style's gap (2 mm).
fn shape_px(style: &UiStyle, tenths: u32) -> u32 {
    (style.gap * tenths / 20).max(1)
}

/// The radius of `widget`'s corners in pixels, by its
/// [`corners_setting`], else by
/// [`WIDGET_CORNERS`](crate::builtin::settings::WIDGET_CORNERS); `None`
/// for the interface's.
pub fn corners(env: &Env<'_>, widget: &str) -> Option<u16> {
    let value = chosen(
        env,
        widget,
        "corners",
        WIDGET_CORNERS.static_str(),
        CORNERS_AUTO,
    );
    let style = UiStyle::new(env);
    // As the interface's corners are (`UiStyle::new`).
    let radius = match settings::CORNERS.iter().position(|c| *c == value)? {
        0 => 0,
        1 => shape_px(&style, 8),
        2 => shape_px(&style, 20),
        _ => shape_px(&style, 35),
    };
    Some(radius as u16)
}

/// The thickness of `widget`'s frame line in pixels, by its
/// [`frame_line_setting`], else by
/// [`WIDGET_FRAME_LINE`](crate::builtin::settings::WIDGET_FRAME_LINE);
/// `None` for the interface's.
pub fn frame_line(env: &Env<'_>, widget: &str) -> Option<u16> {
    let value = chosen(
        env,
        widget,
        "frame-line",
        WIDGET_FRAME_LINE.static_str(),
        LINE_AUTO,
    );
    let tenths = match value {
        LINE_THIN => 2,
        LINE_MEDIUM => 4,
        LINE_THICK => 8,
        _ => return None,
    };
    Some(shape_px(&UiStyle::new(env), tenths) as u16)
}

/// The shadow `widget`'s plate casts, by its [`shadow_setting`], else by
/// [`WIDGET_SHADOW`](crate::builtin::settings::WIDGET_SHADOW).
pub fn shadow(env: &Env<'_>, widget: &str) -> Shadow {
    match chosen(
        env,
        widget,
        "drop-shadow",
        WIDGET_SHADOW.static_str(),
        SHADOW_NONE,
    ) {
        SHADOW_HARD => Shadow::Hard,
        _ => Shadow::None,
    }
}

/// The gap between widgets in pixels, by
/// [`WIDGET_GAP`](crate::builtin::settings::WIDGET_GAP): what a screen
/// placing widgets gives its grid
/// ([`Geometry::of`](super::grid::Geometry::of)) or stack.
pub fn gap(env: &Env<'_>) -> u16 {
    let style = UiStyle::new(env);
    let gap = match env.settings().text(WIDGET_GAP.static_str()) {
        GAP_SMALL => style.gap / 2,
        GAP_LARGE => style.gap * 2,
        _ => style.gap,
    };
    gap as u16
}

/// The padding value of `widget` with that of all widgets
/// ([`WIDGET_PADDING`](crate::builtin::settings::WIDGET_PADDING)).
pub(crate) fn padding_value<'a>(env: &'a Env<'_>, widget: &str) -> &'a str {
    chosen(
        env,
        widget,
        "padding",
        WIDGET_PADDING.static_str(),
        "core.padding.auto",
    )
}

/// `style` with `widget`'s corners and frame line.
pub(crate) fn styled(env: &Env<'_>, widget: &str, style: UiStyle) -> UiStyle {
    let mut style = match corners(env, widget) {
        Some(radius) => style.with_corner(radius),
        None => style,
    };
    if let Some(line) = frame_line(env, widget) {
        style.line = u32::from(line);
    }
    style
}

/// How round `widget`'s plate is, in pixels, with `style` as [`styled`]
/// makes it: as its frame's corners; without a frame square, unless the
/// reader chose corners (for it or for all widgets).
pub(crate) fn plate_corner(env: &Env<'_>, widget: &str, style: &UiStyle, frame: Frame) -> u32 {
    match frame {
        Frame::None => corners(env, widget).map_or(0, u32::from),
        _ => style.corner,
    }
}

/// Where `widget`'s plate lies in its place `area` (less the room its
/// shadow takes) and how round it is: what lies under it fills that
/// shape alone (the grid's plate), so nothing shows past its corners.
pub(crate) fn plate_shape(env: &Env<'_>, widget: &str, area: Rect) -> (Rect, u32) {
    let style = styled(env, widget, UiStyle::new(env));
    let frame = super::frame(env, widget);
    let plate = plate_of(area, shadow_offset(env, widget, &style, frame));
    (plate, plate_corner(env, widget, &style, frame))
}

/// How far `widget`'s shadow falls, in pixels: none where it casts none
/// or nothing is drawn to cast it (no frame or only a line, and no paper
/// of its own).
pub(crate) fn shadow_offset(env: &Env<'_>, widget: &str, style: &UiStyle, frame: Frame) -> u16 {
    let opaque = matches!(frame, Frame::Card | Frame::Dark)
        || matches!(colours::paper(env, widget), ColourSpec::Is(_));
    if shadow(env, widget) == Shadow::Hard && opaque {
        shape_px(style, 10) as u16
    } else {
        0
    }
}

/// The colour of the shadow below a plate of `paper`: black, except below
/// a plate (nearly) as black, which a black shadow would not stand out
/// from: there dark gray (owner decision 106), drawn as grays are,
/// dithered where the panel has few. A dark gray plate keeps a black one.
pub(crate) fn shadow_colour(paper: Color) -> Color {
    let dark_gray = Tone::DarkGray.color();
    if paper.luma() < dark_gray.luma() / 2 {
        dark_gray
    } else {
        Color::BLACK
    }
}

/// `area` without the room its shadow takes, right and below.
pub(crate) fn plate_of(area: Rect, offset: u16) -> Rect {
    Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(offset),
        area.height.saturating_sub(offset),
    )
}

/// The setting of how round `widget`'s corners are, `<widget>.corners`;
/// [`corners`] reads it.
pub fn corners_setting(widget: &Id) -> SettingSpec {
    choice_setting(
        widget,
        "corners",
        "core.setting.corners",
        CORNER_CHOICES,
        CORNERS_AUTO,
    )
    .description("core.setting.corners.help")
}

/// [`corners_setting`] kept in the program: `id` must be
/// `<widget>.corners`.
pub const fn corners_spec(id: &'static str, widget: Id) -> SettingSpec {
    SettingSpec::choice(
        id,
        "core.setting.corners",
        CORNER_CHOICES,
        CORNERS_AUTO,
        widget,
    )
    .description("core.setting.corners.help")
}

/// The setting of how thick `widget`'s frame line is,
/// `<widget>.frame-line`; [`frame_line`] reads it.
pub fn frame_line_setting(widget: &Id) -> SettingSpec {
    choice_setting(
        widget,
        "frame-line",
        "core.setting.frame-line",
        LINE_CHOICES,
        LINE_AUTO,
    )
    .description("core.setting.frame-line.help")
}

/// [`frame_line_setting`] kept in the program: `id` must be
/// `<widget>.frame-line`.
pub const fn frame_line_spec(id: &'static str, widget: Id) -> SettingSpec {
    SettingSpec::choice(
        id,
        "core.setting.frame-line",
        LINE_CHOICES,
        LINE_AUTO,
        widget,
    )
    .description("core.setting.frame-line.help")
}

/// The setting of `widget`'s shadow, `<widget>.drop-shadow`; [`shadow`]
/// reads it.
pub fn shadow_setting(widget: &Id) -> SettingSpec {
    choice_setting(
        widget,
        "drop-shadow",
        "core.setting.shadow",
        SHADOWS,
        SHADOW_NONE,
    )
    .description("core.setting.shadow.help")
}

/// [`shadow_setting`] kept in the program: `id` must be
/// `<widget>.drop-shadow`.
pub const fn shadow_spec(id: &'static str, widget: Id) -> SettingSpec {
    SettingSpec::choice(id, "core.setting.shadow", SHADOWS, SHADOW_NONE, widget)
        .description("core.setting.shadow.help")
}

/// [`WIDGET_CORNERS`](crate::builtin::settings::WIDGET_CORNERS) in
/// `group` (the kernel's).
pub const fn all_corners_spec(group: Id) -> SettingSpec {
    SettingSpec::choice(
        WIDGET_CORNERS.static_str(),
        "core.setting.widget-corners",
        CORNER_CHOICES,
        CORNERS_AUTO,
        group,
    )
    .advanced()
    .description("core.setting.widget-corners.help")
}

/// [`WIDGET_FRAME_LINE`](crate::builtin::settings::WIDGET_FRAME_LINE) in
/// `group` (the kernel's).
pub const fn all_frame_line_spec(group: Id) -> SettingSpec {
    SettingSpec::choice(
        WIDGET_FRAME_LINE.static_str(),
        "core.setting.widget-frame-line",
        LINE_CHOICES,
        LINE_AUTO,
        group,
    )
    .advanced()
    .description("core.setting.widget-frame-line.help")
}

/// [`WIDGET_PADDING`](crate::builtin::settings::WIDGET_PADDING) in
/// `group` (the kernel's): one of [`PADDINGS`](super::PADDINGS),
/// automatic leaving each widget its own.
pub const fn all_padding_spec(group: Id) -> SettingSpec {
    SettingSpec::choice(
        WIDGET_PADDING.static_str(),
        "core.setting.widget-padding",
        super::PADDINGS,
        "core.padding.auto",
        group,
    )
    .advanced()
    .description("core.setting.widget-padding.help")
}

/// [`WIDGET_SHADOW`](crate::builtin::settings::WIDGET_SHADOW) in `group`
/// (the kernel's).
pub const fn all_shadow_spec(group: Id) -> SettingSpec {
    SettingSpec::choice(
        WIDGET_SHADOW.static_str(),
        "core.setting.widget-shadow",
        SHADOWS,
        SHADOW_NONE,
        group,
    )
    .advanced()
    .description("core.setting.widget-shadow.help")
}

/// [`WIDGET_GAP`](crate::builtin::settings::WIDGET_GAP) in `group` (the
/// kernel's).
pub const fn all_gap_spec(group: Id) -> SettingSpec {
    SettingSpec::choice(
        WIDGET_GAP.static_str(),
        "core.setting.widget-gap",
        GAPS,
        GAP_NORMAL,
        group,
    )
    .advanced()
    .description("core.setting.widget-gap.help")
}
