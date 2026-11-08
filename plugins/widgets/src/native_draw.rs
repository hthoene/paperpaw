//! Drawing shared by the widgets of the pack.
use alloc::vec::Vec;
use paperpaw_sdk::{
    native::{Error, Host},
    time::LocalTime,
};
use paperpaw_ui_kit::widget::{Cx, cx::local_time_now};
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

/// What a widget that shows the date shows, as a number that changes with the
/// day (0 where the time is not known): it needs no drawing context.
///
/// # Errors
/// Host refusals.
pub fn today(host: &Host<'_>) -> Result<u64, Error> {
    Ok(local_time_now(host)?.map_or(0, |t| day_mark(&t)))
}

/// What a widget that shows the time shows, as a number that changes with the
/// minute (0 where the time is not known).
///
/// # Errors
/// Host refusals.
pub fn minute(host: &Host<'_>) -> Result<u64, Error> {
    Ok(local_time_now(host)?.map_or(0, |t| {
        day_mark(&t) * 10_000 + u64::from(t.hour) * 100 + u64::from(t.minute)
    }))
}

/// A number for the date of `t`, never 0.
#[must_use]
pub fn day_mark(t: &LocalTime) -> u64 {
    u64::from(t.year.unsigned_abs()) * 10_000 + u64::from(t.month) * 100 + u64::from(t.day) + 1
}

/// `hash` taken on over `bytes` (FNV-1a): a number that changes when what it
/// was made of does, to tell a change without keeping the text.
#[must_use]
pub fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash ^ 0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}
