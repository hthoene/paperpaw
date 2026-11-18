//! The block stream of `core.format` op 8 (`document.md` 4): a block of the
//! document model as a record list, written in fragments that end between records.
use super::lists::Budget;
use super::{Error, FormatCall, FormatOp, schema};
use crate::document::{AnnotationKind, Block, BlockKind, MAX_COLUMNS, VerticalAlign};
use paperpaw_lang::{Direction, Lang, WritingMode};

/// `fmt.end` flag: the last block of the section.
const END_LAST_BLOCK: u32 = 1;
/// `fmt.end` flag: the block goes on in another fragment.
const END_MORE: u32 = 2;
/// Block flag: the block continues the previous one.
const FLAG_CONTINUES: u32 = 1;
/// Block flag: the next block continues this one.
const FLAG_CONTINUED: u32 = 2;
/// Block flags of a table row: header, first and last row.
const FLAG_HEADER: u32 = 1 << 8;
const FLAG_FIRST: u32 = 1 << 9;
const FLAG_LAST: u32 = 1 << 10;
/// Most inline languages of a block (`document.md` 4).
const INLINE_LANGS: usize = 15;
/// The longest tag [`tag`] writes: language, script and region.
const TAG_BYTES: usize = 16;

/// A block to answer with, and where it sits: the format's own picture number
/// for an image block, whether it is the last block of its section and the source
/// offset of the next block.
///
/// The record limits of `document.md` 4 apply without a refusal: an annotation's
/// text is cut at 1,024 bytes (on a character boundary) and only the first 15
/// inline languages are written.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct BlockReply<'a> {
    block: &'a Block,
    image_index: u32,
    last_in_section: bool,
    next: u32,
}
impl<'a> BlockReply<'a> {
    /// The reply for `block`, which is not the last of its section, with no picture
    /// index and the next block at the block's own end.
    #[must_use]
    pub fn new(block: &'a Block) -> Self {
        Self {
            block,
            image_index: 0,
            last_in_section: false,
            next: block.end,
        }
    }
    /// The format's own number of the block's picture (op 18's `index`).
    #[must_use]
    pub const fn image_index(mut self, index: u32) -> Self {
        self.image_index = index;
        self
    }
    /// The block is the last of its section.
    #[must_use]
    pub const fn last_in_section(mut self) -> Self {
        self.last_in_section = true;
        self
    }
    /// The source offset of the next block, where the host asks next.
    #[must_use]
    pub const fn next(mut self, offset: u32) -> Self {
        self.next = offset;
        self
    }
}

impl FormatCall<'_> {
    /// Write one fragment of the block's record list into `out`, from the record
    /// the call's `cursor` names (0 for the first). Whole records only; the fragment
    /// ends with `fmt.end`: `more` with the next record index and the block's
    /// generation when the block goes on (the host asks again with them), else the
    /// last-block flag and the next block's offset. Nothing allocates, and the
    /// list is the same on every call for the same block.
    ///
    /// # Errors
    /// State outside `block`, or for a continuation whose generation is not this
    /// block's (the block changed: the host starts again); Invalid for a cursor past
    /// the record list; NoSpace when `out` cannot hold the first record asked for and
    /// the closing record (3,840 bytes always do).
    pub fn reply_block(&self, reply: &BlockReply<'_>, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::Block])?;
        let block = reply.block;
        let generation = identity(self.section(), block.start, self.conversion());
        let cursor = if self.cursor() == 0 {
            0
        } else if self.generation() == generation {
            self.cursor()
        } else {
            return Err(Error::State);
        };
        let mut weights = [0u8; 2 * MAX_COLUMNS];
        let columns = columns(block, &mut weights);
        let mut walk = Walk {
            block,
            image_index: reply.image_index,
            columns,
            maps: block.map.checkpoints(),
            phase: Phase::Head,
            index: 0,
            position: 0,
        };
        let mut scratch = Scratch::default();
        for _ in 0..cursor {
            if walk.next(&mut scratch).is_none() {
                return Err(Error::Invalid);
            }
        }
        let mut budget = Budget::new(out.len().min(schema::MAX_REPLY)).ok_or(Error::NoSpace)?;
        let mut writer = schema::ReplyWriter::new(out).map_err(Error::from_status)?;
        let mut next = cursor;
        let mut more = false;
        while let Some(record) = walk.next(&mut scratch) {
            if !budget.take(bound(&record)) {
                more = true;
                break;
            }
            writer.push(&record).map_err(Error::from_status)?;
            next += 1;
        }
        if more && next == cursor {
            // The first record of this fragment does not fit.
            return Err(Error::NoSpace);
        }
        let end = if more {
            schema::FmtEnd::new(END_MORE, 0, generation, next, None)
        } else {
            let last = if reply.last_in_section {
                END_LAST_BLOCK
            } else {
                0
            };
            schema::FmtEnd::new(last, 0, generation, reply.next, None)
        };
        writer
            .push(&schema::Record::FmtEnd(end))
            .map_err(Error::from_status)?;
        writer.finish().map_err(Error::from_status)
    }
}

