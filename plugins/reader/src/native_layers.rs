//! The reader's side of the page layers (`docs/abi/page-layers.md`, ADR 0032):
//! the stages `reader.text@1` (a block's text before its layout: room, words
//! replaced, glosses, blocks inserted) and `reader.layer@1` (what a plugin
//! draws over a laid-out page). No host here: inputs are bytes out, replies
//! bytes in (`native_view/layers.rs` runs the stages), so all of it is tested.
//!
//! Each stage is used with one plugin for the book on show: the first in stage
//! order whose answer to the book's ask says so ([`Chosen`]). Its answers are
//! kept: those for blocks ([`Answers`], at most [`KEPT_BYTES`] of replies) as
//! long as the book is open, those for pages ([`page::Pages`]) for the page on
//! show and the one before. A block is asked once, a page once after its
//! layout: a page turn asks nothing it asked before. Without a plugin nothing
//! is kept and nothing is asked again until a setting changes.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    document::{Annotation, AnnotationKind, Block, BlockKind, Run, TextStyle},
    native::Error,
};

use crate::stages::{reader_layer_v1 as layer, reader_text_v1 as text};

pub mod page;

/// The most reply bytes kept for the blocks of a book, and the most blocks.
pub const KEPT_BYTES: usize = 12 * 1024;
pub const KEPT_BLOCKS: usize = 64;
/// The most replacements and glosses of one answer that are applied.
const MOST_EDITS: usize = 64;
/// The most room above or below a line, in hundredths of an em.
const MOST_ROOM: u32 = 400;
/// The most heap one block's edit holds.
const BUDGET: usize = 16 * 1024;

/// Which of the two stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Layer,
}

impl Kind {
    /// The stage's key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Text => text::KEY,
            Self::Layer => layer::KEY,
        }
    }
}

/// The plugin a stage is used with for the book on show: its place in the
/// stage's order, its id (checked with every reply) and its signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chosen {
    pub cursor: u32,
    pub id: String,
    /// The plugin's answer folded with its registration's.
    pub signature: u64,
    /// Its pages are asked again with each sentence read aloud.
    pub aloud: bool,
    /// Where the book's text goes in the text area, in thousandths of it
    /// (`x`, `y`, `width`, `height`): the rest is the layer's. None: all of it.
    pub region: Option<[u16; 4]>,
}

/// The smallest side of a region, in thousandths of the text area.
const LEAST_REGION: u32 = 100;

/// The region a `layer.region` record asks for, where it is one inside the
/// text area at least a tenth of it on each side.
fn region_of(r: &layer::LayerRegion) -> Option<[u16; 4]> {
    let fits = |at: u32, side: u32| {
        side >= LEAST_REGION && at.checked_add(side).is_some_and(|end| end <= 1000)
    };
    (fits(r.x, r.width) && fits(r.y, r.height)).then_some([
        r.x as u16,
        r.y as u16,
        r.width as u16,
        r.height as u16,
    ])
}

impl Chosen {
    /// The plugin `subscriber` at `cursor`, registered with `registered`, as
    /// its answer `payload` to a book's ask of `kind` says: chosen where it
    /// answers that it adds to the book (a signature other than 0).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn from_reply(
        kind: Kind,
        (cursor, subscriber, registered): (u32, &str, u64),
        payload: &[u8],
    ) -> Result<Option<Self>, Error> {
        let (mut region, mut rows) = (None, layer::reply_decode(payload).ok());
        let (signature, aloud) = match kind {
            Kind::Text => match text::reply_decode(payload).ok().and_then(|mut r| r.next()) {
                Some(Ok(text::Record::TextUses(uses))) => (uses.signature, false),
                _ => (0, false),
            },
            Kind::Layer => match rows.as_mut().and_then(Iterator::next) {
                Some(Ok(layer::Record::LayerUses(uses))) => {
                    if let Some(Ok(layer::Record::LayerRegion(r))) =
                        rows.as_mut().and_then(Iterator::next)
                    {
                        region = region_of(&r);
                    }
                    (uses.signature, uses.aloud != 0)
                }
                _ => (0, false),
            },
        };
        if signature == 0 {
            return Ok(None);
        }
        Ok(Some(Self {
            cursor,
            id: copy(subscriber)?,
            signature: signature.rotate_left(7) ^ registered,
            aloud,
            region,
        }))
    }
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

/// `text` cut to at most `most` bytes, at a character.
fn cut(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    out.resize(len, 0);
    Ok(out)
}

fn encoded(len: Result<usize, i32>, mut out: Vec<u8>) -> Result<Vec<u8>, Error> {
    let len = len.map_err(|_| Error::Invalid)?;
    out.truncate(len);
    Ok(out)
}

