//! The text of a Mobipocket book: HTML of a small, loose kind (`<p>`,
//! `<b>`, `<i>`, `<br/>`, `<img recindex=…/>`, `<a filepos=…>`, no quotes
//! around some values, tags in capitals), characters in UTF-8 or
//! Windows-1252, written as the text records' bytes one after the other.
//!
//! [`Converter`] reads those bytes once, in the order they come, and
//! writes blocks (a paragraph, a heading, a picture) in the form
//! [`crate::cache`] keeps and reads: so that a book of any length is
//! converted with the memory of one block, and any block is read again
//! on its own.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::DocumentError;

use crate::cache::{Block, Kind, Output};
use crate::mem::{self, Refused};

/// How the characters of the text are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Utf8,
    Windows1252,
}

const CP1252: [char; 32] = [
    '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}', 'Ž',
    '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{fffd}',
    'ž', 'Ÿ',
];

fn cp1252(byte: u8) -> char {
    match byte {
        0x80..=0x9f => CP1252[usize::from(byte - 0x80)],
        b => char::from(b),
    }
}

/// `bytes` as text, onto `out` (for the strings of the headers).
pub(crate) fn decode_into(
    bytes: &[u8],
    encoding: Encoding,
    out: &mut String,
) -> Result<(), Refused> {
    match encoding {
        Encoding::Utf8 => {
            for chunk in bytes.utf8_chunks() {
                mem::push_str(out, chunk.valid())?;
                if !chunk.invalid().is_empty() {
                    mem::push_char(out, char::REPLACEMENT_CHARACTER)?;
                }
            }
        }
        Encoding::Windows1252 => {
            for &byte in bytes {
                mem::push_char(out, cp1252(byte))?;
            }
        }
    }
    Ok(())
}

/// A piece of text is cut at the first space after this many bytes.
const SPLIT: usize = 3000;
/// A paragraph with no space in it is cut at this many bytes.
const HARD: usize = 8000;
/// A tag longer than this is read, but only its start is kept.
const TAG: usize = 1024;
/// Most checkpoints of a block's offsets kept.
const MOST_CHECKS: usize = 96;
/// Longest alternative text of a picture kept.
const ALT: usize = 200;

const BOLD: u16 = 1;
const ITALIC: u16 = 2;
const UNDERLINE: u16 = 1 << 10;
const STRIKE: u16 = 1 << 11;
const SUPERSCRIPT: u16 = 1 << 8;
const SUBSCRIPT: u16 = 1 << 9;

/// What the converter asks of the book about pictures.
pub(crate) trait Pictures {
    /// The size a picture is decoded to, if the record `index` (counted
    /// from the first picture, 1 for the first) holds one that can be.
    ///
    /// # Errors
    /// A card or memory that fails: the conversion stops with it.
    fn size(&mut self, index: u32) -> Result<Option<(u32, u32)>, DocumentError>;
}

/// Whether a tag ends the block of text before it.
fn is_boundary(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "li"
            | "ul"
            | "ol"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "tr"
            | "pre"
            | "hr"
            | "body"
            | "html"
            | "section"
            | "center"
            | "mbp:pagebreak"
    )
}

/// The value of attribute `name` in the tag `tag`, loosely: with or
/// without quotes.
fn attribute<'t>(tag: &'t [u8], name: &str) -> Option<&'t [u8]> {
    let mut i = 0;
    while i < tag.len() {
        // A name starts after white space.
        if i > 0 && !tag[i - 1].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let rest = &tag[i..];
        if rest.len() > name.len()
            && rest[..name.len()].eq_ignore_ascii_case(name.as_bytes())
            && rest[name.len()..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'=')
        {
            let after = &rest[name.len()..];
            let eq = after.iter().position(|&b| b == b'=')?;
            let value = after[eq + 1..].trim_ascii_start();
            return Some(match value.first() {
                Some(&q @ (b'"' | b'\'')) => {
                    let inner = &value[1..];
                    &inner[..inner.iter().position(|&b| b == q).unwrap_or(inner.len())]
                }
                _ => {
                    let end = value
                        .iter()
                        .position(u8::is_ascii_whitespace)
                        .unwrap_or(value.len());
                    &value[..end]
                }
            });
        }
        i += 1;
    }
    None
}

