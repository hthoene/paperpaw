//! Drawing shared by the widgets of the pack.
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::{
    Paint,
    widget::{Align, Look, text::Wrapped},
};

/// Says in `look`'s area that the time is not known yet, in place of what a
/// widget would show of it: on a device without a clock of its own, until
/// Wi-Fi sets it.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn no_time(cx: &mut Cx<'_>, out: &mut Vec<Paint>, look: &Look) -> Result<(), Error> {
    let message = cx.tr("widgets.time.unknown")?;
    message_in(cx, out, look, &message)
}

/// `text` centred and fitted in `look`'s area, in its ink.
///
/// # Errors
/// Font refusals, [`Error::NoMemory`].
pub fn message_in(cx: &Cx<'_>, out: &mut Vec<Paint>, look: &Look, text: &str) -> Result<(), Error> {
    let area = look.area;
    let style = cx.style();
    let wrapped = Wrapped::fitting(
        &cx.font,
        text,
        (style.text, style.small),
        (area.width, area.height),
        false,
    )?;
    wrapped.draw(out, &cx.font, area, Align::Center, look.ink)?;
    Ok(())
}
