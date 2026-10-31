//! Page layout: turns a [`Document`] into pages.
//!
//! Layout is deterministic and starts at any [`Locator`] that begins a line,
//! so a reader only needs to remember where pages start, never the pages
//! themselves. Text is broken greedily per paragraph; because greedy breaking
//! is stable, laying out from the middle of a paragraph yields exactly the
//! same lines as laying out the whole paragraph.
//!
//! The engine knows nothing about fonts or drawing: a [`Typesetter`]
//! measures characters in each [`Face`] (character style, body text or
//! heading level) and kerns pairs of them, and the engine returns
//! positioned lines with their runs of faces. Pairs are kerned within a
//! word and a face only: a line's width is that of its words drawn one by
//! one, each run of a face at once.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

pub use paperpaw_document::ImageRef;
pub use paperpaw_document::TextStyle;
use paperpaw_document::{
    Align, AnnotationKind, Block, BlockKind, Document, DocumentError, Locator, MAX_LEADING_ABOVE,
    Storage,
};
pub use paperpaw_text::Hyphenate;
use paperpaw_text::{Line, wrap_lines_balanced_kerned_with_cost, wrap_lines_kerned};

/// Largest extra space per word gap for justification, relative to the
/// line height.
const MAX_JUSTIFY_GAP: f32 = 0.6;
/// Extra space above a heading, relative to a body line.
const HEADING_SPACE: f32 = 0.6;
/// The least height of a line, in pixels, so a page holds a bounded
/// number of them whatever a [`Typesetter`] says.
const MIN_LINE_HEIGHT: f32 = 1.0;

/// How text is set: its character style and whether it is body text or a
/// heading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Face {
    pub style: TextStyle,
    /// 0 for body text, 1 (most important) to 6 for headings.
    pub heading: u8,
    /// The book's text size in percent of body text; 0 for the reader's
    /// (body text, or by heading level).
    pub size: u16,
}

/// Changes whenever the same input is laid out differently, so readers
/// rebuild page indexes they cached.
pub const VERSION: u32 = 8;

/// Measures text for the layout engine; readers implement it with their
/// fonts.
pub trait Typesetter {
    /// Advance width of `c` in `face`.
    fn advance(&self, face: Face, c: char) -> f32;

    /// How much `right` moves when it follows `left` in `face`, in
    /// pixels (negative: closer): the font's kerning, as drawing text of
    /// that face applies it. Asked only for two characters of one word
    /// and one face (also for a hyphen after a letter); 0 by default.
    fn kern(&self, face: Face, left: char, right: char) -> f32 {
        let _ = (face, left, right);
        0.0
    }

    /// Lines in `face`: `(ascent, line height)`, i.e. the distance from
    /// the top of a line box to its baseline and between baselines.
    fn line(&self, face: Face) -> (f32, f32);
}

/// Which of a book's own styles are followed (see [`Style::book`]): each
/// can be left to the reader alone, e.g. a book's alignment but not its
/// margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BookStyles(u16);

impl BookStyles {
    /// None: the reader's settings alone.
    pub const NONE: BookStyles = BookStyles(0);
    /// A block's alignment ([`Block::align`]).
    pub const ALIGN: BookStyles = BookStyles(1);
    /// A first line's indent ([`Block::indent`]).
    pub const INDENT: BookStyles = BookStyles(1 << 1);
    /// Text sizes ([`Block::size`]).
    pub const SIZE: BookStyles = BookStyles(1 << 2);
    /// The space above blocks ([`Block::space`]).
    pub const SPACING: BookStyles = BookStyles(1 << 3);
    /// Font families the book embeds ([`TextStyle::family`]).
    pub const FONTS: BookStyles = BookStyles(1 << 4);
    /// Margins at the sides of blocks ([`Block::margins`]).
    pub const MARGINS: BookStyles = BookStyles(1 << 5);
    /// The distance between lines ([`Block::line_height`]).
    pub const LINE_HEIGHT: BookStyles = BookStyles(1 << 6);
    /// All of them.
    pub const ALL: BookStyles = BookStyles(0x7F);

