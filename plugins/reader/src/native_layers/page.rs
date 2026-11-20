//! The pages of `reader.layer@1`: a laid-out page is shown to the plugin in
//! whole lines, as many as one ask holds, each with the places of its words;
//! what the plugin answers is kept with the page ([`Pages`]) and drawn over
//! its text whenever the page is drawn ([`draw`]).
use crate::native_word::Word;
use alloc::vec::Vec;
use core::ops::Range;
use paperpaw_sdk::{layout::Page, native::Error};

use super::buffer;
use crate::stages::reader_layer_v1 as layer;

/// The pages whose answers are kept: the one on show and the one before.
pub const PAGES: usize = 2;
/// The most answer bytes kept for one page.
pub const MOST_BYTES: usize = layer::MAX_REPLY;
/// The bytes of one packed word: `x`, `width`, `at`, each u16 little-endian.
const WORD_BYTES: usize = 6;
/// The most words of a line shown.
const WORDS: usize = layer::LAYER_LINE_WORDS_MAX_BYTES / WORD_BYTES;
/// The largest box, line or text a layer draws, in pixels.
const MOST_SIDE: u32 = 4096;
const MOST_THICKNESS: u32 = 64;
const SIZES: (u32, u32) = (4, 200);

/// What a page's answer is kept by: where the page starts, and the words
/// read aloud on it where the plugin follows reading aloud.
pub type Key = ((u32, u32), Option<(u32, u32)>);

/// A page's key and the answers kept for it.
type KeptPage = (Key, Vec<Vec<u8>>);

/// The answers kept for pages, by where they start, the newest last.
#[derive(Debug, Default)]
pub struct Pages {
    kept: Vec<KeptPage>,
}

impl Pages {
    /// The answers of the page kept by `start`, if it was asked.
    #[must_use]
    pub fn get(&self, start: Key) -> Option<&[Vec<u8>]> {
        self.kept
            .iter()
            .find(|(at, _)| *at == start)
            .map(|(_, replies)| replies.as_slice())
    }

    /// Keeps `replies` for the page kept by `start`; the oldest page goes.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn keep(&mut self, start: Key, replies: Vec<Vec<u8>>) -> Result<(), Error> {
        self.kept.retain(|(at, _)| *at != start);
        while self.kept.len() >= PAGES {
            self.kept.remove(0);
        }
        self.kept.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.kept.push((start, replies));
        Ok(())
    }

    /// Forgets every page.
    pub fn clear(&mut self) {
        self.kept = Vec::new();
    }
}

/// A line of the page as the ask shows it, its words packed.
#[derive(Debug)]
pub struct Row {
    x: i32,
    baseline: i32,
    width: u32,
    size: u32,
    leading: u32,
    top: i32,
    section: u32,
    offset: u32,
    inserted: bool,
    words: Vec<u8>,
}

/// The byte each of the line's words starts at in `text`: its pieces between
/// spaces that are not empty, as the layout counts its words.
fn starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    let mut at = 0;
    text.split(' ').filter_map(move |piece| {
        let start = at;
        at += piece.len() + 1;
        (!piece.is_empty()).then_some(start)
    })
}

/// The rows of `page`, its words those of `words` (the reader's word boxes,
/// drawn with the text area's top left at `(left, top)`; the page's own
/// lines are laid out `(dx, dy)` from there, in a layer's region). A word without a
/// box is in its line 0 wide at 0, so that the words of a line are counted
/// as reading aloud counts them ([`aloud`]).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn rows(
    page: &Page,
    words: &[Word],
    (left, top): (u16, u16),
    (dx, dy): (i32, i32),
) -> Result<Vec<Row>, Error> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(page.lines.len())
        .map_err(|_| Error::NoMemory)?;
    let mut next = 0;
    for line in &page.lines {
        let mut packed = Vec::new();
        let line_x = line.x + dx as f32;
        let (mut right, mut size) = (line_x, 0_u32);
        let mut high = i32::MAX;
        if !line.inserted {
            for (&(source, _), at) in line.words.iter().zip(starts(&line.text)).take(WORDS) {
                // The box of the word: the next ones in order, a few looked at.
                let found = words
                    .get(next..)
                    .unwrap_or_default()
                    .iter()
                    .take(4)
                    .position(|w| w.start.section == line.section && w.start.offset == source);
                packed
                    .try_reserve(WORD_BYTES)
                    .map_err(|_| Error::NoMemory)?;
                let at = u16::try_from(at).unwrap_or(u16::MAX).to_le_bytes();
                let Some(found) = found else {
                    packed.extend_from_slice(&[0, 0, 0, 0, at[0], at[1]]);
                    continue;
                };
                let word = &words[next + found];
                next += found + 1;
                let x = word.area.x.saturating_sub(left);
                packed.extend_from_slice(&x.to_le_bytes());
                packed.extend_from_slice(&word.area.width.to_le_bytes());
                packed.extend_from_slice(&at);
                right = right.max(f32::from(x) + f32::from(word.area.width));
                size = size.max(u32::from(word.area.height));
                high = high.min(i32::from(word.area.y) - i32::from(top));
            }
        }
        let baseline = (line.baseline + 0.5) as i32 + dy;
        rows.push(Row {
            x: line_x as i32,
            baseline,
            width: (right - line_x).max(0.0) as u32,
            size,
            leading: line.leading as u32,
            // Where its words' boxes start; without any, its baseline.
            top: if high == i32::MAX { baseline } else { high },
            section: line.section,
            offset: line.words.first().map_or(0, |w| w.0),
            inserted: line.inserted,
            words: packed,
        });
    }
    Ok(rows)
}

