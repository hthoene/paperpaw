//! The text of `word/document.xml`, turned into the blocks of the cache
//! (see [`crate::cache`]) as it streams out of the archive.
//!
//! A paragraph is a block (long ones are cut at a space), a heading one of
//! its level, a list item one that begins with its bullet or number, a table
//! row one with its cells set apart by `|`, a picture one of its own (the
//! picture itself is taken out of the archive when its page shows). Places
//! in the text are counted as the bytes of the blocks, one more for each.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Align, DocumentError};
use paperpaw_sdk::xml::{Attrs, Token, local_name, try_decode};

use crate::cache::{Block, EXTERNAL, Kind, Output};
use crate::mem;
use crate::parts::{
    Parts, archive_path, attr, clean_id, copy, flag, is_address, keep, picture_kind,
};
use crate::{
    BOLD, ITALIC, MAX_ALT, MAX_DEPTH, MAX_IMAGES, MAX_PATH, MAX_PENDING, MAX_TEXT_BYTES, MAX_TOC,
    MAX_TOC_TITLE, STRIKE, SUB, SUP, UNDER,
};

/// A paragraph is cut at the next space after this many bytes of text.
const SOFT: usize = 3000;
/// Text with no space is cut here (above `SOFT`: else the cut at a space never
/// came, and every long paragraph was cut at this many bytes, in a word).
const HARDEST: usize = 4000;
/// A paragraph with more runs of its own style is cut here.
const MOST_RUNS: usize = 512;
/// Pictures that are blocks of their own, at most (the rest leave their
/// alternative text).
const MOST_SHOWN: usize = 300;

// The styles of the text, as the cache stores them.
const STYLE_BOLD: u16 = 1;
const STYLE_ITALIC: u16 = 2;
const STYLE_SUP: u16 = 1 << 8;
const STYLE_SUB: u16 = 1 << 9;
const STYLE_UNDER: u16 = 1 << 10;
const STYLE_STRIKE: u16 = 1 << 11;
/// The formats of a run, and the style of each.
const STYLES: [(u8, u16); 6] = [
    (BOLD, STYLE_BOLD),
    (ITALIC, STYLE_ITALIC),
    (UNDER, STYLE_UNDER),
    (STRIKE, STYLE_STRIKE),
    (SUP, STYLE_SUP),
    (SUB, STYLE_SUB),
];

/// The words of `text` set apart by single spaces, at most `MAX_TOC_TITLE`
/// characters.
fn toc_title(text: &str) -> Result<String, mem::Refused> {
    let mut out = String::new();
    out.try_reserve_exact(text.len().min(MAX_TOC_TITLE * 4))
        .map_err(|_| mem::Refused)?;
    let mut chars = 0;
    for (i, word) in text.split_whitespace().enumerate() {
        for c in core::iter::once(' ').filter(|_| i > 0).chain(word.chars()) {
            if chars >= MAX_TOC_TITLE {
                return Ok(out);
            }
            out.push(c);
            chars += 1;
        }
    }
    Ok(out)
}

/// A name as the number a link and its bookmark share (31 bits).
pub(crate) fn hash(name: &str) -> u32 {
    let mut h = 0x811c_9dc5u32;
    for b in name.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h & !EXTERNAL
}