    pub const fn has(self, other: BookStyles) -> bool {
        self.0 & other.0 == other.0
    }

    /// These and `other`'s.
    #[must_use]
    pub const fn with(self, other: BookStyles) -> BookStyles {
        BookStyles(self.0 | other.0)
    }

    /// These without `other`'s.
    #[must_use]
    pub const fn without(self, other: BookStyles) -> BookStyles {
        BookStyles(self.0 & !other.0)
    }

    /// For a layout's key.
    pub const fn bits(self) -> u16 {
        self.0
    }
}

/// Typographic settings for a page, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Style {
    /// Extra space between paragraphs.
    pub paragraph_spacing: f32,
    /// Indent of the first line of a paragraph.
    pub indent: f32,
    /// The body text size, for a book's indents and spaces given in ems.
    pub em: f32,
    /// Stretch spaces so lines fill the width (except a paragraph's last).
    /// The reader's choice wins over a book's justified text; centred and
    /// right-aligned blocks keep their alignment.
    pub justify: bool,
    /// The fewest lines of a paragraph left alone at the top of a page (a
    /// widow): lines that would leave fewer take more along to the next
    /// page. 1 lets lines fall where they fall.
    pub widows: u8,
    /// The fewest lines of a paragraph left alone at the bottom of a page
    /// (an orphan): a paragraph that would leave fewer goes to the next
    /// page whole, with a heading just before it. 1 lets lines fall where
    /// they fall.
    pub orphans: u8,
    /// Which of the book's own styles are followed; all by default.
    pub book: BookStyles,
    /// Paragraphs broken over all their lines, for lines that end more
    /// evenly ([`paperpaw_text::wrap_lines_balanced`]), not line by line;
    /// off by default.
    pub balanced: bool,
}

impl Style {
    /// Made of what it needs; the rest as set with its methods.
    pub const fn new(paragraph_spacing: f32, indent: f32, em: f32, justify: bool) -> Self {
        Self {
            paragraph_spacing,
            indent,
            em,
            justify,
            widows: 1,
            orphans: 1,
            book: BookStyles::ALL,
            balanced: false,
        }
    }

    /// The same, breaking paragraphs over all their lines, or line by line.
    pub const fn balanced(mut self, balanced: bool) -> Self {
        self.balanced = balanced;
        self
    }

    /// The same, following only `book` of the book's own styles.
    pub const fn book(mut self, book: BookStyles) -> Self {
        self.book = book;
        self
    }

    /// The same, keeping at least `widows` lines of a paragraph at the top
    /// of a page and `orphans` at the bottom (see [`Style::widows`]).
    pub const fn lines_kept(mut self, widows: u8, orphans: u8) -> Self {
        self.widows = widows;
        self.orphans = orphans;
        self
    }
}

/// Text of a line set in one face: byte range of [`PageLine::text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRun {
    pub start: usize,
    pub end: usize,
    pub face: Face,
}

/// A line of text at its position on the page.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageLine {
    /// Left edge relative to the content box.
    pub x: f32,
    /// Baseline relative to the content box's top.
    pub baseline: f32,
    pub text: String,
    /// Extra width to add to every space (justification).
    pub word_spacing: f32,
    /// The faces of `text`, in order and covering all of it.
    pub runs: Vec<LineRun>,
    /// The section the line is from.
    pub section: u32,
    /// Where each word of `text` (separated by spaces) starts and ends in
    /// the section's source, for positions that outlast the layout (e.g.
    /// highlights).
    pub words: Vec<(u32, u32)>,
    /// The room above the line's letters its block asked for
    /// ([`Block::leading_above`]), in pixels: part of the line's height,
    /// between the line before and this one's ascent, where a page layer
    /// draws (an interlinear reading). 0 for none.
    pub leading: f32,
    /// The line is of a block a text filter inserted
    /// ([`Block::inserted`]): its words have no places (`words` is
    /// empty), nothing on it is chosen, kept or read as the book's.
    pub inserted: bool,
    /// Annotations set above the line's words (ruby, glosses). Reserved:
    /// empty until the layout sets them (ADR 0032).
    pub annotations: Vec<LineAnnotation>,
}

