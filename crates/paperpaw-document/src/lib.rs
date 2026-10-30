//! The format-independent document model of PaperPaw.
//!
//! Format plugins (TXT, EPUB, FB2, …) turn files into [`Document`]s; layout
//! engines turn documents into pages. Neither side knows the other.
//!
//! - A document consists of **sections** (one for TXT, one per spine item
//!   for EPUB).
//! - Each section is a sequence of **blocks** (paragraphs, headings, …)
//!   with normalized text.
//! - A [`Locator`] identifies a position by section and *source offset*, a
//!   position in the original file data, which stays valid regardless of
//!   fonts, screen size or text normalization (see ADR 0007).
//!
//! Documents are read block by block, so even very large books need only a
//! few kilobytes of RAM.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_platform::StorageError;

pub use paperpaw_lang::{Direction, Lang, WritingMode};

pub use paperpaw_platform::{DirEntry, NoStorage, Storage};

/// A reading position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Locator {
    pub section: u32,
    /// Offset in the section's source data (e.g. bytes of a TXT file).
    pub offset: u32,
}

impl Locator {
    pub const START: Locator = Locator {
        section: 0,
        offset: 0,
    };

    pub const fn new(section: u32, offset: u32) -> Self {
        Self { section, offset }
    }
}

/// Bibliographic information.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    /// BCP 47 language tag, e.g. `de` or `en-GB`.
    pub language: Option<String>,
    /// The series the book belongs to, if it says so.
    pub series: Option<String>,
    /// Its number in the series, e.g. `2` or `2.5`, as written.
    pub series_index: Option<String>,
    /// Its subjects or tags, e.g. an EPUB's `dc:subject` (where Calibre
    /// puts its tags).
    pub subjects: Vec<String>,
    /// What the book is about, as plain text (a blurb), if it says.
    pub description: Option<String>,
    /// The way its pages go: right to left in manga and in books in
    /// Arabic or Hebrew.
    pub direction: PageProgression,
    /// How its lines go: across, or down in columns (an EPUB's
    /// `primary-writing-mode`).
    pub writing_mode: WritingMode,
}

/// The way a book's pages go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PageProgression {
    #[default]
    LeftToRight,
    RightToLeft,
}

impl Metadata {
    /// Its lines going as `writing_mode` says.
    pub fn writing_mode(mut self, writing_mode: WritingMode) -> Self {
        self.writing_mode = writing_mode;
        self
    }

    /// Its pages going `direction`.
    pub fn direction(mut self, direction: PageProgression) -> Self {
        self.direction = direction;
        self
    }

    pub fn title(mut self, title: impl Into<Option<String>>) -> Self {
        self.title = title.into();
        self
    }

    pub fn authors(mut self, authors: Vec<String>) -> Self {
        self.authors = authors;
        self
    }

    pub fn language(mut self, language: impl Into<Option<String>>) -> Self {
        self.language = language.into();
        self
    }

    pub fn series(mut self, series: impl Into<Option<String>>) -> Self {
        self.series = series.into();
        self
    }

    pub fn series_index(mut self, series_index: impl Into<Option<String>>) -> Self {
        self.series_index = series_index.into();
        self
    }

    pub fn subjects(mut self, subjects: Vec<String>) -> Self {
        self.subjects = subjects;
        self
    }

    pub fn description(mut self, description: impl Into<Option<String>>) -> Self {
        self.description = description.into();
        self
    }
}

/// The role of a block; layout engines style blocks by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BlockKind {
    Paragraph,
    /// Heading level 1 (most important) to 6.
    Heading(u8),
    /// Text whose line breaks and spacing must be kept.
    Preformatted,
    /// An image ([`Block::image`]); `text` is its alternative text.
    Image,
}

/// A decoded image in storage, in the `paperpaw-image` gray format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// The character style of a run of text: bold, italic, small capitals,
/// superscript or subscript, underlined or struck through, the font
/// family embedded in the document, if any (see [`Document::fonts`]), and
/// the language of the run if it is not the block's (see
/// [`Block::inline_langs`]).
///
/// Bits 12 to 23 and 28 to 31 are reserved: zero now. A meaning is given
/// to one of them only with a new `paperpaw_layout::VERSION`, never by
/// reading a bit already set differently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextStyle(u32);

/// Most embedded font families a document can use.
pub const MAX_FAMILIES: u8 = 31;

/// Most languages a block's runs can name besides the block's own
/// ([`Block::inline_langs`]).
pub const MAX_INLINE_LANGS: u8 = 15;

/// The bits of everything but the family (bits 2 to 6) and the inline
/// language (bits 24 to 27).
const FLAGS: u32 = 0b1111_1000_0011;