/// The ask of a book at `path` in `language` for `kind`.
///
/// # Errors
/// [`Error::NoMemory`], [`Error::Invalid`] where it cannot be encoded.
pub fn book_input(kind: Kind, path: &str, language: &str) -> Result<Vec<u8>, Error> {
    let (path, language) = (cut(path, 512), cut(language, 16));
    let mut out = buffer(560)?;
    let len = match kind {
        Kind::Text => text::TextBook::new(path, language).encode(&mut out),
        Kind::Layer => {
            let row = layer::Record::LayerBook(layer::LayerBook::new(path, language));
            layer::input_encode(&[row], &mut out)
        }
    };
    encoded(len, out)
}

/// The ask of `block` of `section` (its text cut at the stage's bound).
///
/// # Errors
/// As [`book_input`].
pub fn block_input(section: u32, block: &Block) -> Result<Vec<u8>, Error> {
    let kind = match block.kind {
        BlockKind::Paragraph => 0,
        BlockKind::Heading(_) => 1,
        _ => 2,
    };
    let shown = cut(&block.text, text::TEXT_BLOCK_TEXT_MAX_BYTES);
    let mut out = buffer(shown.len() + 32)?;
    let len = text::TextBlock::new(section, block.start, kind, shown).encode(&mut out);
    encoded(len, out)
}

/// The answer for one block of the book.
#[derive(Debug)]
struct Kept {
    section: u32,
    start: u32,
    end: u32,
    /// The bytes of the block's text (to bound what is asked ahead).
    len: u32,
    reply: Vec<u8>,
}

/// The answers for the blocks of the book on show, oldest first.
#[derive(Debug, Default)]
pub struct Answers {
    kept: Vec<Kept>,
    bytes: usize,
}

impl Answers {
    /// The block kept that holds `offset` of `section`: its end and its
    /// text's length.
    #[must_use]
    pub fn around(&self, section: u32, offset: u32) -> Option<(u32, u32)> {
        self.kept
            .iter()
            .find(|k| k.section == section && k.start <= offset && offset < k.end)
            .map(|k| (k.end, k.len))
    }

    /// The answer kept for the block that starts at `start` of `section`.
    #[must_use]
    pub fn reply(&self, section: u32, start: u32) -> Option<&[u8]> {
        self.kept
            .iter()
            .find(|k| k.section == section && k.start == start)
            .map(|k| k.reply.as_slice())
    }

    /// Keeps `reply` for `block` of `section`; the oldest go where the
    /// answers would pass their bounds.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn keep(&mut self, section: u32, block: &Block, reply: &[u8]) -> Result<(), Error> {
        let reply = if reply.len() <= 1 {
            // An empty batch says nothing: nothing to keep but the place.
            Vec::new()
        } else {
            let mut kept = Vec::new();
            kept.try_reserve_exact(reply.len())
                .map_err(|_| Error::NoMemory)?;
            kept.extend_from_slice(reply);
            kept
        };
        while !self.kept.is_empty()
            && (self.kept.len() >= KEPT_BLOCKS || self.bytes + reply.len() > KEPT_BYTES)
        {
            let gone = self.kept.remove(0);
            self.bytes -= gone.reply.len();
        }
        self.kept.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.bytes += reply.len();
        self.kept.push(Kept {
            section,
            start: block.start,
            end: block.end.max(block.start.saturating_add(1)),
            len: u32::try_from(block.text.len()).unwrap_or(u32::MAX),
            reply,
        });
        Ok(())
    }

    /// Forgets every answer.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// What an answer asks of a block, checked.
