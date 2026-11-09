//! What the books widgets share: the library service for the length of a
//! callback, and the caption above a widget.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    native::{Error, Host, fonts::TextOrigin, library::Library},
};
use paperpaw_ui_kit::widget::{
    Cx, Look,
    layout::{self, caption_height},
};
use paperpaw_ui_kit::{Paint, mem, paint, text};

/// The size of each buffer handed to the library: the most a reply holds.
pub const BUFFER: usize = mem::SERVICE_BUFFER;

/// Runs `work` with the library service open and releases it whatever
/// `work` ends in; a failure of `work` is reported before one of the release.
///
/// # Errors
/// The host's refusals, whatever `work` returns.
pub fn with_library<T>(
    host: &Host<'_>,
    work: impl FnOnce(&Library) -> Result<T, Error>,
) -> Result<T, Error> {
    let library = Library::open(host)?;
    let done = work(&library);
    let released = library.release(host);
    let done = done?;
    released?;
    Ok(done)
}

/// A pair of buffers for the calls of the library.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn buffers() -> Result<(Vec<u8>, Vec<u8>), Error> {
    Ok((mem::buffer(BUFFER)?, mem::buffer(BUFFER)?))
}

/// The caption of the widget (the translation `key`), drawn at the top of its
/// area if the widget shows one and there is room for it (three times its
/// height); the area left below it, else all of the area.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn caption(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    key: &str,
) -> Result<Rect, Error> {
    if !cx.settings.shows("caption")? {
        return Ok(look.area);
    }
    let (small, gap) = (cx.style().small, cx.style().gap);
    if look.area.height < caption_height(&cx.font, small, gap)?.saturating_mul(3) {
        return Ok(look.area);
    }
    let text = cx.tr(key)?;
    let align = cx.settings.align()?;
    layout::caption(
        out,
        &cx.font,
        (small, gap),
        (look.area, align, look.faint),
        &text,
    )
}

/// Pushes the text `shown` at `size`, from `x` on `baseline`, in `color`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn push_text(
    out: &mut Vec<Paint>,
    shown: &str,
    (size, x, baseline): (f32, f32, i32),
    color: Color,
) -> Result<(), Error> {
    paint::push(
        out,
        Paint::Text {
            text: text::copy(shown)?,
            size,
            origin: TextOrigin::new(x, baseline),
            color,
            clip: None,
        },
    )
}
