//! Drawing shared by the widgets of the pack.
use crate::native_cx::Cx;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
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

/// The lighter of the look's ink and paper, and the darker one: e.g. the lit
/// and the dark part of the moon.
#[must_use]
pub fn light_and_dark(look: &Look) -> (paperpaw_sdk::Color, paperpaw_sdk::Color) {
    if look.ink.luma() > look.paper.luma() {
        (look.ink, look.paper)
    } else {
        (look.paper, look.ink)
    }
}

/// `hour` (0 to 23) on a twelve-hour clock: 1 to 12.
#[must_use]
pub const fn twelve(hour: u8) -> u8 {
    match hour % 12 {
        0 => 12,
        h => h,
    }
}

/// What a widget that shows the date shows: drawn again when the day changes.
///
/// # Errors
/// Host refusals.
pub fn today(cx: &Cx<'_>) -> Result<alloc::string::String, Error> {
    Ok(match cx.local_time()? {
        Some(t) => alloc::format!("{}-{}-{}", t.year, t.month, t.day),
        None => alloc::string::String::new(),
    })
}

/// What a widget that shows the time shows: drawn again each minute.
///
/// # Errors
/// Host refusals.
pub fn minute(cx: &Cx<'_>) -> Result<alloc::string::String, Error> {
    Ok(match cx.local_time()? {
        Some(t) => alloc::format!("{}-{}-{} {}:{}", t.year, t.month, t.day, t.hour, t.minute),
        None => alloc::string::String::new(),
    })
}