/// Where the inline language's index starts.
const LANG_SHIFT: u32 = 24;

impl TextStyle {
    pub const REGULAR: TextStyle = TextStyle(0);
    pub const BOLD: TextStyle = TextStyle(1);
    pub const ITALIC: TextStyle = TextStyle(2);
    pub const BOLD_ITALIC: TextStyle = TextStyle(3);
    /// Small capitals: lower case letters set as smaller capitals.
    pub const SMALL_CAPS: TextStyle = TextStyle(0b1000_0000);
    /// Set smaller and raised, e.g. a footnote's mark.
    pub const SUPERSCRIPT: TextStyle = TextStyle(1 << 8);
    /// Set smaller and lowered, e.g. a chemical formula's numbers.
    pub const SUBSCRIPT: TextStyle = TextStyle(1 << 9);
    /// A line under the text.
    pub const UNDERLINE: TextStyle = TextStyle(1 << 10);
    /// A line through the text.
    pub const STRIKE: TextStyle = TextStyle(1 << 11);

    pub const fn bold(self) -> bool {
        self.0 & 1 != 0
    }

    pub const fn italic(self) -> bool {
        self.0 & 2 != 0
    }

    pub const fn small_caps(self) -> bool {
        self.0 & Self::SMALL_CAPS.0 != 0
    }

    pub const fn superscript(self) -> bool {
        self.0 & Self::SUPERSCRIPT.0 != 0
    }

    pub const fn subscript(self) -> bool {
        self.0 & Self::SUBSCRIPT.0 != 0
    }

    pub const fn underline(self) -> bool {
        self.0 & Self::UNDERLINE.0 != 0
    }

    pub const fn strike(self) -> bool {
        self.0 & Self::STRIKE.0 != 0
    }

    /// This style without what `other` sets (its family aside), e.g.
    /// without the lines a reader chose not to see.
    pub const fn without(self, other: TextStyle) -> TextStyle {
        TextStyle(self.0 & !(other.0 & FLAGS))
    }

    /// The embedded font family (1 to [`MAX_FAMILIES`]); 0 for the
    /// reader's font.
    pub const fn family(self) -> u8 {
        ((self.0 >> 2) & MAX_FAMILIES as u32) as u8
    }

    /// This style in embedded family `family` (0: the reader's font).
    pub const fn with_family(self, family: u8) -> TextStyle {
        let family = if family > MAX_FAMILIES { 0 } else { family };
        let mask = (MAX_FAMILIES as u32) << 2;
        TextStyle((self.0 & !mask) | ((family as u32) << 2))
    }

    /// The run's language: 1 to [`MAX_INLINE_LANGS`], counting from one,
    /// in its block's [`Block::inline_langs`]; 0 for the block's own
    /// [`Block::lang`]. See [`Block::lang_of`].
    pub const fn lang_index(self) -> u8 {
        ((self.0 >> LANG_SHIFT) & MAX_INLINE_LANGS as u32) as u8
    }

    /// This style in the block's inline language `index` (0: the block's
    /// own).
    pub const fn with_lang_index(self, index: u8) -> TextStyle {
        let index = if index > MAX_INLINE_LANGS { 0 } else { index };
        let mask = (MAX_INLINE_LANGS as u32) << LANG_SHIFT;
        TextStyle((self.0 & !mask) | ((index as u32) << LANG_SHIFT))
    }

    /// Both styles combined, e.g. bold text in an italic paragraph; the
    /// family and the language are `other`'s if it has them.
    pub const fn union(self, other: TextStyle) -> TextStyle {
        let family = if other.family() != 0 {
            other.family()
        } else {
            self.family()
        };
        let lang = if other.lang_index() != 0 {
            other.lang_index()
        } else {
            self.lang_index()
        };
        TextStyle((self.0 | other.0) & FLAGS)
            .with_family(family)
            .with_lang_index(lang)
    }

    /// For storing styles compactly.
    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn from_bits(bits: u32) -> TextStyle {
        TextStyle(bits)
    }
}

/// A run of styled text: byte range `start..end` of a block's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Run {
    pub start: u32,
    pub end: u32,
    pub style: TextStyle,
}

impl Run {
    pub const fn new(start: u32, end: u32, style: TextStyle) -> Run {
        Run { start, end, style }
    }
}

/// Horizontal alignment of a block's lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Align {
    /// As the reader prefers (justified or flush left).
    #[default]
    Auto,
    Start,
    Center,
    End,
    Justify,
}

impl Align {
    /// For storing alignments compactly.
    pub const fn bits(self) -> u8 {
        match self {
            Align::Auto => 0,
            Align::Start => 1,
            Align::Center => 2,
            Align::End => 3,
            Align::Justify => 4,
        }
    }