/// An annotation set above a line ([`Block::annotations`]), where the
/// layout put it. Reserved: none are set yet (ADR 0032).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LineAnnotation {
    /// Left edge relative to the content box, and its width.
    pub x: f32,
    pub width: f32,
    /// Its baseline relative to the content box's top.
    pub baseline: f32,
    pub text: String,
    pub kind: AnnotationKind,
    /// Its text size in pixels.
    pub size: f32,
}

/// An image at its position and size on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    /// Left and top edges relative to the content box.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub image: ImageRef,
}

/// One laid-out page.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Page {
    /// Position of the first character on the page.
    pub start: Locator,
    /// Position where the next page starts; equal to `start` past the end.
    pub end: Locator,
    pub lines: Vec<PageLine>,
    pub images: Vec<PageImage>,
}

impl Page {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(start: Locator, end: Locator, lines: Vec<PageLine>, images: Vec<PageImage>) -> Self {
        Self {
            start,
            end,
            lines,
            images,
        }
    }
}

impl Page {
    /// Whether this page is past the end of the document.
    pub fn is_past_end(&self) -> bool {
        self.lines.is_empty() && self.images.is_empty()
    }
}

/// Source offsets of the words in `range` of a block's text.
fn word_sources(block: &Block, range: core::ops::Range<usize>) -> Vec<(u32, u32)> {
    let text = &block.text[range.clone()];
    let mut words = Vec::new();
    let mut at = range.start;
    for word in text.split(' ') {
        // The end after the last character: the offset of the text after
        // the word may lie past the block's end.
        if let Some((last, c)) = word.char_indices().last() {
            let end = block.source_offset(at + last) + c.len_utf8() as u32;
            words.push((block.source_offset(at), end));
        }
        at += word.len() + 1;
    }
    words
}

/// The face of a block's text at byte `index`; the book's own fonts only
/// where `book` follows them.
fn face_at(block: &Block, base: Face, index: usize, book: BookStyles) -> Face {
    let style = base.style.union(block.style_at(index));
    Face {
        style: if book.has(BookStyles::FONTS) {
            style
        } else {
            style.with_family(0)
        },
        heading: base.heading,
        size: base.size,
    }
}

/// The runs of faces for `range` of the block's text, as ranges of a line
/// text starting at `range.start`, plus `extra` bytes at the end (a hyphen).
fn line_runs(
    block: &Block,
    base: Face,
    range: core::ops::Range<usize>,
    extra: usize,
    book: BookStyles,
) -> Vec<LineRun> {
    let mut runs: Vec<LineRun> = Vec::new();
    for (i, _) in block.text[range.clone()].char_indices() {
        let face = face_at(block, base, range.start + i, book);
        match runs.last_mut() {
            Some(run) if run.face == face => run.end = i,
            _ => runs.push(LineRun {
                start: i,
                end: i,
                face,
            }),
        }
    }
    // Ends are exclusive: every run reaches the next one or the line end.
    let len = range.len() + extra;
    for k in 0..runs.len() {
        runs[k].end = runs.get(k + 1).map_or(len, |next| next.start);
    }
    if runs.is_empty() && len > 0 {
        runs.push(LineRun {
            start: 0,
            end: len,
            face: base,
        });
    }
    runs
}

/// The heading level of a block (0: none) and the face of its text; the
/// book's size only where `style` follows it.
fn block_face(block: &Block, style: &Style) -> (u8, Face) {
    let heading = match block.kind {
        BlockKind::Heading(level) => level.clamp(1, 6),
        _ => 0,
    };
    let face = Face {
        style: if heading > 0 {
            TextStyle::BOLD
        } else {
            TextStyle::REGULAR
        },
        heading,
        size: book_size(block, style).unwrap_or(0),
    };
    (heading, face)
}

/// The book's text size of a block, where `style` follows it.
fn book_size(block: &Block, style: &Style) -> Option<u16> {
    block.size.filter(|_| style.book.has(BookStyles::SIZE))
}

