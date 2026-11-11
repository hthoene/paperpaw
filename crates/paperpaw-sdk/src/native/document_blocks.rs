//! The block stream of an open document (`docs/abi/document.md` 4, op 6): the
//! block at or after a position, in fragments the opener joins.
use super::*;
use crate::document::{Align, BlockKind, TextStyle, VerticalAlign};
use paperpaw_lang::{Direction, WritingMode};

/// `service.end` flag: the last block of the section.
const END_LAST_BLOCK: u32 = 1;
/// `service.end` flag: the block goes on in another fragment.
const END_MORE: u32 = 2;
/// Block flags (`document.md` 4).
const FLAG_CONTINUES: u32 = 1;
const FLAG_CONTINUED: u32 = 2;
const FLAG_HEADER: u32 = 1 << 8;
const FLAG_FIRST: u32 = 1 << 9;
const FLAG_LAST: u32 = 1 << 10;

/// Where a block's next fragment starts: the record index and the block's
/// identity. Start a block with [`BlockCursor::start`]; continue only with the
/// cursor [`BlockFragment::continuation`] returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockCursor {
    record: u32,
    generation: u32,
}
impl BlockCursor {
    /// The first fragment of the block at a position.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            record: 0,
            generation: 0,
        }
    }
}

/// The header of a block (`doc_block`), borrowed from the reply buffer. A value
/// the model leaves to the reader reads as `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockHead<'a>(wire::ServiceDocBlock<'a>);
impl<'a> BlockHead<'a> {
    /// What the block is. A kind a later format adds is [`BlockKind::Extended`];
    /// one that does not fit reads as a paragraph.
    /// A heading level outside 1 to 6 is clamped into it, without an error: the
    /// host checks the level, so the facade only keeps it in range.
    #[must_use]
    pub fn kind(&self) -> BlockKind {
        match self.0.kind {
            0 => BlockKind::Paragraph,
            1 => BlockKind::Heading(u8::try_from(self.0.level).unwrap_or(6).clamp(1, 6)),
            2 => BlockKind::Preformatted,
            3 => BlockKind::Image,
            4 => BlockKind::TableRow,
            n => u16::try_from(n - 5).map_or(BlockKind::Paragraph, BlockKind::Extended),
        }
    }
    /// The next block's first source offset is [`end`](Self::end); this block
    /// starts at this source offset.
    #[must_use]
    pub const fn start(&self) -> u32 {
        self.0.start
    }
    /// The source offset after the block.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.0.end
    }
    /// The block continues the previous one (the format's own split of a long paragraph).
    #[must_use]
    pub const fn continues(&self) -> bool {
        self.0.flags & FLAG_CONTINUES != 0
    }
    /// The next block continues this one.
    #[must_use]
    pub const fn continued(&self) -> bool {
        self.0.flags & FLAG_CONTINUED != 0
    }
    /// A table row: it is a header row.
    #[must_use]
    pub const fn header_row(&self) -> bool {
        self.0.flags & FLAG_HEADER != 0
    }
    /// A table row: it is the first row.
    #[must_use]
    pub const fn first_row(&self) -> bool {
        self.0.flags & FLAG_FIRST != 0
    }
    /// A table row: it is the last row.
    #[must_use]
    pub const fn last_row(&self) -> bool {
        self.0.flags & FLAG_LAST != 0
    }
    /// How the lines are aligned; a value this build does not know is auto.
    #[must_use]
    pub fn align(&self) -> Align {
        Align::from_bits(u8::try_from(self.0.align).unwrap_or(0))
    }
    /// The first line's indent, `None` for the reader's.
    #[must_use]
    pub const fn indent(&self) -> Option<i32> {
        if self.0.indent == i32::MIN {
            None
        } else {
            Some(self.0.indent)
        }
    }
    /// The space after the block, `None` for the reader's.
    #[must_use]
    pub const fn space(&self) -> Option<u32> {
        unset(self.0.space)
    }
    /// The text size, `None` for the reader's.
    #[must_use]
    pub const fn size(&self) -> Option<u32> {
        unset(self.0.size)
    }
    /// The line height, `None` for the reader's.
    #[must_use]
    pub const fn line_height(&self) -> Option<u32> {
        unset(self.0.line_height)
    }
    /// The margins at the start and the end.
    #[must_use]
    pub const fn margins(&self) -> (u32, u32) {
        (self.0.margin_start, self.0.margin_end)
    }
    /// The direction; a value this build does not know is auto.
    #[must_use]
    pub fn direction(&self) -> Direction {
        match self.0.dir {
            1 => Direction::Ltr,
            2 => Direction::Rtl,
            _ => Direction::Auto,
        }
    }
    /// The writing mode; a value this build does not know is horizontal.
    #[must_use]
    pub fn writing_mode(&self) -> WritingMode {
        match self.0.writing_mode {
            1 => WritingMode::VerticalRl,
            2 => WritingMode::VerticalLr,
            3 => WritingMode::SidewaysRl,
            4 => WritingMode::SidewaysLr,
            _ => WritingMode::HorizontalTb,
        }
    }
    /// The block's language as a BCP 47 tag, empty when unknown.
    #[must_use]
    pub const fn lang(&self) -> &'a str {
        self.0.lang
    }
    /// How many characters the drop cap spans, 0 for none.
    #[must_use]
    pub const fn drop_cap(&self) -> u32 {
        self.0.drop_cap
    }
    /// The whole length of the block's text, which its [`BlockRecord::Text`]
    /// parts add up to over all fragments. It is not checked against the parts
    /// here: clamp it before slicing.
    #[must_use]
    pub const fn text_bytes(&self) -> u32 {
        self.0.text_bytes
    }
}
const fn unset(value: u32) -> Option<u32> {
    if value == u32::MAX { None } else { Some(value) }
}

