//! What the bar's buttons do with the chosen words: copy them, look them up.
use alloc::string::String;
use paperpaw_sdk::{
    Rect,
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
pub fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', '»', '«', '›', '‹', ')', ']', '“'])
        .ends_with(['.', '!', '?', '…'])
}

/// The text of the page's words `lo..=hi`, a space between them.
pub fn text_of(words: &[WordBox], lo: usize, hi: usize) -> Result<String, Error> {
    let mut text = String::new();
    for (at, word) in words.iter().enumerate().take(hi + 1).skip(lo) {
        // The second part of a word the layout broke says the word again.
        if at > lo && crate::native_selection::continues(words, at) {
            continue;
        }
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
    let mut place = [0; ANCHOR_BYTES];
    let anchor = anchor(ctx.words, lo, hi).map_or("", |r| anchor_text(r, &mut place));
    let params = [
        Param::new("word", phrase)?,
        Param::new("language", ctx.language)?,
        Param::new("anchor", anchor)?,
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

/// The most bytes of an `anchor` hand-over param: four u16 and three commas.
const ANCHOR_BYTES: usize = 24;

/// The box the words `lo..=hi` take in the first line they are in: where a
/// lookup's window goes next to them (the hand-over param `anchor`).
#[must_use]
pub fn anchor(words: &[WordBox], lo: usize, hi: usize) -> Option<Rect> {
    let first = words.get(lo)?.area;
    let line = words.iter().take(hi + 1).skip(lo).map(|w| w.area);
    let same = line.filter(|a| a.y == first.y && a.height == first.height);
    let (left, right) = same.fold((first.x, first.x), |(l, r), a| {
        (l.min(a.x), r.max(a.x.saturating_add(a.width)))
    });
    Some(Rect::new(left, first.y, right - left, first.height))
}

/// `area` as the param `anchor` says it: `x,y,width,height` in the screen's
/// pixels, written into `out`.
pub fn anchor_text(area: Rect, out: &mut [u8; ANCHOR_BYTES]) -> &str {
    let mut len = 0;
    for (n, value) in [area.x, area.y, area.width, area.height]
        .into_iter()
        .enumerate()
    {
        if n > 0 {
            out[len] = b',';
            len += 1;
        }
        let mut digits = [0_u8; 5];
        let (mut v, mut d) = (value, 0);
        loop {
            digits[d] = b'0' + (v % 10) as u8;
            (v, d) = (v / 10, d + 1);
            if v == 0 {
                break;
            }
        }
        for digit in digits[..d].iter().rev() {
            out[len] = *digit;
            len += 1;
        }
    }
    core::str::from_utf8(&out[..len]).unwrap_or_default()
}

/// The note *Note* starts: where it goes, and the note there is to edit.
pub type NoteAsk = edit::Target;

/// The stretch of the book from `start` to `end` (by place).
#[must_use]
pub const fn span_of((start, end): ((u32, u32), (u32, u32))) -> Span {
    Span {
        start,
        end,
        noted: false,
    }
}

/// How the chosen stretch `span` stands with the highlights the bar names its
/// items by: `None` where none overlaps, else whether the first has a note.
#[must_use]
pub fn standing(ctx: &Ctx<'_>, span: &Span) -> Option<bool> {
    ctx.book?
        .highlights
        .iter()
        .find(|h| edit::overlaps(h, span))
        .map(|h| h.noted)
}

/// What a refused change is: said (it could not be saved), the page goes on.
/// Only a lack of memory is the host's to hear.
fn said<T>(host: &Host<'_>, done: Result<T, Error>) -> Result<Option<T>, Error> {
    match done {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => {
            say_not_saved(host);
            Ok(None)
        }
    }
}

/// *Highlight* / *Remove highlight* on `span`, with `text` (its start, as much
/// as the highlight keeps).
///
/// # Errors
/// `NoMemory`; a change the library refuses is said.
pub fn highlight(host: &Host<'_>, ctx: &Ctx<'_>, span: Span, text: &str) -> Result<(), Error> {
    let done = with_new(host, ctx, span, text, |library, new| {
        edit::toggle(host, library, ctx.path, new)
    });
    said(host, done).map(|_| ())
}

/// *Note* / *Edit note* on `span`: the highlight there is made if there is
/// none, and the note it has is given back to be edited.
///
/// # Errors
/// `NoMemory`; a change the library refuses is said.
pub fn note(
    host: &Host<'_>,
    ctx: &Ctx<'_>,
    span: Span,
    text: &str,
) -> Result<Option<NoteAsk>, Error> {
    let done = with_new(host, ctx, span, text, |library, new| {
        edit::begin_note(host, library, ctx.path, new)
    });
    said(host, done)
}

/// `change` with the library and the highlight `span` with `text` would be.
fn with_new<T>(
    host: &Host<'_>,
    ctx: &Ctx<'_>,
    span: Span,
    text: &str,
    change: impl FnOnce(&paperpaw_sdk::native::library::Library, &edit::New<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let book = ctx.book.ok_or(Error::State)?;
    let library = book.library.ok_or(Error::State)?;
    let text = edit::snippet(text)?;
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
            drawn: book.highlights,
        },
    )
}