    pub const fn from_bits(bits: u8) -> Align {
        match bits {
            1 => Align::Start,
            2 => Align::Center,
            3 => Align::End,
            4 => Align::Justify,
            _ => Align::Auto,
        }
    }
}

/// A block of text, e.g. a paragraph.
///
/// `text` is normalized (decoded to UTF-8, whitespace collapsed); `map`
/// converts between byte indices into `text` and source offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Block {
    pub kind: BlockKind,
    /// The block continues the previous one (a very long paragraph split
    /// into pieces): no first-line indent, no paragraph spacing.
    pub continues: bool,
    /// The next block continues this one (see `continues`): what lies
    /// near its end is resolved with the next one read ahead (bidi, #33),
    /// which is read only then. Documents that do not say so leave it
    /// `false`, and the pieces are resolved without reading ahead.
    pub continued: bool,
    /// Source offset of the block's first byte.
    pub start: u32,
    /// Source offset just after the block.
    pub end: u32,
    pub text: String,
    pub map: OffsetMap,
    /// Styled runs of `text`, sorted and not overlapping; text outside any
    /// run is regular.
    pub runs: Vec<Run>,
    pub align: Align,
    /// The book's indent of the first line, in hundredths of an em
    /// (negative: it hangs out into [`Block::margins`]); `None` leaves it
    /// to the reader.
    pub indent: Option<i16>,
    /// The book's space above the block, in hundredths of an em; `None`
    /// leaves it to the reader.
    pub space: Option<u16>,
    /// The book's text size, in percent of body text; `None` leaves it to
    /// the reader (body text, or by heading level).
    pub size: Option<u16>,
    /// The book's margins at the start and the end of the lines (left and
    /// right in left-to-right text), in hundredths of an em: a block
    /// quote's, a poem's, a hanging indent's room. 0 for none.
    pub margins: (u16, u16),
    /// The book's distance between lines, in percent of the text size;
    /// `None` leaves it to the reader.
    pub line_height: Option<u16>,
    /// Room added above each of the block's lines, in hundredths of an em
    /// of its text, at most [`MAX_LEADING_ABOVE`]: for what a plugin's
    /// page layer draws over the lines (a translation between them, an
    /// interlinear reading), which a text filter asks for. The layout adds
    /// it to every line's height above its letters, so pages break and
    /// words are hit where they are drawn; layers find it as each line's
    /// `leading`. `None` (or 0) for none. Not the book's: formats leave it.
    pub leading_above: Option<u16>,
    /// The image of an [`BlockKind::Image`] block.
    pub image: Option<ImageRef>,
    /// Blocks a text filter inserted after this one (a translation, a
    /// gloss, a note), laid out after it, at most [`MAX_INSERTED`] of
    /// them with [`MAX_INSERTED_TEXT`] bytes of text in all. They have no
    /// place of their own in the book: their `start` and `end` are this
    /// block's end, their words no places, so positions, progress,
    /// bookmarks and highlights keep to the book's own text. A page does
    /// not break inside them: this block's last line goes on to the next
    /// page with them where they fit there whole, else what fits stays and
    /// the rest goes (they have no place a page could start from: keep
    /// them shorter than a page). Not run through the filters again;
    /// formats leave it empty.
    pub inserted: Vec<Block>,
    /// Words or letters annotated above them: ruby, furigana, phonetics,
    /// glosses ([`Annotation`]). Reserved: the layout does not set them
    /// yet (see ADR 0032), formats and filters may fill it already.
    pub annotations: Vec<Annotation>,
    /// The way its lines go (an EPUB's `dir`, CSS `direction`): `Auto`
    /// takes it from the first strong letter (UAX #9 P2, P3).
    pub dir: Direction,
    /// How its lines go: across, or down in columns. Only
    /// [`WritingMode::HorizontalTb`] is laid out yet.
    pub writing_mode: WritingMode,
    /// Its language (an EPUB's `lang`, `xml:lang`, else the book's):
    /// for the shaper, fonts, hyphenation. [`Lang::NONE`] if unknown.
    pub lang: Lang,
    /// The other languages its runs are in, which their
    /// [`TextStyle::lang_index`] names from one, at most
    /// [`MAX_INLINE_LANGS`]: a Persian word in an Arabic book. `None` in
    /// almost every block.
    pub inline_langs: Option<alloc::boxed::Box<[Lang]>>,
    /// The lines its first letter spans as a drop capital; 0 for none.
    /// Reserved: the layout does not set drop capitals yet.
    pub drop_cap: u8,
}