fn number(bytes: &[u8]) -> Option<u32> {
    let text = core::str::from_utf8(bytes).ok()?;
    text.trim().parse().ok()
}

/// The character an entity (`amp` of `&amp;`, `#233`, `#xe9`) stands for.
fn entity(name: &[u8]) -> Option<char> {
    let name = core::str::from_utf8(name).ok()?;
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|&c| c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "shy" => '\u{ad}',
        "ndash" => '–',
        "mdash" => '—',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "hellip" => '…',
        "copy" => '©',
        "eacute" => 'é',
        "egrave" => 'è',
        "agrave" => 'à',
        "auml" => 'ä',
        "ouml" => 'ö',
        "uuml" => 'ü',
        "szlig" => 'ß',
        _ => return None,
    })
}

/// Reads the text and writes its blocks.
pub(crate) struct Converter {
    encoding: Encoding,
    pub(crate) out: Output,
    /// The source offset of the next byte.
    pos: u32,
    // The tag being read.
    in_tag: bool,
    quote: Option<u8>,
    tag: Vec<u8>,
    tag_start: u32,
    // The entity being read, and where it began.
    entity: Option<(Vec<u8>, u32)>,
    // A character of several bytes being read (UTF-8).
    character: Vec<u8>,
    character_at: u32,
    /// Inside `<head>`, `<style>` or `<script>`: not text.
    skipping: bool,
    // The block being made.
    block_start: u32,
    kind: Kind,
    align: paperpaw_sdk::document::Align,
    centred: u32,
    text: String,
    runs: Vec<(u16, u16, u16)>,
    checks: Vec<(u16, u32)>,
    shift: i64,
    continued: bool,
    bullets: bool,
    // Styles.
    bold: u32,
    italic: u32,
    underline: u32,
    strike: u32,
    sup: u32,
    sub: u32,
    run_from: usize,
    run_style: u16,
    // An anchor with a position open, and where in the text it began.
    link: Option<(u32, usize)>,
}

impl Converter {
    pub(crate) fn new(encoding: Encoding, out: Output) -> Self {
        Self {
            encoding,
            out,
            pos: 0,
            in_tag: false,
            quote: None,
            tag: Vec::new(),
            tag_start: 0,
            entity: None,
            character: Vec::new(),
            character_at: 0,
            skipping: false,
            block_start: 0,
            kind: Kind::Paragraph,
            align: paperpaw_sdk::document::Align::Auto,
            centred: 0,
            text: String::new(),
            runs: Vec::new(),
            checks: Vec::new(),
            shift: 0,
            continued: false,
            bullets: false,
            bold: 0,
            italic: 0,
            underline: 0,
            strike: 0,
            sup: 0,
            sub: 0,
            run_from: 0,
            run_style: 0,
            link: None,
        }
    }

    /// The bytes of the text from where the last call ended.
    pub(crate) fn feed(&mut self, bytes: &[u8], pictures: &mut dyn Pictures) {
        for &byte in bytes {
            if self.out.failed() {
                return;
            }
            self.byte(byte, pictures);
            self.pos = self.pos.wrapping_add(1);
        }
    }

    /// The end of the text: the last block is written.
    pub(crate) fn finish(&mut self) {
        self.close_entity();
        self.flush_character();
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
        if self.in_tag {
            if let Some(quote) = self.quote {
                if byte == quote {
                    self.quote = None;
                }
            } else if byte == b'>' {
                self.in_tag = false;
                self.end_tag(pictures);
                return;
            } else if byte == b'"' || byte == b'\'' {
                self.quote = Some(byte);
            }
            if self.tag.len() < TAG {
                let pushed = mem::push(&mut self.tag, byte);
                self.kept(pushed);
            }
            return;
        }
        if byte == b'<' {
            self.close_entity();
            self.flush_character();
            self.in_tag = true;
            self.quote = None;
            self.tag.clear();
            self.tag_start = self.pos;
            return;
        }
        if self.skipping {
            return;
        }
        if let Some((name, at)) = self.entity.as_mut() {
            if byte == b';' {
                let at = *at;
                let resolved = entity(name);
                let name = core::mem::take(name);
                self.entity = None;
                match resolved {
                    Some(c) => self.character(c, at),
                    None => {
                        self.character('&', at);
                        self.literal(&name, at);
                        self.character(';', at + 1 + name.len() as u32);
                    }
                }
                return;
            }
            if (byte.is_ascii_alphanumeric() || byte == b'#') && name.len() < 10 {
                if mem::push(name, byte).is_err() {
                    self.out.refuse();
                }
                return;
            }
            self.close_entity();
        }
        if byte == b'&' {
            self.flush_character();
            self.entity = Some((Vec::new(), self.pos));
            return;
        }
        match self.encoding {
            Encoding::Windows1252 => self.character(cp1252(byte), self.pos),
            Encoding::Utf8 => self.utf8(byte),
        }
    }

