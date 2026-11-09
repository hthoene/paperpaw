//! "Highlight": one of one's own highlights, from the books read lately or
//! from all, another each day (or each time it shows): the text in quotes,
//! the book's title below. A tap opens the book at its place.
//!
//! The highlights are the library's (`core.library`): the books come from the
//! catalog (the ones opened last, or the first [`MAX_BOOKS`] by title), and a
//! book's highlights are counted and then read by their place in its
//! annotations, so nothing but the one shown is kept.
use crate::native_books::{buffers, caption, push_text, with_library};
use crate::native_quote::Showings;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        commands::CommandSender,
        formatting::Arg,
        library::{
            AnnotationCursor, AnnotationKind, AnnotationTextKind, AnnotationTextQuery, BookRef,
            CatalogCursor, Library, Query, Sort,
        },
        ui::{Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Paint, geometry, mem, text,
    widget::{Cx, Settings, TextSize, layout::label, text::Wrapped},
};

/// The callback token of the widget as declared.
pub const TOKEN: u32 = 115;
/// The books read lately that highlights are chosen from.
const RECENT_BOOKS: usize = 12;
/// The books looked at, at most, where highlights come from all.
const MAX_BOOKS: usize = 200;
/// The most of a highlight's text, or of its note, that is read (the library
/// cuts its previews at 512 bytes; the widget shows a few lines).
const TEXT_LIMIT: usize = 2048;
/// Where highlights come from: the books read lately, or all.
const SOURCES: [&str; 2] = ["widgets.highlight.recent", "widgets.highlight.all"];
/// When another shows: each day, or each time the widget shows.
const CHANGES: [&str; 2] = ["widgets.highlight.daily", "widgets.highlight.shown"];
/// The caption.
const CAPTION: &str = "widgets.highlight.caption";

/// A book of the catalog: its key in the generation it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    key: u32,
    generation: u32,
}

/// A highlight as shown, and where to find it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    book: Held,
    /// The book's annotation generation when it was read.
    annotations: u32,
    section: u32,
    offset: u32,
    conversion: Option<u64>,
    title: String,
    author: String,
    text: String,
    note: String,
}

/// The highlight chosen, and what it was chosen for (the source, the seed, the
/// catalog's generation).
#[derive(Debug, Default)]
pub struct Cache {
    kept: Option<((bool, u64, u32), Option<Quote>)>,
}

/// A number that looks random, the same for the same `seed`.
#[must_use]
pub const fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The place in `count` things that `seed` picks first, and the one it picks
/// among `count` highlights of the book it stops at.
#[must_use]
pub fn picks(seed: u64, books: usize, highlights: usize) -> (usize, usize) {
    let first = if books == 0 {
        0
    } else {
        (mix(seed) % books as u64) as usize
    };
    let at = if highlights == 0 {
        0
    } else {
        (mix(seed ^ 0x5851_f42d_4c95_7f2d) % highlights as u64) as usize
    };
    (first, at)
}

/// The library service with the buffers of its calls.
struct Scan<'a, 'h> {
    host: &'a Host<'h>,
    library: &'a Library,
    input: Vec<u8>,
    out: Vec<u8>,
}

/// What of an annotation is kept while its page is in use.
struct Found {
    id: u32,
    section: u32,
    offset: u32,
    conversion: Option<u64>,
    text: String,
    note: String,
    text_cut: bool,
    note_cut: bool,
    generation: u32,
}

impl Scan<'_, '_> {
    /// The books highlights are chosen from: the first [`RECENT_BOOKS`] opened
    /// last, or the first [`MAX_BOOKS`] by title.
    fn books(&mut self, all: bool) -> Result<Vec<Held>, Error> {
        let limit = if all { MAX_BOOKS } else { RECENT_BOOKS };
        let mut books = Vec::new();
        let mut cursor = CatalogCursor::start();
        loop {
            let query = if all {
                Query::new()
            } else {
                Query::new().sort(Sort::RecentlyOpened).descending(true)
            };
            let query = query.exclude_archive(false).cursor(cursor);
            let page = self
                .library
                .query(self.host, &query, &mut self.input, &mut self.out)?;
            let end = page.end();
            for row in page.rows() {
                let row = row?;
                // The books opened come first; the rest were never read.
                if books.len() >= limit || !all && !row.opened() {
                    return Ok(books);
                }
                mem::push(
                    &mut books,
                    Held {
                        key: row.key(),
                        generation: end.generation(),
                    },
                )?;
            }
            match end.next() {
                Some(next) => cursor = next,
                None => return Ok(books),
            }
        }
    }