/// The ems of a block's text, in pixels: the book's are those of the
/// block's own text.
fn block_em(block: &Block, style: &Style) -> f32 {
    style.em * f32::from(book_size(block, style).unwrap_or(100)) / 100.0
}

/// Where a block's lines start and how wide they are: the first line
/// (indented where the block starts, by half a line at most, perhaps
/// hanging out into the book's margin) and the others, within the book's
/// margins, each kept to a quarter of the width.
#[derive(Debug, Clone, Copy)]
struct Measure {
    first: (f32, f32),
    rest: (f32, f32),
}

impl Measure {
    fn of(block: &Block, style: &Style, width: f32, indent: f32) -> Measure {
        let (start, end) = match style.book.has(BookStyles::MARGINS) {
            true => {
                let em = block_em(block, style);
                let side = |m: u16| (f32::from(m) / 100.0 * em).clamp(0.0, width / 4.0);
                (side(block.margins.0), side(block.margins.1))
            }
            false => (0.0, 0.0),
        };
        let rest = (start, (width - start - end).max(1.0));
        // Indented by half a line at most, so a narrow page keeps room.
        let first_x = (start + indent).clamp(0.0, start + rest.1 / 2.0);
        Measure {
            first: (first_x, (width - end - first_x).max(1.0)),
            rest,
        }
    }

    /// Where line `n` starts and how wide it is.
    fn line(&self, n: usize) -> (f32, f32) {
        if n == 0 { self.first } else { self.rest }
    }
}

/// The lines of `block` from byte `from` of its text on (a line's start),
/// their ranges in its text, and where they are set: broken line by line
/// from there, or (balanced) over the whole block, from where it starts
/// (`indent` is its first line's), and those from `from` on taken.
fn block_lines(
    block: &Block,
    from: usize,
    base: Face,
    cx: &LayoutContext<'_>,
    heading: u8,
    indent: f32,
    justify_limit: Option<f32>,
) -> Vec<(Line, (f32, f32))> {
    let (style, width) = (cx.style, cx.width);
    let advance = |i: usize, c: char| {
        cx.typesetter
            .advance(face_at(block, base, i, style.book), c)
    };
    // A pair in one face: the character before `i` and `right`, the text's
    // own there or a hyphen set like the character before it.
    let kern = |i: usize, left: char, right: char| {
        let face = face_at(block, base, i - left.len_utf8(), style.book);
        let own = block.text[i..].starts_with(right);
        if own && face_at(block, base, i, style.book) != face {
            return 0.0;
        }
        cx.typesetter.kern(face, left, right)
    };
    let hyphenate = cx.hyphenate.filter(|_| heading == 0);
    if style.balanced {
        let measure = Measure::of(block, style, width, indent);
        let lines = wrap_lines_balanced_kerned_with_cost(
            &block.text,
            |n| measure.line(n).1,
            advance,
            kern,
            hyphenate,
            |free, spaces| justification_cost(free, spaces, justify_limit),
        );
        return lines
            .into_iter()
            .enumerate()
            .filter(|(_, line)| line.range.start >= from)
            .map(|(n, line)| (line, measure.line(n)))
            .collect();
    }
    // A page that goes on with a block sets its lines as wide as those
    // after a first line.
    let measure = match from == 0 && !block.continues {
        true => Measure::of(block, style, width, indent),
        false => {
            let rest = Measure::of(block, style, width, 0.0).rest;
            Measure { first: rest, rest }
        }
    };
    let lines = wrap_lines_kerned(
        &block.text[from..],
        |n| measure.line(n).1,
        |i, c| advance(from + i, c),
        |i, left, right| kern(from + i, left, right),
        hyphenate,
    );
    lines
        .into_iter()
        .enumerate()
        .map(|(n, mut line)| {
            line.range = from + line.range.start..from + line.range.end;
            (line, measure.line(n))
        })
        .collect()
}

/// Prefer gaps that can take the spare width, while retaining the
/// ordinary paragraph and hyphen costs. Beyond the drawing limit, the
/// undistributed width also counts: a shorter line should be exceptional.
fn justification_cost(free: f32, spaces: usize, limit: Option<f32>) -> f32 {
    let Some(limit) = limit.filter(|_| spaces > 0) else {
        return 0.0;
    };
    let extra = free / spaces as f32;
    let left = (free - spaces as f32 * limit).max(0.0);
    extra * extra + left * left
}