/// The block's identity across fragments: a hash of its section, its start and the
/// conversion, never 0 (`document.md` 4).
fn identity(section: u32, start: u32, conversion: u64) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for byte in section
        .to_le_bytes()
        .into_iter()
        .chain(start.to_le_bytes())
        .chain(conversion.to_le_bytes())
    {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash.max(1)
}

/// The table's column weights as little-endian `u16`s in `out`, at most 16.
fn columns<'o>(block: &Block, out: &'o mut [u8; 2 * MAX_COLUMNS]) -> &'o [u8] {
    let Some(table) = block.table.as_deref() else {
        return &[];
    };
    let mut used = 0;
    let (slots, _) = out.as_chunks_mut::<2>();
    for (weight, slot) in table.columns.iter().zip(slots) {
        *slot = weight.to_le_bytes();
        used += 2;
    }
    &out[..used]
}

/// A BCP 47 tag of a language, written into `buf` (language, script, region).
fn tag(lang: Lang, buf: &mut [u8; TAG_BYTES]) -> &str {
    let mut used = 0;
    let code = lang.language();
    let parts = [Some(code.as_str()), lang.script(), lang.region()];
    for part in parts.into_iter().flatten().filter(|p| !p.is_empty()) {
        let dash = usize::from(used > 0);
        let Some(room) = buf.get_mut(used..used + dash + part.len()) else {
            break;
        };
        if dash == 1 {
            room[0] = b'-';
        }
        room[dash..].copy_from_slice(part.as_bytes());
        used += dash + part.len();
    }
    core::str::from_utf8(&buf[..used]).unwrap_or("")
}

#[derive(Default)]
struct Scratch {
    tag: [u8; TAG_BYTES],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Head,
    Text,
    Runs,
    Maps,
    Image,
    Columns,
    Cells,
    Rubies,
    Langs,
}

/// The block's record list, one record per step, in the order of `document.md` 4.
struct Walk<'b, M> {
    block: &'b Block,
    image_index: u32,
    columns: &'b [u8],
    maps: M,
    phase: Phase,
    index: usize,
    position: usize,
}
impl<'b, M: Iterator<Item = (u32, u32)>> Walk<'b, M> {
    /// The next record, or `None` at the end of the list. A record borrows the
    /// block or `scratch` and is written before the next step.
    fn next<'s>(&mut self, scratch: &'s mut Scratch) -> Option<schema::Record<'s>>
    where
        'b: 's,
    {
        let block = self.block;
        loop {
            match self.phase {
                Phase::Head => {
                    self.phase = Phase::Text;
                    return Some(schema::Record::FmtBlock(head(
                        block,
                        tag(block.lang, &mut scratch.tag),
                    )));
                }
                Phase::Text => {
                    if let Some(rest) = block.text.get(self.position..).filter(|r| !r.is_empty()) {
                        let part = cut(rest, schema::FMT_TEXT_PART_TEXT_MAX_BYTES);
                        self.position += part.len();
                        return Some(schema::Record::FmtTextPart(schema::FmtTextPart {
                            text: part,
                        }));
                    }
                    self.phase = Phase::Runs;
                }
                Phase::Runs => {
                    if let Some(run) = block.runs.get(self.index) {
                        self.index += 1;
                        return Some(schema::Record::FmtRun(schema::FmtRun {
                            start: run.start,
                            end: run.end,
                            style: run.style.bits(),
                        }));
                    }
                    self.phase = Phase::Maps;
                }
                Phase::Maps => {
                    if let Some((text, source)) = self.maps.next() {
                        return Some(schema::Record::FmtMap(schema::FmtMap { text, source }));
                    }
                    self.phase = Phase::Image;
                }
                Phase::Image => {
                    self.phase = Phase::Columns;
                    if let Some(image) = &block.image {
                        return Some(schema::Record::FmtImage(schema::FmtImage {
                            path: &image.path,
                            index: self.image_index,
                            width: image.width,
                            height: image.height,
                            flags: 0,
                        }));
                    }
                }
                Phase::Columns => {
                    self.phase = Phase::Cells;
                    self.index = 0;
                    if block.table.is_some() {
                        return Some(schema::Record::FmtColumns(schema::FmtColumns {
                            weights: self.columns,
                        }));
                    }
                }
                Phase::Cells => {
                    if let Some(record) = cell(block, self.index) {
                        self.index += 1;
                        return Some(record);
                    }
                    self.phase = Phase::Rubies;
                    self.index = 0;
                }
                Phase::Rubies => {
                    if let Some(record) = ruby(block, self.index) {
                        self.index += 1;
                        return Some(record);
                    }
                    self.phase = Phase::Langs;
                    self.index = 0;
                }
                Phase::Langs => {
                    let lang = block
                        .inline_langs
                        .as_deref()
                        .and_then(|langs| langs.get(self.index))
                        .filter(|_| self.index < INLINE_LANGS)?;
                    self.index += 1;
                    return Some(schema::Record::FmtInlineLang(schema::FmtInlineLang {
                        lang: tag(*lang, &mut scratch.tag),
                    }));
                }
            }
        }
    }
}