    /// How many highlights with a text the book has, and its annotation
    /// generation.
    fn count(&mut self, book: Held) -> Result<(usize, u32), Error> {
        let reference = BookRef::catalog(book.key, book.generation)?;
        let mut cursor = AnnotationCursor::start();
        let mut count = 0;
        loop {
            let page = self.library.annotations(
                self.host,
                reference,
                cursor,
                &mut self.input,
                &mut self.out,
            )?;
            let end = page.end();
            let generation = end.generation();
            for row in page.rows() {
                let row = row?;
                count += usize::from(is_highlight(row.kind(), row.text()));
            }
            match end.next() {
                Some(next) if end.total() > 0 => cursor = next,
                _ => return Ok((count, generation)),
            }
        }
    }

    /// The `n`th highlight with a text of the book.
    fn nth(&mut self, book: Held, n: usize) -> Result<Option<Found>, Error> {
        let reference = BookRef::catalog(book.key, book.generation)?;
        let mut cursor = AnnotationCursor::start();
        let mut seen = 0;
        loop {
            let page = self.library.annotations(
                self.host,
                reference,
                cursor,
                &mut self.input,
                &mut self.out,
            )?;
            let end = page.end();
            for row in page.rows() {
                let row = row?;
                if !is_highlight(row.kind(), row.text()) {
                    continue;
                }
                if seen == n {
                    return Ok(Some(Found {
                        id: row.id(),
                        section: row.start_section(),
                        offset: row.start_offset(),
                        conversion: row.conversion(),
                        text: text::copy(row.text())?,
                        note: text::copy(row.note())?,
                        text_cut: row.text_cut(),
                        note_cut: row.note_cut(),
                        generation: end.generation(),
                    }));
                }
                seen += 1;
            }
            match end.next() {
                Some(next) => cursor = next,
                None => return Ok(None),
            }
        }
    }

    /// The whole string `kind` of annotation `id` of the book, up to
    /// [`TEXT_LIMIT`] bytes, cut at a character's end.
    fn whole(&mut self, book: Held, id: u32, kind: AnnotationTextKind) -> Result<String, Error> {
        let reference = BookRef::catalog(book.key, book.generation)?;
        let mut whole = String::new();
        let mut offset = 0u32;
        loop {
            let query = AnnotationTextQuery::new(id, kind).from(offset);
            let part = self.library.annotation_text(
                self.host,
                reference,
                query,
                &mut self.input,
                &mut self.out,
            )?;
            let (text, next) = (part.text(), part.next_offset());
            if next.is_some_and(|next| text.is_empty() || next <= offset) {
                return Err(Error::Invalid);
            }
            let piece = cut(text, TEXT_LIMIT.saturating_sub(whole.len()));
            whole
                .try_reserve(piece.len())
                .map_err(|_| Error::NoMemory)?;
            whole.push_str(piece);
            match next {
                Some(next) if piece.len() == text.len() => offset = next,
                _ => return Ok(whole),
            }
        }
    }

    /// The highlight `seed` picks among the `books`: one of them, and one of
    /// its highlights; books without any are passed over.
    fn pick(&mut self, books: &[Held], seed: u64) -> Result<Option<Quote>, Error> {
        let (first, _) = picks(seed, books.len(), 0);
        for step in 0..books.len() {
            let book = books[(first + step) % books.len()];
            let (count, generation) = match self.count(book) {
                Ok(counted) => counted,
                // A book that went since the catalog was read.
                Err(Error::State | Error::NotFound) => continue,
                Err(error) => return Err(error),
            };
            if count == 0 {
                continue;
            }
            let (_, at) = picks(seed, books.len(), count);
            let found = match self.nth(book, at) {
                Ok(Some(found)) => found,
                Ok(None) | Err(Error::State | Error::NotFound) => continue,
                Err(error) => return Err(error),
            };
            return self.quote(book, found, generation).map(Some);
        }
        Ok(None)
    }

    /// `found` of `book` as a quote: its text and note whole where they were
    /// cut, the book's title and author.
    fn quote(&mut self, book: Held, mut found: Found, annotations: u32) -> Result<Quote, Error> {
        if found.text_cut {
            found.text = self.whole(book, found.id, AnnotationTextKind::Text)?;
        }
        if found.note_cut {
            found.note = self.whole(book, found.id, AnnotationTextKind::Note)?;
        }
        let row = self.library.book(
            self.host,
            BookRef::catalog(book.key, book.generation)?,
            &mut self.input,
            &mut self.out,
        )?;
        let title = if row.title().is_empty() {
            text::copy(row.path().rsplit('/').next().unwrap_or(""))?
        } else {
            text::copy(row.title())?
        };
        Ok(Quote {
            book,
            annotations: if found.generation == 0 {
                annotations
            } else {
                found.generation
            },
            section: found.section,
            offset: found.offset,
            conversion: found.conversion,
            title,
            author: text::copy(row.author())?,
            text: found.text,
            note: found.note,
        })
    }