/// How a block's lines are aligned.
fn block_align(block: &Block, style: &Style, heading: u8) -> Align {
    let align = match style.book.has(BookStyles::ALIGN) {
        true => block.align,
        false => Align::Auto,
    };
    match align {
        Align::Auto if heading > 0 => Align::Start,
        Align::Auto | Align::Justify if style.justify => Align::Justify,
        Align::Auto | Align::Justify => Align::Start,
        align => align,
    }
}

/// The indent of a block's first line, where it starts (not where a page
/// continues it).
fn first_indent(block: &Block, style: &Style, align: Align) -> f32 {
    let em = block_em(block, style);
    let book = block.indent.filter(|_| style.book.has(BookStyles::INDENT));
    if block.continues || block.kind != BlockKind::Paragraph {
        0.0
    } else if let Some(hanging) = book.filter(|&i| i < 0) {
        // A hanging indent, whatever the alignment.
        f32::from(hanging) / 100.0 * em
    } else if matches!(align, Align::Start | Align::Justify) {
        book.map_or(style.indent, |i| f32::from(i) / 100.0 * em)
    } else {
        0.0
    }
}

/// What laying out a book's pages needs, the same for each of them: the
/// document and the storage it is read from, how wide a line is, the
/// style, how text is measured, and how words are hyphenated, if they
/// are.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct LayoutContext<'a> {
    pub document: &'a dyn Document,
    pub storage: &'a dyn Storage,
    /// The width of a line.
    pub width: f32,
    pub style: &'a Style,
    pub typesetter: &'a dyn Typesetter,
    /// Words that do not fit at the end of a line are hyphenated with it.
    pub hyphenate: Option<Hyphenate<'a>>,
}

impl<'a> LayoutContext<'a> {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(
        document: &'a dyn Document,
        storage: &'a dyn Storage,
        width: f32,
        style: &'a Style,
        typesetter: &'a dyn Typesetter,
    ) -> Self {
        Self {
            document,
            storage,
            width,
            style,
            typesetter,
            hyphenate: None,
        }
    }

    pub fn hyphenate(mut self, hyphenate: impl Into<Option<Hyphenate<'a>>>) -> Self {
        self.hyphenate = hyphenate.into();
        self
    }
}

impl core::fmt::Debug for LayoutContext<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LayoutContext")
            .field("width", &self.width)
            .field("style", self.style)
            .field("hyphenate", &self.hyphenate.is_some())
            .finish_non_exhaustive()
    }
}

/// The start of the line `at` falls in, the block laid out from its
/// start as [`layout_page`] does: a page shown from a position kept with
/// another layout (other fonts, a turned screen) or found by a search
/// starts with that whole line, not inside it. Before a block (at the
/// start of a section, say), the start of that block, as a page from
/// there starts.
pub fn line_start(cx: &LayoutContext<'_>, at: Locator) -> Result<Locator, DocumentError> {
    let LayoutContext {
        document,
        storage,
        style,
        ..
    } = *cx;
    let Some(block) = document.block_at(storage, at)? else {
        return Ok(at);
    };
    if at.offset < block.start {
        return Ok(Locator::new(at.section, block.start));
    }
    if block.kind == BlockKind::Image || at.offset == block.start {
        return Ok(at);
    }
    let from = block.text_index(at.offset);
    if from == 0 {
        return Ok(at);
    }
    let (heading, base) = block_face(&block, style);
    let align = block_align(&block, style, heading);
    let indent = first_indent(&block, style, align);
    let justify_limit = (align == Align::Justify).then(|| {
        let em = block_em(&block, style);
        block_line(&block, style, cx.typesetter.line(base), em).1 * MAX_JUSTIFY_GAP
    });
    let lines = block_lines(&block, 0, base, cx, heading, indent, justify_limit);
    let start = lines
        .iter()
        .rev()
        .map(|(line, _)| line.range.start)
        .find(|start| *start <= from)
        .unwrap_or(0);
    Ok(Locator::new(at.section, block.source_offset(start)))
}

