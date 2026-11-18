//! The text of an RTF document, read once as a stream.
//!
//! RTF is groups (`{ … }`), control words (`\b`, `\par`, `荤`, `\'e9`)
//! and text. [`Converter`] is a state machine that is given the file's bytes
//! one chunk after the other (a control word, a `\'hh` or a picture may be
//! cut between two chunks) and writes blocks (a paragraph, a heading, a
//! picture) in the form [`crate::cache`] keeps and reads: a document of any
//! length is converted with the memory of one block, a stack of groups
//! (at most [`MAX_GROUPS`] deep) and a small table of style names.
//!
//! What is kept: paragraphs, headings (the outline level, or a style
//! named `heading N` or `Title`), bold, italic, underline, strike-through,
//! super- and subscript, alignment, bulleted and numbered lists (the list
//! text the writer made, else a bullet), tables (a row a line, the cells
//! set apart by `|`), line breaks, `\uN` characters and `\'hh` bytes of the
//! document's code page, and JPEG and PNG pictures (their hexadecimal text is
//! left where it is and decoded when the picture shows).
//!
//! What is left out, safely: every destination the converter does not
//! know (`{\*\…}`, fonts, colours, headers, footers, footnotes, fields'
//! codes, objects, the other kinds of pictures, hidden text), and what no
//! reader can show.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, DocumentError};

use crate::cache::{Block, Kind, Output};
use crate::charset::Charset;
use crate::mem::{self, Refused};

/// A piece of text is cut at the first space after this many bytes.
const SPLIT: usize = 3000;
/// A paragraph with no space in it is cut at this many bytes.
const HARD: usize = 8000;
/// Most checkpoints of a block's offsets kept.
const MOST_CHECKS: usize = 96;
/// Groups nested deeper than this are read at this depth.
const MAX_GROUPS: usize = 64;
/// The longest control word read (the rest of a longer one is dropped).
const MAX_WORD: usize = 32;
/// Styles named as headings kept, and the longest style name looked at.
const MAX_STYLES: usize = 256;
const STYLE_NAME: usize = 24;
/// Pictures taken at most, and the most hexadecimal text of one.
const MAX_PICTURES: u32 = 1000;
const MAX_PICTURE: u32 = 16 * 1024 * 1024;

const BOLD: u16 = 1;
const ITALIC: u16 = 2;
const UNDERLINE: u16 = 1 << 10;
const STRIKE: u16 = 1 << 11;
const SUPERSCRIPT: u16 = 1 << 8;
const SUBSCRIPT: u16 = 1 << 9;
/// Hidden text (`\v`): not a style of the run, only a state of the group.
const HIDDEN: u16 = 1 << 15;

/// What the converter asks of the book about pictures.
pub(crate) trait Pictures {
    /// The size a picture is decoded to, if the hexadecimal text at the
    /// source offsets `start..end` is one that can be.
    ///
    /// # Errors
    /// A card or memory that fails: the conversion stops with it.
    fn size(&mut self, start: u32, end: u32) -> Result<Option<(u32, u32)>, DocumentError>;
}

/// What a group is the content of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dest {
    Text,
    /// The style sheet: only the names of the styles are read.
    Sheet,
    /// A picture: only where its data is.
    Pict,
    /// The text a writer made for a list item (`\listtext`, `\pntext`).
    Marker,
}

/// What a group sets and its inner groups inherit; a closing brace
/// brings back the one of the group outside.
#[derive(Debug, Clone, Copy)]
struct Group {
    style: u16,
    skip: bool,
    dest: Dest,
    /// Characters a `\uN` is followed by as a fallback (`\ucN`).
    uc: u8,
    align: Align,
    heading: u8,
    list: bool,
}

#[derive(Debug, Clone, Copy)]
enum Tok {
    Text,
    /// After a backslash.
    Escape,
    /// In a control word, its name or its number.
    Word,
    /// After `\'`, with the first digit if it is read.
    Hex(Option<u8>),
    /// In the bytes after `\binN`.
    Bin,
}