/// The most blocks a text filter inserts after one ([`Block::inserted`]).
pub const MAX_INSERTED: usize = 4;
/// The most text, in bytes, of the blocks inserted after one.
pub const MAX_INSERTED_TEXT: usize = 4096;

/// A text set small above a part of a block's text (see
/// [`Block::annotations`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Annotation {
    /// The byte range of the block's text it stands above.
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub kind: AnnotationKind,
    /// Its size in percent of the text below it; 0 for the reader's.
    pub size: u16,
}

impl Annotation {
    pub fn new(start: u32, end: u32, text: impl Into<String>, kind: AnnotationKind) -> Self {
        Self {
            start,
            end,
            text: text.into(),
            kind,
            size: 0,
        }
    }
}

/// What an [`Annotation`] is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnnotationKind {
    /// Ruby: how characters are read (furigana, pinyin).
    #[default]
    Ruby,
    /// How a word sounds (phonetic script).
    Phonetic,
    /// What a word means (an interlinear gloss).
    Gloss,
}

/// The most room [`Block::leading_above`] adds above a line: four ems.
pub const MAX_LEADING_ABOVE: u16 = 400;

impl Block {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(kind: BlockKind, start: u32, end: u32, text: String) -> Self {
        Self {
            kind,
            start,
            end,
            text,
            continues: false,
            continued: false,
            map: OffsetMap::IDENTITY,
            runs: Vec::new(),
            align: Align::Auto,
            indent: None,
            space: None,
            size: None,
            margins: (0, 0),
            line_height: None,
            leading_above: None,
            image: None,
            inserted: Vec::new(),
            annotations: Vec::new(),
            dir: Direction::Auto,
            writing_mode: WritingMode::HorizontalTb,
            lang: Lang::NONE,
            inline_langs: None,
            drop_cap: 0,
        }
    }

    pub fn dir(mut self, dir: Direction) -> Self {
        self.dir = dir;
        self
    }

    pub fn writing_mode(mut self, writing_mode: WritingMode) -> Self {
        self.writing_mode = writing_mode;
        self
    }

    pub fn lang(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }

    /// Its runs' other languages (see [`Block::inline_langs`]); more than
    /// [`MAX_INLINE_LANGS`] are dropped, none is `None`.
    pub fn inline_langs(mut self, langs: &[Lang]) -> Self {
        let langs = &langs[..langs.len().min(MAX_INLINE_LANGS as usize)];
        self.inline_langs = (!langs.is_empty()).then(|| langs.into());
        self
    }

    pub fn drop_cap(mut self, lines: u8) -> Self {
        self.drop_cap = lines;
        self
    }

    pub fn continues(mut self, continues: bool) -> Self {
        self.continues = continues;
        self
    }

    /// See [`Block::continued`].
    #[must_use]
    pub fn continued(mut self, continued: bool) -> Self {
        self.continued = continued;
        self
    }

    pub fn map(mut self, map: OffsetMap) -> Self {
        self.map = map;
        self
    }

    pub fn runs(mut self, runs: Vec<Run>) -> Self {
        self.runs = runs;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn indent(mut self, indent: impl Into<Option<i16>>) -> Self {
        self.indent = indent.into();
        self
    }

    pub fn space(mut self, space: impl Into<Option<u16>>) -> Self {
        self.space = space.into();
        self
    }

    pub fn size(mut self, size: impl Into<Option<u16>>) -> Self {
        self.size = size.into();
        self
    }

    pub fn margins(mut self, start: u16, end: u16) -> Self {
        self.margins = (start, end);
        self
    }

    pub fn line_height(mut self, line_height: impl Into<Option<u16>>) -> Self {
        self.line_height = line_height.into();
        self
    }

    pub fn leading_above(mut self, leading_above: impl Into<Option<u16>>) -> Self {
        self.leading_above = leading_above.into();
        self
    }

    pub fn image(mut self, image: impl Into<Option<ImageRef>>) -> Self {
        self.image = image.into();
        self
    }

    /// Inserts `block` after this one (see [`Block::inserted`]): whether
    /// it went in, within [`MAX_INSERTED`] blocks and
    /// [`MAX_INSERTED_TEXT`] bytes. It takes no place in the book.
    pub fn insert_after(&mut self, mut block: Block) -> bool {
        let used: usize = self.inserted.iter().map(|b| b.text.len()).sum();
        if self.inserted.len() >= MAX_INSERTED || used + block.text.len() > MAX_INSERTED_TEXT {
            return false;
        }
        block.anchor_at(self.end);
        self.inserted.push(block);
        true
    }

    /// Keeps the blocks inserted after this one within their bounds, each
    /// at this block's end without places or blocks of its own.
    pub fn bound_inserted(&mut self) {
        self.inserted.truncate(MAX_INSERTED);
        let mut room = MAX_INSERTED_TEXT;
        let end = self.end;
        self.inserted.retain_mut(|block| {
            if block.text.len() > room {
                return false;
            }
            room -= block.text.len();
            block.anchor_at(end);
            true
        });
    }

    /// Placed at `end` of the book, its text at no place of its own.
    fn anchor_at(&mut self, end: u32) {
        self.start = end;
        self.end = end;
        self.map = OffsetMap::IDENTITY;
        self.continues = false;
        self.continued = false;
        self.inserted.clear();
    }
}

impl Block {
    /// The style of the character at byte `index` of `text`.
    pub fn style_at(&self, index: usize) -> TextStyle {
        let index = index as u32;
        let at = self.runs.partition_point(|run| run.end <= index);
        match self.runs.get(at) {
            Some(run) if run.start <= index => run.style,
            _ => TextStyle::REGULAR,
        }
    }