    /// An entity that did not end or is not known: its text as it is.
    fn close_entity(&mut self) {
        if let Some((name, at)) = self.entity.take() {
            self.character('&', at);
            self.literal(&name, at + 1);
        }
    }

    fn literal(&mut self, name: &[u8], at: u32) {
        for (i, &b) in name.iter().enumerate() {
            self.character(char::from(b), at + 1 + i as u32);
        }
    }

    fn utf8(&mut self, byte: u8) {
        if byte < 0x80 {
            self.flush_character();
            self.character(char::from(byte), self.pos);
            return;
        }
        if byte >= 0xc0 {
            self.flush_character();
            self.character_at = self.pos;
        } else if self.character.is_empty() {
            // A continuation byte with no start: not a character.
            self.character(char::REPLACEMENT_CHARACTER, self.pos);
            return;
        }
        let pushed = mem::push(&mut self.character, byte);
        if self.kept(pushed).is_none() {
            return;
        }
        let need = match self.character[0] {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            _ => 4,
        };
        if self.character.len() == need {
            self.flush_character();
        }
    }

    /// A character of several bytes that is not whole is a replacement.
    fn flush_character(&mut self) {
        if self.character.is_empty() {
            return;
        }
        let at = self.character_at;
        let c = core::str::from_utf8(&self.character)
            .ok()
            .and_then(|s| s.chars().next())
            .unwrap_or(char::REPLACEMENT_CHARACTER);
        self.character.clear();
        self.character(c, at);
    }

    /// A character of the text, which began at source offset `at`.
    fn character(&mut self, c: char, at: u32) {
        if self.skipping {
            return;
        }
        if c.is_ascii_whitespace() {
            let last = self.text.chars().next_back();
            if matches!(last, None | Some(' ' | '\n')) {
                return;
            }
            self.put(' ', at);
            if self.text.len() >= SPLIT {
                self.flush(at.wrapping_add(1), true);
            }
            return;
        }
        if c.is_control() {
            return;
        }
        self.put(c, at);
    }

    fn put(&mut self, c: char, at: u32) {
        if self.text.is_empty() {
            self.bullet_if_needed();
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
            self.flush(at.wrapping_add(1), true);
        }
    }

    fn bullet_if_needed(&mut self) {
        if self.bullets {
            self.bullets = false;
            let pushed = mem::push_str(&mut self.text, "• ");
            self.kept(pushed);
            self.shift = 0;
            self.checks.clear();
        }
    }