/// Lays out the page starting at `start`, `height` high, as `cx` says
/// (see [`LayoutContext`]).
///
/// Headings are set in their own face, never indented, justified or
/// hyphenated, and never left alone at the bottom of a page.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn layout_page(
    cx: &LayoutContext<'_>,
    start: Locator,
    height: f32,
) -> Result<Page, DocumentError> {
    let LayoutContext {
        document,
        storage,
        width,
        style,
        typesetter,
        ..
    } = *cx;
    let mut page = Page {
        start,
        end: start,
        lines: Vec::new(),
        images: Vec::new(),
    };
    let mut at = start;
    let mut y = 0.0f32;
    let body_line = typesetter.line(Face::default()).1.max(MIN_LINE_HEIGHT);
    // The heading laid out last, while nothing else followed it on the
    // page: the lines before it, where it starts, and whether anything
    // came before it. A paragraph that goes on whole takes it along.
    let mut heading_before: Option<(usize, Locator, bool)> = None;

    loop {
        let Some(block) = document.block_at(storage, at)? else {
            // End of the section: chapters start on a new page.
            let next = at
                .section
                .checked_add(1)
                .filter(|&n| n < document.sections());
            if let Some(next) = next {
                at = Locator::new(next, 0);
                if page.is_past_end() {
                    page.start = at;
                    continue;
                }
            }
            break;
        };
        // Every block takes the layout further, or it would never end.
        if block.end <= at.offset {
            return Err(malformed());
        }
        let from = if at.offset > block.start {
            block.text_index(at.offset)
        } else {
            0
        };
        if page.is_past_end() && from == 0 && at.offset < block.start {
            page.start = Locator::new(at.section, block.start);
        }
        // What follows a heading, if this block does.
        let after_heading = heading_before.take();
        if block.kind == BlockKind::Image
            && let Some(image) = &block.image
        {
            // A picture without size has nothing to show (and no scale).
            if image.width == 0 || image.height == 0 {
                at = Locator::new(at.section, block.end);
                continue;
            }
            // Scaled to fit the content box, keeping its proportions.
            let scale = (width / image.width as f32).min(height / image.height as f32);
            let (w, h) = (image.width as f32 * scale, image.height as f32 * scale);
            let spacing = if page.is_past_end() {
                0.0
            } else {
                style.paragraph_spacing.max(body_line * 0.5)
            };
            if !page.is_past_end() && y + spacing + h > height {
                return ended(page, Locator::new(at.section, block.start));
            }
            if page.is_past_end() {
                page.start = Locator::new(at.section, block.start);
            }
            y += spacing;
            page.images.push(PageImage {
                x: (width - w) / 2.0,
                y,
                width: w,
                height: h,
                image: image.clone(),
            });
            y += h + spacing;
            at = Locator::new(at.section, block.end);
            continue;
        }
        let (heading, base) = block_face(&block, style);
        let em = block_em(&block, style);
        let (ascent, line_height) = block_line(&block, style, typesetter.line(base), em);
        // Room a plugin asked for above every line: it is part of each
        // line's height, so the page breaks with it.
        let leading = block_leading(&block, em);
        let letters = line_height;
        let (ascent, line_height) = (ascent + leading, line_height + leading);
        let lines_before = page.lines.len();
        let had_content = !page.is_past_end();
        let at_block_start = from == 0 && !block.continues;
        // The book's space where it sets one; at the top of a page only
        // where a chapter starts (e.g. a title set lower).
        let book_space = block
            .space
            .filter(|_| style.book.has(BookStyles::SPACING))
            .map(|s| f32::from(s) / 100.0 * em);
        if page.is_past_end() && at_block_start && book_space.is_some() {
            let first = document.block_at(storage, Locator::new(at.section, 0))?;
            if first.is_some_and(|f| f.start == block.start) {
                y += book_space.unwrap_or(0.0).min(height / 3.0);
            }
        }
        if !page.is_past_end() && at_block_start {
            y += book_space.unwrap_or(style.paragraph_spacing);
            if heading > 0 {
                y += body_line * HEADING_SPACE;
            }
        }
        let align = block_align(&block, style, heading);
        let indent = if at_block_start {
            first_indent(&block, style, align)
        } else {
            0.0
        };
        // Balanced, the whole block is broken from its start, its first
        // line indented as there.
        let block_indent = match style.balanced {
            true => first_indent(&block, style, align),
            false => indent,
        };
        let justify_limit = (align == Align::Justify).then_some(letters * MAX_JUSTIFY_GAP);
        let lines = block_lines(&block, from, base, cx, heading, block_indent, justify_limit);

        for (n, (line, (first_x, available))) in lines.iter().enumerate() {
            let (first_x, available) = (*first_x, *available);
            let fits = y + line_height <= height;
            if !fits && !page.is_past_end() {
                // Widows and orphans: as many lines go on as the next page
                // needs, or the paragraph goes whole where too few would
                // stay here.
                let left = lines.len() - n;
                let back = usize::from(style.widows).saturating_sub(left).min(n);
                let stay = n - back;
                let orphans = usize::from(style.orphans.max(1));
                let alone = stay < orphans;
                let line_start = |i: usize| {
                    Locator::new(at.section, block.source_offset(lines[i].0.range.start))
                };
                if alone && had_content {
                    match after_heading {
                        // The heading just before goes along.
                        Some((kept, start, true)) => {
                            page.lines.truncate(kept);
                            return ended(page, start);
                        }
                        // The paragraph goes whole.
                        None => {
                            page.lines.truncate(page.lines.len() - n);
                            return ended(page, line_start(0));
                        }
                        // Under a heading that starts the page nothing can
                        // go: it stays as it falls.
                        Some(_) => {}
                    }
                } else if back > 0 && !alone {
                    page.lines.truncate(page.lines.len() - back);
                    return ended(page, line_start(stay));
                }
                // Starting the page, it cannot go whole: then it stays as it
                // falls rather than leaving an orphan here.
                return ended(page, line_start(n));
            }
            let free = (available - line.width).max(0.0);
            let (x, word_spacing) = match align {
                Align::Center => (first_x + free / 2.0, 0.0),
                Align::End => (first_x + free, 0.0),
                Align::Justify if !line.last && line.spaces > 0 => {
                    let extra = free / line.spaces as f32;
                    // Bound wide gaps continuously. A line that cannot
                    // reach the edge keeps a ragged end instead of
                    // suddenly losing all of its justification.
                    (first_x, extra.min(letters * MAX_JUSTIFY_GAP))
                }
                _ => (first_x, 0.0),
            };
            let mut line_text = String::from(&block.text[line.range.clone()]);
            if line.hyphen {
                line_text.push('-');
            }
            let range = line.range.clone();
            let words = word_sources(&block, range.clone());
            page.lines.push(PageLine {
                x,
                baseline: y + ascent,
                text: line_text,
                word_spacing,
                runs: line_runs(&block, base, range, usize::from(line.hyphen), style.book),
                section: at.section,
                words,
                leading,
                inserted: false,
                annotations: Vec::new(),
            });
            y += line_height;
        }
        // What a filter inserted after the block, below its last line and
        // never apart from it.
        if !block.inserted.is_empty() {
            let (placed, below, whole) = inserted_lines(&block, cx, at.section, y, height);
            if !whole {
                // Not all of it fits: the page ends before the block's last
                // line, which goes on the next page with it, where all of
                // it fits there. Where it would not fit on any page (or
                // that line starts the page), what fits stays here and the
                // rest goes: what is inserted has no place to go on from.
                let last = page.lines.len().saturating_sub(1);
                let (_, tall, _) = inserted_lines(&block, cx, at.section, 0.0, f32::INFINITY);
                if page.lines.len() > lines_before
                    && last > 0
                    && line_height + tall <= height
                    && let Some((line, _)) = lines.last()
                {
                    page.lines.truncate(last);
                    let start = Locator::new(at.section, block.source_offset(line.range.start));
                    return ended(page, start);
                }
                page.lines.extend(placed);
                page.end = Locator::new(at.section, block.end);
                return Ok(page);
            }
            page.lines.extend(placed);
            y = below;
        }
        // A heading needs at least one line of what follows on its page.
        let heading_start = Locator::new(at.section, block.source_offset(from));
        if heading > 0 && had_content && y + body_line > height {
            page.lines.truncate(lines_before);
            return ended(page, heading_start);
        }
        if heading > 0 {
            heading_before = Some((lines_before, heading_start, had_content));
        }
        at = Locator::new(at.section, block.end);
    }
    page.end = at;
    Ok(page)
}

