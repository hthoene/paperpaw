//! What each widget of the pack does, by the token it was declared with.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{Error, Host},
};
use paperpaw_ui_kit::Paint;
use paperpaw_ui_kit::widget::Cx;

/// The behaviour of one widget.
#[derive(Debug, Clone, Copy)]
pub struct Behaviour {
    /// The natural height at a width; 0 shows nothing.
    pub height: fn(&mut Cx<'_>, u16) -> Result<u16, Error>,
    /// Lays the widget out in a rectangle.
    pub draw: fn(&mut Cx<'_>, &mut Vec<Paint>, Rect) -> Result<(), Error>,
    /// A number that changes when what it shows changes (0: nothing shown); it
    /// needs neither font nor look, so no drawing context is opened for it.
    pub shows: fn(&Host<'_>, &str) -> Result<u64, Error>,
}

/// The behaviour of the widget declared with `token`.
#[must_use]
pub fn behaviour(token: u32) -> Option<&'static Behaviour> {
    static DAY: Behaviour = Behaviour {
        height: crate::native_day::height,
        draw: crate::native_day::draw,
        shows: crate::native_day::shows,
    };
    match token {
        100 => Some(&DAY),
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
