//! Small helpers of the reading screen: a redraw, a refusal of the layout as a
//! host error, a message over the whole screen.

use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    document::DocumentError,
    native::{
        Error, Host,
        callbacks::DrawFrame,
        commands::{CommandSender, RedrawMode},
        document::{DocId, Documents},
        fonts::{Font, FontRole},
        snapshot,
    },
};
use paperpaw_ui_kit::{Messages, Style as UiStyle, mem::push, paint, text};

pub(super) fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(mode)
}

/// What a layout's refusal is as a host error.
pub(super) fn document_error_kind(error: &DocumentError) -> Option<Error> {
    match error {
        DocumentError::OutOfMemory => Some(Error::NoMemory),
        DocumentError::NotReady => Some(Error::Busy),
        _ => None,
    }
}

/// Says why there is no page: the text over the whole screen.
pub(super) fn say(host: &Host<'_>, frame: &mut DrawFrame<'_>, key: &str) -> Result<(), Error> {
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let said = say_with(host, frame, &font, (key, None));
    let released = font.release();
    said.and(released)
}

fn say_with(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    (key, strip): (&str, Option<u16>),
) -> Result<(), Error> {
    let display = snapshot::capture_snapshot(host)?;
    let style = UiStyle::read(host, &display, font)?;
    // The whole screen, or a strip at its bottom.
    let (width, height) = (display.display_width(), display.display_height());
    let high = strip.map_or(height, |strip| strip.min(height));
    let area = paperpaw_sdk::Rect::new(0, height - high, width, high);
    let mut messages = Messages::new()?;
    let shown = messages.copy(host, key)?;
    let mut body = Vec::new();
    push(&mut body, paint::Paint::Fill(area, style.paper))?;
    text::message(&mut body, font, &style, area, &shown)?;
    paint::replay(frame, font, body.iter())
}

/// A short note over the bottom `strip` rows of the screen, the rest left as it is.
pub(super) fn note(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    key: &str,
    strip: u16,
) -> Result<(), Error> {
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let said = say_with(host, frame, &font, (key, Some(strip)));
    let released = font.release();
    said.and(released)
}

/// The language a book is read in: its own, or the UI language where the
/// format names none (a text file), as in Beta 3, so the voice reading aloud
/// is the reader's. Empty only where the UI language cannot be read either
/// (the host's voice then).
///
/// # Errors
/// `NoMemory`.
pub(super) fn book_language(host: &Host<'_>, declared: &str) -> Result<String, Error> {
    if !declared.is_empty() {
        return text::copy(declared);
    }
    let mut scratch = [0; 64];
    text::copy(paperpaw_sdk::native::i18n::language(host, &mut scratch).unwrap_or(""))
}

/// What the format tells of a book, as the screen keeps it.
pub(super) struct BookFacts {
    pub(super) sections: u32,
    /// Its title; the path where the format gives none.
    pub(super) title: String,
    /// Its language (a BCP 47 tag), the UI's where the format names none.
    pub(super) language: String,
    /// The script its language is written in, where a pack of letters is offered.
    pub(super) script: Option<&'static str>,
    pub(super) rtl: bool,
}

/// The facts of document `id` (the book at `path`); none where the host cannot
/// tell them (the screen says so).
///
/// # Errors
/// `NoMemory`.
#[inline(never)]
pub(super) fn book_facts(
    host: &Host<'_>,
    docs: &Documents,
    id: DocId,
    path: &str,
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<Option<BookFacts>, Error> {
    match docs.metadata(host, id, input, out) {
        Ok(meta) => Ok(Some(BookFacts {
            sections: meta.sections().max(1),
            title: text::copy(crate::native_place::title_of(meta.title(), path))?,
            language: book_language(host, meta.language())?,
            script: crate::native_script_hint::declared(meta.language()),
            rtl: meta.right_to_left(),
        })),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}