/// A block's lines: `(ascent, line height)`, the reader's (`reader`, as
/// the typesetter says) or, where `style` follows it, the book's
/// distance between lines in percent of the block's text size (`em`),
/// kept between its size and three times it, the text centred in it.
fn block_line(block: &Block, style: &Style, reader: (f32, f32), em: f32) -> (f32, f32) {
    let (ascent, line_height) = (reader.0, reader.1.max(MIN_LINE_HEIGHT));
    match block
        .line_height
        .filter(|_| style.book.has(BookStyles::LINE_HEIGHT))
    {
        Some(percent) => {
            let book = (em * f32::from(percent.clamp(100, 300)) / 100.0).max(MIN_LINE_HEIGHT);
            (ascent + (book - line_height) / 2.0, book)
        }
        None => (ascent, line_height),
    }
}

/// The lines of the blocks inserted after `block` ([`Block::inserted`]),
/// set from `y` down on a page `height` high, as many as fit; where they
/// end, and whether all fit. They take the paragraph spacing before
/// each, their own face, size and alignment, and no places.
fn inserted_lines(
    block: &Block,
    cx: &LayoutContext<'_>,
    section: u32,
    mut y: f32,
    height: f32,
) -> (Vec<PageLine>, f32, bool) {
    let (style, typesetter) = (cx.style, cx.typesetter);
    let mut placed = Vec::new();
    for inserted in &block.inserted {
        if inserted.kind == BlockKind::Image || inserted.text.is_empty() {
            continue;
        }
        let (heading, base) = block_face(inserted, style);
        let em = block_em(inserted, style);
        let (ascent, line_height) = block_line(inserted, style, typesetter.line(base), em);
        let leading = block_leading(inserted, em);
        let (ascent, line_height) = (ascent + leading, line_height + leading);
        let align = block_align(inserted, style, heading);
        y += style.paragraph_spacing;
        let indent = first_indent(inserted, style, align);
        for (line, (first_x, available)) in
            block_lines(inserted, 0, base, cx, heading, indent, None)
        {
            if y + line_height > height {
                return (placed, y, false);
            }
            let free = (available - line.width).max(0.0);
            let x = match align {
                Align::Center => first_x + free / 2.0,
                Align::End => first_x + free,
                _ => first_x,
            };
            let mut text = String::from(&inserted.text[line.range.clone()]);
            if line.hyphen {
                text.push('-');
            }
            let runs = line_runs(
                inserted,
                base,
                line.range.clone(),
                usize::from(line.hyphen),
                style.book,
            );
            placed.push(PageLine {
                x,
                baseline: y + ascent,
                text,
                word_spacing: 0.0,
                runs,
                section,
                words: Vec::new(),
                leading,
                inserted: true,
                annotations: Vec::new(),
            });
            y += line_height;
        }
    }
    (placed, y, true)
}

/// The room above each of `block`'s lines that a plugin asked for
/// ([`Block::leading_above`]), in pixels for its text size `em`.
fn block_leading(block: &Block, em: f32) -> f32 {
    let hundredths = block.leading_above.unwrap_or(0).min(MAX_LEADING_ABOVE);
    em * f32::from(hundredths) / 100.0
}

/// `page`, ending at `end`. A page that ends where it starts, or before,
/// would be shown again and again.
fn ended(mut page: Page, end: Locator) -> Result<Page, DocumentError> {
    if end <= page.start {
        return Err(malformed());
    }
    page.end = end;
    Ok(page)
}

fn malformed() -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed("a block that does not advance"))
}

#[cfg(test)]
mod tests;
