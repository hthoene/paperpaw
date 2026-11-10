//! Joins the fragments of a block of `core.document` (op 6, `document.md` 4)
//! into the [`Block`] the layout lays out. A block travels in fragments of
//! whole records; the head comes first, the text in parts, runs, offset
//! checkpoints, inline languages and annotations after it. What is checked
//! here is what the layout relies on: runs sorted, inside the text and on
//! character boundaries, checkpoints in order, a text within [`MAX_TEXT`].
//! Every allocation is fallible. A picture or a table row is not carried over
//! yet (`PORTING-7.md`): such a block reads as a paragraph of its text.
use alloc::vec::Vec;
use paperpaw_sdk::lang::{Direction, Lang, WritingMode};
use paperpaw_sdk::{
    document::{Align, Annotation, AnnotationKind, Block, BlockKind, Run, TextStyle},
    native::{
        Error,
        document::{BlockFragment, BlockRecord},
    },
};

/// Longest text of one block taken, in bytes (the host's cap is larger).
pub const MAX_TEXT: usize = 64 * 1024;
/// Most runs, checkpoints and annotations of one block taken.
pub const MAX_PARTS: usize = 2048;
/// Most inline languages of a block.
pub const MAX_LANGS: usize = 15;

/// The head of a block, as plain data.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Head<'a> {
    /// What the block is.
    pub kind: BlockKind,
    /// First and last source offset.
    pub start: u32,
    /// See `start`.
    pub end: u32,
    /// The block goes on from the one before / into the next.
    pub continues: bool,
    /// See `continues`.
    pub continued: bool,
    /// How its lines are aligned.
    pub align: Align,
    /// The book's first-line indent, hundredths of an em.
    pub indent: Option<i32>,
    /// The book's space above, hundredths of an em.
    pub space: Option<u32>,
    /// The book's text size, percent.
    pub size: Option<u32>,
    /// The book's line height, percent.
    pub line_height: Option<u32>,
    /// Margins at the start and the end, hundredths of an em.
    pub margins: (u32, u32),
    /// Its direction.
    pub direction: Direction,
    /// Its writing mode.
    pub writing_mode: WritingMode,
    /// Its language tag, empty when unknown.
    pub lang: &'a str,
    /// Lines of a drop capital.
    pub drop_cap: u32,
    /// The whole length of its text, in bytes.
    pub text_bytes: u32,
}

