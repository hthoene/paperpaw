//! The status bar: the status items (the time, Wi-Fi, the battery, any
//! plugin's) on a plate or on the picture itself, along the top or the
//! bottom, as wide as the screen or a compact pill at the left, in the
//! middle or at the right. It shows always, never, or for a few seconds
//! after a press ([`Visible`]).

use alloc::string::ToString;
use alloc::vec::Vec;

use paperpaw_sdk::status::Place;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{WidgetTarget, line_height};
use paperpaw_sdk::{Canvas, Color, Env, Id, Params, Rect, StatusFrame};

use crate::settings;

/// The bar's items, as a place of status items (the settings that list
/// them).
pub(crate) const PLACE: Place = Place::new(
    Id::new_static("picture-home.bar"),
    "picture-home.setting.bar",
    [
        Id::new_static(settings::BAR_LEFT.static_str()),
        Id::new_static(settings::BAR_CENTER.static_str()),
        Id::new_static(settings::BAR_RIGHT.static_str()),
    ],
);

/// When a bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visible {
    Off,
    /// After a press, for a few seconds.
    Auto,
    On,
}

impl Visible {
    pub(crate) fn of(value: &str) -> Self {
        match settings::BARS.iter().position(|b| *b == value) {
            Some(0) => Visible::Off,
            Some(1) => Visible::Auto,
            _ => Visible::On,
        }
    }
}

/// What the bar is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
    /// A plate as light as the page.
    Normal,
    /// A black plate.
    Inverted,
    /// A light gray plate (dithered where the panel has few grays).
    Light,
    /// A dark gray plate.
    Dark,
    /// No plate: the items on the picture, in the ink that reads on it.
    Clear,
}

impl Look {
    pub(crate) fn of(value: &str) -> Self {
        match settings::BAR_LOOKS.iter().position(|l| *l == value) {
            Some(1) => Look::Inverted,
            Some(2) => Look::Light,
            Some(3) => Look::Dark,
            Some(4) => Look::Clear,
            _ => Look::Normal,
        }
    }

    /// The plate's gray and the ink on it; no plate for [`Look::Clear`].
    fn plate(self) -> Option<(Color, Color)> {
        match self {
            Look::Normal => Some((Color::WHITE, Color::BLACK)),
            Look::Inverted => Some((Color::BLACK, Color::WHITE)),
            Look::Light => Some((Color::gray(165), Color::BLACK)),
            Look::Dark => Some((Color::gray(85), Color::WHITE)),
            Look::Clear => None,
        }
    }
}

/// Where the bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placing {
    pub(crate) bottom: bool,
    pub(crate) compact: bool,
    /// 0 left, 1 middle, 2 right.
    pub(crate) align: u8,
}

impl Placing {
    pub(crate) fn of(env: &Env<'_>) -> Self {
        let text = |s: &paperpaw_sdk::setting::Setting<paperpaw_sdk::setting::Text>| {
            env.settings().text(s.as_str()).to_string()
        };
        Self {
            bottom: text(&settings::BAR_AT) == settings::BAR_ATS[1],
            compact: text(&settings::BAR_SHAPE) == settings::BAR_SHAPES[1],
            align: settings::BAR_ALIGNS
                .iter()
                .position(|a| *a == text(&settings::BAR_ALIGN))
                .unwrap_or(2) as u8,
        }
    }
}

/// The height of the bar's strip: a line of small text with room round it.
pub(crate) fn height(env: &Env<'_>) -> u16 {
    let style = UiStyle::new(env);
    line_height(env, style.small()) + style.gap()
}

/// Where the bar's plate is in a screen `width` wide, its strip being
/// `strip` (the whole width of it, at the top or the bottom).
pub(crate) fn rect(env: &Env<'_>, placing: Placing, strip: Rect) -> Rect {
    let style = UiStyle::new(env);
    let margin = style.margin().max(style.gap() / 2);
    let full = Rect::new(
        strip.x + margin,
        strip.y,
        strip.width.saturating_sub(2 * margin),
        strip.height,
    );
    if !placing.compact {
        return full;
    }
    let width = (full.width * 55 / 100).min(full.width);
    let x = match placing.align {
        0 => full.x,
        1 => full.x + (full.width - width) / 2,
        _ => full.x + full.width - width,
    };
    Rect::new(x, full.y, width, full.height)
}

/// The status items in `area`, in `ink`.
fn frame<'a>(env: &Env<'_>, area: Rect, ink: Color, context: &'a Params) -> StatusFrame<'a> {
    let style = UiStyle::new(env);
    StatusFrame::new(area, true, style.small(), ink, context)
        .font(paperpaw_sdk::builtin::fonts::UI.static_str())
}

/// The items' room inside the plate.
fn inner(env: &Env<'_>, plate: Rect) -> Rect {
    let pad = UiStyle::new(env).gap();
    Rect::new(
        plate.x + pad,
        plate.y,
        plate.width.saturating_sub(2 * pad),
        plate.height,
    )
}

/// Draws the bar on its plate; where it has none, in the ink that reads
/// over a picture `brightness` bright there (dark text where it is not
/// known).
pub(crate) fn draw(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    plate: Rect,
    look: Look,
    brightness: Option<u8>,
) {
    let style = UiStyle::new(env);
    let ink = match look.plate() {
        Some((paper, ink)) => {
            canvas.fill_round_rect(
                i32::from(plate.x),
                i32::from(plate.y),
                u32::from(plate.width),
                u32::from(plate.height),
                u32::from(style.corner()),
                paper,
            );
            ink
        }
        None => ink_over(brightness),
    };
    let context = Params::new();
    PLACE.draw(canvas, env, &frame(env, inner(env, plate), ink, &context));
}

/// The ink that reads over a picture `brightness` bright.
pub(crate) fn ink_over(brightness: Option<u8>) -> Color {
    if brightness.is_some_and(|b| b < 128) {
        Color::WHITE
    } else {
        Color::BLACK
    }
}

/// Where a tap on the bar does something (the items that do something).
pub(crate) fn targets(env: &Env<'_>, plate: Rect) -> Vec<WidgetTarget> {
    let context = Params::new();
    PLACE
        .targets(env, &frame(env, inner(env, plate), Color::BLACK, &context))
        .into_iter()
        .map(|t| WidgetTarget::touch(t.area, t.command))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_and_visibility_are_read_by_their_values() {
        assert_eq!(Visible::of(settings::BARS[0]), Visible::Off);
        assert_eq!(Visible::of(settings::BARS[1]), Visible::Auto);
        assert_eq!(Visible::of("anything"), Visible::On);
        for (i, look) in [
            Look::Normal,
            Look::Inverted,
            Look::Light,
            Look::Dark,
            Look::Clear,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(Look::of(settings::BAR_LOOKS[i]), look);
        }
    }

    #[test]
    fn plates_have_ink_that_reads_on_them() {
        for look in [Look::Normal, Look::Inverted, Look::Light, Look::Dark] {
            let (paper, ink) = look.plate().unwrap();
            let gap = i32::from(paper.luma()) - i32::from(ink.luma());
            assert!(gap.abs() > 120, "{look:?}");
        }
        assert!(Look::Clear.plate().is_none());
    }

    #[test]
    fn clear_ink_follows_the_picture() {
        assert_eq!(ink_over(Some(40)), Color::WHITE);
        assert_eq!(ink_over(Some(200)), Color::BLACK);
        assert_eq!(ink_over(None), Color::BLACK);
    }
}