    /// The language of the character at byte `index` of `text`: its run's
    /// inline language, else the block's.
    pub fn lang_at(&self, index: usize) -> Lang {
        self.lang_of(self.style_at(index))
    }

    /// The language a run of `style` is in, in this block.
    pub fn lang_of(&self, style: TextStyle) -> Lang {
        match style.lang_index() {
            0 => self.lang,
            i => self
                .inline_langs
                .as_deref()
                .and_then(|langs| langs.get(i as usize - 1))
                .copied()
                .unwrap_or(self.lang),
        }
    }

    /// Replaces bytes `range` of `text` (on character boundaries) with
    /// `with`, e.g. in a text filter: styles move along, and positions in
    /// the source stay right around it (the new text maps to where the old
    /// one was).
    pub fn replace(&mut self, range: core::ops::Range<usize>, with: &str) {
        let (from, to) = (range.start, range.end.min(self.text.len()));
        if from > to || !self.text.is_char_boundary(from) || !self.text.is_char_boundary(to) {
            return;
        }
        let shift = |i: u32| -> u32 { (i as usize + with.len() - (to - from)) as u32 };
        let (source_from, source_to) = (self.source_offset(from), self.source_offset(to));
        let mut checkpoints: Vec<(u32, u32)> = self
            .map
            .checkpoints
            .iter()
            .copied()
            .filter(|&(t, _)| (t as usize) < from)
            .collect();
        checkpoints.push((from as u32, source_from));
        checkpoints.push(((from + with.len()) as u32, source_to));
        checkpoints.extend(
            self.map
                .checkpoints
                .iter()
                .filter(|&&(t, _)| t as usize > to)
                .map(|&(t, s)| (shift(t), s)),
        );
        // One checkpoint per text index, the last one wins.
        let mut map = OffsetMap::default();
        for (t, s) in checkpoints {
            map.checkpoint(t as usize, s);
        }
        self.map = map;
        // The new text takes the style of the first run reaching into the
        // replaced text; runs after it that reach in too start behind it.
        let end = (from + with.len()) as u32;
        let mut claimed = false;
        for run in &mut self.runs {
            let (start, stop) = (run.start as usize, run.end as usize);
            if stop <= from {
                continue;
            }
            if start >= to {
                run.start = shift(run.start);
            } else if claimed {
                run.start = end;
            } else {
                run.start = run.start.min(from as u32);
                claimed = true;
            }
            run.end = if stop >= to { shift(run.end) } else { end };
        }
        self.runs.retain(|run| run.start < run.end);
        self.text.replace_range(from..to, with);
    }

    /// The source offset of byte `index` of `text`.
    pub fn source_offset(&self, index: usize) -> u32 {
        if index >= self.text.len() {
            return self.end;
        }
        self.map.source_offset(self.start, index)
    }

    /// The byte index in `text` for a source offset inside the block
    /// (rounded down to a character boundary).
    pub fn text_index(&self, offset: u32) -> usize {
        if offset <= self.start {
            return 0;
        }
        if offset >= self.end {
            return self.text.len();
        }
        let mut index = self.map.text_index(self.start, offset).min(self.text.len());
        while !self.text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }
}

/// Maps text indices of a block to source offsets.
///
/// Normalization (dropping `\r`, decoding legacy encodings, collapsing
/// spaces) shifts positions. Instead of storing an offset per byte, the map
/// stores checkpoints where the shift changes; between checkpoints both
/// advance in lockstep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OffsetMap {
    /// `(text index, source offset)` pairs, sorted by text index.
    checkpoints: Vec<(u32, u32)>,
}

impl OffsetMap {
    /// Text and source advance in lockstep from the block start.
    pub const IDENTITY: OffsetMap = OffsetMap {
        checkpoints: Vec::new(),
    };