/// The part of the text area `full` a layer's region (thousandths of it)
/// gives the book's text: never empty.
#[must_use]
pub fn within(full: crate::native_pages::Box, [x, y, w, h]: [u16; 4]) -> crate::native_pages::Box {
    let part = |side: u16, n: u16| (u32::from(side) * u32::from(n) / 1000) as u16;
    let mut text = full;
    text.x = full.x.saturating_add(part(full.width, x));
    text.y = full.y.saturating_add(part(full.height, y));
    text.width = part(full.width, w).max(1);
    text.height = part(full.height, h).max(1);
    text
}

/// The words `spoken` of `page` (numbered as `native_speech::words` numbers
/// them: the lines a plugin inserted have none) as the stage says them: the
/// line and the word in it of the first, and of the one after the last.
/// None for no words.
#[must_use]
pub fn aloud(page: &Page, spoken: Range<usize>) -> Option<layer::LayerAloud> {
    if spoken.is_empty() {
        return None;
    }
    let (mut from, mut to, mut seen) = (None, None, 0_usize);
    let mut lines = 0_usize;
    for (n, line) in page.lines.iter().enumerate() {
        lines = n + 1;
        if line.inserted {
            continue;
        }
        let count = line.text.split(' ').filter(|w| !w.is_empty()).count();
        let place = |word: usize| (n as u32, (word - seen) as u32);
        if from.is_none() && spoken.start < seen + count {
            from = Some(place(spoken.start));
        }
        if spoken.end <= seen + count {
            to = Some(place(spoken.end));
            break;
        }
        seen += count;
    }
    let ((line, word), (end_line, end_word)) = (from?, to.unwrap_or((lines as u32, 0)));
    Some(layer::LayerAloud::new(line, word, end_line, end_word))
}

/// The ask of the lines of `page` from line `first` on, as many as one ask
/// holds, the text area `size`, with the words read aloud; with the number
/// of the line after the last it holds.
///
/// # Errors
/// [`Error::NoMemory`]; [`Error::Invalid`] where not one line fits.
pub fn fire(
    page: &Page,
    rows: &[Row],
    (first, aloud): (usize, Option<&layer::LayerAloud>),
    (width, height): (u32, u32),
) -> Result<(Vec<u8>, usize), Error> {
    let mut out = buffer(layer::MAX_INPUT)?;
    let mut writer = layer::InputWriter::new(&mut out).map_err(|_| Error::NoSpace)?;
    let lines = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let head = layer::LayerPage::new(width, height, lines, first as u32);
    writer
        .push(&layer::Record::LayerPage(head))
        .map_err(|_| Error::Invalid)?;
    if let Some(aloud) = aloud {
        writer
            .push(&layer::Record::LayerAloud(*aloud))
            .map_err(|_| Error::Invalid)?;
    }
    let mut next = first;
    for (row, line) in rows.iter().zip(&page.lines).skip(first) {
        let line = layer::LayerLine::new(
            row.x,
            row.baseline,
            row.width,
            row.size,
            row.leading,
            row.top,
            row.section,
            row.offset,
            u32::from(row.inserted),
            super::cut(&line.text, layer::LAYER_LINE_TEXT_MAX_BYTES),
            &row.words,
        );
        if writer.push(&layer::Record::LayerLine(line)).is_err() {
            break;
        }
        next += 1;
    }
    if next == first && first < rows.len() {
        return Err(Error::Invalid);
    }
    let len = writer.finish().map_err(|_| Error::Invalid)?;
    out.truncate(len);
    Ok((out, next))
}

/// What the plugin is shown of the page on show with a tap or a key: the
/// page's head (its lines not shown again) and the event.
#[derive(Clone, Copy, Debug)]
pub enum Event<'a> {
    /// A tap at `(x, y)` from the text area's top left, on the book's region
    /// or not.
    Tap(i32, i32, bool),
    /// A key action.
    Key(&'a str),
}

