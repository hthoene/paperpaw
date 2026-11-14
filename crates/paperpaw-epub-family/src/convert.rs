//! Converts an XHTML chapter into PaperPaw's compact block format while it
//! is inflated, and stores the result in the plugin's cache:
//!
//! - `<n>.txt`: records of an 8-byte header (kind, heading level, flags,
//!   alignment, text length and number of style runs as u16), the block's
//!   UTF-8 text and its style runs (start, end and style as u16).
//! - `<n>.idx`: for every record, the offset of its text as u32 (for
//!   binary search), the book's indent as i16 and the book's space above
//!   as u16, in hundredths of an em ([`NO_INDENT`] and [`NO_SPACE`] if it
//!   sets none), its text size as u16 in percent of body text (0 if it
//!   sets none), its margins at the start and the end of the lines as u16
//!   each, in hundredths of an em, and its distance between lines as u16
//!   in percent of its text size (0 if it sets none), its language as
//!   `Lang` bits (u32), its direction (u8: 0 from its text, 1 left to
//!   right, 2 right to left), the number of its inline languages (u8,
//!   their `Lang` bits as u32 each after its style runs), the lines of
//!   its drop capital (u8, 0 for none) and a byte 0.
//!   Kept apart from the
//!   text so positions in it stay the same when more is stored per
//!   block.
//! - `<n>.anc`: `id<TAB>offset` lines for anchors (table of contents).
//! - `<n>.lnk`: `start<TAB>end<TAB>href` lines for links, in order: `href`
//!   as an archive path with its fragment (`text/ch2.xhtml#note3`), or as
//!   written for links out of the book.
//! - `<n>.pos`: `offset<TAB>path` lines: the element each record comes
//!   from, as a path below `<body>` such as `div[2]/p[22]`, for positions
//!   other readers understand (KOReader's XPointers).
//! - `<n>.xpt`: `offset<TAB>node<TAB>char` lines: where the text of each
//!   text node of the chapter begins in `<n>.txt` (and goes on, where
//!   what PaperPaw shows leaves out characters the node has), as the node
//!   below `<body>` such as `div[2]/p[22]/em[1]/text()[1]` and the
//!   character in it, for exact positions in KOReader's XPointers. Nodes
//!   and characters are counted as KOReader's engine (crengine) keeps
//!   them: white space folded into one space, white space alone at the
//!   start of a block dropped.
//! - `<n>.end`: the length of `<n>.txt` as u32. Written last, so its
//!   presence marks a complete conversion.
//!
//! All files are appended in small pieces while converting, and positions
//! in a chapter are offsets into `<n>.txt`, so neither converting nor
//! reading ever holds more than a block in memory. The pictures of a
//! chapter are listed in `<n>.pic` while it is read and taken in after
//! it, so they do not take memory either. What is kept per open element
//! is bounded, and elements nested deeper than [`MAX_DEPTH`] are read as
//! part of the deepest one, so no markup, however deep, exhausts memory;
//! what is written for a chapter is bounded by its size (see
//! [`budget`]).
//!
//! HTML files (e.g. web pages) are converted the same way, read as
//! browsers do (see [`Source::Html`]), and FictionBooks, their elements
//! read as the HTML ones they look like (see [`Source::Fb2`]).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Align, BlockKind, DocumentError, Run, TextStyle};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::lang::{Direction, Lang};
use paperpaw_sdk::xml::{Token, Tokenizer, local_name, try_decode};
use paperpaw_sdk::zip::{Entry, ZipArchive};

use crate::css::{self, Props, Rules};
use crate::fallible::{self, TryPush, TryPushStr};

mod decode;
mod direction;
mod elements;
mod lists;
use crate::package::error;
pub(crate) use decode::{decode_entry, decode_file, decode_image};
use decode::{decode_range, image_size};
pub(crate) use direction::{BookText, dir_from_bits};
use elements::{element_props, furniture, heading_level, is_block, is_capital, is_void, styled};

/// Size of a record header.
pub(crate) const HEADER: u32 = 8;
/// Bytes per record in `<n>.idx`.
pub(crate) const INDEX: u32 = 24;
/// A record without the book's indent or space.
pub(crate) const NO_INDENT: i16 = i16::MIN;
pub(crate) const NO_SPACE: u16 = u16::MAX;
/// Size of a style run after a record's text.
pub(crate) const RUN: usize = 8;
/// The most a block is set in from each side by the elements around it,
/// in hundredths of an em (the layout also keeps it to a quarter of a
/// line).
const MAX_MARGIN: u16 = 1000;
/// Longer paragraphs are split into continued blocks; no record's text
/// is longer.
pub(crate) const LIMIT: usize = 4000;
/// Output is written to storage in chunks of about this size.
const FLUSH: usize = 1024;
/// Index and anchors are written in chunks of about this size.
const FLUSH_SMALL: usize = 256;
/// Elements nested deeper are read as part of the deepest one.
pub(crate) const MAX_DEPTH: usize = 128;
/// Element names are kept to this length (no HTML name is longer).
const MAX_NAME: usize = 32;
/// Names of children counted per element; children of further names are
/// numbered as if each were the first of its name.
const MAX_CHILD_NAMES: usize = 16;
/// Element paths (`div[2]/p[22]`) grow to at most this length; elements
/// further down share the path of the last that fit.
const MAX_PATH: usize = 160;
/// A block of text from more text nodes than this, or whose text nodes'
/// paths take more bytes, is continued in the next block, as one longer
/// than [`LIMIT`] is.
const MAX_ORIGINS: usize = 256;
const MAX_ORIGIN_BYTES: usize = 8 * 1024;
/// Pictures told apart per chapter, so each is listed once; more are
/// listed as they come.
const MAX_SEEN_PICTURES: usize = 512;
/// Link targets longer than this are left out (no reader stores them).
const MAX_HREF: usize = 2000;

const FLAG_CONTINUES: u8 = 1;
/// The next record continues it.
const FLAG_CONTINUED: u8 = 2;

/// Where a chapter's markup comes from.
pub(crate) enum Source<'a> {
    /// A chapter of an EPUB.
    Zip {
        archive: &'a ZipArchive,
        entry: &'a Entry,
    },
    /// An HTML file in storage. `<script>` and `<style>` hold raw text,
    /// elements may be left open, and only the reading matter is shown:
    /// the `focus` element (`main` or `article`) if the page has one, and
    /// never navigation, forms and the like. Links are made absolute with
    /// `base` (the page's address), else left relative to the file.
    Html {
        path: &'a str,
        focus: Option<&'a str>,
        base: Option<&'a str>,
        /// Whether its pictures are shown: those on the storage, beside a
        /// file made from Markdown (web pages' are not fetched).
        pictures: bool,
    },
    /// A FictionBook in storage: one XML file. Section titles get the
    /// anchors `fb2-toc-<n>` for the table of contents; pictures come
    /// from its `<binary>` elements.
    Fb2 {
        path: &'a str,
        encoding: crate::fb2::Encoding,
    },
}

/// A markup read as HTML, as FictionBook is (see `crate::fb2`): the HTML
/// element each of its elements is read as, and what entering one asks of
/// the converter. XHTML needs none.
pub(crate) trait Dialect {
    /// Element `tag` entered.
    fn enter(
        &mut self,
        tag: &str,
        attrs: &paperpaw_sdk::xml::Attrs<'_>,
    ) -> Result<Entered, DocumentError>;
    /// Element `tag` left: the HTML element it was read as.
    fn leave(&mut self, tag: &str) -> Result<String, DocumentError>;
    /// Where the picture `href` points is kept as, if not as a path
    /// relative to the chapter.
    fn picture(&self, href: &str) -> Result<Option<String>, DocumentError>;
}