    /// Records that text index `text` corresponds to source offset `source`.
    /// Checkpoints must be added in increasing order.
    pub fn checkpoint(&mut self, text: usize, source: u32) {
        let text = text as u32;
        if let Some(last) = self.checkpoints.last_mut()
            && last.0 == text
        {
            last.1 = source;
            return;
        }
        self.checkpoints.push((text, source));
    }

    fn source_offset(&self, start: u32, index: usize) -> u32 {
        let index = index as u32;
        let i = self.checkpoints.partition_point(|&(text, _)| text <= index);
        match i.checked_sub(1).map(|i| self.checkpoints[i]) {
            Some((text, source)) => source + (index - text),
            None => start + index,
        }
    }

    fn text_index(&self, start: u32, offset: u32) -> usize {
        let i = self
            .checkpoints
            .partition_point(|&(_, source)| source <= offset);
        // Before the first checkpoint, text and source start together.
        let (text, source) = i.checked_sub(1).map_or((0, start), |i| self.checkpoints[i]);
        // Inside a gap (e.g. a dropped `\r`) map to the checkpoint after it.
        let next_text = self.checkpoints.get(i).map_or(u32::MAX, |c| c.0);
        text.saturating_add(offset.saturating_sub(source))
            .min(next_text) as usize
    }
}

/// Why a document could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentError {
    Storage(StorageError),
    /// The file is not valid for this format.
    Malformed(Cow<'static, str>),
    /// No format plugin can open the file.
    UnsupportedFormat,
    /// The device has too little free memory for this document.
    OutOfMemory,
    /// Not there yet (still coming, e.g. streamed): readers wait for
    /// the event `core.document-ready` naming it and open it again.
    NotReady,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::Storage(error) => write!(f, "storage: {error}"),
            DocumentError::Malformed(message) => write!(f, "malformed document: {message}"),
            DocumentError::UnsupportedFormat => f.write_str("unsupported format"),
            DocumentError::OutOfMemory => f.write_str("not enough memory"),
            DocumentError::NotReady => f.write_str("not ready yet"),
        }
    }
}

impl core::error::Error for DocumentError {}

impl From<StorageError> for DocumentError {
    fn from(error: StorageError) -> Self {
        DocumentError::Storage(error)
    }
}

/// A font embedded in a document, e.g. with CSS `@font-face` in an EPUB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedFont {
    /// The family, as in [`TextStyle::family`].
    pub family: u8,
    pub bold: bool,
    pub italic: bool,
}

/// An entry of a document's table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub title: String,
    /// Nesting depth, 0 for top-level entries.
    pub level: u8,
    pub at: Locator,
}

/// A link in a section's text: source offsets `start..end`, and where it
/// goes (see [`Document::link_target`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Link {
    pub start: u32,
    pub end: u32,
    pub href: String,
    pub kind: LinkKind,
}

impl Link {
    pub fn new(start: u32, end: u32, href: impl Into<String>) -> Link {
        Link {
            start,
            end,
            href: href.into(),
            kind: LinkKind::Plain,
        }
    }

    pub fn kind(mut self, kind: LinkKind) -> Link {
        self.kind = kind;
        self
    }
}

/// What a [`Link`] is, as the book marks it (an EPUB's `epub:type`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LinkKind {
    /// Any other link.
    #[default]
    Plain,
    /// A note's mark (`noteref`): it leads to a footnote or an endnote.
    NoteRef,
    /// From a note back to its mark (`backlink`).
    Back,
}