/// The `index`th cell of a table row: its end, spans, alignments and width weight.
fn cell(block: &Block, index: usize) -> Option<schema::Record<'_>> {
    let table = block.table.as_deref()?;
    let cell = table.cells.get(index).filter(|_| index < MAX_COLUMNS)?;
    Some(schema::Record::FmtCell(schema::FmtCell {
        end: cell.end,
        span: u32::from(cell.span),
        rows: u32::from(cell.rows),
        align: u32::from(cell.align.bits()),
        vertical: match cell.vertical {
            VerticalAlign::Middle => 1,
            VerticalAlign::Bottom => 2,
            _ => 0,
        },
        width: table.weights().nth(index).unwrap_or(1),
    }))
}

/// The `index`th annotation of a block. Its text is cut at 1,024 bytes.
fn ruby(block: &Block, index: usize) -> Option<schema::Record<'_>> {
    let note = block.annotations.get(index)?;
    Some(schema::Record::FmtRuby(schema::FmtRuby {
        start: note.start,
        end: note.end,
        kind: match note.kind {
            AnnotationKind::Ruby => 0,
            AnnotationKind::Phonetic => 1,
            AnnotationKind::Other(n) => 3 + u32::from(n),
            _ => 2,
        },
        size: u32::from(note.size),
        text: cut(&note.text, schema::FMT_RUBY_TEXT_MAX_BYTES),
    }))
}

/// The block's header record (`document.md` 4): `i32::MIN` and `u32::MAX` say "the
/// reader's" for what the model leaves unset.
fn head<'a>(block: &Block, lang: &'a str) -> schema::FmtBlock<'a> {
    let (kind, level) = match block.kind {
        BlockKind::Paragraph => (0, 0),
        BlockKind::Heading(level) => (1, u32::from(level)),
        BlockKind::Preformatted => (2, 0),
        BlockKind::Image => (3, 0),
        BlockKind::TableRow => (4, 0),
        BlockKind::Extended(n) => (5 + u32::from(n), 0),
        _ => (0, 0),
    };
    let mut flags = 0;
    if block.continues {
        flags |= FLAG_CONTINUES;
    }
    if block.continued {
        flags |= FLAG_CONTINUED;
    }
    if let Some(table) = block.table.as_deref() {
        flags |= if table.header { FLAG_HEADER } else { 0 }
            | if table.first { FLAG_FIRST } else { 0 }
            | if table.last { FLAG_LAST } else { 0 };
    }
    schema::FmtBlock {
        kind,
        level,
        flags,
        start: block.start,
        end: block.end,
        align: u32::from(block.align.bits()),
        indent: block.indent.map_or(i32::MIN, i32::from),
        space: block.space.map_or(u32::MAX, u32::from),
        size: block.size.map_or(u32::MAX, u32::from),
        margin_start: u32::from(block.margins.0),
        margin_end: u32::from(block.margins.1),
        line_height: block.line_height.map_or(u32::MAX, u32::from),
        dir: match block.dir {
            Direction::Ltr => 1,
            Direction::Rtl => 2,
            _ => 0,
        },
        writing_mode: match block.writing_mode {
            WritingMode::VerticalRl => 1,
            WritingMode::VerticalLr => 2,
            WritingMode::SidewaysRl => 3,
            WritingMode::SidewaysLr => 4,
            _ => 0,
        },
        lang,
        drop_cap: u32::from(block.drop_cap),
        text_bytes: u32::try_from(block.text.len()).unwrap_or(u32::MAX),
    }
}

/// An upper bound of a record's bytes on the wire: its framing, its fixed fields
/// and its strings with their length prefixes.
fn bound(record: &schema::Record<'_>) -> usize {
    let framing = 5;
    framing
        + match record {
            schema::Record::FmtBlock(b) => 16 * 4 + 2 + b.lang.len(),
            schema::Record::FmtTextPart(t) => 2 + t.text.len(),
            schema::Record::FmtRun(_) => 12,
            schema::Record::FmtMap(_) => 8,
            schema::Record::FmtImage(i) => 16 + 2 + i.path.len(),
            schema::Record::FmtColumns(c) => 2 + c.weights.len(),
            schema::Record::FmtCell(_) => 24,
            schema::Record::FmtRuby(r) => 16 + 2 + r.text.len(),
            schema::Record::FmtInlineLang(l) => 2 + l.lang.len(),
            _ => 64,
        }
}

fn cut(text: &str, cap: usize) -> &str {
    let mut end = text.len().min(cap);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}