#[derive(Debug, Default)]
struct Asked<'a> {
    room: (u32, u32),
    replaced: Vec<(u32, u32, &'a str)>,
    glosses: Vec<(u32, u32, u32, u32, &'a str)>,
    inserted: Vec<(&'a str, u32, bool)>,
}

impl<'a> Asked<'a> {
    /// The rows of `reply` for a block whose asked text is `shown`: ranges
    /// off it or off its characters left out, replacements that overlap one
    /// before them too.
    fn of(reply: &'a [u8], shown: &str) -> Result<Self, Error> {
        let mut asked = Self::default();
        let fits = |from: u32, to: u32| {
            let (from, to) = (from as usize, to as usize);
            from <= to
                && to <= shown.len()
                && shown.is_char_boundary(from)
                && shown.is_char_boundary(to)
        };
        for row in text::reply_decode(reply).map_err(|_| Error::Invalid)? {
            match row.map_err(|_| Error::Invalid)? {
                text::Record::TextRoom(room) => asked.room = (room.above, room.below),
                text::Record::TextReplace(r) if fits(r.from, r.to) => {
                    if asked.replaced.len() < MOST_EDITS {
                        asked.replaced.try_reserve(1).map_err(|_| Error::NoMemory)?;
                        asked.replaced.push((r.from, r.to, r.with));
                    }
                }
                text::Record::TextGloss(g) if fits(g.from, g.to) && g.from < g.to => {
                    if asked.glosses.len() < MOST_EDITS {
                        asked.glosses.try_reserve(1).map_err(|_| Error::NoMemory)?;
                        asked.glosses.push((g.from, g.to, g.kind, g.size, g.text));
                    }
                }
                text::Record::TextInsert(i) => {
                    asked.inserted.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    asked.inserted.push((i.text, i.size, i.italic != 0));
                }
                _ => {}
            }
        }
        paperpaw_sdk::sort::sort_unstable_by_key(&mut asked.replaced, |r| (r.0, r.1));
        let mut end = 0;
        asked.replaced.retain(|&(from, to, _)| {
            let apart = from >= end;
            if apart {
                end = to.max(from);
            }
            apart
        });
        Ok(asked)
    }

    /// Where the range `from..to` of the asked text is once the replacements
    /// are in; `None` where it holds a part of one.
    fn moved(&self, from: u32, to: u32) -> Option<(u32, u32)> {
        let mut shift: i64 = 0;
        for &(r_from, r_to, with) in &self.replaced {
            if from < r_to && to > r_from || (r_from == r_to && from < r_from && r_from < to) {
                return None;
            }
            if r_to <= from {
                shift += with.len() as i64 - i64::from(r_to - r_from);
            }
        }
        let from = u32::try_from(i64::from(from) + shift).ok()?;
        let to = u32::try_from(i64::from(to) + shift).ok()?;
        Some((from, to))
    }
}

/// `reply` (a kept answer) applied to `block`: its room, its replacements
/// (last first, the block changed whole or not at all), its glosses as the
/// block's annotations and its blocks inserted after it.
///
/// # Errors
/// [`Error::Invalid`] for a reply that is no answer of the stage,
/// [`Error::NoMemory`]; the block keeps what was applied before.
pub fn apply(reply: &[u8], block: &mut Block) -> Result<(), Error> {
    if reply.is_empty() {
        return Ok(());
    }
    let asked = Asked::of(reply, cut(&block.text, text::TEXT_BLOCK_TEXT_MAX_BYTES))?;
    let (above, below) = asked.room;
    if above > 0 {
        let above = above.min(MOST_ROOM) as u16;
        block.leading_above = Some(block.leading_above.unwrap_or(0).max(above));
    }
    if below > 0 {
        // Room below: an annotation without text (`paperpaw-layout`).
        let mut room = Annotation::new(0, 0, String::new(), AnnotationKind::Gloss);
        room.size = below.min(MOST_ROOM) as u16;
        block
            .annotations
            .try_reserve(1)
            .map_err(|_| Error::NoMemory)?;
        block.annotations.push(room);
    }
    let replaced = !asked.replaced.is_empty()
        && block
            .try_transform(BUDGET, |edit| {
                for &(from, to, with) in asked.replaced.iter().rev() {
                    edit.try_replace(from as usize..to as usize, with)?;
                }
                Ok::<(), paperpaw_sdk::document::EditError>(())
            })
            .is_ok();
    for &(from, to, kind, size, note) in &asked.glosses {
        let place = if replaced {
            asked.moved(from, to)
        } else {
            Some((from, to))
        };
        let Some((from, to)) = place else {
            continue;
        };
        let kind = match kind {
            0 => AnnotationKind::Ruby,
            1 => AnnotationKind::Phonetic,
            _ => AnnotationKind::Gloss,
        };
        let mut annotation = Annotation::new(from, to, copy(note)?, kind);
        annotation.size = u16::try_from(size).unwrap_or(0);
        block
            .annotations
            .try_reserve(1)
            .map_err(|_| Error::NoMemory)?;
        block.annotations.push(annotation);
    }
    for &(note, size, italic) in &asked.inserted {
        if note.is_empty() {
            continue;
        }
        let end = block.end;
        let mut inserted = Block::new(BlockKind::Paragraph, end, end, copy(note)?);
        inserted.size = u16::try_from(size).ok().filter(|&s| s > 0);
        if italic {
            let len = u32::try_from(note.len()).unwrap_or(u32::MAX);
            inserted.runs.push(Run::new(0, len, TextStyle::ITALIC));
        }
        block.insert_after(inserted);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