/// A run of the block's text with one style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockRun(wire::ServiceDocRun);
impl BlockRun {
    /// The first byte of the run in the block's text.
    #[must_use]
    pub const fn start(&self) -> u32 {
        self.0.start
    }
    /// The byte after the run.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.0.end
    }
    /// The run's style.
    #[must_use]
    pub const fn style(&self) -> TextStyle {
        TextStyle::from_bits(self.0.style)
    }
}

/// A checkpoint tying a byte of the block's text to a source offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockMap(wire::ServiceDocMap);
impl BlockMap {
    /// A byte of the block's text.
    #[must_use]
    pub const fn text(&self) -> u32 {
        self.0.text
    }
    /// The source offset it came from.
    #[must_use]
    pub const fn source(&self) -> u32 {
        self.0.source
    }
}

/// The picture of an image block (`doc_image`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockImage(wire::ServiceDocImage);
impl BlockImage {
    /// The host's drawing handle; it may end, ask again with [`index`](Self::index).
    #[must_use]
    pub const fn image(&self) -> u32 {
        self.0.image
    }
    /// The format's own number of the picture, stable for the document's life.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.0.index
    }
    /// The picture's width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.0.width
    }
    /// The picture's height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.0.height
    }
    /// The picture is in colour.
    #[must_use]
    pub const fn colour(&self) -> bool {
        self.0.flags & 1 != 0
    }
    /// The picture is not decoded yet.
    #[must_use]
    pub const fn pending(&self) -> bool {
        self.0.flags & 2 != 0
    }
}

/// A cell of a table row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockCell(wire::ServiceDocCell);
impl BlockCell {
    /// The byte after the cell's text in the block's text.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.0.end
    }
    /// How many columns it spans.
    #[must_use]
    pub const fn span(&self) -> u32 {
        self.0.span
    }
    /// How many rows it spans.
    #[must_use]
    pub const fn rows(&self) -> u32 {
        self.0.rows
    }
    /// How its lines are aligned; a value this build does not know is auto.
    #[must_use]
    pub fn align(&self) -> Align {
        Align::from_bits(u8::try_from(self.0.align).unwrap_or(0))
    }
    /// Where its text sits in the cell; a value this build does not know is the top.
    #[must_use]
    pub const fn vertical(&self) -> VerticalAlign {
        match self.0.vertical {
            1 => VerticalAlign::Middle,
            2 => VerticalAlign::Bottom,
            _ => VerticalAlign::Top,
        }
    }
    /// The width weight of its column.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.0.width
    }
}

/// A ruby, phonetic or gloss annotation over a range of the block's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockRuby<'a>(wire::ServiceDocRuby<'a>);
impl<'a> BlockRuby<'a> {
    /// The first byte of the annotated text.
    #[must_use]
    pub const fn start(&self) -> u32 {
        self.0.start
    }
    /// The byte after the annotated text.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.0.end
    }
    /// What kind of annotation; a value this build does not know is a gloss.
    #[must_use]
    pub fn kind(&self) -> crate::document::AnnotationKind {
        use crate::document::AnnotationKind;
        match self.0.kind {
            0 => AnnotationKind::Ruby,
            1 => AnnotationKind::Phonetic,
            2 => AnnotationKind::Gloss,
            n => u16::try_from(n - 3).map_or(AnnotationKind::Gloss, AnnotationKind::Other),
        }
    }
    /// Its size as a percent of the text below, 0 for the reader's.
    #[must_use]
    pub const fn size(&self) -> u32 {
        self.0.size
    }
    /// The annotation's text.
    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.0.text
    }
}

