//! What a group's settings do, shown above them: a widget another plugin made to
//! preview the group (the reader's sample of text for the reading settings), in
//! the room the list keeps for it (`ItemList::set_head`), as Beta 3 showed it.
//!
//! The host makes, measures and composes another plugin's widget; this screen
//! asks for each at the draw, and lets it go at its end. A host that refuses
//! (the X4 until its nested-call gate is open, #250) leaves the page without the
//! preview: nothing is reserved for it.
use super::GroupView;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::DrawFrame,
    snapshot,
    ui::{Compose, MadeWidget, Ui, WidgetHandle},
};
use paperpaw_ui_kit::{
    Footer,
    mem::{SERVICE_BUFFER, buffer},
    paint,
};

/// Where the widget is made: the place the preview shows in.
const PLACE: &str = "settings.preview";
/// The most of the screen the preview takes: a third.
const SHARE: u32 = 3;

/// A widget made for the page, and the room it asked for.
struct Made {
    widget: WidgetHandle,
    height: u16,
}

/// Makes the widget `id` and asks how high it wants to be at `width` pixels, at
/// most `most`.
fn make(host: &Host<'_>, ui: &Ui, id: &str, width: u32, most: u32) -> Result<Made, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let widget = match ui.widget_make(host, id, PLACE, &mut input, &mut out)? {
        MadeWidget::Widget(widget) => widget,
        _ => return Err(Error::NotFound),
    };
    let height = ui
        .widget_measure(host, widget, width, &mut input, &mut out)
        .map(|size| size.height().min(most));
    match height {
        Ok(height) => Ok(Made {
            widget,
            height: u16::try_from(height).unwrap_or(u16::MAX),
        }),
        Err(error) => {
            ui.widget_drop(host, widget, &mut input, &mut out).ok();
            Err(error)
        }
    }
}

/// Lets the widget go; a refusal is of no matter (the host drops it when the
/// screen closes).
fn release(host: &Host<'_>, ui: &Ui, widget: WidgetHandle) {
    if let (Ok(mut input), Ok(mut out)) = (buffer(SERVICE_BUFFER), buffer(64)) {
        ui.widget_drop(host, widget, &mut input, &mut out).ok();
    }
}

/// Draws the group's page: the list, with the preview in the room it keeps.
/// Without a preview widget, or when the host refuses one, the plain list.
///
/// # Errors
/// What drawing the list refuses; a refused preview is not one.
pub(super) fn draw(
    host: &Host<'_>,
    ui: &Ui,
    view: &mut GroupView,
    frame: &mut DrawFrame<'_>,
    title: &str,
) -> Result<(), Error> {
    let mut made = None;
    if let Some(id) = view.preview.as_deref().filter(|_| !view.preview_refused) {
        let display = snapshot::capture_snapshot(host)?;
        let most = u32::from(display.display_height()) / SHARE;
        match make(host, ui, id, u32::from(display.display_width()), most) {
            Ok(m) => made = Some(m),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Refused: the page goes without it from now on.
            Err(_) => view.preview_refused = true,
        }
    }
    view.list
        .set_head_height(made.as_ref().map_or(0, |m| m.height));
    let drawn = view.list.draw_with(host, frame, title, true, Footer::None);
    let Some(made) = made else {
        return drawn;
    };
    let composed = match (drawn.as_ref(), view.list.head_rect()) {
        (Ok(()), Some(room)) => {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(64)?);
            let at = paint::frame_rect(room);
            ui.widget_place(host, made.widget, at, &mut input, &mut out)
                .and_then(|()| Ui::compose(host, Compose::Widget(made.widget), at))
        }
        _ => Ok(()),
    };
    release(host, ui, made.widget);
    if let Err(error) = composed {
        if error == Error::NoMemory {
            return Err(error);
        }
        // Refused at the draw: this frame keeps the room, the next has none.
        view.preview_refused = true;
    }
    drawn
}