/// Whether `href` leads out of the document, e.g. to a web page: it has a
/// scheme such as `https:` or `mailto:`.
pub fn is_external(href: &str) -> bool {
    href.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// An open document.
pub trait Document {
    fn metadata(&self) -> &Metadata;

    /// Number of sections (at least 1).
    fn sections(&self) -> u32;

    /// The page of the printed edition at `at`, as the book names it
    /// (an EPUB's `page-list`), e.g. `123` or `xii`; `None` if it names
    /// none there.
    fn page_label(&self, storage: &dyn Storage, at: Locator) -> Option<String> {
        let _ = (storage, at);
        None
    }

    /// Each section is the picture of a whole page (a comic, pages made
    /// for one screen): readers show them edge to edge, without margins
    /// or status lines.
    fn whole_pages(&self) -> bool {
        false
    }

    /// Length of a section in source units, for progress calculation.
    fn section_len(&self, section: u32) -> u32;

    /// The block containing `at`, or the first block after it. `None` when
    /// `at` is at or past the end of its section.
    ///
    /// Block boundaries must not depend on `at`: every position inside a
    /// block yields the same block.
    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError>;

    /// Number of table of contents entries; 0 if the format or file has
    /// none. Tables of contents can be long, so they are read entry by
    /// entry instead of all at once.
    fn toc_len(&self, storage: &dyn Storage) -> u32 {
        let _ = storage;
        0
    }

    /// Table of contents entry `index`, in reading order. Its locator may
    /// point at the start of the entry's section when the exact position
    /// is costly to find (e.g. needs converting a chapter); use
    /// [`Document::toc_target`] before jumping there.
    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        let _ = (storage, index);
        None
    }

    /// The exact position of table of contents entry `index`.
    fn toc_target(&self, storage: &dyn Storage, index: u32) -> Option<Locator> {
        self.toc_entry(storage, index).map(|entry| entry.at)
    }

    /// Makes `image` (of one of its blocks) ready to be drawn, for
    /// documents that decode pictures when a page shows them rather than
    /// with their chapter: the picture as decoded now (maybe at another
    /// path), `None` if there was nothing to do or it cannot be decoded.
    /// Readers ask before they draw a page with pictures, not while they
    /// only lay pages out.
    fn prepare_image(&self, storage: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        let _ = (storage, image);
        None
    }

    /// The cover as a small gray image in storage, decoded on first use;
    /// `None` if the book has none (or the format no covers).
    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        let _ = storage;
        Ok(None)
    }

    /// Position `at` as other readers write it, to sync reading positions
    /// and highlights with them: for EPUB, KOReader's XPointer, down to the
    /// character where `at` is in text (e.g.
    /// `/body/DocFragment[3]/body/div[2]/p[5]/text()[1].12`), else to the
    /// element (`/body/DocFragment[3]/body/div[2]/p[5]`). `None` where the
    /// format has no such positions.
    fn xpointer(&self, storage: &dyn Storage, at: Locator) -> Option<String> {
        let _ = (storage, at);
        None
    }

    /// The position of an XPointer (see [`Document::xpointer`]), down to
    /// the character where it points into text the document shows, else
    /// to the paragraph; `None` if it does not fit the document.
    fn locate_xpointer(&self, storage: &dyn Storage, xpointer: &str) -> Option<Locator> {
        let _ = (storage, xpointer);
        None
    }

    /// The fonts the document embeds; text in their families carries the
    /// family in its [`TextStyle`].
    fn fonts(&self, storage: &dyn Storage) -> Vec<EmbeddedFont> {
        let _ = storage;
        Vec::new()
    }

    /// The data (an OpenType/TrueType file) of embedded font `index` of
    /// [`Document::fonts`].
    fn font_data(&self, storage: &dyn Storage, index: usize) -> Result<Vec<u8>, DocumentError> {
        let _ = (storage, index);
        Err(DocumentError::Malformed(Cow::Borrowed("no embedded fonts")))
    }

    /// The links of `section` that overlap source offsets `from..to`, in
    /// order.
    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        let _ = (storage, section, from, to);
        Vec::new()
    }

    /// Where link `href` (of [`Document::links`]) leads in the document;
    /// `None` for links out of it ([`is_external`]) or to nothing.
    fn link_target(&self, storage: &dyn Storage, href: &str) -> Option<Locator> {
        let _ = (storage, href);
        None
    }

    /// Reading progress of `at` from 0.0 to 1.0.
    /// A question asked after this release, by its key and its
    /// parameters (bytes the key's documentation describes): what
    /// PaperPaw asks of documents later reaches documents built now,
    /// which answer `None` to keys they do not know.
    fn extended(&self, storage: &dyn Storage, key: &str, params: &[u8]) -> Option<Vec<u8>> {
        let _ = (storage, key, params);
        None
    }

    fn progress(&self, at: Locator) -> f32 {
        let total: u64 = (0..self.sections())
            .map(|s| u64::from(self.section_len(s)))
            .sum();
        if total == 0 {
            return 0.0;
        }
        let before: u64 = (0..at.section.min(self.sections()))
            .map(|s| u64::from(self.section_len(s)))
            .sum();
        ((before + u64::from(at.offset)) as f32 / total as f32).min(1.0)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn replacing_keeps_styles_and_positions() {
        let mut block = Block {
            kind: BlockKind::Paragraph,
            continues: false,
            continued: false,
            start: 100,
            end: 120,
            text: String::from("Mr. Smith is here."),
            map: OffsetMap::IDENTITY,
            runs: alloc::vec![Run::new(4, 9, TextStyle::BOLD)],
            align: Align::default(),
            indent: None,
            space: None,
            size: None,
            margins: (0, 0),
            line_height: None,
            leading_above: None,
            image: None,
            inserted: Vec::new(),
            annotations: Vec::new(),
            dir: Direction::Auto,
            writing_mode: WritingMode::HorizontalTb,
            lang: Lang::NONE,
            inline_langs: None,
            drop_cap: 0,
        };
        block.replace(0..3, "Mister");
        assert_eq!(block.text, "Mister Smith is here.");
        // "Smith" is still bold and still where it was in the book.
        assert_eq!(block.runs[0].start, 7);
        assert_eq!(block.runs[0].end, 12);
        assert_eq!(block.source_offset(7), 104);
        assert_eq!(block.source_offset(0), 100);
        assert_eq!(block.text_index(104), 7);
        assert_eq!(block.source_offset(block.text.len() - 1), 117);
    }
    use super::*;

    #[test]
    fn small_caps_keep_bold_italic_and_family() {
        let style = TextStyle::SMALL_CAPS
            .union(TextStyle::BOLD)
            .with_family(MAX_FAMILIES);
        assert!(style.small_caps() && style.bold() && !style.italic());
        assert_eq!(style.family(), MAX_FAMILIES);
        assert_eq!(TextStyle::from_bits(style.bits()), style);
        // Too many families: the reader's font, the flags kept.
        let other = TextStyle::SMALL_CAPS.with_family(MAX_FAMILIES + 1);
        assert!(other.small_caps());
        assert_eq!(other.family(), 0);
        assert!(!TextStyle::ITALIC.with_family(5).small_caps());
    }

    fn block(text: &str, start: u32, end: u32, map: OffsetMap) -> Block {
        Block {
            kind: BlockKind::Paragraph,
            continues: false,
            continued: false,
            start,
            end,
            text: String::from(text),
            map,
            runs: Vec::new(),
            align: Align::Auto,
            indent: None,
            space: None,
            size: None,
            margins: (0, 0),
            line_height: None,
            leading_above: None,
            image: None,
            inserted: Vec::new(),
            annotations: Vec::new(),
            dir: Direction::Auto,
            writing_mode: WritingMode::HorizontalTb,
            lang: Lang::NONE,
            inline_langs: None,
            drop_cap: 0,
        }
    }

    #[test]
    fn external_links_have_a_scheme() {
        for href in ["https://example.org/a", "mailto:a@b.c", "HTTP://X"] {
            assert!(is_external(href), "{href}");
        }
        for href in ["text/ch2.xhtml#note", "#note", "c:/x", "../a:b", ""] {
            assert!(!is_external(href), "{href}");
        }
    }

    #[test]
    fn styles_by_run() {
        let mut b = block("plain bold italic", 0, 17, OffsetMap::IDENTITY);
        b.runs = alloc::vec![
            Run::new(6, 10, TextStyle::BOLD),
            Run::new(11, 17, TextStyle::ITALIC),
        ];
        assert_eq!(b.style_at(0), TextStyle::REGULAR);
        assert_eq!(b.style_at(6), TextStyle::BOLD);
        assert_eq!(b.style_at(10), TextStyle::REGULAR);
        assert_eq!(b.style_at(16), TextStyle::ITALIC);
        assert!(TextStyle::BOLD.union(TextStyle::ITALIC).italic());
    }

    #[test]
    fn identity_mapping() {
        let b = block("hello world", 100, 111, OffsetMap::IDENTITY);
        assert_eq!(b.source_offset(6), 106);
        assert_eq!(b.text_index(106), 6);
        assert_eq!(b.source_offset(99), 111);
        assert_eq!(b.text_index(50), 0);
        assert_eq!(b.text_index(500), 11);
    }

    #[test]
    fn checkpoints_account_for_dropped_bytes() {
        // Source "ab\r\ncd" at 0 → text "ab cd": "\r\n" became one space.
        let mut map = OffsetMap::default();
        map.checkpoint(3, 4);
        let b = block("ab cd", 0, 6, map);
        assert_eq!(b.source_offset(1), 1);
        assert_eq!(b.source_offset(2), 2);
        assert_eq!(b.source_offset(3), 4);
        assert_eq!(b.source_offset(4), 5);
        assert_eq!(b.text_index(4), 3);
        assert_eq!(b.text_index(5), 4);
        assert_eq!(b.text_index(3), 3);
    }

    #[test]
    fn text_index_respects_char_boundaries() {
        // "ä" is two bytes in UTF-8 but one byte in Windows-1252.
        let mut map = OffsetMap::default();
        map.checkpoint(3, 2); // after "xä"
        let b = block("xäy", 0, 3, map);
        assert_eq!(b.text_index(2), 3);
        assert_eq!(b.source_offset(3), 2);
        assert!(b.text.is_char_boundary(b.text_index(1)));
    }
}