    /// The book's annotation generation now.
    fn annotation_generation(&mut self, book: Held) -> Result<u32, Error> {
        let page = self.library.annotations(
            self.host,
            BookRef::catalog(book.key, book.generation)?,
            AnnotationCursor::start(),
            &mut self.input,
            &mut self.out,
        )?;
        Ok(page.end().generation())
    }
}

/// Whether an annotation of `kind` with `text` is a highlight with something
/// marked.
fn is_highlight(kind: AnnotationKind, text: &str) -> bool {
    kind == AnnotationKind::Highlight && !text.trim().is_empty()
}

/// `text` cut to at most `limit` bytes at a character's end.
fn cut(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// The highlight of the day, or of this showing: read anew where the
/// library's catalog or the book's annotations changed (or the source or the
/// day did), else kept.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn choose<'c>(
    host: &Host<'_>,
    settings: &Settings<'_, '_>,
    cache: &'c mut Cache,
    showings: &Showings,
) -> Result<Option<&'c Quote>, Error> {
    let all = settings.with_text("source", |value| value == SOURCES[1])?;
    let each_showing = settings.with_text("change", |value| value == CHANGES[1])?;
    let today = crate::native_draw::epoch_day(host)?.max(0) as u64;
    // Read anew (after events, a book closed), it shows the same until it
    // shows again.
    let seed = if each_showing {
        today.rotate_left(32) ^ u64::from(showings.count())
    } else {
        today
    };
    with_library(host, |library| {
        let (input, out) = buffers()?;
        let mut scan = Scan {
            host,
            library,
            input,
            out,
        };
        let generation = library
            .state(host, &mut scan.input, &mut scan.out)?
            .generation();
        let key = (all, seed, generation);
        if let Some((kept, quote)) = &cache.kept
            && *kept == key
        {
            let current = match quote {
                None => true,
                Some(quote) => match scan.annotation_generation(quote.book) {
                    Ok(now) => now == quote.annotations,
                    Err(Error::State | Error::NotFound) => false,
                    Err(error) => return Err(error),
                },
            };
            if current {
                return Ok(());
            }
        }
        let books = scan.books(all)?;
        let quote = scan.pick(&books, seed)?;
        cache.kept = Some((key, quote));
        Ok(())
    })?;
    Ok(cache.kept.as_ref().and_then(|(_, quote)| quote.as_ref()))
}

/// What the widget shows, to tell when it changed: a number made of the
/// highlight chosen (its text, note, book and author) and the parts shown.
///
/// # Errors
/// The host's refusals.
pub fn shows_with(
    host: &Host<'_>,
    widget: &str,
    cache: &mut Cache,
    showings: &Showings,
) -> Result<u64, Error> {
    let settings = Settings::new(host, widget);
    let flags = [
        u8::from(settings.shows("caption")?),
        u8::from(settings.shows("marks")?),
        u8::from(settings.shows("title")?),
        u8::from(settings.shows("author")?),
        u8::from(settings.shows("note")?),
    ];
    let mut mark = crate::native_draw::fnv(1, &flags);
    match choose(host, &settings, cache, showings)? {
        None => mark = crate::native_draw::fnv(mark, &[0]),
        Some(quote) => {
            for part in [&quote.text, &quote.note, &quote.title, &quote.author] {
                mark = crate::native_draw::fnv(mark, part.as_bytes());
                mark = crate::native_draw::fnv(mark, &[0xff]);
            }
        }
    }
    Ok(mark)
}

/// The height: three rows and the frame.
///
/// # Errors
/// Never; the signature is the widgets'.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    Ok(cx
        .style()
        .row
        .saturating_mul(3)
        .saturating_add(cx.shape.frame_height()))
}

/// A highlight laid out in a widget's area.
#[derive(Debug, PartialEq)]
struct Laid {
    /// The mark, the text, the note and the book together, in the middle.
    block: Rect,
    /// A large opening mark above the text, in the upper part of its line:
    /// half a line high.
    marks: bool,
    mark_h: u16,
    /// The text, as said, in its lines at its size.
    lines: Vec<String>,
    size: f32,
    text_h: u16,
    /// The note below the text, if shown.
    note: bool,
    /// The book (and its author) below, if shown.
    source: Option<String>,
}

