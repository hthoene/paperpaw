//! What each widget of the pack does, by the token it was declared with.
use crate::native_cx::Cx;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::Paint;

/// The behaviour of one widget.
#[derive(Debug, Clone, Copy)]
pub struct Behaviour {
    /// The natural height at a width; 0 shows nothing.
    pub height: fn(&mut Cx<'_>, u16) -> Result<u16, Error>,
    /// Lays the widget out in a rectangle.
    pub draw: fn(&mut Cx<'_>, &mut Vec<Paint>, Rect) -> Result<(), Error>,
    /// What it shows now, to tell whether it changed.
    pub shows: fn(&Cx<'_>) -> Result<String, Error>,
}

/// The behaviour of the widget declared with `token`.
#[must_use]
pub fn behaviour(token: u32) -> Option<&'static Behaviour> {
    static DAY: Behaviour = Behaviour {
        height: crate::native_day::height,
        draw: crate::native_day::draw,
        shows: crate::native_day::shows,
    };
    static TILES: Behaviour = Behaviour {
        height: crate::native_tiles::height,
        draw: crate::native_tiles::draw,
        shows: crate::native_tiles::shows,
    };
    static CALENDAR: Behaviour = Behaviour {
        height: crate::native_calendar::height,
        draw: crate::native_calendar::draw,
        shows: crate::native_calendar::shows,
    };
    match token {
        102 => Some(&CALENDAR),
        100 => Some(&DAY),
        101 => Some(&TILES),
        _ => None,
    }
}

/// The token of the widget the host calls `widget` (`widgets.day`, or placed
/// `widgets.day@2`).
#[must_use]
pub fn token_of(widget: &str) -> Option<u32> {
    let base = paperpaw_ui_kit::widget::settings::base(widget);
    let name = base.strip_prefix("widgets.")?;
    crate::declarations::WIDGETS
        .iter()
        .find(|def| def.name == name)
        .map(|def| def.token)
}