/// The ask of `event` on the page of `lines` lines that starts at `start`,
/// in a text area of `(width, height)`.
///
/// # Errors
/// [`Error::NoMemory`], [`Error::Invalid`] where it cannot be encoded.
pub fn event_input(
    event: Event<'_>,
    (section, offset): (u32, u32),
    lines: usize,
    (width, height): (u32, u32),
) -> Result<Vec<u8>, Error> {
    let mut out = buffer(160)?;
    let mut writer = layer::InputWriter::new(&mut out).map_err(|_| Error::NoSpace)?;
    let lines = u32::try_from(lines).unwrap_or(u32::MAX);
    let head = layer::LayerPage::new(width, height, lines, lines);
    let row = match event {
        Event::Tap(x, y, book) => {
            layer::Record::LayerTap(layer::LayerTap::new(x, y, u32::from(book), section, offset))
        }
        Event::Key(action) => layer::Record::LayerKey(layer::LayerKey::new(
            super::cut(action, layer::LAYER_KEY_ACTION_MAX_BYTES),
            section,
            offset,
        )),
    };
    for row in [layer::Record::LayerPage(head), row] {
        writer.push(&row).map_err(|_| Error::Invalid)?;
    }
    let len = writer.finish().map_err(|_| Error::Invalid)?;
    out.truncate(len);
    Ok(out)
}

/// Whether `reply` (the answer to an [`event_input`]) takes the event, and
/// whether its drawing then stands for the page's layer.
#[must_use]
pub fn taken(reply: &[u8]) -> Option<bool> {
    layer::reply_decode(reply)
        .ok()?
        .flatten()
        .find_map(|row| match row {
            layer::Record::LayerTaken(t) => Some(t.redraw != 0),
            _ => None,
        })
}

/// What a layer is drawn with: in grays, the reading font for its text.
pub trait Pen {
    /// Fills `width` by `height` at `(x, y)`.
    ///
    /// # Errors
    /// The host's refusal.
    fn fill(&mut self, at: (i32, i32), size: (u32, u32), gray: u8) -> Result<(), Error>;

    /// A line from `from` to `to`, `width` pixels wide.
    ///
    /// # Errors
    /// The host's refusal.
    fn segment(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        width: u32,
        gray: u8,
    ) -> Result<(), Error>;

    /// `text` with its pen at `at` (`x` and the row of its baseline), as
    /// `look` says: its size in pixels, its gray, in italics or not.
    ///
    /// # Errors
    /// The host's refusal.
    fn text(&mut self, at: (i32, i32), look: (f32, u8, bool), text: &str) -> Result<(), Error>;
}

/// A tone of the stage: a gray, or none (256 and more).
fn gray(tone: u32) -> Option<u8> {
    u8::try_from(tone).ok()
}

/// The kept `replies` drawn with the text area's top left at `(left, top)`,
/// in their order. A record that is not the stage's is left out.
///
/// # Errors
/// What `pen` refuses.
pub fn draw(pen: &mut impl Pen, replies: &[Vec<u8>], (left, top): (i32, i32)) -> Result<(), Error> {
    let at = |x: i32, y: i32| (left.saturating_add(x), top.saturating_add(y));
    for reply in replies {
        let Ok(rows) = layer::reply_decode(reply) else {
            continue;
        };
        for row in rows.flatten() {
            match row {
                layer::Record::LayerBox(b) => {
                    let (w, h) = (b.width.min(MOST_SIDE), b.height.min(MOST_SIDE));
                    let (x, y) = at(b.x, b.y);
                    if let Some(fill) = gray(b.fill) {
                        pen.fill((x, y), (w, h), fill)?;
                    }
                    let t = b.thickness.min(MOST_THICKNESS).min(w).min(h);
                    if let (Some(ink), true) = (gray(b.ink), t > 0) {
                        let (wi, hi) = (w as i32, h as i32);
                        pen.fill((x, y), (w, t), ink)?;
                        pen.fill((x, y + hi - t as i32), (w, t), ink)?;
                        pen.fill((x, y), (t, h), ink)?;
                        pen.fill((x + wi - t as i32, y), (t, h), ink)?;
                    }
                }
                layer::Record::LayerSegment(s) => {
                    if let Some(ink) = gray(s.ink) {
                        let width = s.thickness.clamp(1, MOST_THICKNESS);
                        pen.segment(at(s.x1, s.y1), at(s.x2, s.y2), width, ink)?;
                    }
                }
                layer::Record::LayerText(t) => {
                    if let (Some(ink), false) = (gray(t.ink), t.text.is_empty()) {
                        let size = t.size.clamp(SIZES.0, SIZES.1) as f32;
                        pen.text(at(t.x, t.y), (size, ink, t.italic != 0), t.text)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