/// A picture whose data is being passed.
#[derive(Debug, Clone, Copy)]
struct Pict {
    /// How many groups were open where it began.
    depth: usize,
    /// 1 JPEG, 2 PNG, 0 not said (yet).
    kind: u8,
    /// Where the hexadecimal data begins (0: not yet).
    start: u32,
    /// Where `\pict` is.
    open: u32,
}

/// The value of a hexadecimal digit.
fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

/// The heading level a style's name stands for (0: none).
fn style_level(name: &[u8]) -> u8 {
    match name {
        b"title" => 1,
        [
            b'h',
            b'e',
            b'a',
            b'd',
            b'i',
            b'n',
            b'g',
            b' ',
            level @ b'1'..=b'6',
        ] => level - b'0',
        _ => 0,
    }
}

/// Whether a destination the converter does not use ends its group's
/// content (the starred ones all do, but the ones it knows).
fn skipped(name: &str) -> bool {
    matches!(
        name,
        "fonttbl"
            | "colortbl"
            | "info"
            | "header"
            | "headerl"
            | "headerr"
            | "headerf"
            | "footer"
            | "footerl"
            | "footerr"
            | "footerf"
            | "footnote"
            | "object"
            | "objdata"
            | "listtable"
            | "listoverridetable"
            | "rsidtbl"
            | "generator"
            | "themedata"
            | "colorschememapping"
            | "latentstyles"
            | "datastore"
            | "pgptbl"
            | "revtbl"
            | "xmlnstbl"
            | "fldinst"
            | "nonshppict"
            | "pnseclvl"
            | "tc"
            | "tcn"
            | "xe"
            | "txe"
            | "rxe"
            | "bkmkstart"
            | "bkmkend"
            | "private"
            | "falt"
            | "panose"
    )
}

/// Reads the bytes and writes the blocks.
pub(crate) struct Converter {
    pub(crate) out: Output,
    charset: Charset,
    /// The source offset of the next byte.
    pos: u32,
    tok: Tok,
    /// Where the token being read began.
    tok_start: u32,
    // The control word being read.
    word: [u8; MAX_WORD],
    word_len: usize,
    param: Option<i64>,
    negative: bool,
    in_param: bool,
    /// Bytes of binary data left to pass.
    bin: u32,
    /// `\*` was the last token.
    star: bool,
    group: Group,
    stack: Vec<Group>,
    /// Groups opened beyond [`MAX_GROUPS`] and not closed.
    deep: u32,
    /// Characters of a `\uN`'s fallback still to pass.
    skip_chars: u8,
    /// The first half of a surrogate pair of `\uN`s, and where it was.
    high: Option<(u16, u32)>,
    // The block being made.
    block_start: u32,
    text: String,
    runs: Vec<(u16, u16, u16)>,
    checks: Vec<(u16, u32)>,
    shift: i64,
    continued: bool,
    /// The paragraph has its list text (written or made).
    marker: bool,
    in_row: bool,
    run_from: usize,
    run_style: u16,
    // The style sheet: styles that are headings, and the name being read.
    styles: Vec<(u16, u8)>,
    sheet_number: Option<u16>,
    sheet_name: [u8; STYLE_NAME],
    sheet_len: usize,
    // Pictures.
    pict: Option<Pict>,
    pictures: u32,
}

