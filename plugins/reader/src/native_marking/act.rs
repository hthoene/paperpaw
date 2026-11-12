//! What the bar's buttons do with the chosen words: copy them, look them up.
use alloc::string::String;
use paperpaw_sdk::{
    document::Locator,
    native::{
        Error, Host,
        commands::CommandSender,
        ui::{Open, Param, TextWrite, Ui},
    },
    reading::WordBox,
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

use super::Ctx;
use crate::native_bookmark::{percent_of, say_not_saved};
use crate::native_marks::{Span, edit};
use crate::native_place::wall_seconds;
use crate::native_words::{self, Looked};

/// Most words of a sentence on either side of a looked-up word.
pub const SENTENCE_WORDS: usize = 30;

/// Longest a text copied is: more would not be one a person chose by touch.
pub const MAX_COPY: usize = 16 * 1024;

/// A part of a copied text, in bytes: well within one request.
const PART: usize = 1_000;

/// Whether `word` ends a sentence (closing quotes and brackets after its
/// punctuation are part of it).
fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', '»', '«', '›', '‹', ')', ']', '“'])
        .ends_with(['.', '!', '?', '…'])
}

/// The text of the page's words `lo..=hi`, a space between them.
pub fn text_of(words: &[WordBox], lo: usize, hi: usize) -> Result<String, Error> {
    let mut text = String::new();
    for word in words.get(lo..=hi).unwrap_or(&[]) {
        let room = word.text.len() + 1;
        text.try_reserve(room).map_err(|_| Error::NoMemory)?;
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word.text);
        if text.len() > MAX_COPY {
            break;
        }
    }
    Ok(text)
}

/// The sentence around words `lo..=hi` of a page, as far as the page has it.
pub fn sentence(words: &[WordBox], lo: usize, hi: usize) -> Result<String, Error> {
    let ends = |w: &WordBox| ends_sentence(&w.text);
    let mut first = lo;
    while first > 0 && lo - first < SENTENCE_WORDS && !ends(&words[first - 1]) {
        first -= 1;
    }
    let mut last = hi;
    while last + 1 < words.len() && last - hi < SENTENCE_WORDS && !ends(&words[last]) {
        last += 1;
    }
    text_of(words, first, last)
}

/// Copies `text` to the clipboard, in parts.
pub fn copy(host: &Host<'_>, ui: &Ui, text: &str) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut offset = 0usize;
    loop {
        let mut end = (offset + PART).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let last = end >= text.len();
        let part = text.get(offset..end).unwrap_or("");
        ui.clipboard_write(
            host,
            u32::try_from(offset).map_err(|_| Error::NoSpace)?,
            part,
            if last {
                TextWrite::Last
            } else {
                TextWrite::Continue
            },
            &mut input,
            &mut out,
        )?;
        if last {
            return Ok(());
        }
        offset = end;
    }
}

/// Looks the words up: opens the lookup (`core.lookup`) with the phrase and the
/// book's language, and runs the stage `reader.word-looked-up` so a vocabulary
/// list takes the word with its sentence. Words that are only punctuation look
/// up nothing. The stage's refusals do not concern the lookup.
pub fn look_up(
    host: &Host<'_>,
    ctx: &Ctx<'_>,
    lo: usize,
    hi: usize,
    (start, _end): (Locator, Locator),
) -> Result<(), Error> {
    let text = text_of(ctx.words, lo, hi)?;
    let phrase = text.trim_matches(|c: char| !c.is_alphanumeric());
    if phrase.is_empty() {
        return Ok(());
    }
    let params = [
        Param::new("word", phrase)?,
        Param::new("language", ctx.language)?,
    ];
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let ui = ctx.ui.ok_or(Error::State)?;
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.lookup")?.handover(handover))?;
    let sentence = sentence(ctx.words, lo, hi)?;
    // A refused run does not stop the lookup.
    native_words::run(
        host,
        &Looked {
            word: phrase,
            sentence: &sentence,
            language: ctx.language,
            path: ctx.path,
            title: ctx.title,
            section: start.section,
            offset: start.offset,
        },
    )
    .ok();
    Ok(())
}

/// The note *Note* starts: where it goes, and the note there is to edit.
pub type NoteAsk = edit::Target;

/// The stretch of the book the words `lo..=hi` of the page are.
fn span_of(ctx: &Ctx<'_>, (lo, hi): (usize, usize)) -> Option<Span> {
    let (first, last) = (ctx.words.get(lo)?, ctx.words.get(hi)?);
    Some(Span {
        start: (first.start.section, first.start.offset),
        end: (last.end.section, last.end.offset),
        noted: false,
    })
}

/// How the chosen words `lo..=hi` stand with the highlights the bar names its
/// items by: `None` where none overlaps, else whether the first has a note.
#[must_use]
pub fn standing(ctx: &Ctx<'_>, range: (usize, usize)) -> Option<bool> {
    let span = span_of(ctx, range)?;
    ctx.book?
        .highlights
        .iter()
        .find(|h| edit::overlaps(h, &span))
        .map(|h| h.noted)
}

/// What a refused change is: said (by why it was refused), the page goes on.
/// Only a lack of memory is the host's to hear.
fn said<T>(host: &Host<'_>, done: Result<T, Error>) -> Result<Option<T>, Error> {
    match done {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(error) => {
            say_not_saved(host, error);
            Ok(None)
        }
    }
}

/// *Highlight* / *Remove highlight* on the words `lo..=hi`.
///
/// # Errors
/// `NoMemory`; a change the library refuses is said.
pub fn highlight(host: &Host<'_>, ctx: &Ctx<'_>, range: (usize, usize)) -> Result<(), Error> {
    let done = with_new(host, ctx, range, |library, new| {
        edit::toggle(host, library, ctx.path, new)
    });
    said(host, done).map(|_| ())
}

/// *Note* / *Edit note* on the words `lo..=hi`: the highlight there is made if
/// there is none, and the note it has is given back to be edited.
///
/// # Errors
/// `NoMemory`; a change the library refuses is said.
pub fn note(
    host: &Host<'_>,
    ctx: &Ctx<'_>,
    range: (usize, usize),
) -> Result<Option<NoteAsk>, Error> {
    let done = with_new(host, ctx, range, |library, new| {
        edit::begin_note(host, library, ctx.path, new)
    });
    said(host, done)
}

/// `change` with the library and the highlight the words `lo..=hi` would be.
fn with_new<T>(
    host: &Host<'_>,
    ctx: &Ctx<'_>,
    range: (usize, usize),
    change: impl FnOnce(&paperpaw_sdk::native::library::Library, &edit::New<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let (Some(book), Some(span)) = (ctx.book, span_of(ctx, range)) else {
        return Err(Error::State);
    };
    let library = book.library.ok_or(Error::State)?;
    let text = edit::snippet(&text_of(ctx.words, range.0, range.1)?)?;
    let ppm = book
        .progress
        .map_or(book.stored_ppm, |p| p.ppm(span.start.0, span.start.1));
    change(
        library,
        &edit::New {
            span,
            percent: percent_of(ppm),
            conversion: book.conversion,
            now: wall_seconds(host),
            text: &text,
        },
    )
}