    fn style(&self) -> u16 {
        let mut style = 0;
        for (depth, bit) in [
            (self.bold, BOLD),
            (self.italic, ITALIC),
            (self.underline, UNDERLINE),
            (self.strike, STRIKE),
            (self.sup, SUPERSCRIPT),
            (self.sub, SUBSCRIPT),
        ] {
            if depth > 0 {
                style |= bit;
            }
        }
        style
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

    /// Where text index `index` of the block is in the source.
    fn source_at(&self, index: usize) -> u32 {
        let i = self
            .checks
            .partition_point(|&(text, _)| usize::from(text) <= index);
        match i.checked_sub(1).map(|i| self.checks[i]) {
            Some((text, source)) => source.wrapping_add((index - usize::from(text)) as u32),
            None => self.block_start.wrapping_add(index as u32),
        }
    }

    /// Ends the block being made at source offset `end`; the next begins
    /// there. `split` for a long paragraph cut in two.
    fn flush(&mut self, end: u32, split: bool) {
        self.close_run();
        self.close_link(true);
        let trimmed = self.text.trim_end_matches([' ', '\n']).len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() {
            self.runs
                .retain(|r| r.0 < r.1 && usize::from(r.1) <= trimmed);
            let level = match self.kind {
                Kind::Heading(level) => Some(level),
                _ => None,
            };
            if let Some(level) = level.filter(|&l| l <= 3) {
                self.out.toc(self.block_start, level - 1, &self.text);
            }
            self.out.block(&Block {
                kind: self.kind,
                align: self.align,
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
        self.runs.clear();
        self.checks.clear();
        self.text.clear();
        self.shift = 0;
        self.block_start = end;
        self.run_from = 0;
        if self.link.is_some() {
            self.link = self.link.map(|(filepos, _)| (filepos, 0));
        }
    }

    /// The anchor open ends here (or goes on in the next block).
    fn close_link(&mut self, going_on: bool) {
        let Some((filepos, from)) = self.link else {
            return;
        };
        if self.text.len() > from {
            let (start, end) = (self.source_at(from), self.source_at(self.text.len()));
            self.out.link(start, end, filepos);
        }
        self.link = if going_on { Some((filepos, 0)) } else { None };
    }

    fn end_tag(&mut self, pictures: &mut dyn Pictures) {
        let tag = core::mem::take(&mut self.tag);
        let start = self.tag_start;
        let next = self.pos.wrapping_add(1);
        let closing = tag.first() == Some(&b'/');
        let body = if closing { &tag[1..] } else { &tag[..] };
        let name_end = body
            .iter()
            .position(|b| b.is_ascii_whitespace() || *b == b'/')
            .unwrap_or(body.len());
        // Every tag the converter knows has a short name; a longer one is
        // none of them.
        let mut name = [0u8; 16];
        let name = match name.get_mut(..name_end) {
            Some(room) => {
                room.copy_from_slice(&body[..name_end]);
                room.make_ascii_lowercase();
                core::str::from_utf8(room).unwrap_or("")
            }
            None => "",
        };
        self.tag = tag;
        self.tag_named(name, closing, start, next, pictures);
        self.tag.clear();
    }

    fn tag_named(
        &mut self,
        name: &str,
        closing: bool,
        start: u32,
        next: u32,
        pictures: &mut dyn Pictures,
    ) {
        if matches!(name, "head" | "style" | "script" | "title") {
            self.skipping = !closing;
            return;
        }
        if self.skipping {
            return;
        }
        if is_boundary(name) {
            self.flush(start, false);
            self.block_start = start;
            if closing {
                self.kind = Kind::Paragraph;
                if name == "center" {
                    self.centred = self.centred.saturating_sub(1);
                }
                self.align = self.base_align();
                return;
            }
            self.kind = match name.as_bytes() {
                [b'h', level @ b'1'..=b'6'] => Kind::Heading(level - b'0'),
                _ => Kind::Paragraph,
            };
            if name == "center" {
                self.centred += 1;
            }
            self.align = self.base_align();
            if let Some(value) = attribute(&self.tag, "align") {
                self.align = match value.to_ascii_lowercase().as_slice() {
                    b"center" => paperpaw_sdk::document::Align::Center,
                    b"right" => paperpaw_sdk::document::Align::End,
                    b"left" => paperpaw_sdk::document::Align::Start,
                    b"justify" => paperpaw_sdk::document::Align::Justify,
                    _ => self.align,
                };
            }
            if name == "li" {
                self.bullets = true;
            }
            if name == "hr" {
                self.align = paperpaw_sdk::document::Align::Center;
                let pushed = mem::push_str(&mut self.text, "* * *");
                self.kept(pushed);
                self.flush(next, false);
                self.align = self.base_align();
            }
            return;
        }
        let depth = match name {
            "b" | "strong" => &mut self.bold,
            "i" | "em" | "cite" | "dfn" => &mut self.italic,
            "u" | "ins" => &mut self.underline,
            "s" | "strike" | "del" => &mut self.strike,
            "sup" => &mut self.sup,
            "sub" => &mut self.sub,
            "br" => {
                let trimmed = self.text.trim_end_matches(' ').len();
                self.text.truncate(trimmed);
                if !self.text.is_empty() && !self.text.ends_with('\n') {
                    let pushed = mem::push_char(&mut self.text, '\n');
                    self.kept(pushed);
                }
                return;
            }
            "a" => {
                if closing {
                    self.close_link(false);
                } else if let Some(filepos) = attribute(&self.tag, "filepos").and_then(number) {
                    self.close_link(false);
                    self.link = Some((filepos, self.text.len()));
                }
                return;
            }
            "img" | "image" => {
                self.picture(start, next, pictures);
                return;
            }
            _ => return,
        };
        *depth = if closing {
            depth.saturating_sub(1)
        } else {
            depth.saturating_add(1)
        };
        self.styled();
    }

    fn base_align(&self) -> paperpaw_sdk::document::Align {
        if self.centred > 0 {
            paperpaw_sdk::document::Align::Center
        } else {
            paperpaw_sdk::document::Align::Auto
        }
    }

    /// A picture: the text before it is a block, the picture another.
    fn picture(&mut self, start: u32, next: u32, pictures: &mut dyn Pictures) {
        let Some(index) = attribute(&self.tag, "recindex").and_then(number) else {
            return;
        };
        let size = match pictures.size(index) {
            Ok(size) => size,
            Err(error) => {
                self.out.note(error);
                return;
            }
        };
        let Some((width, height)) = size else {
            return;
        };
        let was_continued = self.continued;
        self.flush(start, true);
        self.block_start = start;
        let mut alt = String::new();
        if let Some(value) = attribute(&self.tag, "alt") {
            let mut raw = String::new();
            let decoded = decode_into(value, self.encoding, &mut raw);
            if self.kept(decoded).is_none() {
                return;
            }
            for word in raw.split_whitespace() {
                if alt.len() + word.len() + 1 > ALT {
                    break;
                }
                let added = if alt.is_empty() {
                    Ok(())
                } else {
                    mem::push_char(&mut alt, ' ')
                }
                .and_then(|()| mem::push_str(&mut alt, word));
                if self.kept(added).is_none() {
                    return;
                }
            }
        }
        self.out.block(&Block {
            kind: Kind::Image,
            align: paperpaw_sdk::document::Align::Center,
            continues: false,
            continued: false,
            start,
            end: next,
            text: &alt,
            runs: &[],
            checks: &[],
            image: Some((index, width, height)),
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
    fn attributes_with_and_without_quotes() {
        assert_eq!(
            attribute(b"a filepos=0000123 x", "filepos"),
            Some(&b"0000123"[..])
        );
        assert_eq!(
            attribute(b"IMG RecIndex = \"00007\" alt='A b'", "recindex"),
            Some(&b"00007"[..])
        );
        assert_eq!(attribute(b"img alt='A b' src=x", "alt"), Some(&b"A b"[..]));
        assert_eq!(attribute(b"img data-recindex=3", "recindex"), None);
        assert_eq!(attribute(b"p", "align"), None);
        assert_eq!(number(b"0000042"), Some(42));
        assert_eq!(number(b"4x"), None);
    }

    #[test]
    fn entities() {
        assert_eq!(entity(b"amp"), Some('&'));
        assert_eq!(entity(b"#233"), Some('é'));
        assert_eq!(entity(b"#xe9"), Some('é'));
        assert_eq!(entity(b"#0"), None);
        assert_eq!(entity(b"#99999999999"), None);
        assert_eq!(entity(b"unknown"), None);
    }

    #[test]
    fn windows_1252_text() {
        let mut out = String::new();
        decode_into(b"caf\xe9 \x93hi\x94 \x80", Encoding::Windows1252, &mut out).unwrap();
        assert_eq!(out, "café “hi” €");
        let mut out = String::new();
        decode_into("café".as_bytes(), Encoding::Utf8, &mut out).unwrap();
        assert_eq!(out, "café");
    }
}