impl Converter {
    pub(crate) fn new(charset: Charset, out: Output) -> Self {
        Self {
            out,
            charset,
            pos: 0,
            tok: Tok::Text,
            tok_start: 0,
            word: [0; MAX_WORD],
            word_len: 0,
            param: None,
            negative: false,
            in_param: false,
            bin: 0,
            star: false,
            group: Group {
                style: 0,
                skip: false,
                dest: Dest::Text,
                uc: 1,
                align: Align::Auto,
                heading: 0,
                list: false,
            },
            stack: Vec::new(),
            deep: 0,
            skip_chars: 0,
            high: None,
            block_start: 0,
            text: String::new(),
            runs: Vec::new(),
            checks: Vec::new(),
            shift: 0,
            continued: false,
            marker: false,
            in_row: false,
            run_from: 0,
            run_style: 0,
            styles: Vec::new(),
            sheet_number: None,
            sheet_name: [0; STYLE_NAME],
            sheet_len: 0,
            pict: None,
            pictures: 0,
        }
    }

    /// The bytes of the document from where the last call ended.
    pub(crate) fn feed(&mut self, bytes: &[u8], pictures: &mut dyn Pictures) {
        for &byte in bytes {
            if self.out.failed() {
                return;
            }
            self.byte(byte, pictures);
            self.pos = self.pos.wrapping_add(1);
        }
    }

    /// The end of the document: the last block is written.
    pub(crate) fn finish(&mut self) {
        self.flush(self.pos, false);
    }