/// The book and its author, as shown below the text.
fn source_of(cx: &Cx<'_>, quote: &Quote) -> Result<String, Error> {
    let title = cx.settings.shows("title")?;
    let author = cx.settings.shows("author")? && !quote.author.is_empty();
    match (title, author) {
        (true, true) => text::concat(&[&quote.title, " · ", &quote.author]),
        (true, false) => text::copy(&quote.title),
        (false, true) => text::copy(&quote.author),
        (false, false) => Ok(String::new()),
    }
}

/// `quote` laid out in `area` (below the caption): where the text does not fit
/// whole, the mark goes first, then the note, then the book; what is left is
/// shortened, broken between words only.
fn lay_out(cx: &mut Cx<'_>, area: Rect, quote: &Quote) -> Result<Laid, Error> {
    let (small, title, gap) = (cx.style().small, cx.style().title, cx.style().gap);
    let source = source_of(cx, quote)?;
    let small_h = geometry::line_px(&cx.font, small)?;
    let mut note = cx.settings.shows("note")? && !quote.note.is_empty();
    let mut with_source = !source.is_empty();
    let mut marks = cx.settings.shows("marks")?;
    let mark_size = title * 2.0;
    let (max, min) = match cx.settings.size()? {
        TextSize::Fit => (title * 1.6, small),
        other => {
            let size = other.text(cx.style());
            (size, size)
        }
    };
    let (lines, size, mark_h, below) = loop {
        let below = (u16::from(with_source).saturating_mul(small_h))
            .saturating_add(u16::from(note).saturating_mul(small_h));
        let below = if below > 0 {
            below.saturating_add(gap / 2)
        } else {
            0
        };
        let mark_h = if marks {
            geometry::line_px(&cx.font, mark_size)? / 2
        } else {
            0
        };
        let said = if marks {
            text::copy(&quote.text)?
        } else {
            cx.format(
                "widgets.highlight.quoted",
                &[Arg::Text {
                    name: "text",
                    value: &quote.text,
                }],
            )?
        };
        let room = area.height.saturating_sub(below.saturating_add(mark_h));
        let wrapped = Wrapped::fitting(&cx.font, &said, (max, min), (area.width, room), false)?;
        if wrapped.whole() {
            break (
                mem::copy_strings(wrapped.lines())?,
                wrapped.size(),
                mark_h,
                below,
            );
        }
        if marks {
            marks = false;
        } else if note {
            note = false;
        } else if with_source {
            with_source = false;
        } else {
            // As much as fits, ending in "…" (a word too long shortened, the
            // rest left out).
            let wrapped = Wrapped::new(&cx.font, &said, min, (area.width, room), false)?;
            break (mem::copy_strings(wrapped.lines())?, min, mark_h, below);
        }
    };
    let room = area.height.saturating_sub(below.saturating_add(mark_h));
    let line = geometry::line_px(&cx.font, size)?;
    let text_h = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_mul(line)
        .min(room);
    let total = mark_h.saturating_add(text_h).saturating_add(below);
    let total = total.min(area.height);
    Ok(Laid {
        block: Rect::new(
            area.x,
            area.y.saturating_add(area.height.saturating_sub(total) / 2),
            area.width,
            total,
        ),
        marks,
        mark_h,
        lines,
        size,
        text_h,
        note,
        source: with_source.then_some(source),
    })
}

/// What the widget shows, with no choice kept (as the minute tick of a plugin
/// that keeps none would ask).
///
/// # Errors
/// The host's refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    shows_with(host, widget, &mut Cache::default(), &Showings::default())
}

/// Draws the widget in `area`, choosing the highlight anew.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    draw_with(cx, out, area, (&mut Cache::default(), &Showings::default()))
}

/// Draws the widget in `area`, the highlight chosen kept in `cache`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_with(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    (cache, showings): (&mut Cache, &Showings),
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    // Where the library refuses a call, the widget says there is none; only
    // a refused allocation fails the draw.
    let chosen = match choose(cx.host, &cx.settings, cache, showings) {
        Ok(chosen) => chosen,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => None,
    };
    let Some(quote) = chosen else {
        let message = cx.tr("widgets.highlight.none")?;
        return crate::native_draw::message_in(cx, out, &look, &message);
    };
    let rest = caption(cx, out, &look, CAPTION)?;
    let laid = lay_out(cx, rest, quote)?;
    paint(cx, out, &look, quote, &laid)
}