/// An element of a [`Dialect`] entered.
#[derive(Debug, Default)]
pub(crate) struct Entered {
    /// The HTML element it is read as.
    pub name: String,
    /// An anchor to set where it begins (e.g. for the contents).
    pub anchor: Option<String>,
    /// A line break first, if text came before it in the block.
    pub line_break: bool,
}

pub(crate) use crate::cache_paths::CachePaths;

/// Images are decoded at most this large where the screen does not say
/// (see `OpenOptions::image_fit`); readers scale them to the page.
pub(crate) const IMAGE_FIT: Fit = paperpaw_sdk::IMAGE_FIT;

pub(crate) fn encode_kind(kind: BlockKind) -> (u8, u8) {
    match kind {
        BlockKind::Heading(level) => (1, level),
        BlockKind::Preformatted => (2, 0),
        BlockKind::Image => (3, 0),
        _ => (0, 0),
    }
}

pub(crate) fn decode_kind(kind: u8, level: u8) -> BlockKind {
    match kind {
        1 => BlockKind::Heading(level.clamp(1, 6)),
        2 => BlockKind::Preformatted,
        3 => BlockKind::Image,
        _ => BlockKind::Paragraph,
    }
}

/// The cache name of the image at `archive_path`, without its kind (see
/// `pictures`).
pub(crate) fn image_path(dir: &str, archive_path: &str) -> Result<String, DocumentError> {
    let hash = paperpaw_sdk::digest::fnv1a64(archive_path);
    fallible::try_format!("{dir}/img-{hash:016x}")
}

/// The larger of two spaces, as collapsing margins are.
fn larger(a: Option<u16>, b: Option<u16>) -> Option<u16> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

pub(crate) fn continues(flags: u8) -> bool {
    flags & FLAG_CONTINUES != 0
}

pub(crate) fn continued(flags: u8) -> bool {
    flags & FLAG_CONTINUED != 0
}

/// An open element: the style and alignment its content inherits.
struct Frame {
    tag: String,
    style: TextStyle,
    align: Align,
    /// The direction of the paragraphs it holds, inherited; a block
    /// element's `dir` or CSS `direction` sets it.
    dir: Direction,
    /// Its language, inherited.
    lang: Lang,
    /// The book's indent, inherited as in CSS.
    indent: Option<i16>,
    /// The book's text size in percent of body text, inherited.
    size: Option<u16>,
    /// How far its blocks are set in from the start and the end of the
    /// lines: its own margin and padding at the sides and those of the
    /// blocks around it, in hundredths of an em.
    margins: (u16, u16),
    /// The book's distance between lines, in percent of the text size,
    /// inherited.
    line_height: Option<u16>,
    /// The items of a list (`<ol>`, `<ul>`) so far, and the marker style
    /// of its items, inherited.
    numbered: Option<u32>,
    list_style: Option<css::ListStyle>,
    /// The margin and padding below the element, taken over by the next
    /// block.
    after: (Option<u16>, Option<u16>),
    /// The element's content is not shown (`<head>`, `display: none`, …).
    hides: bool,
    /// The control that ends its text set apart (see [`direction`]).
    closer: Option<char>,
    /// A link (`<a href>`): where it starts, and where it goes.
    link: Option<(u32, String)>,
    /// The reading matter of an HTML page: its content is shown.
    focus: bool,
    /// How many children of each name (by its hash) it has had so far.
    children: Vec<(u64, u32)>,
    /// The length of the converter's element path before this element.
    path: usize,
    /// Its children so far, elements and text nodes as crengine keeps
    /// them, and of these the text nodes.
    nodes: u32,
    texts: u32,
    /// It is a block, in which white space alone before anything else is
    /// no text node for crengine.
    block: bool,
}

/// The element paths of text nodes, each kept once and named by its number:
/// a shared string with a fallible constructor (`Rc` has none). Paths no
/// origin, node or pending space names are dropped now and then
/// ([`Converter::compact_paths`]).
#[derive(Default)]
struct NodePaths {
    list: Vec<alloc::boxed::Box<str>>,
    /// The length at which the paths are looked through next.
    compact_at: usize,
}

/// A path in [`NodePaths`].
type NodeId = u32;

/// Paths kept before the unused ones are looked for.
const PATHS_MIN: usize = 64;

impl NodePaths {
    fn add(&mut self, path: &str) -> Result<NodeId, DocumentError> {
        let mut text = fallible::string_with(path.len())?;
        text.try_push_str(path)?;
        let id = NodeId::try_from(self.list.len()).map_err(|_| fallible::OOM)?;
        // Exact room, so `into_boxed_str` does not allocate again.
        fallible::push(&mut self.list, text.into_boxed_str())?;
        Ok(id)
    }

    fn get(&self, id: NodeId) -> &str {
        self.list.get(id as usize).map_or("", |path| path)
    }
}

/// The text node being read, counted as crengine keeps it.
struct TextNode {
    /// Below `<body>`, e.g. `div[2]/p[3]/text()[1]`.
    path: NodeId,
    /// Characters so far.
    chars: u32,
    /// The last of them is a (folded) space.
    space: bool,
    /// Only white space so far.
    blank: bool,
    /// Its parent had no children before it.
    first: bool,
}

/// Where a character of the block being collected comes from: its byte
/// in the block's text, the text node and the character in it. Only the
/// first of characters that follow each other in a node is kept.
struct Origin {
    at: u32,
    node: NodeId,
    char: u32,
}

/// How an HTML page is read.
struct Html {
    /// The element holding the reading matter.
    focus: Option<String>,
    /// The page's address, for absolute links.
    base: Option<String>,
    /// Whether its pictures are shown (see [`Source::Html`]).
    pictures: bool,
}

struct Converter<'a> {
    storage: &'a dyn Storage,
    paths: &'a CachePaths,
    /// Bytes written to `<n>.txt` plus those pending in `out`.
    length: u32,
    out: Vec<u8>,
    index: Vec<u8>,
    anchors: String,
    /// Pending `<n>.lnk` lines.
    links: String,
    /// The chapter's archive path, for links within it.
    chapter: String,
    /// Reading HTML: the page's address, if known (else `None` for
    /// EPUB chapters and HTML without one).
    html: Option<Html>,
    /// Reading markup that is not HTML, e.g. a FictionBook.
    dialect: Option<crate::fb2::Fb2Dialect>,
    /// Pending `<n>.pos` lines.
    positions: String,
    /// Pending `<n>.xpt` lines.
    xpointers: String,
    /// The text node being read, if any.
    node: Option<TextNode>,
    /// Where the characters of `text` come from, and the bytes of the
    /// paths of their text nodes.
    origins: Vec<Origin>,
    origin_bytes: usize,
    /// Characters of `text` since the last origin.
    since_origin: u32,
    /// Where the next character pushed comes from, if from a text node.
    origin: Option<(NodeId, u32)>,
    /// Where the pending space comes from.
    space_origin: Option<(NodeId, u32)>,
    /// The paths of the text nodes the origins name.
    node_paths: NodePaths,
    /// The path of the innermost open element below `<body>`, e.g.
    /// `div[2]/p[22]` (indexes count siblings of the same name).
    path: String,
    /// Where `<body>` is in the stack, while it is open.
    body: Option<usize>,
    /// The element path of the block being collected.
    block_path: String,
    text: String,
    /// Style runs of `text`.
    runs: Vec<Run>,
    /// Alignment of the current block, set by its first character.
    align: Option<Align>,
    /// The book's language and direction, for text that sets none.
    book: BookText,
    /// The direction and language of the current block, set by its first
    /// character, and the other languages in it.
    dir: Option<Direction>,
    lang: Option<Lang>,
    inline_langs: Vec<Lang>,
    /// The lines of the current block's drop capital, and the depth of
    /// the element that asked for it while it is open.
    drop_cap: (u8, Option<usize>),
    /// A list item's marker, waiting for the item's text.
    marker: Option<lists::Marker>,
    /// The book's indent of the current block, set by its first character.
    indent: Option<i16>,
    /// The book's text size of the current block, set likewise.
    size: Option<u16>,
    /// The book's margins at the sides of the current block and its
    /// distance between lines, set likewise.
    margins: (u16, u16),
    line_height: Option<u16>,
    /// The book's space above the next block, as in CSS: the largest of
    /// the margins met since the last block (margins collapse) plus all
    /// paddings.
    margin: Option<u16>,
    padding: Option<u16>,
    /// Open elements.
    stack: Vec<Frame>,
    /// Elements open below the deepest kept in `stack` (see
    /// [`MAX_DEPTH`]).
    overflow: u32,
    /// The book's stylesheet rules, plus those of `<style>` elements.
    rules: Rules,
    /// Whether the book's CSS applies.
    styles: bool,
    css: css::Parser,
    /// Directory of the chapter in the archive, for relative links.
    base: String,
    /// Pending `<n>.pic` lines: the pictures to take in after the
    /// chapter, by where they are (see [`Converter::image`]).
    pictures: String,
    /// Hashes of the pictures listed so far, sorted, to list each once.
    seen: Vec<u64>,
    /// Bytes written for the chapter so far, and the most there may be
    /// (see [`budget`]). Once that is reached, nothing more is read.
    written: core::cell::Cell<u64>,
    budget: u64,
    full: bool,
    kind: BlockKind,
    continues: bool,
    /// Depth inside elements whose content is not shown.
    hidden: u32,
    /// Depth inside `<pre>`.
    pre: u32,
    space: bool,
    /// Style where the pending space was, e.g. inside a bold word group.
    space_style: TextStyle,
    failed: Option<DocumentError>,
}