    /// What `result` says, a refusal noted for good.
    fn kept<T>(&mut self, result: Result<T, Refused>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(Refused) => {
                self.out.refuse();
                None
            }
        }
    }

    fn byte(&mut self, byte: u8, pictures: &mut dyn Pictures) {
        match self.tok {
            Tok::Text => self.text_byte(byte, pictures),
            Tok::Bin => {
                self.bin = self.bin.saturating_sub(1);
                if self.bin == 0 {
                    self.tok = Tok::Text;
                }
            }
            Tok::Escape => self.escape(byte),
            Tok::Word => self.word_byte(byte, pictures),
            Tok::Hex(first) => match (hex(byte), first) {
                (Some(low), None) => self.tok = Tok::Hex(Some(low)),
                (Some(low), Some(high)) => {
                    self.tok = Tok::Text;
                    self.hex_byte(high << 4 | low);
                }
                // Not a digit: the `\'` was no byte.
                (None, _) => {
                    self.tok = Tok::Text;
                    self.byte(byte, pictures);
                }
            },
        }
    }

    fn text_byte(&mut self, byte: u8, pictures: &mut dyn Pictures) {
        match byte {
            b'\\' => {
                self.tok = Tok::Escape;
                self.tok_start = self.pos;
            }
            b'{' => self.open_group(),
            b'}' => self.close_group(pictures),
            // Line ends in the file are not text.
            b'\r' | b'\n' => {}
            _ => self.plain(byte),
        }
    }

    fn escape(&mut self, byte: u8) {
        self.tok = Tok::Text;
        let at = self.tok_start;
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' => {
                self.tok = Tok::Word;
                self.word[0] = byte;
                self.word_len = 1;
                self.param = None;
                self.negative = false;
                self.in_param = false;
            }
            b'\'' => self.tok = Tok::Hex(None),
            b'\\' | b'{' | b'}' => self.plain(byte),
            b'~' => self.symbol('\u{a0}', at),
            b'_' => self.symbol('-', at),
            b'*' => self.star = true,
            b'\r' | b'\n' if !self.group.skip => self.paragraph_end(at),
            _ => {}
        }
    }

    fn word_byte(&mut self, byte: u8, pictures: &mut dyn Pictures) {
        if !self.in_param {
            if byte.is_ascii_alphabetic() {
                if self.word_len < MAX_WORD {
                    self.word[self.word_len] = byte;
                    self.word_len += 1;
                }
                return;
            }
            if byte == b'-' {
                self.negative = true;
                self.in_param = true;
                return;
            }
        }
        if byte.is_ascii_digit() {
            self.in_param = true;
            let digit = i64::from(byte - b'0');
            self.param = Some(
                self.param
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit)
                    .min(i64::from(i32::MAX)),
            );
            return;
        }
        if self.negative {
            self.param = Some(-self.param.unwrap_or(0));
        }
        self.tok = Tok::Text;
        self.word();
        if self.bin > 0 {
            self.tok = Tok::Bin;
        }
        // One space ends the word; any other byte is read as it is.
        if byte != b' ' {
            self.byte(byte, pictures);
        }
    }

    // ---- groups ----

    fn open_group(&mut self) {
        self.star = false;
        self.skip_chars = 0;
        self.high = None;
        if self.stack.len() >= MAX_GROUPS {
            self.deep = self.deep.saturating_add(1);
            return;
        }
        let pushed = mem::push(&mut self.stack, self.group);
        self.kept(pushed);
    }

    fn close_group(&mut self, pictures: &mut dyn Pictures) {
        self.skip_chars = 0;
        self.high = None;
        if self
            .pict
            .is_some_and(|pict| pict.depth == self.stack.len() + self.deep as usize)
        {
            self.end_picture(pictures);
        }
        if self.deep > 0 {
            self.deep -= 1;
            return;
        }
        if let Some(outer) = self.stack.pop() {
            let before = self.group.style;
            self.group = outer;
            if before != self.group.style {
                self.styled();
            }
        }
    }

    // ---- characters ----

    /// A byte of the text.
    fn plain(&mut self, byte: u8) {
        if self.skip_chars > 0 {
            self.skip_chars -= 1;
            return;
        }
        if self.group.skip {
            return;
        }
        match self.group.dest {
            Dest::Pict => {
                if hex(byte).is_some() {
                    let at = self.pos;
                    if let Some(pict) = self.pict.as_mut().filter(|p| p.kind != 0 && p.start == 0) {
                        pict.start = at;
                    }
                }
            }
            Dest::Sheet => self.sheet_char(byte),
            _ => self.character(self.charset.decode(byte), self.pos),
        }
    }

    /// A `\'hh` byte.
    fn hex_byte(&mut self, byte: u8) {
        if self.skip_chars > 0 {
            self.skip_chars -= 1;
            return;
        }
        if self.group.skip || matches!(self.group.dest, Dest::Pict | Dest::Sheet) {
            return;
        }
        self.character(self.charset.decode(byte), self.tok_start);
    }

    /// A character a control symbol or word stands for.
    fn symbol(&mut self, c: char, at: u32) {
        if self.group.skip || matches!(self.group.dest, Dest::Pict | Dest::Sheet) {
            return;
        }
        self.character(c, at);
    }

    fn unicode(&mut self, param: Option<i64>, at: u32) {
        let unit = (param.unwrap_or(0) & 0xffff) as u16;
        self.skip_chars = self.group.uc;
        match unit {
            0xd800..=0xdbff => self.high = Some((unit, at)),
            0xdc00..=0xdfff => {
                let c = self.high.take().and_then(|(high, first)| {
                    let code =
                        0x1_0000 + (u32::from(high - 0xd800) << 10) + u32::from(unit - 0xdc00);
                    char::from_u32(code).map(|c| (c, first))
                });
                match c {
                    Some((c, first)) => self.character(c, first),
                    None => self.character(char::REPLACEMENT_CHARACTER, at),
                }
            }
            _ => {
                if let Some(c) = char::from_u32(u32::from(unit)) {
                    self.character(c, at);
                }
            }
        }
    }

    fn sheet_char(&mut self, byte: u8) {
        if byte == b';' {
            if let Some(number) = self.sheet_number.take() {
                let level = style_level(&self.sheet_name[..self.sheet_len]);
                if level > 0 && self.styles.len() < MAX_STYLES {
                    let pushed = mem::push(&mut self.styles, (number, level));
                    self.kept(pushed);
                }
            }
            self.sheet_len = 0;
        } else if let Some(slot) = self.sheet_name.get_mut(self.sheet_len) {
            *slot = byte.to_ascii_lowercase();
            self.sheet_len += 1;
        }
    }

    /// A character of the text, which began at source offset `at`.
    fn character(&mut self, c: char, at: u32) {
        self.high = None;
        if self.group.style & HIDDEN != 0 {
            return;
        }
        if c.is_ascii_whitespace() {
            let last = self.text.chars().next_back();
            if matches!(last, None | Some(' ' | '\n')) {
                return;
            }
            self.put(' ', at);
            if self.text.len() >= SPLIT {
                self.flush(at.wrapping_add(1).max(self.pos), true);
            }
            return;
        }
        // The private use area is where a symbol font puts its bullets.
        if ('\u{e000}'..='\u{f8ff}').contains(&c) {
            if self.group.dest == Dest::Marker {
                self.put('•', at);
            }
            return;
        }
        if c.is_control() {
            return;
        }
        if self.group.dest == Dest::Marker && matches!(c, '\u{b7}' | '\u{a7}') {
            self.put('•', at);
            return;
        }
        self.put(c, at);
    }

    fn put(&mut self, c: char, at: u32) {
        if self.text.is_empty() && self.group.list && !self.marker {
            self.marker = true;
            let pushed = mem::push_str(&mut self.text, "• ");
            self.kept(pushed);
            self.shift = 0;
            self.checks.clear();
        }
        let shift = self.text.len() as i64 - i64::from(at.wrapping_sub(self.block_start));
        if shift != self.shift && self.checks.len() < MOST_CHECKS && self.text.len() < 60_000 {
            let pushed = mem::push(&mut self.checks, (self.text.len() as u16, at));
            if self.kept(pushed).is_none() {
                return;
            }
            self.shift = shift;
        }
        let pushed = mem::push_char(&mut self.text, c);
        if self.kept(pushed).is_none() {
            return;
        }
        // Text with no space to cut at (Chinese, say) is cut where it is.
        if self.text.len() >= HARD {
            self.flush(at.wrapping_add(1).max(self.pos), true);
        }
    }

    // ---- styles and blocks ----

    fn style(&self) -> u16 {
        self.group.style & !HIDDEN
    }

    fn close_run(&mut self) {
        if self.run_style != 0 && self.text.len() > self.run_from {
            let run = (self.run_from as u16, self.text.len() as u16, self.run_style);
            let pushed = mem::push(&mut self.runs, run);
            self.kept(pushed);
        }
        self.run_from = self.text.len();
    }

    fn styled(&mut self) {
        let style = self.style();
        if style != self.run_style {
            self.close_run();
            self.run_style = style;
        }
    }

    fn kind(&self) -> Kind {
        match self.group.heading {
            0 => Kind::Paragraph,
            level => Kind::Heading(level),
        }
    }

    /// Ends the block being made at source offset `end`; the next begins
    /// there. `split` for a long paragraph cut in two.
    fn flush(&mut self, end: u32, split: bool) {
        self.close_run();
        let trimmed = self.text.trim_end_matches([' ', '\n']).len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() {
            self.runs
                .retain(|r| r.0 < r.1 && usize::from(r.1) <= trimmed);
            let kind = self.kind();
            if let Kind::Heading(level) = kind
                && level <= 3
            {
                self.out.toc(self.block_start, level - 1, &self.text);
            }
            self.out.block(&Block {
                kind,
                align: self.group.align,
                continues: split,
                continued: self.continued,
                start: self.block_start,
                end,
                text: &self.text,
                runs: &self.runs,
                checks: &self.checks,
                image: None,
            });
            self.continued = split;
        } else if !split {
            self.continued = false;
        }
        if !split {
            self.marker = false;
        }
        self.runs.clear();
        self.checks.clear();
        self.text.clear();
        self.shift = 0;
        self.block_start = end;
        self.run_from = 0;
    }

    /// `\par` and the like: the paragraph ends (inside a table's row the
    /// cell goes on).
    fn paragraph_end(&mut self, at: u32) {
        if self.in_row {
            self.character(' ', at);
        } else {
            self.flush(at, false);
        }
    }

    /// `\line`: the line ends where it is.
    fn line_break(&mut self) {
        let trimmed = self.text.trim_end_matches(' ').len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            let pushed = mem::push_char(&mut self.text, '\n');
            self.kept(pushed);
        }
    }

    fn cell_end(&mut self, at: u32) {
        self.in_row = true;
        let trimmed = self.text.trim_end_matches(' ').len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() {
            self.put(' ', at);
        }
        self.put('|', at);
        self.put(' ', at);
    }

    fn row_end(&mut self, at: u32) {
        let trimmed = self.text.trim_end_matches([' ', '|']).len();
        self.text.truncate(trimmed);
        self.flush(at, false);
        self.in_row = false;
    }

    // ---- control words ----

    fn flag(&mut self, bit: u16, param: Option<i64>) {
        if param == Some(0) {
            self.group.style &= !bit;
        } else {
            self.group.style |= bit;
        }
        self.styled();
    }

    fn word(&mut self) {
        let words = self.word;
        let name = core::str::from_utf8(&words[..self.word_len]).unwrap_or("");
        let param = self.param;
        let at = self.tok_start;
        let star = core::mem::take(&mut self.star);
        if name == "bin" {
            if let Some(n) = param.and_then(|n| u32::try_from(n).ok()) {
                self.bin = n;
            }
            return;
        }
        self.skip_chars = 0;
        if self.group.skip || self.deep > 0 && name != "par" {
            return;
        }
        // A destination written `{\*\name` is left out unless it is known.
        if star && name != "shppict" {
            self.group.skip = true;
            return;
        }
        if self.group.dest == Dest::Sheet {
            if name == "s" {
                self.sheet_number = param.and_then(|n| u16::try_from(n).ok());
            }
            return;
        }
        if self.group.dest == Dest::Pict {
            self.picture_word(name);
            return;
        }
        if skipped(name) {
            self.group.skip = true;
            return;
        }
        match name {
            "stylesheet" => {
                self.group.dest = Dest::Sheet;
                self.sheet_number = None;
                self.sheet_len = 0;
            }
            "pict" => self.begin_picture(at),
            "listtext" | "pntext" => {
                self.group.dest = Dest::Marker;
                self.marker = true;
            }
            "b" => self.flag(BOLD, param),
            "i" => self.flag(ITALIC, param),
            "ul" | "uld" | "uldb" | "ulw" | "ulth" | "ulwave" | "uldash" | "uldashd"
            | "uldashdd" => self.flag(UNDERLINE, param),
            "ulnone" => self.flag(UNDERLINE, Some(0)),
            "strike" | "striked" => self.flag(STRIKE, param),
            "super" => {
                self.group.style &= !SUBSCRIPT;
                self.flag(SUPERSCRIPT, param);
            }
            "sub" => {
                self.group.style &= !SUPERSCRIPT;
                self.flag(SUBSCRIPT, param);
            }
            "nosupersub" => {
                self.group.style &= !(SUPERSCRIPT | SUBSCRIPT);
                self.styled();
            }
            "v" => self.flag(HIDDEN, param),
            "plain" => {
                self.group.style = 0;
                self.styled();
            }
            _ => self.layout_word(name, param, at),
        }
    }

    /// The control words of paragraphs, tables and special characters.
    fn layout_word(&mut self, name: &str, param: Option<i64>, at: u32) {
        match name {
            "pard" => {
                self.group.align = Align::Auto;
                self.group.heading = 0;
                self.group.list = false;
                self.in_row = false;
            }
            "ql" => self.group.align = Align::Start,
            "qr" => self.group.align = Align::End,
            "qc" => self.group.align = Align::Center,
            "qj" => self.group.align = Align::Justify,
            "outlinelevel" => {
                self.group.heading = match param {
                    Some(level @ 0..=5) => level as u8 + 1,
                    _ => 0,
                };
            }
            "s" => {
                let number = param.and_then(|n| u16::try_from(n).ok());
                let found = self.styles.iter().find(|s| Some(s.0) == number);
                if let Some(&(_, level)) = found {
                    self.group.heading = level;
                }
            }
            "ls" => self.group.list = param.is_some_and(|n| n > 0),
            "pnlvl" => self.group.list = true,
            "par" | "sect" | "page" => self.paragraph_end(at),
            "line" => self.line_break(),
            "tab" | "emspace" | "enspace" | "qmspace" => self.symbol(' ', at),
            "cell" | "nestcell" => self.cell_end(at),
            "row" | "nestrow" => self.row_end(at),
            "trowd" | "intbl" => self.in_row = true,
            "emdash" => self.symbol('—', at),
            "endash" => self.symbol('–', at),
            "bullet" => self.symbol('•', at),
            "lquote" => self.symbol('‘', at),
            "rquote" => self.symbol('’', at),
            "ldblquote" => self.symbol('“', at),
            "rdblquote" => self.symbol('”', at),
            "u" => self.unicode(param, at),
            "uc" => {
                self.group.uc = param.map_or(1, |n| n.clamp(0, 8) as u8);
            }
            _ => {}
        }
    }

    // ---- pictures ----

    fn begin_picture(&mut self, at: u32) {
        if self.pict.is_some() || self.pictures >= MAX_PICTURES {
            self.group.skip = true;
            return;
        }
        self.group.dest = Dest::Pict;
        self.pict = Some(Pict {
            depth: self.stack.len() + self.deep as usize,
            kind: 0,
            start: 0,
            open: at,
        });
    }

    fn picture_word(&mut self, name: &str) {
        match name {
            "jpegblip" | "pngblip" => {
                let kind = if name == "jpegblip" { 1 } else { 2 };
                if let Some(pict) = self.pict.as_mut() {
                    pict.kind = kind;
                }
            }
            // A kind of picture that is not decoded.
            "emfblip" | "wmetafile" | "dibitmap" | "wbitmap" | "macpict" | "pmmetafile" => {
                self.group.skip = true;
            }
            _ => {}
        }
    }

    /// The closing brace of a picture's group, at the current offset: the
    /// text before it is a block, the picture another.
    fn end_picture(&mut self, pictures: &mut dyn Pictures) {
        let Some(pict) = self.pict.take() else {
            return;
        };
        let end = self.pos;
        if pict.kind == 0
            || pict.start == 0
            || end <= pict.start
            || end - pict.start > MAX_PICTURE
            || self.group.skip
        {
            return;
        }
        let size = match pictures.size(pict.start, end) {
            Ok(size) => size,
            Err(error) => {
                self.out.note(error);
                return;
            }
        };
        let Some((width, height)) = size else {
            return;
        };
        self.pictures += 1;
        self.out.picture(pict.start, end);
        let was_continued = self.continued;
        self.flush(pict.open, true);
        self.block_start = pict.open;
        let next = end.wrapping_add(1);
        self.out.block(&Block {
            kind: Kind::Image,
            align: Align::Center,
            continues: false,
            continued: false,
            start: pict.open,
            end: next,
            text: "",
            runs: &[],
            checks: &[],
            image: Some((self.pictures, width, height)),
        });
        // What follows the picture goes on with the paragraph.
        self.continued = was_continued || !self.text.is_empty();
        self.block_start = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_names() {
        assert_eq!(style_level(b"heading 2"), 2);
        assert_eq!(style_level(b"title"), 1);
        assert_eq!(style_level(b"heading 7"), 0);
        assert_eq!(style_level(b"normal"), 0);
        assert_eq!(hex(b'f'), Some(15));
        assert_eq!(hex(b'g'), None);
    }
}