/// Turns the document into blocks.
pub(crate) struct Writer<'a> {
    storage: &'a dyn Storage,
    dir: &'a str,
    parts: &'a Parts,
    pub(crate) out: Output,
    /// Where the next block begins.
    pos: u32,
    pub(crate) failed: Option<DocumentError>,
    /// An allocation was refused: the conversion stops (`OutOfMemory`).
    oom: Cell<bool>,
    /// Inside `mc:Fallback`: the same again, left out.
    skip: u32,
    in_text: bool,
    in_ppr: bool,
    in_rpr: bool,
    in_numpr: bool,
    // The paragraph being read.
    depth: u32,
    open: bool,
    level: u8,
    style_format: u8,
    list: Option<(u32, u8)>,
    align: Align,
    // The run being read: formats set and cleared.
    set: u8,
    clear: u8,
    // The block being made.
    text: String,
    kind: Kind,
    block_align: Align,
    continued: bool,
    runs: Vec<(u16, u16, u16)>,
    run_from: usize,
    run_style: u16,
    /// An open link: its value and where in the text it began.
    link: Option<(u32, usize)>,
    /// The numbers of the list items, by level, and the list they are of.
    counters: [u32; MAX_DEPTH],
    counted: u32,
    pending: Vec<String>,
    alt: String,
    /// Where the pictures are in the archive, in the order they are shown.
    pub(crate) pictures: Vec<String>,
    /// How many pictures are blocks of their own.
    shown: usize,
    toc: usize,
    tables: u32,
    /// Nothing of the table's cell is written yet.
    cell_start: bool,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(storage: &'a dyn Storage, dir: &'a str, parts: &'a Parts) -> Self {
        Self {
            storage,
            dir,
            parts,
            out: Output::default(),
            pos: 0,
            failed: None,
            oom: Cell::new(false),
            skip: 0,
            in_text: false,
            in_ppr: false,
            in_rpr: false,
            in_numpr: false,
            depth: 0,
            open: false,
            level: 0,
            style_format: 0,
            list: None,
            align: Align::Auto,
            set: 0,
            clear: 0,
            text: String::new(),
            kind: Kind::Paragraph,
            block_align: Align::Auto,
            continued: false,
            runs: Vec::new(),
            run_from: 0,
            run_style: 0,
            link: None,
            counters: [0; MAX_DEPTH],
            counted: 0,
            pending: Vec::new(),
            alt: String::new(),
            pictures: Vec::new(),
            shown: 0,
            toc: 0,
            tables: 0,
            cell_start: false,
        }
    }

    /// Whether the conversion has ended (a failure, memory, or the text's cap).
    pub(crate) fn stopped(&self) -> bool {
        self.failed.is_some() || self.oom.get() || self.out.failed() || self.pos > MAX_TEXT_BYTES
    }

    /// The error that stopped the conversion, if one did.
    pub(crate) fn error(&mut self) -> Option<DocumentError> {
        if self.oom.get() {
            return Some(DocumentError::OutOfMemory);
        }
        self.failed.take().or_else(|| self.out.error())
    }

    /// `c` added to the text being made.
    fn put_char(&mut self, c: char) {
        if mem::push_char(&mut self.text, c).is_err() {
            self.oom.set(true);
        }
    }

    /// `more` added to the text being made.
    fn put_str(&mut self, more: &str) {
        if mem::push_str(&mut self.text, more).is_err() {
            self.oom.set(true);
        }
    }

    /// `args` formatted onto the end of the text being made.
    fn put_fmt(&mut self, args: fmt::Arguments<'_>) {
        match mem::formatted(args) {
            Ok(text) => self.put_str(&text),
            Err(_) => self.oom.set(true),
        }
    }

    /// The style of the run being read.
    fn style(&self) -> u16 {
        let formats = (self.style_format | self.set) & !self.clear;
        STYLES
            .iter()
            .filter(|(format, _)| formats & format != 0)
            .fold(0, |style, (_, bit)| style | bit)
    }

    fn close_run(&mut self) {
        if self.run_style != 0 && self.text.len() > self.run_from {
            let run = (self.run_from as u16, self.text.len() as u16, self.run_style);
            keep(&mut self.runs, run, &self.oom);
        }
        self.run_from = self.text.len();
    }

    /// The link open ends here (or goes on in the next block).
    fn close_link(&mut self, going_on: bool) {
        let Some((value, from)) = self.link else {
            return;
        };
        let to = self.text.len();
        if to > from {
            let start = self.pos.wrapping_add(from as u32);
            self.out
                .link(start, self.pos.wrapping_add(to as u32), value);
        }
        self.link = going_on.then_some((value, 0));
    }

    /// Ends the block being made; `split` for a long paragraph cut in two.
    fn flush(&mut self, split: bool) {
        let trimmed = self.text.trim_end_matches([' ', '\n']).len();
        self.text.truncate(trimmed);
        self.close_run();
        self.close_link(true);
        if !split {
            self.link = None;
        }
        if self.text.is_empty() {
            if !split {
                self.continued = false;
            }
        } else {
            let end = self.pos.wrapping_add(trimmed as u32 + 1);
            if let Kind::Heading(level) = self.kind
                && level <= 3
                && self.toc < MAX_TOC
            {
                self.toc += 1;
                match toc_title(&self.text) {
                    Ok(title) => self.out.toc(self.pos, level - 1, &title),
                    Err(_) => self.oom.set(true),
                }
            }
            let runs = core::mem::take(&mut self.runs);
            self.out.block(
                self.storage,
                self.dir,
                &Block {
                    kind: self.kind,
                    align: self.block_align,
                    continues: split,
                    continued: self.continued,
                    start: self.pos,
                    end,
                    text: &self.text,
                    runs: &runs,
                    checks: &[],
                    image: None,
                },
            );
            self.continued = split;
            self.pos = end;
            self.out.flush(self.storage, self.dir, false);
        }
        self.runs.clear();
        self.text.clear();
        self.run_from = 0;
    }

    /// Text in the style of the run being read.
    fn append(&mut self, text: &str) {
        if self.stopped() {
            return;
        }
        self.open_paragraph();
        let style = self.style();
        if style != self.run_style {
            self.close_run();
            self.run_style = style;
            if self.runs.len() >= MOST_RUNS {
                self.flush(true);
            }
        }
        self.cell_start = false;
        for c in text.chars() {
            let c = if c == '\t' || c == '\r' { ' ' } else { c };
            // Spaces run together, and none begins a block.
            if c == ' ' && (self.text.is_empty() || self.text.ends_with([' ', '\n'])) {
                continue;
            }
            self.put_char(c);
            if self.oom.get() {
                return;
            }
            let len = self.text.len();
            if (len >= SOFT && c == ' ') || len >= HARDEST {
                self.flush(true);
                self.run_style = style;
            }
        }
    }

    /// Begins the paragraph, where it has not begun.
    fn open_paragraph(&mut self) {
        if self.open {
            return;
        }
        self.open = true;
        if self.tables > 0 {
            // A row is one block; its paragraphs follow each other.
            if self.text.is_empty() {
                self.kind = Kind::Paragraph;
                self.block_align = Align::Auto;
            }
            if !self.cell_start && !self.text.is_empty() {
                self.put_char(' ');
            }
        } else {
            self.kind = match self.level {
                0 => Kind::Paragraph,
                level => Kind::Heading(level.min(6)),
            };
            self.block_align = self.align;
            // A numbered heading is a heading.
            if let Some((number, level)) = self.list.filter(|(n, _)| *n != 0 && self.level == 0) {
                self.list_prefix(number, level);
            }
        }
        for name in core::mem::take(&mut self.pending) {
            self.out
                .mark(hash(&name), self.pos.wrapping_add(self.text.len() as u32));
        }
    }

    /// The bullet or number an item of a list begins with.
    fn list_prefix(&mut self, number: u32, level: u8) {
        let at = usize::from(level).min(MAX_DEPTH - 1);
        if self.counted != number {
            self.counted = number;
            self.counters = [0; MAX_DEPTH];
        }
        self.counters[at] += 1;
        for deeper in &mut self.counters[at + 1..] {
            *deeper = 0;
        }
        for _ in 0..at {
            self.put_str("  ");
        }
        if self.parts.numbered(number, level) {
            let number = self.counters[at];
            self.put_fmt(format_args!("{number}. "));
        } else {
            self.put_str("• ");
        }
    }

    pub(crate) fn end_paragraph(&mut self) {
        if self.open && self.tables == 0 {
            self.close_link(false);
            self.flush(false);
        }
        self.open = false;
    }

    fn start_paragraph(&mut self) {
        // A paragraph inside another (a text box in a run): the outer one
        // is ended, and goes on in a new one.
        self.end_paragraph();
        self.depth += 1;
        self.level = 0;
        self.style_format = 0;
        self.list = None;
        self.align = Align::Auto;
        self.set = 0;
        self.clear = 0;
    }

    /// A picture: the text before it is a block, the picture another.
    fn picture(&mut self, rid: &str) {
        let Some(rel) = self.parts.rel(rid).filter(|r| r.image && !r.external) else {
            return;
        };
        let Ok(from) = archive_path(&rel.target) else {
            self.oom.set(true);
            return;
        };
        let usable = picture_kind(&from).is_some() && from.len() <= MAX_PATH;
        let alt = core::mem::take(&mut self.alt);
        let index = match self.pictures.iter().position(|p| *p == from) {
            Some(at) => Some(at),
            None if usable && self.pictures.len() < MAX_IMAGES => {
                keep(&mut self.pictures, from, &self.oom);
                Some(self.pictures.len().saturating_sub(1))
            }
            None => None,
        };
        let index = index.filter(|_| self.shown < MOST_SHOWN && !self.oom.get());
        let Some(index) = index else {
            if !alt.is_empty() {
                match mem::formatted(format_args!("[{alt}]")) {
                    Ok(text) => self.append(&text),
                    Err(_) => self.oom.set(true),
                }
            }
            return;
        };
        self.shown += 1;
        self.open_paragraph();
        let continued = self.continued || !self.text.is_empty();
        self.flush(true);
        let end = self.pos.wrapping_add(1);
        self.out.block(
            self.storage,
            self.dir,
            &Block {
                kind: Kind::Image,
                align: Align::Center,
                continues: false,
                continued: false,
                start: self.pos,
                end,
                text: &alt,
                runs: &[],
                checks: &[],
                image: Some((index as u32, 0, 0)),
            },
        );
        self.pos = end;
        // What follows the picture goes on with the paragraph.
        self.continued = continued;
    }

    fn start(&mut self, name: &str, attrs: &Attrs<'_>) {
        if self.skip > 0 {
            self.skip += 1;
            return;
        }
        let name = local_name(name);
        match name {
            "Fallback" => self.skip = 1,
            "p" => self.start_paragraph(),
            "pPr" => self.in_ppr = true,
            "rPr" => self.in_rpr = true,
            _ => {
                self.start_paragraph_property(name, attrs);
                self.start_run(name, attrs);
                self.start_text(name, attrs);
                self.start_object(name, attrs);
            }
        }
    }

    /// Numbering, style, outline level and alignment of a paragraph.
    fn start_paragraph_property(&mut self, name: &str, attrs: &Attrs<'_>) {
        let val = || attr(attrs, "val", &self.oom);
        match name {
            "numPr" if self.in_ppr => {
                self.in_numpr = true;
                let (number, level) = self.list.unwrap_or((0, 0));
                self.list = Some((number, level));
            }
            "ilvl" if self.in_numpr => {
                if let Some(level) = val().and_then(|v| v.parse::<u8>().ok()) {
                    let number = self.list.map_or(0, |(n, _)| n);
                    self.list = Some((number, level));
                }
            }
            "numId" if self.in_numpr => {
                if let Some(number) = val().and_then(|v| v.parse::<u32>().ok()) {
                    let level = self.list.map_or(0, |(_, l)| l);
                    self.list = Some((number, level));
                }
            }
            "pStyle" if self.in_ppr => {
                if let Some(style) = val().and_then(|v| self.parts.style(&v)) {
                    self.level = self.level.max(style.level);
                    self.style_format = style.format;
                    // Where the paragraph names none, its style's list.
                    if self.list.is_none_or(|(n, _)| n == 0) {
                        self.list = style.list;
                    }
                }
            }
            "outlineLvl" if self.in_ppr && self.level == 0 => {
                if let Some(level) = val().and_then(|v| v.parse::<u8>().ok()).filter(|l| *l < 6) {
                    self.level = level + 1;
                }
            }
            "jc" if self.in_ppr => {
                self.align = match val().as_deref() {
                    Some("center") => Align::Center,
                    Some("right" | "end") => Align::Right,
                    _ => Align::Auto,
                };
            }
            _ => {}
        }
    }

    /// Runs and their character formatting.
    fn start_run(&mut self, name: &str, attrs: &Attrs<'_>) {
        let val = || attr(attrs, "val", &self.oom);
        match name {
            "r" => {
                self.set = 0;
                self.clear = 0;
            }
            "b" | "i" | "u" | "strike" | "dstrike" if self.in_rpr && !self.in_ppr => {
                let bit = match name {
                    "b" => BOLD,
                    "i" => ITALIC,
                    "u" => UNDER,
                    _ => STRIKE,
                };
                if flag(attrs, &self.oom) {
                    self.set |= bit;
                    self.clear &= !bit;
                } else {
                    self.clear |= bit;
                    self.set &= !bit;
                }
            }
            "vertAlign" if self.in_rpr && !self.in_ppr => match val().as_deref() {
                Some("superscript") => self.set |= SUP,
                Some("subscript") => self.set |= SUB,
                _ => {}
            },
            "rStyle" if self.in_rpr && !self.in_ppr => {
                if let Some(style) = val().and_then(|v| self.parts.style(&v)) {
                    self.set |= style.format;
                }
            }
            _ => {}
        }
    }

    /// Text, breaks, links and bookmarks.
    fn start_text(&mut self, name: &str, attrs: &Attrs<'_>) {
        match name {
            "t" => self.in_text = true,
            "tab" if !self.in_ppr && !self.in_rpr => self.append(" "),
            "br" | "cr" if !self.in_ppr => {
                let page =
                    attr(attrs, "type", &self.oom).is_some_and(|t| t == "page" || t == "column");
                if !page {
                    self.append("\n");
                }
            }
            "noBreakHyphen" => self.append("\u{2011}"),
            "softHyphen" => self.append("\u{ad}"),
            "hyperlink" => {
                let value = match (
                    attr(attrs, "id", &self.oom),
                    attr(attrs, "anchor", &self.oom),
                ) {
                    (Some(id), _) => self
                        .parts
                        .rel(&id)
                        .filter(|r| r.external && is_address(&r.target))
                        .and_then(|r| self.out.url(&r.target))
                        .map(|n| n | EXTERNAL),
                    (None, Some(anchor)) => match clean_id(&anchor) {
                        Ok(id) => Some(hash(&id)),
                        Err(_) => {
                            self.oom.set(true);
                            None
                        }
                    },
                    _ => None,
                };
                if let Some(value) = value {
                    self.open_paragraph();
                    self.link = Some((value, self.text.len()));
                }
            }
            "bookmarkStart" => {
                let name = attr(attrs, "name", &self.oom).unwrap_or_default();
                let Ok(id) = clean_id(&name) else {
                    self.oom.set(true);
                    return;
                };
                if (!id.is_empty() && !id.starts_with('_'))
                    || id.starts_with("_Toc")
                    || id.starts_with("_Ref")
                {
                    if self.open {
                        let at = self.pos.wrapping_add(self.text.len() as u32);
                        self.out.mark(hash(&id), at);
                    } else if self.pending.len() < MAX_PENDING {
                        keep(&mut self.pending, id, &self.oom);
                    }
                }
            }
            _ => {}
        }
    }

    /// Pictures and tables.
    fn start_object(&mut self, name: &str, attrs: &Attrs<'_>) {
        match name {
            "drawing" | "pict" => self.alt.clear(),
            "docPr" => {
                let alt = attr(attrs, "descr", &self.oom)
                    .filter(|d| !d.trim().is_empty())
                    .or_else(|| attr(attrs, "title", &self.oom));
                if let Some(alt) = alt {
                    let end = alt
                        .char_indices()
                        .nth(MAX_ALT)
                        .map_or(alt.len(), |(at, _)| at);
                    self.alt = copy(alt.get(..end).unwrap_or_default(), &self.oom);
                }
            }
            "blip" => {
                if let Some(rid) = attr(attrs, "embed", &self.oom) {
                    self.picture(&rid);
                }
            }
            "imagedata" => {
                if let Some(rid) = attr(attrs, "id", &self.oom) {
                    self.picture(&rid);
                }
            }
            "tbl" => {
                self.end_paragraph();
                self.tables += 1;
            }
            "tc" if self.tables > 0 => {
                self.end_paragraph();
                if !self.text.is_empty() {
                    self.put_str(" | ");
                }
                self.cell_start = true;
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        match local_name(name) {
            "pPr" => self.in_ppr = false,
            "rPr" => self.in_rpr = false,
            "numPr" => self.in_numpr = false,
            "t" => self.in_text = false,
            "p" => {
                self.end_paragraph();
                self.depth = self.depth.saturating_sub(1);
            }
            "hyperlink" => self.close_link(false),
            "tr" if self.tables > 0 => {
                self.open = false;
                self.close_link(false);
                self.flush(false);
            }
            "tbl" if self.tables > 0 => {
                self.open = false;
                self.close_link(false);
                self.flush(false);
                self.tables -= 1;
            }
            "body" => self.end_paragraph(),
            _ => {}
        }
    }

    pub(crate) fn token(&mut self, token: Token<'_>) {
        if self.stopped() {
            return;
        }
        match token {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => {
                self.start(name, &attrs);
                if self_closing {
                    self.end(name);
                }
            }
            Token::End { name } => self.end(name),
            Token::Text(text) if self.in_text && self.skip == 0 => match try_decode(&text) {
                Ok(text) => self.append(&text),
                Err(_) => self.oom.set(true),
            },
            Token::CData(text) if self.in_text && self.skip == 0 => self.append(text),
            _ => {}
        }
    }

    /// How long the text is, as places in it are counted.
    pub(crate) fn length(&self) -> u32 {
        self.pos
    }

    /// The end of the document: what waits is written.
    pub(crate) fn finish(&mut self) {
        self.end_paragraph();
        self.flush(false);
        self.out.flush(self.storage, self.dir, true);
    }
}
