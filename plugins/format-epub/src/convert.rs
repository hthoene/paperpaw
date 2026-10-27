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
//!   in percent of its text size (0 if it sets none). Kept apart from the
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

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Align, BlockKind, DocumentError, Run, TextStyle};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::xml::{Token, Tokenizer, decode, local_name};
use paperpaw_sdk::zip::{Entry, ZipArchive};

use crate::css::{self, Props, Rules};

mod elements;
use crate::package::error;
use elements::{element_props, furniture, heading_level, is_block, is_void, styled};

/// Size of a record header.
pub(crate) const HEADER: u32 = 8;
/// Bytes per record in `<n>.idx`.
pub(crate) const INDEX: u32 = 16;
/// A record without the book's indent or space.
pub(crate) const NO_INDENT: i16 = i16::MIN;
pub(crate) const NO_SPACE: u16 = u16::MAX;
/// Size of a style run after a record's text.
pub(crate) const RUN: usize = 6;
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
    fn enter(&mut self, tag: &str, attrs: &paperpaw_sdk::xml::Attrs<'_>) -> Entered;
    /// Element `tag` left: the HTML element it was read as.
    fn leave(&mut self, tag: &str) -> String;
    /// Where the picture `href` points is kept as, if not as a path
    /// relative to the chapter.
    fn picture(&self, href: &str) -> Option<String>;
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
pub(crate) fn image_path(dir: &str, archive_path: &str) -> String {
    let hash = paperpaw_sdk::digest::fnv1a64(archive_path);
    alloc::format!("{dir}/img-{hash:016x}")
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

/// An open element: the style and alignment its content inherits.
struct Frame {
    tag: String,
    style: TextStyle,
    align: Align,
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
    /// The items of a numbered list (`<ol>`) so far.
    numbered: Option<u32>,
    /// The margin and padding below the element, taken over by the next
    /// block.
    after: (Option<u16>, Option<u16>),
    /// The element's content is not shown (`<head>`, `display: none`, …).
    hides: bool,
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

/// The text node being read, counted as crengine keeps it.
struct TextNode {
    /// Below `<body>`, e.g. `div[2]/p[3]/text()[1]`.
    path: Rc<str>,
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
    node: Rc<str>,
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
    dialect: Option<Box<dyn Dialect>>,
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
    origin: Option<(Rc<str>, u32)>,
    /// Where the pending space comes from.
    space_origin: Option<(Rc<str>, u32)>,
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
    fn push(&mut self, c: char) {
        let (style, _) = self.frame();
        self.push_styled(c, style);
    }

    /// Appends text in `style`.
    fn push_styled(&mut self, c: char, style: TextStyle) {
        let (_, align) = self.frame();
        if self.text.is_empty() {
            self.align.get_or_insert(align);
            self.indent = self.stack.last().and_then(|f| f.indent);
            self.size = self.stack.last().and_then(|f| f.size);
            self.margins = self.stack.last().map_or((0, 0), |f| f.margins);
            self.line_height = self.stack.last().and_then(|f| f.line_height);
            self.block_path.clone_from(&self.path);
        }
        let start = self.text.len() as u32;
        if let Some((node, char)) = self.origin.take() {
            let follows = self
                .origins
                .last()
                .is_some_and(|o| o.node == node && o.char + self.since_origin == char);
            if !follows {
                if self
                    .origins
                    .last()
                    .is_none_or(|o| !Rc::ptr_eq(&o.node, &node))
                {
                    self.origin_bytes += node.len();
                }
                self.origins.push(Origin {
                    at: start,
                    node,
                    char,
                });
                self.since_origin = 0;
            }
        }
        self.since_origin += 1;
        self.text.push(c);
        let end = self.text.len() as u32;
        if style == TextStyle::REGULAR {
            return;
        }
        match self.runs.last_mut() {
            Some(run) if run.end == start && run.style == style => run.end = end,
            _ => self.runs.push(Run { start, end, style }),
        }
    }

    fn push_str(&mut self, text: &str) {
        for c in text.chars() {
            self.push(c);
        }
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
    fn image(&mut self, href: &str, alt: &str) {
        let path = match self.dialect.as_ref().and_then(|d| d.picture(href)) {
            Some(path) => path,
            None => crate::package::resolve(&self.base, href).0,
        };
        let cache = image_path(&self.paths.dir, &path);
        self.flush();
        self.kind = BlockKind::Image;
        self.text.push_str(&cache);
        self.text.push('\t');
        // As much of the alternative text as the record has room for.
        let room = LIMIT.saturating_sub(self.text.len());
        self.text.push_str(&clean_alt(alt, room));
        self.write_record(BlockKind::Image, self.text.len());
        self.text.clear();
        self.runs.clear();
        self.origins.clear();
        self.origin_bytes = 0;
        self.kind = BlockKind::Paragraph;
        // Listed a line each: a path with a line break (`%0A`) names no
        // picture of the book, and is shown as its alternative text.
        if self.full || path.chars().any(char::is_control) {
            return;
        }
        let hash = paperpaw_sdk::digest::fnv1a64(&path);
        match self.seen.binary_search(&hash) {
            Ok(_) => return,
            Err(at) if self.seen.len() < MAX_SEEN_PICTURES => self.seen.insert(at, hash),
            Err(_) => {}
        }
        self.pictures.push_str(&path);
        self.pictures.push('\n');
        if self.pictures.len() >= FLUSH_SMALL {
            self.write_pictures();
        }
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
                node: last.node.clone(),
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
                    .is_none_or(|before| !Rc::ptr_eq(&before.node, &o.node))
            })
            .map(|(_, o)| o.node.len())
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
    fn target(&self, href: &str) -> Option<String> {
        let href: String = href.trim().chars().filter(|c| !c.is_control()).collect();
        if href.is_empty() || paperpaw_sdk::document::is_external(&href) {
            return (!href.is_empty()).then_some(href);
        }
        if let Some(html) = &self.html {
            return Some(match (href.strip_prefix('#'), &html.base) {
                (Some(_), _) => href,
                (None, Some(base)) => paperpaw_sdk::web::join_url(base, &href),
                (None, None) => crate::package::resolve(&self.base, &href).0,
            });
        }
        let (path, fragment) = if let Some(fragment) = href.strip_prefix('#') {
            (self.chapter.clone(), Some(String::from(fragment)))
        } else {
            crate::package::resolve(&self.base, &href)
        };
        Some(match fragment {
            Some(fragment) => alloc::format!("{path}#{fragment}"),
            None => path,
        })
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn start(&mut self, name: &str, attrs: paperpaw_sdk::xml::Attrs<'_>) {
        if self.full {
            return;
        }
        self.end_node();
        let tag = element_name(name);
        let entered = self.dialect.as_mut().map(|d| d.enter(&tag, &attrs));
        let tag = match entered {
            Some(entered) => {
                if let Some(id) = entered.anchor {
                    self.flush();
                    self.add_anchor(&id);
                }
                if entered.line_break && !self.text.is_empty() {
                    self.push('\n');
                }
                entered.name
            }
            None => tag,
        };
        let props = if self.hidden == 0 {
            element_props(&tag, &attrs, &mut self.rules, self.styles)
        } else {
            Props::default()
        };
        let hides = matches!(tag.as_str(), "head" | "script" | "style" | "title" | "svg")
            || props.hidden == Some(true)
            || self
                .html
                .as_ref()
                .is_some_and(|html| furniture(&tag, &attrs, html.focus.is_some()));
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
        if closes && let Some(open) = open.map(String::from) {
            self.close(&open);
        }
        // Block elements end the block before them (before their anchor is
        // placed, so it points at the new block), in the old style.
        if !hides
            && self.hidden == 0
            && (is_block(&tag) || matches!(tag.as_str(), "li" | "pre" | "hr"))
        {
            self.flush();
        }
        if let Some(id) = attrs.get("id") {
            self.add_anchor(&id);
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
        // Blocks are set in by their own margins at the sides and those of
        // the blocks around them; text elements are not. The body's own are
        // the page's: the reader's margins, set in the settings, are those.
        let parent_margins = self.stack.last().map_or((0, 0), |f| f.margins);
        let margins = match is_block(&tag) || matches!(tag.as_str(), "li" | "pre") {
            true => {
                let (start, end) = match tag.as_str() {
                    "body" => (0, 0),
                    _ => props.sides(),
                };
                (
                    parent_margins.0.saturating_add(start).min(MAX_MARGIN),
                    parent_margins.1.saturating_add(end).min(MAX_MARGIN),
                )
            }
            false => parent_margins,
        };
        let line_height = props
            .line_height
            .or_else(|| self.stack.last().and_then(|f| f.line_height));
        if self.stack.len() >= MAX_DEPTH {
            // Read as part of the deepest element kept.
            self.overflow += 1;
            return;
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
                            parent.children.push((hash, 1));
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
            .map(|(_, value)| decode(value));
        let link = match href {
            Some(href)
                if tag == "a"
                    && !hides
                    && self.hidden == 0
                    && !self.stack.iter().any(|f| f.link.is_some()) =>
            {
                self.target(&href)
                    .filter(|target| target.len() <= MAX_HREF)
                    .map(|target| (self.here(), target))
            }
            _ => None,
        };
        // Its path, below `<body>`.
        let path = self.path.len();
        if self.body.is_some() {
            let segment = alloc::format!("{tag}[{index}]");
            if path + 1 + segment.len() <= MAX_PATH {
                if path > 0 {
                    self.path.push('/');
                }
                self.path.push_str(&segment);
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
        self.stack.push(Frame {
            link,
            focus,
            children: Vec::new(),
            path,
            nodes: 0,
            texts: 0,
            block: is_block(&tag)
                || matches!(tag.as_str(), "li" | "pre" | "td" | "th" | "thead" | "tbody"),
            tag: tag.clone(),
            style: styled(style, &props),
            align: props.align.unwrap_or(align),
            indent,
            size,
            margins,
            line_height,
            numbered: (tag == "ol").then(|| {
                attrs
                    .get("start")
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .map_or(0, |s| s.saturating_sub(1))
            }),
            after: (props.margin_bottom, props.padding_bottom),
            hides,
        });
        if hides {
            self.hidden += 1;
        }
        // Images, also inside <svg> (a common way to show covers).
        if matches!(tag.as_str(), "img" | "image")
            && self.html.as_ref().is_none_or(|html| html.pictures)
            && self.only_svg_hides()
        {
            let href = attrs
                .iter()
                .find(|(name, _)| matches!(local_name(name), "src" | "href"))
                .map(|(_, value)| decode(value).into_owned());
            if let Some(href) = href.filter(|h| !h.is_empty() && !h.starts_with("data:")) {
                let alt = attrs.get("alt").unwrap_or_default();
                self.image(&href, &alt);
            }
            return;
        }
        if self.hidden > 0 {
            return;
        }
        match tag.as_str() {
            "br" => {
                if !self.text.is_empty() {
                    self.push('\n');
                }
                self.space = false;
            }
            "hr" => {
                self.align = Some(Align::Center);
                self.push_str("* * *");
                self.flush();
            }
            "li" => {
                // Numbered in a numbered list, the nearest list's.
                let list = self
                    .stack
                    .iter_mut()
                    .rev()
                    .find(|f| matches!(f.tag.as_str(), "ol" | "ul"));
                match list.and_then(|f| f.numbered.as_mut()) {
                    Some(number) => {
                        *number = number.saturating_add(1);
                        let mark = alloc::format!("{number}. ");
                        self.push_str(&mark);
                    }
                    None => self.push_str("• "),
                }
            }
            "td" | "th" => {
                // A row's cells on its line, set apart.
                if !self.text.is_empty() {
                    self.push_str(" · ");
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
    }

    fn end(&mut self, name: &str) {
        self.end_node();
        let tag = element_name(name);
        let tag = match self.dialect.as_mut() {
            Some(dialect) => dialect.leave(&tag),
            None => tag,
        };
        if self.overflow > 0 {
            // Ends the innermost of the elements not kept.
            self.overflow -= 1;
            return;
        }
        self.close(&tag);
    }

    /// Closes the innermost open element `tag`, and any left open inside
    /// it.
    fn close(&mut self, tag: &str) {
        let Some(open) = self.stack.iter().rposition(|f| f.tag == tag) else {
            return;
        };
        if self.hidden == 0 {
            if tag == "pre" {
                self.flush();
                self.pre = self.pre.saturating_sub(1);
            } else if tag == "li" || is_block(tag) {
                self.flush();
            }
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
                self.links
                    .push_str(&alloc::format!("{start}\t{here}\t{href}\n"));
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
    }

    /// Text of the chapter: with entities to decode, or as written (CDATA).
    fn characters(&mut self, text: &str, cdata: bool) {
        if self.full {
            return;
        }
        if self.stack.last().is_some_and(|f| f.tag == "style") {
            if self.styles {
                self.css.feed(text, &mut self.rules);
            }
            return;
        }
        if self.hidden > 0 {
            return;
        }
        if self.node.is_none() {
            self.start_node();
        }
        let text = if cdata {
            alloc::borrow::Cow::Borrowed(text)
        } else {
            decode(text)
        };
        for c in text.chars() {
            let origin = self.node_char(c);
            if self.pre > 0 {
                match c {
                    '\r' => {}
                    '\t' => {
                        self.origin = origin;
                        self.push(' ');
                    }
                    c => {
                        self.origin = origin;
                        self.push(c);
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
                    self.push_styled(' ', self.space_style);
                }
                self.space = false;
                self.origin = origin;
                self.push(c);
            }
            if self.text.len() >= LIMIT
                || self.origins.len() >= MAX_ORIGINS
                || self.origin_bytes >= MAX_ORIGIN_BYTES
            {
                self.split();
            }
        }
    }

    /// A text node begins: counted as crengine counts it (see
    /// [`Converter::end_node`]).
    fn start_node(&mut self) {
        let mut path = self.path.clone();
        let Some(parent) = self.stack.last_mut() else {
            return;
        };
        parent.texts += 1;
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(&alloc::format!("text()[{}]", parent.texts));
        let first = parent.nodes == 0;
        parent.nodes += 1;
        self.node = Some(TextNode {
            path: Rc::from(path),
            chars: 0,
            space: false,
            blank: true,
            first,
        });
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
    fn node_char(&mut self, c: char) -> Option<(Rc<str>, u32)> {
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
        Some((node.path.clone(), char))
    }

    /// Emits the first part of an overlong block at a space.
    fn split(&mut self) {
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
        self.write_record(kind, cut);
        // Keep the rest in place, so the buffer is never reallocated.
        let rest = self.text.len() - self.text.trim_start().len();
        self.drop_front(rest);
        self.continues = true;
    }

    /// Ends the current block.
    fn flush(&mut self) {
        let trimmed = self.text.trim_end().len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() {
            let kind = self.kind;
            self.write_record(kind, self.text.len());
        }
        self.text.clear();
        self.runs.clear();
        self.origins.clear();
        self.origin_bytes = 0;
        self.since_origin = 0;
        self.align = None;
        self.indent = None;
        self.size = None;
        self.margins = (0, 0);
        self.line_height = None;
        self.kind = BlockKind::Paragraph;
        self.continues = false;
        self.space = false;
    }

    /// Writes the first `len` bytes of the current text as a record and
    /// removes them from the text.
    fn write_record(&mut self, kind: BlockKind, len: usize) {
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
            return;
        }
        let (kind, level) = encode_kind(kind);
        let flags = if self.continues { FLAG_CONTINUES } else { 0 };
        self.positions.push_str(&alloc::format!(
            "{}\t{}\n",
            self.length + HEADER,
            self.block_path
        ));
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
            let line = alloc::format!(
                "{}\t{}\t{}\n",
                self.length + HEADER + origin.at,
                origin.node,
                origin.char
            );
            self.xpointers.push_str(&line);
            if self.xpointers.len() >= FLUSH_SMALL {
                self.write_xpointers();
            }
        }
        let indent = self.indent.unwrap_or(NO_INDENT);
        let space = if self.continues {
            NO_SPACE
        } else {
            let (margin, padding) = (self.margin.take(), self.padding.take());
            css::sum(margin, padding).unwrap_or(NO_SPACE)
        };
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
        if self.index.len() >= FLUSH_SMALL {
            self.write_index();
        }
        let runs: Vec<Run> = self
            .runs
            .iter()
            .filter(|run| (run.start as usize) < len)
            .map(|run| Run {
                end: run.end.min(len as u32),
                ..*run
            })
            .collect();
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
        self.length += HEADER + len as u32 + (runs.len() * RUN) as u32;
        self.drop_front(len);
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
    fn add_anchor(&mut self, id: &str) {
        if self.full || id.chars().any(char::is_control) {
            return;
        }
        let at = self.here();
        self.anchors.push_str(&alloc::format!("{id}\t{at}\n"));
        if self.anchors.len() >= FLUSH_SMALL {
            self.write_anchors();
        }
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
fn element_name(name: &str) -> String {
    let mut name = local_name(name).to_ascii_lowercase();
    let mut end = name.len().min(MAX_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.truncate(end);
    name
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
pub(crate) fn convert(
    storage: &dyn Storage,
    source: &Source<'_>,
    paths: &CachePaths,
    rules: Rules,
    styles: bool,
    color: bool,
    fit: Fit,
) -> Result<u32, DocumentError> {
    let name = match source {
        Source::Zip { entry, .. } => entry.name.as_str(),
        Source::Html { path, .. } | Source::Fb2 { path, .. } => path,
    };
    let base = String::from(name.rfind('/').map_or("", |i| &name[..=i]));
    let html = match source {
        Source::Zip { .. } | Source::Fb2 { .. } => None,
        Source::Html {
            focus,
            base,
            pictures,
            ..
        } => Some(Html {
            focus: focus.map(String::from),
            base: base.map(String::from),
            pictures: *pictures,
        }),
    };
    let dialect: Option<Box<dyn Dialect>> = match source {
        Source::Fb2 { .. } => Some(Box::new(crate::fb2::Fb2Dialect::default())),
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
    let mut converter = Converter {
        storage,
        paths,
        length: 0,
        out,
        index: Vec::with_capacity(FLUSH_SMALL + 4),
        anchors: String::new(),
        links: String::new(),
        chapter: String::from(name),
        positions: String::new(),
        xpointers: String::new(),
        node: None,
        origins: Vec::new(),
        origin_bytes: 0,
        since_origin: 0,
        origin: None,
        space_origin: None,
        path: String::new(),
        body: None,
        block_path: String::new(),
        // Blocks are split before they outgrow this, so it never grows.
        text,
        runs: Vec::new(),
        align: None,
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
        css: css::Parser::new(&base),
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
        match token {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => {
                converter.start(name, attrs);
                // Empty elements, also as HTML writes them (`<br>`).
                if self_closing || is_void(local_name(name)) {
                    converter.end(name);
                }
            }
            Token::End { name } => converter.end(name),
            Token::Text(text) => converter.characters(&text, false),
            Token::CData(text) => converter.characters(text, true),
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
            let mut chunk = [0u8; 1024];
            let mut at = 0u64;
            while !full.get() {
                let n = storage.read_at(path, at, &mut chunk)?;
                if n == 0 {
                    break;
                }
                tokenizer
                    .try_feed(&chunk[..n], &mut handle)
                    .map_err(|_| DocumentError::OutOfMemory)?;
                at += n as u64;
            }
        }
        Source::Fb2 { path, encoding } => {
            crate::fb2::feed(storage, path, *encoding, &mut tokenizer, &mut handle)?;
        }
    }
    tokenizer.finish(&mut handle);
    converter.flush();
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
    take_pictures(storage, source, paths, color, fit);
    let _ = storage.remove(&paths.pictures);
    // Last: its presence marks a complete conversion.
    storage.write(&paths.end, &length.to_le_bytes())?;
    Ok(length)
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
/// that cannot be decoded is shown as its alternative text.
fn take_pictures(
    storage: &dyn Storage,
    source: &Source<'_>,
    paths: &CachePaths,
    color: bool,
    fit: Fit,
) {
    let mut notes: Vec<crate::pictures::Note> = Vec::new();
    let mut wanted: Vec<(String, String, Fit)> = Vec::new();
    let decode_binaries = |wanted: &mut Vec<(String, String, Fit)>| {
        if let Source::Fb2 { path, encoding } = source {
            let _ =
                crate::fb2::decode_binaries(storage, path, *encoding, wanted, &paths.dir, color);
        }
        wanted.clear();
    };
    crate::cached::lines(storage, &paths.pictures, |path| {
        let base = image_path(&paths.dir, path);
        if path.is_empty() || crate::pictures::has(storage, &base, color) {
            return true;
        }
        match source {
            Source::Zip { archive, .. } => match image_size(storage, archive, path) {
                Some((width, height)) if width > 0 && height > 0 => {
                    notes.push(crate::pictures::Note {
                        base,
                        size: fit.size(width, height),
                        source: String::from(path),
                    });
                    if notes.len() >= PICTURE_BATCH {
                        crate::pictures::note_all(storage, &mut notes);
                    }
                }
                _ => {
                    let file = crate::pictures::file(&base, color);
                    let _ = decode_image(storage, archive, path, &file, &paths.dir, fit, color);
                }
            },
            Source::Html { pictures, .. } => {
                if let (true, Ok(size)) = (*pictures, storage.size(path)) {
                    let file = crate::pictures::file(&base, color);
                    let _ = decode_range(storage, path, 0, size, &file, fit, color);
                }
            }
            Source::Fb2 { .. } => {
                wanted.push((String::from(path), crate::pictures::file(&base, color), fit));
                if wanted.len() >= PICTURE_BATCH {
                    decode_binaries(&mut wanted);
                }
            }
        }
        true
    });
    crate::pictures::note_all(storage, &mut notes);
    if !wanted.is_empty() {
        decode_binaries(&mut wanted);
    }
}

/// Alternative text with its whitespace collapsed, cut to at most `max`
/// bytes where a character ends.
fn clean_alt(alt: &str, max: usize) -> String {
    let mut out = String::new();
    for word in alt.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        if out.len() >= max {
            break;
        }
    }
    let mut end = out.len().min(max);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(out[..end].trim_end().len());
    out
}

/// Reads part of a file in storage.
struct StorageSource<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    offset: u64,
    end: u64,
}

impl paperpaw_sdk::image::Source for StorageSource<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, paperpaw_sdk::image::ImageError> {
        let left = (self.end - self.offset).min(buf.len() as u64) as usize;
        if left == 0 {
            return Ok(0);
        }
        let n = self
            .storage
            .read_at(self.path, self.offset, &mut buf[..left])
            .map_err(|_| paperpaw_sdk::image::ImageError::Io)?;
        self.offset += n as u64;
        Ok(n)
    }
}

/// Bytes read from a picture to tell its size.
const SIZE_HEAD: usize = 16 * 1024;

/// The size of picture `path` of the archive, from its first bytes.
fn image_size(storage: &dyn Storage, archive: &ZipArchive, path: &str) -> Option<(u32, u32)> {
    let entry = archive.entry(storage, path).ok()??;
    let mut head = Vec::new();
    head.try_reserve_exact(SIZE_HEAD).ok()?;
    head.resize(SIZE_HEAD, 0);
    let mut filled = 0;
    match archive.stored_range(storage, &entry).ok()? {
        Some((offset, len)) => {
            let want = (len as usize).min(SIZE_HEAD);
            while filled < want {
                let n = storage
                    .read_at(
                        archive.path(),
                        offset + filled as u64,
                        &mut head[filled..want],
                    )
                    .ok()?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
        }
        None => {
            let mut reader = archive.reader(storage, &entry).ok()?;
            while filled < SIZE_HEAD {
                let n = reader.read(storage, &mut head[filled..]).ok()?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
        }
    }
    paperpaw_sdk::image::dimensions(&head[..filled])
}

/// A compressed entry of the archive, inflated as it is read.
struct EntrySource<'a> {
    storage: &'a dyn Storage,
    reader: paperpaw_sdk::zip::EntryReader<'a>,
}

impl paperpaw_sdk::image::Source for EntrySource<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, paperpaw_sdk::image::ImageError> {
        self.reader
            .read(self.storage, buf)
            .map_err(|_| paperpaw_sdk::image::ImageError::Io)
    }
}

/// A source whose first bytes were read to tell its format, given again
/// first.
struct Headed<'a> {
    head: [u8; 8],
    len: usize,
    at: usize,
    rest: &'a mut dyn paperpaw_sdk::image::Source,
}

impl paperpaw_sdk::image::Source for Headed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, paperpaw_sdk::image::ImageError> {
        if self.at < self.len {
            let n = (self.len - self.at).min(buf.len());
            buf[..n].copy_from_slice(&self.head[self.at..self.at + n]);
            self.at += n;
            return Ok(n);
        }
        self.rest.read(buf)
    }
}

/// Decodes the image at `path` in the archive into the cache file
/// `cache`, in colour with `color`, else gray. Stored entries are read in
/// place, compressed ones inflated as they are decoded (or, with memory
/// short for both at once, inflated to a temporary file first).
pub(crate) fn decode_image(
    storage: &dyn Storage,
    archive: &ZipArchive,
    path: &str,
    cache: &str,
    dir: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    let entry = archive
        .entry(storage, path)
        .map_err(error)?
        .ok_or(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "missing image",
        )))?;
    decode_entry(storage, archive, &entry, cache, dir, fit, color)
}

/// Decodes the image of archive entry `entry` as [`decode_image`] does.
pub(crate) fn decode_entry(
    storage: &dyn Storage,
    archive: &ZipArchive,
    entry: &Entry,
    cache: &str,
    dir: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    if let Some((offset, len)) = archive.stored_range(storage, entry).map_err(error)? {
        return decode_range(
            storage,
            archive.path(),
            offset,
            offset + len,
            cache,
            fit,
            color,
        );
    }
    let streamed = match archive.reader(storage, entry) {
        Ok(reader) => {
            let mut source = EntrySource { storage, reader };
            decode_source(storage, &mut source, cache, fit, color)
        }
        Err(paperpaw_sdk::zip::ZipError::OutOfMemory) => {
            Err(paperpaw_sdk::image::ImageError::OutOfMemory)
        }
        Err(e) => return Err(error(e)),
    };
    if streamed != Err(paperpaw_sdk::image::ImageError::OutOfMemory) {
        return streamed.map_err(image_error);
    }
    let temp = alloc::format!("{dir}/image.tmp");
    storage.write(&temp, &[])?;
    let mut failed = None;
    archive
        .stream(storage, entry, &mut |chunk| {
            failed = storage.append(&temp, chunk).err();
            failed.is_none()
        })
        .map_err(error)?;
    let result = match failed {
        Some(e) => Err(DocumentError::Storage(e)),
        None => decode_range(storage, &temp, 0, u64::from(entry.size), cache, fit, color),
    };
    let _ = storage.remove(&temp);
    result
}

/// Decodes the image file `file` into the cache file `cache`, in colour
/// with `color`.
pub(crate) fn decode_file(
    storage: &dyn Storage,
    file: &str,
    cache: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    let end = storage.size(file)?;
    decode_range(storage, file, 0, end, cache, fit, color)
}

/// Decodes the image in `offset..end` of `file` into `cache`.
fn decode_range(
    storage: &dyn Storage,
    file: &str,
    offset: u64,
    end: u64,
    cache: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    let mut source = StorageSource {
        storage,
        path: file,
        offset,
        end,
    };
    decode_source(storage, &mut source, cache, fit, color).map_err(image_error)
}

/// Decodes the image `source` gives into `cache`; nothing is left of
/// `cache` if it fails.
fn decode_source(
    storage: &dyn Storage,
    source: &mut dyn paperpaw_sdk::image::Source,
    cache: &str,
    fit: Fit,
    color: bool,
) -> Result<(), paperpaw_sdk::image::ImageError> {
    use paperpaw_sdk::image::{ColorWriter, Format, GrayWriter, ImageError, decode};

    let mut head = [0u8; 8];
    let mut len = 0;
    while len < head.len() {
        match source.read(&mut head[len..])? {
            0 => break,
            n => len += n,
        }
    }
    let result = match Format::detect(&head[..len]) {
        Some(format) => {
            let mut source = Headed {
                head,
                len,
                at: 0,
                rest: source,
            };
            if color {
                let mut writer = ColorWriter::new(storage, cache);
                decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
            } else {
                let mut writer = GrayWriter::new(storage, cache);
                decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
            }
        }
        None => Err(ImageError::Unsupported("image format")),
    };
    if result.is_err() {
        let _ = storage.remove(cache);
    }
    result
}

fn image_error(e: paperpaw_sdk::image::ImageError) -> DocumentError {
    use paperpaw_sdk::image::ImageError;
    match e {
        ImageError::Io => DocumentError::Storage(paperpaw_sdk::StorageError::Io),
        ImageError::OutOfMemory => DocumentError::OutOfMemory,
        e => DocumentError::Malformed(alloc::borrow::Cow::Owned(alloc::format!("{e}"))),
    }
}

#[cfg(test)]
mod tests;
