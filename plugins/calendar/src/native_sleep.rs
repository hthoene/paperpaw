//! The sleep screen: today and the days after it, as large as the screen
//! lets them be. The page is drawn in grays at once (no flashing between
//! them): the same painter as the *Agenda* widget, given the whole screen.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    native::{Error, Host, callbacks::DrawFrame},
};
use paperpaw_ui_kit::{geometry::clamp_u16, paint, widget::Cx};

use crate::declarations::AGENDA_TOKEN;
use crate::model::Store;
use crate::native_widgets::{self as widgets, Env};

/// The widget whose settings and look the sleep screen takes.
const WIDGET: &str = "calendar.agenda";

/// Draws the sleep screen.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    store: &Store,
    env: &Env,
) -> Result<(), Error> {
    let area = frame.rect;
    frame.draw_canvas()?.fill_rectangle(area, Color::WHITE)?;
    let rect = Rect::new(
        clamp_u16(i64::from(area.x)),
        clamp_u16(i64::from(area.y)),
        clamp_u16(i64::from(area.width)),
        clamp_u16(i64::from(area.height)),
    );
    let mut cx = Cx::open(host, WIDGET)?;
    let mut out = Vec::new();
    let env = Env { full: true, ..*env };
    widgets::draw(&mut cx, &mut out, rect, AGENDA_TOKEN, (store, &env))?;
    paint::replay(frame, &cx.font, &out)
}