/// The highlight as `laid`.
fn paint(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    quote: &Quote,
    laid: &Laid,
) -> Result<(), Error> {
    let (small, title, gap) = (cx.style().small, cx.style().title, cx.style().gap);
    let font = &cx.font;
    let align = cx.settings.align()?;
    let area = laid.block;
    let small_h = geometry::line_px(font, small)?;
    if laid.marks {
        let mark = "\u{201c}";
        let mark_size = title * 2.0;
        let x = align.x(area, font.measure_text(mark_size, mark)?);
        let ascent = geometry::line_metrics(font, mark_size)?.0;
        let baseline = i32::from(area.y)
            .saturating_add(ascent as i32)
            .saturating_sub(i32::from(laid.mark_h / 4));
        push_text(out, mark, (mark_size, x, baseline), look.faint)?;
    }
    // The text's lines.
    let text_area = Rect::new(
        area.x,
        area.y.saturating_add(laid.mark_h),
        area.width,
        laid.text_h,
    );
    let line = geometry::line_px(font, laid.size)?;
    for (n, shown) in laid.lines.iter().enumerate() {
        let y = text_area.y.saturating_add((n as u16).saturating_mul(line));
        if y.saturating_add(line) > text_area.y.saturating_add(text_area.height) {
            break;
        }
        let row = Rect::new(text_area.x, y, text_area.width, line);
        label(out, font, (laid.size, align), row, shown, look.ink)?;
    }
    let mut y = text_area
        .y
        .saturating_add(laid.text_h)
        .saturating_add(gap / 2);
    if laid.note {
        let row = Rect::new(area.x, y, area.width, small_h);
        label(out, font, (small, align), row, &quote.note, look.ink)?;
        y = y.saturating_add(small_h);
    }
    if let Some(source) = &laid.source {
        let row = Rect::new(area.x, y, area.width, small_h);
        label(out, font, (small, align), row, source, look.faint)?;
    }
    Ok(())
}

/// Opens the book of the highlight at its place, in the reader the user bound
/// to `core.reader`. A book that went since (`State`, `NotFound`) opens
/// nothing.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn open(host: &Host<'_>, quote: &Quote) -> Result<(), Error> {
    let path = with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        let reference = BookRef::catalog(quote.book.key, quote.book.generation)?;
        match library.book(host, reference, &mut input, &mut out) {
            Ok(row) => text::copy(row.path()).map(Some),
            Err(Error::State | Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    })?;
    let Some(path) = path else {
        return Ok(());
    };
    let (mut section, mut offset, mut conversion) = ([0; 20], [0; 20], [0; 20]);
    let section = text::decimal_u64(u64::from(quote.section), &mut section);
    let offset = text::decimal_u64(u64::from(quote.offset), &mut offset);
    let mut params = Vec::new();
    params.try_reserve(4).map_err(|_| Error::NoMemory)?;
    params.push(Param::new("path", &path)?);
    params.push(Param::new("section", section)?);
    params.push(Param::new("offset", offset)?);
    if let Some(value) = quote.conversion {
        params.push(Param::new(
            "conversion",
            text::decimal_u64(value, &mut conversion),
        )?);
    }
    let ui = Ui::open(host)?;
    let made = (|| {
        let mut input = mem::buffer(mem::SERVICE_BUFFER)?;
        let mut out = mem::buffer(mem::SERVICE_BUFFER)?;
        ui.handover(host, &params, &mut input, &mut out)
    })();
    let released = ui.release(host);
    let handover = made?;
    released?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_picks_the_same_and_other_seeds_other_things() {
        assert_eq!(picks(7, 5, 4), picks(7, 5, 4));
        let firsts: alloc::collections::BTreeSet<usize> =
            (0..40).map(|day| picks(day, 5, 4).0).collect();
        assert!(firsts.len() > 2, "{firsts:?}");
        let ats: alloc::collections::BTreeSet<usize> =
            (0..40).map(|day| picks(day, 5, 4).1).collect();
        assert!(ats.len() > 2, "{ats:?}");
    }

    #[test]
    fn a_pick_is_always_inside_what_there_is() {
        for seed in 0..200 {
            let (book, at) = picks(seed, 7, 3);
            assert!(book < 7 && at < 3);
        }
        assert_eq!(picks(5, 0, 0), (0, 0));
    }

    #[test]
    fn a_text_is_cut_at_a_character() {
        assert_eq!(cut("abc", 8), "abc");
        assert_eq!(cut("äöü", 3), "ä");
        assert_eq!(cut("äöü", 4), "äö");
        assert_eq!(cut("abc", 0), "");
    }

    #[test]
    fn only_a_highlight_with_a_text_counts() {
        assert!(is_highlight(AnnotationKind::Highlight, "text"));
        assert!(!is_highlight(AnnotationKind::Highlight, "  "));
        assert!(!is_highlight(AnnotationKind::Bookmark, "text"));
    }
}