/// One record of a fragment as plain data.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Piece<'a> {
    /// The head.
    Head(Head<'a>),
    /// A part of the text.
    Text(&'a str),
    /// A styled run.
    Run(u32, u32, TextStyle),
    /// An offset checkpoint: text index, source offset.
    Map(u32, u32),
    /// An inline language.
    Lang(&'a str),
    /// An annotation: start, end, kind, size, text.
    Ruby(u32, u32, AnnotationKind, u32, &'a str),
}

/// The pieces of a fragment: the records that matter here, as data.
pub fn pieces<'a>(fragment: &BlockFragment<'a>) -> impl Iterator<Item = Piece<'a>> + use<'a> {
    fragment.records().filter_map(|record| {
        Some(match record {
            BlockRecord::Head(h) => Piece::Head(Head {
                kind: h.kind(),
                start: h.start(),
                end: h.end(),
                continues: h.continues(),
                continued: h.continued(),
                align: h.align(),
                indent: h.indent(),
                space: h.space(),
                size: h.size(),
                line_height: h.line_height(),
                margins: h.margins(),
                direction: h.direction(),
                writing_mode: h.writing_mode(),
                lang: h.lang(),
                drop_cap: h.drop_cap(),
                text_bytes: h.text_bytes(),
            }),
            BlockRecord::Text(text) => Piece::Text(text),
            BlockRecord::Run(r) => Piece::Run(r.start(), r.end(), r.style()),
            BlockRecord::Map(m) => Piece::Map(m.text(), m.source()),
            BlockRecord::InlineLang(tag) => Piece::Lang(tag),
            BlockRecord::Ruby(r) => Piece::Ruby(r.start(), r.end(), r.kind(), r.size(), r.text()),
            _ => return None,
        })
    })
}

/// The block being joined.
#[derive(Debug, Default)]
pub struct Assembler {
    block: Option<Block>,
    langs: Vec<Lang>,
    /// The text length the head said, and the checkpoints taken.
    expect: usize,
    maps: usize,
    last_map: u32,
}

impl Assembler {
    /// Nothing joined yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            block: None,
            langs: Vec::new(),
            expect: 0,
            maps: 0,
            last_map: 0,
        }
    }

    /// Forgets the block being joined (the host said it changed: start again).
    pub fn restart(&mut self) {
        self.block = None;
        self.langs.clear();
        self.expect = 0;
        self.maps = 0;
        self.last_map = 0;
    }

    /// Takes the pieces of one fragment. `last` says the fragment ends the
    /// block; the block is then returned, checked.
    ///
    /// # Errors
    /// `Invalid` for a piece out of order, a run or checkpoint the layout
    /// cannot take, or a text that does not add up; `NoSpace` for a block
    /// over the limits; `NoMemory`.
    pub fn push<'a>(
        &mut self,
        pieces: impl IntoIterator<Item = Piece<'a>>,
        last: bool,
    ) -> Result<Option<Block>, Error> {
        for piece in pieces {
            self.take(piece).inspect_err(|_| self.restart())?;
        }
        if !last {
            return Ok(None);
        }
        let block = self.block.take().ok_or(Error::Invalid)?;
        let langs = core::mem::take(&mut self.langs);
        let expect = core::mem::take(&mut self.expect);
        self.maps = 0;
        finish(block, &langs, expect).map(Some)
    }

    fn take(&mut self, piece: Piece<'_>) -> Result<(), Error> {
        if let Piece::Head(head) = piece {
            if self.block.is_some() || head.text_bytes as usize > MAX_TEXT {
                return Err(if self.block.is_some() {
                    Error::Invalid
                } else {
                    Error::NoSpace
                });
            }
            let mut text = alloc::string::String::new();
            text.try_reserve_exact(head.text_bytes as usize)
                .map_err(|_| Error::NoMemory)?;
            self.expect = head.text_bytes as usize;
            self.maps = 0;
            self.last_map = 0;
            self.block = Some(start(&head, text));
            return Ok(());
        }
        let block = self.block.as_mut().ok_or(Error::Invalid)?;
        match piece {
            Piece::Text(part) => {
                // The head said how long the text is: no part may take it past that.
                if block.text.len() + part.len() > self.expect {
                    return Err(Error::Invalid);
                }
                block
                    .text
                    .try_reserve(part.len())
                    .map_err(|_| Error::NoMemory)?;
                block.text.push_str(part);
            }
            Piece::Run(from, to, style) => {
                full(block.runs.len())?;
                block.runs.try_reserve(1).map_err(|_| Error::NoMemory)?;
                block.runs.push(Run::new(from, to, style));
            }
            Piece::Map(text, source) => {
                full(self.maps)?;
                // Order checked here, so a refusal of the map is the heap's.
                if text < self.last_map {
                    return Err(Error::Invalid);
                }
                self.last_map = text;
                self.maps += 1;
                block
                    .map
                    .try_checkpoint(text as usize, source)
                    .map_err(|_| Error::NoMemory)?;
            }
            Piece::Lang(tag) => {
                if self.langs.len() >= MAX_LANGS {
                    return Err(Error::NoSpace);
                }
                self.langs.try_reserve(1).map_err(|_| Error::NoMemory)?;
                self.langs.push(Lang::parse(tag));
            }
            Piece::Ruby(from, to, kind, size, text) => {
                full(block.annotations.len())?;
                block
                    .annotations
                    .try_reserve(1)
                    .map_err(|_| Error::NoMemory)?;
                let mut note = Annotation::new(from, to, alloc::string::String::new(), kind);
                note.text
                    .try_reserve_exact(text.len())
                    .map_err(|_| Error::NoMemory)?;
                note.text.push_str(text);
                note.size = u16::try_from(size).unwrap_or(0);
                block.annotations.push(note);
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}

const fn full(len: usize) -> Result<(), Error> {
    if len >= MAX_PARTS {
        Err(Error::NoSpace)
    } else {
        Ok(())
    }
}

fn narrow(value: Option<u32>) -> Option<u16> {
    value.map(|v| u16::try_from(v).unwrap_or(u16::MAX))
}

fn start(head: &Head<'_>, text: alloc::string::String) -> Block {
    // A picture or a table row is not carried over: it reads as a paragraph.
    let kind = match head.kind {
        BlockKind::Image | BlockKind::TableRow => BlockKind::Paragraph,
        kind => kind,
    };
    let mut block = Block::new(kind, head.start, head.end, text);
    block.continues = head.continues;
    block.continued = head.continued;
    block.align = head.align;
    block.indent = head
        .indent
        .map(|i| i16::try_from(i).unwrap_or(if i < 0 { i16::MIN } else { i16::MAX }));
    block.space = narrow(head.space);
    block.size = narrow(head.size);
    block.line_height = narrow(head.line_height);
    block.margins = (
        u16::try_from(head.margins.0).unwrap_or(u16::MAX),
        u16::try_from(head.margins.1).unwrap_or(u16::MAX),
    );
    block.dir = head.direction;
    block.writing_mode = head.writing_mode;
    block.lang = Lang::parse(head.lang);
    block.drop_cap = u8::try_from(head.drop_cap).unwrap_or(u8::MAX);
    block
}

/// The joined block, checked: the text is what the head said, runs and
/// annotations lie inside it on character boundaries and runs are sorted.
fn finish(mut block: Block, langs: &[Lang], expect: usize) -> Result<Block, Error> {
    if block.text.len() != expect {
        return Err(Error::Invalid);
    }
    let text = &block.text;
    let ok = |at: u32| {
        let at = at as usize;
        at <= text.len() && text.is_char_boundary(at)
    };
    let mut before = 0u32;
    for run in &block.runs {
        if run.start < before || run.end < run.start || !ok(run.start) || !ok(run.end) {
            return Err(Error::Invalid);
        }
        before = run.end;
    }
    if block
        .annotations
        .iter()
        .any(|a| a.end < a.start || !ok(a.start) || !ok(a.end))
    {
        return Err(Error::Invalid);
    }
    if !langs.is_empty() {
        let mut kept = Vec::new();
        kept.try_reserve_exact(langs.len())
            .map_err(|_| Error::NoMemory)?;
        kept.extend_from_slice(langs);
        block.inline_langs = Some(kept.into_boxed_slice());
    }
    Ok(block)
}

#[cfg(test)]
mod tests;