/// A table row's column weights, at most 16, in the order of its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockColumns<'a>(&'a [u8]);
impl BlockColumns<'_> {
    /// How many columns the row has.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len() / 2
    }
    /// The row has no column weights.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The weights of the columns, in order.
    pub fn weights(&self) -> impl Iterator<Item = u16> + use<'_> {
        self.0
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
    }
}

/// One record of a block's list, in the order of `document.md` 4.
///
/// The facade checks the framing of a block, not what its numbers mean: the
/// offsets and lengths of runs, maps, cells and rubies and `text_bytes` are the
/// format's, checked against the text only by the host. A reader clamps them to
/// the text it joined before it slices with them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockRecord<'a> {
    /// The header; only in the block's first fragment.
    Head(BlockHead<'a>),
    /// A part of the text (at most 3,072 bytes); the parts joined in order are the text.
    Text(&'a str),
    /// A styled run.
    Run(BlockRun),
    /// An offset checkpoint.
    Map(BlockMap),
    /// The picture of an image block.
    Image(BlockImage),
    /// A table row's column weights (at most 16).
    Columns(BlockColumns<'a>),
    /// A cell of a table row.
    Cell(BlockCell),
    /// An annotation.
    Ruby(BlockRuby<'a>),
    /// A language of a run: the n-th (from 1) is what `TextStyle::lang_index` n names.
    InlineLang(&'a str),
}

/// One fragment of a block's record list, validated in full before any record is
/// exposed, borrowing the caller's reply buffer.
#[derive(Debug)]
pub struct BlockFragment<'a> {
    bytes: &'a [u8],
    count: usize,
    first: bool,
    more: Option<BlockCursor>,
    last_in_section: bool,
    next: u32,
}

impl<'a> BlockFragment<'a> {
    fn decode(bytes: &'a [u8], cursor: BlockCursor) -> Result<Self, Error> {
        let mut rest = bytes;
        let mut count = 0usize;
        let mut heads = 0usize;
        let end = loop {
            let (record, n) = RawRecord::prefix(rest).map_err(|_| Error::Invalid)?;
            rest = rest.get(n..).ok_or(Error::Invalid)?;
            if record.tag() == wire::ServiceEnd::TAG {
                break record
                    .decode::<wire::ServiceEnd>()
                    .map_err(|_| Error::Invalid)?;
            }
            // The header is the first record of the first fragment and nowhere else.
            let is_head = record.tag() == wire::ServiceDocBlock::TAG;
            if is_head {
                heads += 1;
            }
            if is_head != (count == 0 && cursor.record == 0) {
                return Err(Error::Invalid);
            }
            record_of(record)?;
            count += 1;
        };
        if !rest.is_empty() || heads != usize::from(cursor.record == 0) {
            return Err(Error::Invalid);
        }
        let more = end.flags & END_MORE != 0;
        if end.flags & !(END_LAST_BLOCK | END_MORE) != 0
            || (more && end.flags & END_LAST_BLOCK != 0)
        {
            return Err(Error::Invalid);
        }
        let seen = u32::try_from(count).map_err(|_| Error::Invalid)?;
        // A block whose identity changed under a continuation is State, like a
        // page whose list changed: the reader starts the block again.
        if cursor.generation != 0 && end.generation != 0 && cursor.generation != end.generation {
            return Err(Error::State);
        }
        let continuation = if more {
            // The record the next fragment starts from, under the block's identity.
            if end.generation == 0
                || count == 0
                || end.next != cursor.record.checked_add(seen).ok_or(Error::Invalid)?
            {
                return Err(Error::Invalid);
            }
            Some(BlockCursor {
                record: end.next,
                generation: end.generation,
            })
        } else {
            None
        };
        Ok(Self {
            bytes,
            count,
            first: cursor.record == 0,
            more: continuation,
            last_in_section: end.flags & END_LAST_BLOCK != 0,
            next: end.next,
        })
    }

    /// Number of records in this fragment.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether this fragment has no record.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// This is the block's first fragment (it starts with the header).
    #[must_use]
    pub const fn is_first(&self) -> bool {
        self.first
    }
    /// Where the block goes on, or `None` when this fragment ends it. Ask again
    /// with this cursor and the same position.
    #[must_use]
    pub const fn continuation(&self) -> Option<BlockCursor> {
        self.more
    }
    /// The block is the last of its section; only meaningful where the block ends here.
    #[must_use]
    pub const fn last_in_section(&self) -> bool {
        self.more.is_none() && self.last_in_section
    }
    /// The source offset of the next block, where the block ends in this fragment.
    #[must_use]
    pub const fn next_block(&self) -> Option<u32> {
        if self.more.is_none() {
            Some(self.next)
        } else {
            None
        }
    }
    /// Iterate the borrowed records without allocation.
    pub fn records(&self) -> impl Iterator<Item = BlockRecord<'a>> + use<'a> {
        let (mut bytes, mut left) = (self.bytes, self.count);
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let (record, n) = RawRecord::prefix(bytes).ok()?;
            bytes = bytes.get(n..)?;
            record_of(record).ok()
        })
    }
}

/// A record of the list as its typed view; an unknown record is Invalid.
fn record_of(record: RawRecord<'_>) -> Result<BlockRecord<'_>, Error> {
    let bad = |_| Error::Invalid;
    Ok(match record.tag() {
        wire::ServiceDocBlock::TAG => BlockRecord::Head(BlockHead(record.decode().map_err(bad)?)),
        wire::ServiceDocTextPart::TAG => BlockRecord::Text(
            record
                .decode::<wire::ServiceDocTextPart<'_>>()
                .map_err(bad)?
                .text,
        ),
        wire::ServiceDocRun::TAG => BlockRecord::Run(BlockRun(record.decode().map_err(bad)?)),
        wire::ServiceDocMap::TAG => BlockRecord::Map(BlockMap(record.decode().map_err(bad)?)),
        wire::ServiceDocImage::TAG => BlockRecord::Image(BlockImage(record.decode().map_err(bad)?)),
        wire::ServiceDocColumns::TAG => {
            let weights = record
                .decode::<wire::ServiceDocColumns<'_>>()
                .map_err(bad)?
                .weights;
            // Little-endian `u16`s, at most 16.
            if weights.len() % 2 != 0 || weights.len() > 32 {
                return Err(Error::Invalid);
            }
            BlockRecord::Columns(BlockColumns(weights))
        }
        wire::ServiceDocCell::TAG => BlockRecord::Cell(BlockCell(record.decode().map_err(bad)?)),
        wire::ServiceDocRuby::TAG => BlockRecord::Ruby(BlockRuby(record.decode().map_err(bad)?)),
        wire::ServiceDocInlineLang::TAG => BlockRecord::InlineLang(
            record
                .decode::<wire::ServiceDocInlineLang<'_>>()
                .map_err(bad)?
                .lang,
        ),
        _ => return Err(Error::Invalid),
    })
}

impl Documents {
    /// Read one fragment of the block at or after `at` (op 6). A block travels in
    /// fragments of whole records; start with [`BlockCursor::start`], and while
    /// [`BlockFragment::continuation`] is `Some`, ask again for the same position
    /// with that cursor. The reader joins the fragments into one block. When the
    /// block ends, [`BlockFragment::next_block`] is where the next block starts.
    /// `input` is 256 bytes; `out` 4,096.
    ///
    /// # Errors
    /// Invalid for a malformed fragment (an unknown record, a header anywhere but
    /// first, a continuation that does not follow); NotFound for a section past the
    /// end; State for a continuation whose block changed (start again from
    /// [`BlockCursor::start`]); Busy while the document is preparing; otherwise the
    /// host's refusals.
    pub fn block_at<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        at: DocPosition,
        cursor: BlockCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<BlockFragment<'a>, Error> {
        let ask = wire::ServiceDocAt {
            document: document.0,
            section: at.section,
            offset: at.offset,
            insert: 0,
            cursor: cursor.record,
            generation: cursor.generation,
        };
        let n = self.call(host, schema::OP_BLOCK, &[&ask], input, out)?;
        BlockFragment::decode(out.get(..n).ok_or(Error::Invalid)?, cursor)
    }

    /// Has the format decode picture `index` of `document` now (op 16,
    /// `document.md` 5): for a picture whose block came without an image id
    /// (`BlockImage::image` 0: none was free, or it was not decoded yet) or
    /// whose id has ended (`NotFound` at a draw). `side` 0 is the page's
    /// size, else the longest side for a zoom (at most 1,600; larger is
    /// clamped). The picture with its new image id, or the live one where
    /// nothing was to do. `input` is 64 bytes; `out` 4,096.
    ///
    /// # Errors
    /// Busy while all of the plugin's image ids are out and none drawn yet
    /// (draw, then ask again); NotFound for an index the document does not
    /// have; Invalid for a malformed reply; otherwise the host's refusals.
    pub fn ask_image(
        &self,
        host: &Host<'_>,
        document: DocId,
        (index, side): (u32, u32),
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<BlockImage, Error> {
        let ask = wire::ServiceDocImageAsk {
            document: document.0,
            index,
            side,
        };
        let n = self.call(host, schema::OP_IMAGE, &[&ask], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceDocImage>()
            .map_err(|_| Error::Invalid)?;
        if row.index != index {
            return Err(Error::Invalid);
        }
        Ok(BlockImage(row))
    }
}