impl Converter<'_> {
    fn frame(&self) -> (TextStyle, Align) {
        self.stack
            .last()
            .map_or((TextStyle::REGULAR, Align::Auto), |f| (f.style, f.align))
    }

    /// Appends text in the current style.
    fn push(&mut self, c: char) -> Result<(), DocumentError> {
        let (style, _) = self.frame();
        self.push_styled(c, style)
    }

    /// Appends text in `style`.
    fn push_styled(&mut self, c: char, style: TextStyle) -> Result<(), DocumentError> {
        if self.text.is_empty() && self.marker.is_some() {
            self.place_marker()?;
        }
        let (_, align) = self.frame();
        let (dir, lang) = self
            .stack
            .last()
            .map_or((self.book.dir, self.book.lang), |f| (f.dir, f.lang));
        if self.text.is_empty() {
            if self.align.is_none() {
                self.align = Some(align);
            }
            self.dir.get_or_insert(dir);
            self.lang.get_or_insert(lang);
            // The block's, not a drop capital's (its large size).
            let cap = self.drop_cap.1.map(|at| at.checked_sub(1));
            let block = cap.map_or(self.stack.last(), |at| at.and_then(|at| self.stack.get(at)));
            self.indent = block.and_then(|f| f.indent);
            self.size = block.and_then(|f| f.size);
            self.margins = block.map_or((0, 0), |f| f.margins);
            self.line_height = block.and_then(|f| f.line_height);
            self.block_path.clear();
            self.block_path.try_push_str(&self.path)?;
        }
        let start = self.text.len() as u32;
        if let Some((node, char)) = self.origin.take() {
            let follows = self
                .origins
                .last()
                .is_some_and(|o| o.node == node && o.char + self.since_origin == char);
            if !follows {
                if self.origins.last().is_none_or(|o| o.node != node) {
                    self.origin_bytes += self.node_paths.get(node).len();
                }
                self.origins.try_push(Origin {
                    at: start,
                    node,
                    char,
                })?;
                self.since_origin = 0;
            }
        }
        self.since_origin += 1;
        self.text.try_push(c)?;
        let end = self.text.len() as u32;
        // Text in another language than its block's: by its place among
        // the block's others.
        let style = match self.lang.is_some_and(|own| own != lang) {
            true => style.with_lang_index(direction::inline_index(&mut self.inline_langs, lang)?),
            false => style,
        };
        if style == TextStyle::REGULAR {
            return Ok(());
        }
        match self.runs.last_mut() {
            Some(run) if run.end == start && run.style == style => run.end = end,
            _ => self.runs.try_push(Run::new(start, end, style))?,
        }
        Ok(())
    }

    fn push_str(&mut self, text: &str) -> Result<(), DocumentError> {
        for c in text.chars() {
            self.push(c)?;
        }
        Ok(())
    }

    /// Whether only `<svg>` elements hide the current content.
    fn only_svg_hides(&self) -> bool {
        self.stack
            .iter()
            .filter(|f| f.hides)
            .all(|f| f.tag == "svg")
    }

    /// An image block for `href` (relative to the chapter; in a
    /// FictionBook, `#` and a binary's id).
    fn image(&mut self, href: &str, alt: &str) -> Result<(), DocumentError> {
        let own = match self.dialect.as_ref() {
            Some(dialect) => dialect.picture(href)?,
            None => None,
        };
        let path = match own {
            Some(path) => path,
            None => crate::package::try_resolve(&self.base, href)?.0,
        };
        let cache = image_path(&self.paths.dir, &path)?;
        self.flush()?;
        self.kind = BlockKind::Image;
        self.text.try_push_str(&cache)?;
        self.text.try_push('\t')?;
        // As much of the alternative text as the record has room for.
        let room = LIMIT.saturating_sub(self.text.len());
        clean_alt(alt, room, &mut self.text)?;
        self.write_record(BlockKind::Image, self.text.len(), false)?;
        self.text.clear();
        self.runs.clear();
        self.origins.clear();
        self.origin_bytes = 0;
        self.kind = BlockKind::Paragraph;
        // Listed a line each: a path with a line break (`%0A`) names no
        // picture of the book, and is shown as its alternative text.
        if self.full || path.chars().any(char::is_control) {
            return Ok(());
        }
        let hash = paperpaw_sdk::digest::fnv1a64(&path);
        match self.seen.binary_search(&hash) {
            Ok(_) => return Ok(()),
            Err(at) if self.seen.len() < MAX_SEEN_PICTURES => {
                fallible::reserve(&mut self.seen, 1)?;
                self.seen.insert(at, hash);
            }
            Err(_) => {}
        }
        self.pictures.try_push_str(&path)?;
        self.pictures.try_push('\n')?;
        if self.pictures.len() >= FLUSH_SMALL {
            self.write_pictures();
        }
        Ok(())
    }

    /// Removes the first `n` bytes of the text, keeping the runs and the
    /// origins in step.
    fn drop_front(&mut self, n: usize) {
        let gone = self.origins.partition_point(|o| (o.at as usize) < n);
        // The rest goes on in the node of the last origin removed.
        let carry = (gone > 0
            && n < self.text.len()
            && self.origins.get(gone).is_none_or(|o| o.at as usize != n))
        .then(|| {
            let last = &self.origins[gone - 1];
            Origin {
                at: n as u32,
                node: last.node,
                char: last.char + self.text[last.at as usize..n].chars().count() as u32,
            }
        });
        let last_gone = gone == self.origins.len();
        self.origins.drain(..gone);
        if let Some(carry) = carry {
            if last_gone {
                self.since_origin = self.text[n..].chars().count() as u32;
            }
            self.origins.insert(0, carry);
        }
        for origin in &mut self.origins {
            origin.at -= n as u32;
        }
        self.origin_bytes = self
            .origins
            .iter()
            .enumerate()
            .filter(|(i, o)| {
                i.checked_sub(1)
                    .and_then(|i| self.origins.get(i))
                    .is_none_or(|before| before.node != o.node)
            })
            .map(|(_, o)| self.node_paths.get(o.node).len())
            .sum();
        self.text.replace_range(..n, "");
        let n = n as u32;
        self.runs.retain_mut(|run| {
            run.start = run.start.saturating_sub(n);
            run.end = run.end.saturating_sub(n);
            run.end > run.start
        });
    }

    /// Where the next character of the current block will be.
    fn position(&self) -> u32 {
        self.length + HEADER + self.text.len() as u32
    }

    /// Where an element starting now begins: after the space still to
    /// come, or at the next block if none is being collected.
    fn here(&self) -> u32 {
        if self.text.is_empty() {
            self.length + HEADER
        } else {
            self.position() + u32::from(self.space)
        }
    }

    /// Where a link goes, as stored in `<n>.lnk`; `None` for none.
    fn target(&self, href: &str) -> Result<Option<String>, DocumentError> {
        let mut clean = String::new();
        fallible::reserve_str(&mut clean, href.trim().len())?;
        clean.extend(href.trim().chars().filter(|c| !c.is_control()));
        let href = clean;
        if href.is_empty() || paperpaw_sdk::document::is_external(&href) {
            return Ok((!href.is_empty()).then_some(href));
        }
        if let Some(html) = &self.html {
            return Ok(Some(match (href.strip_prefix('#'), &html.base) {
                (Some(_), _) => href,
                (None, Some(base)) => fallible::join_url(base, &href)?,
                (None, None) => crate::package::try_resolve(&self.base, &href)?.0,
            }));
        }
        let (path, fragment) = if let Some(fragment) = href.strip_prefix('#') {
            (
                fallible::string(&self.chapter)?,
                Some(fallible::string(fragment)?),
            )
        } else {
            crate::package::try_resolve(&self.base, &href)?
        };
        Ok(Some(match fragment {
            Some(fragment) => fallible::try_format!("{path}#{fragment}")?,
            None => path,
        }))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn start(
        &mut self,
        name: &str,
        attrs: paperpaw_sdk::xml::Attrs<'_>,
    ) -> Result<(), DocumentError> {
        if self.full {
            return Ok(());
        }
        self.end_node();
        let tag = element_name(name)?;
        let entered = match self.dialect.as_mut() {
            Some(dialect) => Some(dialect.enter(&tag, &attrs)?),
            None => None,
        };
        let tag = match entered {
            Some(entered) => {
                if let Some(id) = entered.anchor {
                    self.flush()?;
                    self.add_anchor(&id)?;
                }
                if entered.line_break && !self.text.is_empty() {
                    self.push('\n')?;
                }
                entered.name
            }
            None => tag,
        };
        let props = if self.hidden == 0 {
            element_props(&tag, &attrs, &mut self.rules, self.styles)?
        } else {
            Props::default()
        };
        let mut hides = matches!(tag.as_str(), "head" | "script" | "style" | "title" | "svg")
            || props.hidden == Some(true);
        if !hides && let Some(html) = &self.html {
            hides = furniture(&tag, &attrs, html.focus.is_some())?;
        }
        // HTML leaves paragraphs and list items open: the next one ends
        // them. So do careless EPUBs with paragraphs, which XHTML never
        // nests.
        let open = self.stack.last().map(|f| f.tag.as_str());
        let closes = match (self.html.is_some(), tag.as_str()) {
            _ if self.overflow > 0 => false,
            (false, "p") => self.dialect.is_none() && open == Some("p"),
            (false, _) => false,
            (true, "li") => matches!(open, Some("li" | "p")),
            (true, "dt" | "dd") => matches!(open, Some("dt" | "dd" | "p")),
            (true, "td" | "th") => matches!(open, Some("td" | "th")),
            (true, "tr") => matches!(open, Some("tr" | "td" | "th")),
            (true, tag) => is_block(tag) && open == Some("p"),
        };
        if closes && let Some(open) = open.map(fallible::string).transpose()? {
            self.close(&open)?;
        }
        // Block elements end the block before them (before their anchor is
        // placed, so it points at the new block), in the old style.
        if !hides
            && self.hidden == 0
            && (is_block(&tag) || matches!(tag.as_str(), "li" | "pre" | "hr"))
        {
            self.flush()?;
        }
        if let Some(id) = attrs.try_get("id").map_err(|_| fallible::OOM)? {
            self.add_anchor(&id)?;
            if self.links.len() >= FLUSH_SMALL {
                self.write_links();
            }
        }
        // Self-closing elements are ended right away by the caller.
        let (style, align) = self.frame();
        if self.text.is_empty() {
            self.margin = larger(self.margin, props.margin_top);
            self.padding = css::sum(self.padding, props.padding_top);
        }
        let indent = props
            .indent
            .or_else(|| self.stack.last().and_then(|f| f.indent));
        let parent_size = self.stack.last().and_then(|f| f.size);
        let size = match props.size {
            Some(size) => Some(size.of(parent_size.unwrap_or(100))),
            None => parent_size,
        };
        // A drop capital: an element that starts a paragraph asks for one.
        if self.text.is_empty() && !is_block(&tag) && self.drop_cap.1.is_none() {
            let lines = elements::drop_cap(&props, size, parent_size);
            self.drop_cap = (lines, (lines > 0).then_some(self.stack.len()));
        }
        // Paragraphs take the direction of the block elements around them
        // (inline ones are not set apart yet); text the language of the
        // innermost element that names one.
        let (parent_dir, parent_lang) = self
            .stack
            .last()
            .map_or((self.book.dir, self.book.lang), |f| (f.dir, f.lang));
        let paragraphs = is_block(&tag) || matches!(tag.as_str(), "html" | "body" | "li" | "pre");
        let dir = direction::own_dir(&attrs, props.direction)?
            .filter(|_| paragraphs)
            .unwrap_or(parent_dir);
        let lang = direction::own_lang(&attrs)?.unwrap_or(parent_lang);
        let controls = direction::inline_controls(&tag, &attrs, paragraphs)?;
        // Blocks are set in by their own margins at the sides and those of
        // the blocks around them; text elements are not. The body's own are
        // the page's: the reader's margins, set in the settings, are those.
        let parent_margins = self.stack.last().map_or((0, 0), |f| f.margins);
        let margins = match is_block(&tag) || matches!(tag.as_str(), "li" | "pre") {
            true => direction::set_in(parent_margins, props.sides(), dir, tag == "body"),
            false => parent_margins,
        };
        let line_height = props
            .line_height
            .or_else(|| self.stack.last().and_then(|f| f.line_height));
        if self.stack.len() >= MAX_DEPTH {
            // Read as part of the deepest element kept.
            self.overflow += 1;
            return Ok(());
        }
        // Its number among its parent's children of the same name.
        let hash = paperpaw_sdk::digest::fnv1a64(&tag);
        let index = match self.stack.last_mut() {
            Some(parent) => {
                parent.nodes += 1;
                match parent.children.iter_mut().find(|(t, _)| *t == hash) {
                    Some((_, count)) => {
                        *count += 1;
                        *count
                    }
                    None => {
                        if parent.children.len() < MAX_CHILD_NAMES {
                            parent.children.try_push((hash, 1))?;
                        }
                        1
                    }
                }
            }
            None => 1,
        };
        // `href`, also as FictionBook writes it (`l:href`). A link inside
        // a link is none (as in HTML), so one target at most is kept.
        let href = attrs
            .iter()
            .find(|(name, _)| local_name(name) == "href")
            .map(|(_, value)| try_decode(value))
            .transpose()
            .map_err(|_| fallible::OOM)?;
        let link = match href {
            Some(href)
                if tag == "a"
                    && !hides
                    && self.hidden == 0
                    && !self.stack.iter().any(|f| f.link.is_some()) =>
            {
                self.target(&href)?
                    .filter(|target| target.len() <= MAX_HREF)
                    .map(|target| (self.here(), target))
            }
            _ => None,
        };
        // Its path, below `<body>`.
        let path = self.path.len();
        if self.body.is_some() {
            let segment = fallible::try_format!("{tag}[{index}]")?;
            if path + 1 + segment.len() <= MAX_PATH {
                if path > 0 {
                    self.path.try_push('/')?;
                }
                self.path.try_push_str(&segment)?;
            }
        } else if tag == "body" {
            self.body = Some(self.stack.len());
        }
        let focus = self
            .html
            .as_ref()
            .and_then(|html| html.focus.as_deref())
            .is_some_and(|focus| focus == tag && !self.stack.iter().any(|f| f.focus));
        if focus {
            // The reading matter begins: shown, unless hidden otherwise.
            self.hidden = self.hidden.saturating_sub(1);
        }
        let frame_tag = fallible::string(&tag)?;
        self.stack.try_push(Frame {
            link,
            focus,
            children: Vec::new(),
            path,
            nodes: 0,
            texts: 0,
            block: is_block(&tag)
                || matches!(tag.as_str(), "li" | "pre" | "td" | "th" | "thead" | "tbody"),
            tag: frame_tag,
            style: styled(style, &props),
            align: props.align.unwrap_or(align),
            dir,
            lang,
            indent,
            size,
            margins,
            line_height,
            numbered: lists::counter(&tag, &attrs)?,
            list_style: lists::list_style(&tag, &props, self.stack.last()),
            after: (props.margin_bottom, props.padding_bottom),
            hides,
            closer: controls.map(|(_, close)| close),
        })?;
        if hides {
            self.hidden += 1;
        }
        if let Some((open, _)) = controls {
            self.open_inline(open)?;
        }
        // Images, also inside <svg> (a common way to show covers).
        if matches!(tag.as_str(), "img" | "image")
            && self.html.as_ref().is_none_or(|html| html.pictures)
            && self.only_svg_hides()
        {
            let href = attrs
                .iter()
                .find(|(name, _)| matches!(local_name(name), "src" | "href"))
                .map(|(_, value)| try_decode(value))
                .transpose()
                .map_err(|_| fallible::OOM)?;
            if let Some(href) = href.filter(|h| !h.is_empty() && !h.starts_with("data:")) {
                let alt = attrs
                    .try_get("alt")
                    .map_err(|_| fallible::OOM)?
                    .unwrap_or_default();
                self.image(&href, &alt)?;
            }
            return Ok(());
        }
        if self.hidden > 0 {
            return Ok(());
        }
        match tag.as_str() {
            "br" => {
                if !self.text.is_empty() {
                    self.push('\n')?;
                }
                self.space = false;
            }
            "hr" => {
                self.align = Some(Align::Center);
                self.push_str("* * *")?;
                self.flush()?;
            }
            "li" => self.list_item(&attrs)?,
            "td" | "th" => {
                // A row's cells on its line, set apart.
                if !self.text.is_empty() {
                    self.push_str(" · ")?;
                }
                self.space = false;
                self.space_origin = None;
            }
            "pre" => {
                self.kind = BlockKind::Preformatted;
                self.pre += 1;
            }
            tag if is_block(tag) => {
                if let Some(level) = heading_level(tag) {
                    self.kind = BlockKind::Heading(level);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn end(&mut self, name: &str) -> Result<(), DocumentError> {
        self.end_node();
        let tag = element_name(name)?;
        let tag = match self.dialect.as_mut() {
            Some(dialect) => dialect.leave(&tag)?,
            None => tag,
        };
        if self.overflow > 0 {
            // Ends the innermost of the elements not kept.
            self.overflow -= 1;
            return Ok(());
        }
        self.close(&tag)
    }

    /// Closes the innermost open element `tag`, and any left open inside
    /// it.
    fn close(&mut self, tag: &str) -> Result<(), DocumentError> {
        let Some(open) = self.stack.iter().rposition(|f| f.tag == tag) else {
            return Ok(());
        };
        self.marker = self.marker.take().filter(|_| tag != "li");
        if self.hidden == 0 {
            if tag == "pre" {
                self.flush()?;
                self.pre = self.pre.saturating_sub(1);
            } else if tag == "li" || is_block(tag) {
                self.flush()?;
            }
        }
        self.close_inline(open)?;
        if self.drop_cap.1 == Some(open) && !is_capital(&self.text) {
            self.drop_cap = (0, None);
        }
        if let Some(frame) = self.stack.get(open) {
            self.path.truncate(frame.path);
        }
        if self.body.is_some_and(|body| body >= open) {
            self.body = None;
        }
        let (mut margin, mut padding) = (None, None);
        let here = self.position();
        for frame in self.stack.drain(open..) {
            if frame.hides {
                self.hidden = self.hidden.saturating_sub(1);
            }
            if frame.focus {
                self.hidden += 1;
            }
            if let Some((start, href)) = frame.link
                && here > start
                && !self.full
            {
                fallible::try_write!(&mut self.links, "{start}\t{here}\t{href}\n")?;
            }
            margin = larger(margin, frame.after.0);
            padding = css::sum(padding, frame.after.1);
        }
        // Written as they come: a chapter of notes has many links and few
        // ids, and its links once filled tens of KB before an id came.
        if self.links.len() >= FLUSH_SMALL {
            self.write_links();
        }
        if self.text.is_empty() {
            self.margin = larger(self.margin, margin);
            self.padding = css::sum(self.padding, padding);
        }
        Ok(())
    }

    /// Text of the chapter: with entities to decode, or as written (CDATA).
    fn characters(&mut self, text: &str, cdata: bool) -> Result<(), DocumentError> {
        if self.full {
            return Ok(());
        }
        if self.stack.last().is_some_and(|f| f.tag == "style") {
            if self.styles {
                self.css.feed(text, &mut self.rules);
            }
            return Ok(());
        }
        if self.hidden > 0 {
            return Ok(());
        }
        if self.node.is_none() {
            self.start_node()?;
        }
        let text = if cdata {
            alloc::borrow::Cow::Borrowed(text)
        } else {
            try_decode(text).map_err(|_| fallible::OOM)?
        };
        for c in text.chars() {
            let origin = self.node_char(c);
            if self.pre > 0 {
                match c {
                    '\r' => {}
                    '\t' => {
                        self.origin = origin;
                        self.push(' ')?;
                    }
                    c => {
                        self.origin = origin;
                        self.push(c)?;
                    }
                }
            } else if c == '\u{ad}' {
                // Soft hyphens: dropped until hyphenation uses them.
            } else if c.is_whitespace() && c != '\u{a0}' {
                if !self.space {
                    self.space_origin = origin;
                }
                self.space = true;
                self.space_style = self.frame().0;
            } else {
                if self.space && !self.text.is_empty() && !self.text.ends_with(['\n', ' ']) {
                    self.origin = self.space_origin.take();
                    self.push_styled(' ', self.space_style)?;
                }
                self.space = false;
                self.origin = origin;
                self.push(c)?;
            }
            if self.text.len() >= LIMIT
                || self.origins.len() >= MAX_ORIGINS
                || self.origin_bytes >= MAX_ORIGIN_BYTES
            {
                self.split()?;
            }
        }
        Ok(())
    }

    /// A text node begins: counted as crengine counts it (see
    /// [`Converter::end_node`]).
    fn start_node(&mut self) -> Result<(), DocumentError> {
        if self.stack.is_empty() {
            return Ok(());
        }
        if self.node_paths.list.len() >= self.node_paths.compact_at.max(PATHS_MIN) {
            self.compact_paths()?;
        }
        let Some(parent) = self.stack.last_mut() else {
            return Ok(());
        };
        parent.texts += 1;
        let mut path = fallible::string_with(self.path.len() + 24)?;
        path.try_push_str(&self.path)?;
        if !path.is_empty() {
            path.try_push('/')?;
        }
        fallible::try_write!(&mut path, "text()[{}]", parent.texts)?;
        let first = parent.nodes == 0;
        parent.nodes += 1;
        let path = self.node_paths.add(&path)?;
        self.node = Some(TextNode {
            path,
            chars: 0,
            space: false,
            blank: true,
            first,
        });
        Ok(())
    }

    /// Drops the paths nothing names any more, and numbers the rest again.
    /// The next look comes when as many again are kept (at least
    /// [`PATHS_MIN`]).
    fn compact_paths(&mut self) -> Result<(), DocumentError> {
        let mut used: Vec<NodeId> = Vec::new();
        fallible::reserve(&mut used, self.origins.len() + 3)?;
        used.extend(self.node.iter().map(|node| node.path));
        used.extend(self.origin.iter().map(|(node, _)| *node));
        used.extend(self.space_origin.iter().map(|(node, _)| *node));
        used.extend(self.origins.iter().map(|origin| origin.node));
        paperpaw_sdk::sort::sort_unstable(&mut used);
        used.dedup();
        let mut kept = Vec::new();
        fallible::reserve(&mut kept, used.len())?;
        for id in &used {
            if let Some(path) = self.node_paths.list.get_mut(*id as usize) {
                kept.push(core::mem::take(path));
            }
        }
        let renumber = |id: &mut NodeId| {
            if let Ok(at) = used.binary_search(id) {
                *id = at as NodeId;
            }
        };
        self.node
            .iter_mut()
            .for_each(|node| renumber(&mut node.path));
        self.origin.iter_mut().for_each(|(node, _)| renumber(node));
        self.space_origin
            .iter_mut()
            .for_each(|(node, _)| renumber(node));
        self.origins
            .iter_mut()
            .for_each(|origin| renumber(&mut origin.node));
        self.node_paths.compact_at = 2 * kept.len();
        self.node_paths.list = kept;
        Ok(())
    }

    /// The text node ends (an element begins or ends): white space alone
    /// as the first child of a block is none for crengine.
    fn end_node(&mut self) {
        let Some(node) = self.node.take() else {
            return;
        };
        if node.blank
            && node.first
            && self.pre == 0
            && let Some(parent) = self.stack.last_mut()
            && parent.block
        {
            parent.texts -= 1;
            parent.nodes -= 1;
        }
    }

    /// Counts `c` in the text node as crengine keeps it: tabs and line
    /// breaks become spaces, and spaces that follow each other one.
    /// Where `c` is in it.
    fn node_char(&mut self, c: char) -> Option<(NodeId, u32)> {
        let pre = self.pre > 0;
        let node = self.node.as_mut()?;
        let char = if pre {
            if c == '\r' {
                return None;
            }
            node.chars += 1;
            node.chars - 1
        } else if matches!(c, ' ' | '\t' | '\r' | '\n') {
            if !node.space {
                node.chars += 1;
                node.space = true;
            }
            node.chars - 1
        } else {
            node.chars += 1;
            node.space = false;
            node.chars - 1
        };
        if !matches!(c, ' ' | '\t' | '\r' | '\n') {
            node.blank = false;
        }
        Some((node.path, char))
    }

    /// Emits the first part of an overlong block at a space.
    fn split(&mut self) -> Result<(), DocumentError> {
        // The last character boundary within the limit.
        let mut limit = LIMIT.min(self.text.len());
        while !self.text.is_char_boundary(limit) {
            limit -= 1;
        }
        let cut = self.text[..limit]
            .rfind([' ', '\n'])
            .filter(|&i| i > 0)
            .unwrap_or(limit);
        let kind = self.kind;
        self.write_record(kind, cut, true)?;
        // Keep the rest in place, so the buffer is never reallocated.
        let rest = self.text.len() - self.text.trim_start().len();
        self.drop_front(rest);
        self.continues = true;
        Ok(())
    }

    /// Ends the current block.
    fn flush(&mut self) -> Result<(), DocumentError> {
        let trimmed = self.text.trim_end_matches(direction::is_blank).len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() {
            let kind = self.kind;
            self.write_record(kind, self.text.len(), false)?;
        }
        self.text.clear();
        self.runs.clear();
        self.origins.clear();
        self.origin_bytes = 0;
        self.since_origin = 0;
        self.align = None;
        self.dir = None;
        self.lang = None;
        self.inline_langs.clear();
        self.drop_cap = (0, None);
        self.indent = None;
        self.size = None;
        self.margins = (0, 0);
        self.line_height = None;
        self.kind = BlockKind::Paragraph;
        self.continues = false;
        self.space = false;
        Ok(())
    }

    /// The direction of the current block, its first `len` bytes written:
    /// one that goes `Auto` takes its first piece's direction on into the
    /// next pieces of its paragraph.
    fn block_dir(&mut self, len: usize) -> Direction {
        let dir = match self.dir.unwrap_or(self.book.dir) {
            Direction::Auto => {
                let text = self.text.get(..len).unwrap_or_default();
                match paperpaw_sdk::text::bidi::paragraph_level(text).is_some_and(|l| l.is_rtl()) {
                    true => Direction::Rtl,
                    false => Direction::Ltr,
                }
            }
            dir => dir,
        };
        self.dir = Some(dir);
        dir
    }

    /// One entry of the index, in the room asked for before.
    fn push_index_entry(&mut self, indent: i16, space: u16, lang: Lang, dir: Direction) {
        self.index
            .extend_from_slice(&(self.length + HEADER).to_le_bytes());
        self.index.extend_from_slice(&indent.to_le_bytes());
        self.index.extend_from_slice(&space.to_le_bytes());
        let size = self.size.filter(|&s| s != 100).unwrap_or(0);
        self.index.extend_from_slice(&size.to_le_bytes());
        self.index.extend_from_slice(&self.margins.0.to_le_bytes());
        self.index.extend_from_slice(&self.margins.1.to_le_bytes());
        let line_height = self.line_height.unwrap_or(0);
        self.index.extend_from_slice(&line_height.to_le_bytes());
        self.index.extend_from_slice(&lang.to_bits().to_le_bytes());
        let langs = self.inline_langs.len() as u8;
        self.index
            .extend_from_slice(&[direction::dir_bits(dir), langs, self.drop_cap.0, 0]);
    }

    /// Writes the first `len` bytes of the current text as a record and
    /// removes them from the text.
    fn write_record(
        &mut self,
        kind: BlockKind,
        len: usize,
        continued: bool,
    ) -> Result<(), DocumentError> {
        // Records are whole or left out, so what is written stays readable
        // when the chapter's budget runs out.
        let pending = self.out.len()
            + self.index.len()
            + self.anchors.len()
            + self.links.len()
            + self.positions.len()
            + self.xpointers.len();
        if self.full || self.written.get() + pending as u64 >= self.budget {
            self.full = true;
            self.drop_front(len);
            return Ok(());
        }
        let (kind, level) = encode_kind(kind);
        let flags =
            (u8::from(self.continues) * FLAG_CONTINUES) | (u8::from(continued) * FLAG_CONTINUED);
        fallible::try_write!(
            &mut self.positions,
            "{}\t{}\n",
            self.length + HEADER,
            self.block_path
        )?;
        if self.positions.len() >= FLUSH_SMALL {
            self.write_positions();
        }
        // Written as they come: a block of many origins (e.g. a list of
        // links) would grow the buffer far past its size otherwise.
        let origins = self
            .origins
            .iter()
            .take_while(|o| (o.at as usize) < len)
            .count();
        for i in 0..origins {
            let origin = &self.origins[i];
            fallible::try_write!(
                &mut self.xpointers,
                "{}\t{}\t{}\n",
                self.length + HEADER + origin.at,
                self.node_paths.get(origin.node),
                origin.char
            )?;
            if self.xpointers.len() >= FLUSH_SMALL {
                self.write_xpointers();
            }
        }
        let dir = self.block_dir(len);
        let lang = self.lang.unwrap_or(self.book.lang);
        let indent = self.indent.unwrap_or(NO_INDENT);
        let space = if self.continues {
            NO_SPACE
        } else {
            let (margin, padding) = (self.margin.take(), self.padding.take());
            css::sum(margin, padding).unwrap_or(NO_SPACE)
        };
        // One entry of the index, and the record's room in `out`, asked for
        // before anything of them is written: a refusal leaves both whole.
        fallible::reserve(&mut self.index, INDEX as usize)?;
        fallible::reserve(
            &mut self.out,
            usize::from(HEADER as u8)
                + len.min(FLUSH)
                + (self.runs.len() + self.inline_langs.len()) * RUN,
        )?;
        self.push_index_entry(indent, space, lang, dir);
        if self.index.len() >= FLUSH_SMALL {
            self.write_index();
        }
        let mut runs: Vec<Run> = Vec::new();
        fallible::reserve(&mut runs, self.runs.len())?;
        runs.extend(
            self.runs
                .iter()
                .filter(|run| (run.start as usize) < len)
                .map(|run| Run::new(run.start, run.end.min(len as u32), run.style)),
        );
        let align = self.align.unwrap_or_default();
        self.out
            .extend_from_slice(&[kind, level, flags, align.bits()]);
        self.out.extend_from_slice(&(len as u16).to_le_bytes());
        self.out
            .extend_from_slice(&(runs.len() as u16).to_le_bytes());
        if self.out.len() + len > FLUSH {
            // Long blocks go straight to storage instead of growing `out`.
            self.write_out();
            let failed = self.append(&self.paths.text, &self.text.as_bytes()[..len]);
            self.failed = self.failed.take().or(failed);
        } else {
            self.out.extend_from_slice(&self.text.as_bytes()[..len]);
        }
        for run in &runs {
            self.out
                .extend_from_slice(&(run.start as u16).to_le_bytes());
            self.out.extend_from_slice(&(run.end as u16).to_le_bytes());
            self.out.extend_from_slice(&run.style.bits().to_le_bytes());
        }
        for lang in &self.inline_langs {
            self.out.extend_from_slice(&lang.to_bits().to_le_bytes());
        }
        self.length +=
            HEADER + len as u32 + (runs.len() * RUN) as u32 + (self.inline_langs.len() as u32) * 4;
        self.drop_front(len);
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Option<DocumentError> {
        if data.is_empty() || self.failed.is_some() {
            return None;
        }
        self.written.set(self.written.get() + data.len() as u64);
        self.storage
            .append(path, data)
            .err()
            .map(DocumentError::Storage)
    }

    fn write_out(&mut self) {
        let failed = self.append(&self.paths.text, &self.out);
        self.failed = self.failed.take().or(failed);
        self.out.clear();
    }

    /// Notes that anchor `id` is here, written as they come (a book of
    /// many sections would gather them all otherwise). An id with a line
    /// break or tab, which no link can name, is left out.
    fn add_anchor(&mut self, id: &str) -> Result<(), DocumentError> {
        if self.full || id.chars().any(char::is_control) {
            return Ok(());
        }
        let at = self.here();
        fallible::try_write!(&mut self.anchors, "{id}\t{at}\n")?;
        if self.anchors.len() >= FLUSH_SMALL {
            self.write_anchors();
        }
        Ok(())
    }

    fn write_index(&mut self) {
        let failed = self.append(&self.paths.index, &self.index);
        self.failed = self.failed.take().or(failed);
        self.index.clear();
    }

    fn write_positions(&mut self) {
        let failed = self.append(&self.paths.positions, self.positions.as_bytes());
        self.failed = self.failed.take().or(failed);
        self.positions.clear();
    }

    fn write_xpointers(&mut self) {
        let failed = self.append(&self.paths.xpointers, self.xpointers.as_bytes());
        self.failed = self.failed.take().or(failed);
        self.xpointers.clear();
    }

    fn write_anchors(&mut self) {
        let failed = self.append(&self.paths.anchors, self.anchors.as_bytes());
        self.failed = self.failed.take().or(failed);
        self.anchors.clear();
    }

    fn write_links(&mut self) {
        let failed = self.append(&self.paths.links, self.links.as_bytes());
        self.failed = self.failed.take().or(failed);
        self.links.clear();
    }

    fn write_pictures(&mut self) {
        let failed = self.append(&self.paths.pictures, self.pictures.as_bytes());
        self.failed = self.failed.take().or(failed);
        self.pictures.clear();
    }
}

/// An element's name as kept: without prefix, in lower case, at most
/// [`MAX_NAME`] bytes.
fn element_name(name: &str) -> Result<String, DocumentError> {
    let name = local_name(name);
    let mut end = name.len().min(MAX_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    fallible::lowercase(&name[..end])
}

/// The most bytes written for a chapter of `size` bytes: what it says
/// grows a few times when its places are listed (`<n>.pos`, `<n>.xpt`),
/// never by far more, and never past 64 MiB.
fn budget(size: u64) -> u64 {
    size.saturating_mul(16)
        .saturating_add(1 << 20)
        .min(64 << 20)
}

/// Converts a chapter; returns the length of its text file.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "what a chapter is converted with, each of it different; a struct would be built for one call"
)]
pub(crate) fn convert(
    storage: &dyn Storage,
    source: &Source<'_>,
    paths: &CachePaths,
    rules: Rules,
    styles: bool,
    color: bool,
    fit: Fit,
    book: BookText,
) -> Result<u32, DocumentError> {
    let name = match source {
        Source::Zip { entry, .. } => entry.name.as_str(),
        Source::Html { path, .. } | Source::Fb2 { path, .. } => path,
    };
    let base = fallible::string(name.rfind('/').map_or("", |i| &name[..=i]))?;
    let html = match source {
        Source::Zip { .. } | Source::Fb2 { .. } => None,
        Source::Html {
            focus,
            base,
            pictures,
            ..
        } => Some(Html {
            focus: focus.map(fallible::string).transpose()?,
            base: base.map(fallible::string).transpose()?,
            pictures: *pictures,
        }),
    };
    let dialect: Option<crate::fb2::Fb2Dialect> = match source {
        Source::Fb2 { .. } => Some(crate::fb2::Fb2Dialect::default()),
        _ => None,
    };
    let size = match source {
        Source::Zip { entry, .. } => u64::from(entry.size),
        Source::Html { path, .. } | Source::Fb2 { path, .. } => storage.size(path)?,
    };
    paths.begin(storage)?;
    // The buffers that take most, asked for so that a heap too short for
    // them is an error, not a stop.
    let oom = |_| DocumentError::OutOfMemory;
    let mut out = Vec::new();
    out.try_reserve_exact(FLUSH + HEADER as usize)
        .map_err(oom)?;
    let mut text = String::new();
    text.try_reserve_exact(LIMIT + 4).map_err(oom)?;
    let mut index_buffer = Vec::new();
    index_buffer
        .try_reserve_exact(FLUSH_SMALL + 4)
        .map_err(oom)?;
    let mut converter = Converter {
        storage,
        paths,
        length: 0,
        out,
        index: index_buffer,
        anchors: String::new(),
        links: String::new(),
        chapter: fallible::string(name)?,
        positions: String::new(),
        xpointers: String::new(),
        node: None,
        origins: Vec::new(),
        origin_bytes: 0,
        since_origin: 0,
        origin: None,
        space_origin: None,
        node_paths: NodePaths::default(),
        path: String::new(),
        body: None,
        block_path: String::new(),
        // Blocks are split before they outgrow this, so it never grows.
        text,
        runs: Vec::new(),
        align: None,
        book,
        dir: None,
        lang: None,
        inline_langs: Vec::new(),
        drop_cap: (0, None),
        marker: None,
        indent: None,
        size: None,
        margins: (0, 0),
        line_height: None,
        margin: None,
        padding: None,
        stack: Vec::new(),
        overflow: 0,
        rules,
        styles,
        css: css::Parser::new(&base)?,
        base,
        pictures: String::new(),
        seen: Vec::new(),
        written: core::cell::Cell::new(0),
        budget: budget(size),
        full: false,
        kind: BlockKind::Paragraph,
        continues: false,
        // A page with reading matter shows nothing outside it.
        hidden: u32::from(html.as_ref().is_some_and(|h| h.focus.is_some())),
        html,
        dialect,
        pre: 0,
        space: false,
        space_style: TextStyle::REGULAR,
        failed: None,
    };
    let mut tokenizer = match source {
        Source::Zip { .. } | Source::Fb2 { .. } => Tokenizer::new(),
        Source::Html { .. } => Tokenizer::html(),
    };
    let full = core::cell::Cell::new(false);
    let mut handle = |token: Token<'_>| {
        // A refusal for room stops the reading, and ends it as such.
        let result = match token {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => converter.start(name, attrs).and_then(|()| {
                // Empty elements, also as HTML writes them (`<br>`).
                if self_closing || is_void(local_name(name)) {
                    converter.end(name)
                } else {
                    Ok(())
                }
            }),
            Token::End { name } => converter.end(name),
            Token::Text(text) => converter.characters(&text, false),
            Token::CData(text) => converter.characters(text, true),
        };
        if let Err(error) = result {
            converter.failed.get_or_insert(error);
            converter.full = true;
        }
        full.set(converter.full);
    };
    match source {
        // An archive entry is no longer inflated once the budget is spent.
        Source::Zip { archive, entry } => {
            let mut out_of_memory = false;
            archive
                .stream(storage, entry, &mut |chunk| {
                    out_of_memory = tokenizer.try_feed(chunk, &mut handle).is_err();
                    !full.get() && !out_of_memory
                })
                .map_err(error)?;
            if out_of_memory {
                return Err(DocumentError::OutOfMemory);
            }
        }
        Source::Html { path, .. } => {
            feed_html(storage, path, &full, &mut tokenizer, &mut handle)?;
        }
        Source::Fb2 { path, encoding } => {
            crate::fb2::feed(storage, path, *encoding, &mut tokenizer, &mut handle)?;
        }
    }
    tokenizer.finish(&mut handle);
    if let Err(error) = converter.flush() {
        converter.failed.get_or_insert(error);
    }
    converter.write_out();
    converter.write_index();
    converter.write_anchors();
    converter.write_links();
    converter.write_positions();
    converter.write_xpointers();
    converter.write_pictures();
    if let Some(e) = converter.failed {
        return Err(e);
    }
    // Rules of the chapter's own `<style>` dropped: kept so, it would
    // look otherwise for good.
    if converter.rules.out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    let length = converter.length;
    drop(converter);
    let complete = take_pictures(storage, source, paths, color, fit)?;
    let _ = storage.remove(&paths.pictures);
    // Last: its presence marks a complete conversion. Not written where a
    // picture found no room to be decoded: that picture is its alternative
    // text for now, and the next opening converts the chapter again.
    if complete {
        storage.write(&paths.end, &length.to_le_bytes())?;
    }
    Ok(length)
}

/// Feeds the HTML file at `path` to the tokenizer, a piece at a time,
/// until `full` says the budget is spent.
// Its own frame, as a FictionBook's (`fb2::feed`): the piece read is not
// in `convert`'s, which stays under the firmware's frame limit.
#[inline(never)]
fn feed_html(
    storage: &dyn Storage,
    path: &str,
    full: &core::cell::Cell<bool>,
    tokenizer: &mut Tokenizer,
    emit: &mut dyn FnMut(Token<'_>),
) -> Result<(), DocumentError> {
    let mut chunk = [0u8; 1024];
    let mut at = 0u64;
    while !full.get() {
        let n = storage.read_at(path, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        tokenizer
            .try_feed(&chunk[..n], emit)
            .map_err(|_| DocumentError::OutOfMemory)?;
        at += n as u64;
    }
    Ok(())
}

/// Pictures noted or decoded together, so the list of noted ones (or a
/// FictionBook) is read once for each so many.
const PICTURE_BATCH: usize = 32;

/// Takes in the pictures listed in `<n>.pic`, after the text, so only one
/// archive entry is inflated at a time. An EPUB's are decoded when a page
/// shows them; their sizes now, from their first bytes, for the pages to
/// be laid out (a big picture takes seconds to decode on small devices).
/// Those whose size the first bytes do not say are decoded now, as are
/// the pictures beside a file made from Markdown and a FictionBook's. One
/// that cannot be decoded is shown as its alternative text. So is one
/// that found no room to be decoded, but then the answer is `false`: the
/// conversion is not complete, to be made again when there is room.
fn take_pictures(
    storage: &dyn Storage,
    source: &Source<'_>,
    paths: &CachePaths,
    color: bool,
    fit: Fit,
) -> Result<bool, DocumentError> {
    let short = core::cell::Cell::new(false);
    let decoded = |result: Result<(), DocumentError>| {
        if matches!(result, Err(DocumentError::OutOfMemory)) {
            short.set(true);
        }
    };
    let mut notes: Vec<crate::pictures::Note> = Vec::new();
    let mut wanted: Vec<(String, String, Fit)> = Vec::new();
    let mut refused = None;
    let decode_binaries = |wanted: &mut Vec<(String, String, Fit)>| {
        let result = match source {
            Source::Fb2 { path, encoding } => {
                crate::fb2::decode_binaries(storage, path, *encoding, wanted, &paths.dir, color)
            }
            _ => Ok(()),
        };
        wanted.clear();
        decoded(result);
    };
    let mut each = |path: &str| -> Result<(), DocumentError> {
        let base = image_path(&paths.dir, path)?;
        if path.is_empty() || crate::pictures::has(storage, &base, color)? {
            return Ok(());
        }
        match source {
            Source::Zip { archive, .. } => match image_size(storage, archive, path) {
                Some((width, height)) if width > 0 && height > 0 => {
                    notes.try_push(crate::pictures::Note {
                        base,
                        size: fit.size(width, height),
                        source: fallible::string(path)?,
                    })?;
                    if notes.len() >= PICTURE_BATCH {
                        crate::pictures::try_note_all(storage, &mut notes)?;
                    }
                }
                _ => {
                    let file = crate::pictures::try_file(&base, color)?;
                    decoded(decode_image(
                        storage, archive, path, &file, &paths.dir, fit, color,
                    ));
                }
            },
            Source::Html { pictures, .. } => {
                if let (true, Ok(size)) = (*pictures, storage.size(path)) {
                    let file = crate::pictures::try_file(&base, color)?;
                    decoded(decode_range(storage, path, 0, size, &file, fit, color));
                }
            }
            Source::Fb2 { .. } => {
                wanted.try_push((
                    fallible::string(path)?,
                    crate::pictures::try_file(&base, color)?,
                    fit,
                ))?;
                if wanted.len() >= PICTURE_BATCH {
                    decode_binaries(&mut wanted);
                }
            }
        }
        Ok(())
    };
    let read = crate::cached::lines(storage, &paths.pictures, |path| match each(path) {
        Ok(()) => true,
        Err(error) => {
            refused = Some(error);
            false
        }
    });
    if let Some(error) = refused {
        return Err(error);
    }
    read?;
    crate::pictures::try_note_all(storage, &mut notes)?;
    if !wanted.is_empty() {
        decode_binaries(&mut wanted);
    }
    Ok(!short.get())
}

/// Alternative text with its whitespace collapsed, cut to at most `max`
/// bytes where a character ends, written onto `out`.
fn clean_alt(alt: &str, max: usize, out: &mut String) -> Result<(), DocumentError> {
    let base = out.len();
    for word in alt.split_whitespace() {
        if out.len() > base {
            out.try_push(' ')?;
        }
        out.try_push_str(word)?;
        if out.len() - base >= max {
            break;
        }
    }
    let mut end = base + (out.len() - base).min(max);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(base + out[base..end].trim_end().len());
    Ok(())
}

#[cfg(test)]
mod tests;
