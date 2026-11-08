//! A simple HTML mail as text, for the native plugin: the interface-6
//! `html` made fallible and bounded. Tags go, blocks and line breaks become
//! line breaks, list items get a bullet, `<script>`, `<style>` and `<head>`
//! go with what is in them, entities are text. It comes in pieces (a tag or
//! an entity may be cut anywhere) and takes any bytes.
//!
//! What differs from interface 6, all of it where that one would have
//! panicked, aborted or lost the mail:
//!
//! - a refused allocation is [`Error::NoMemory`], never an abort;
//! - text after the limit ([`MAX_TEXT`], or [`HtmlText::with_limit`]) is
//!   dropped, so a mail from anyone cannot grow the text without end
//!   ([`HtmlText::is_cut`] tells);
//! - a non-ASCII character inside `<style>` or `<script>` can no longer cut
//!   the watch for the end tag in the middle of a character (a panic);
//! - a comment longer than a tag (1,024 bytes) ends at its `-->`, where
//!   interface 6 never found the end and dropped the rest of the mail.
use super::mem::{Error, push_char, push_str};
use alloc::string::String;

/// How much text a mail makes at most (bytes); the rest is dropped.
pub const MAX_TEXT: usize = 128 * 1024;

/// The longest tag kept (bytes); what follows is dropped from the tag.
const TAG: usize = 1024;

/// The longest entity name (`#x1F600`).
const ENTITY: usize = 10;

/// The longest end tag watched for (`</script>`).
const END: usize = 9;

/// The state of the text made so far.
#[derive(Debug, Clone)]
pub struct HtmlText {
    state: State,
    tag: String,
    /// The last two characters of the tag were `-`: a `>` ends a comment.
    dashes: u8,
    entity: [u8; ENTITY],
    entity_len: usize,
    /// The end tag that closes what is skipped (`</style>`).
    skipping: Option<&'static str>,
    /// The last bytes seen while skipping, lower case; a character that is
    /// no ASCII is a `0`, which matches nothing.
    tail: [u8; END],
    /// Characters of the text made since the last line break, and
    /// whether the last one was white space (it folds into one space).
    column: usize,
    space: bool,
    breaks: u8,
    /// Bytes of text made, the limit, and whether text was dropped.
    made: usize,
    limit: usize,
    cut: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    Tag,
    Entity,
}

/// Elements whose end or start is a line break.
const BLOCKS: [&str; 22] = [
    "p",
    "div",
    "br",
    "tr",
    "li",
    "ul",
    "ol",
    "table",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "hr",
    "section",
    "article",
    "header",
    "footer",
    "dl",
];

impl Default for HtmlText {
    fn default() -> Self {
        Self::with_limit(MAX_TEXT)
    }
}

impl HtmlText {
    /// A reader that makes at most [`MAX_TEXT`] bytes of text.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A reader that makes at most `limit` bytes of text.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            state: State::Text,
            tag: String::new(),
            dashes: 0,
            entity: [0; ENTITY],
            entity_len: 0,
            skipping: None,
            tail: [0; END],
            column: 0,
            space: false,
            breaks: 0,
            made: 0,
            limit,
            cut: false,
        }
    }

    /// Whether text was dropped for the limit.
    #[must_use]
    pub fn is_cut(&self) -> bool {
        self.cut
    }

    /// Appends the text of `html` to `out`.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the reader is then in no state to go on.
    pub fn push(&mut self, html: &str, out: &mut String) -> Result<(), Error> {
        for c in html.chars() {
            self.char(c, out)?;
        }
        Ok(())
    }

    /// Ends the text (an open tag is dropped, an open entity is text).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn finish(&mut self, out: &mut String) -> Result<(), Error> {
        if self.state == State::Entity {
            self.entity_as_text(out)?;
        }
        self.state = State::Text;
        Ok(())
    }

    /// `&` and the name read so far, as text.
    fn entity_as_text(&mut self, out: &mut String) -> Result<(), Error> {
        let name = self.entity;
        let len = core::mem::take(&mut self.entity_len);
        self.text('&', out)?;
        for byte in name.iter().take(len) {
            self.text(char::from(*byte), out)?;
        }
        Ok(())
    }

    fn char(&mut self, c: char, out: &mut String) -> Result<(), Error> {
        match self.state {
            State::Tag => self.in_tag(c, out),
            State::Entity => self.in_entity(c, out),
            State::Text => {
                if self.skipping.is_some() {
                    self.skip(c);
                } else if c == '<' {
                    self.state = State::Tag;
                    self.tag.clear();
                    self.dashes = 0;
                } else if c == '&' {
                    self.state = State::Entity;
                    self.entity_len = 0;
                } else {
                    self.text(c, out)?;
                }
                Ok(())
            }
        }
    }

    fn in_tag(&mut self, c: char, out: &mut String) -> Result<(), Error> {
        // `<!-- … -->` may hold a `>`; it ends at the `>` after two `-`.
        let ends_comment = c == '>' && self.dashes >= 2;
        self.dashes = if c == '-' {
            self.dashes.saturating_add(1).min(2)
        } else {
            0
        };
        if self.tag.len() < TAG {
            push_char(&mut self.tag, c)?;
        }
        if self.tag.starts_with("!--") && !ends_comment {
            return Ok(());
        }
        if c == '>' {
            let tag = core::mem::take(&mut self.tag);
            self.state = State::Text;
            let done = self.tag_done(&tag, out);
            // The room of the tag is kept for the next one.
            self.tag = tag;
            self.tag.clear();
            return done;
        }
        Ok(())
    }

    fn in_entity(&mut self, c: char, out: &mut String) -> Result<(), Error> {
        if c == ';' {
            self.state = State::Text;
            let len = core::mem::take(&mut self.entity_len);
            let name = self.entity.get(..len).unwrap_or(&[]);
            let decoded = core::str::from_utf8(name).ok().and_then(entity_char);
            match decoded {
                Some(decoded) => self.text(decoded, out),
                None => {
                    self.entity_len = len;
                    self.entity_as_text(out)?;
                    self.text(';', out)
                }
            }
        } else if (c.is_ascii_alphanumeric() || c == '#') && self.entity_len < ENTITY {
            if let (Some(slot), Ok(byte)) = (self.entity.get_mut(self.entity_len), u8::try_from(c))
            {
                *slot = byte;
            }
            self.entity_len += 1;
            Ok(())
        } else {
            // Not an entity: the text it was.
            self.state = State::Text;
            self.entity_as_text(out)?;
            self.char(c, out)
        }
    }

    /// Inside `<style>`/`<script>`/`<head>`: watches for the end tag.
    fn skip(&mut self, c: char) {
        let Some(end) = self.skipping else {
            return;
        };
        self.tail.rotate_left(1);
        let byte = if c.is_ascii() {
            u8::try_from(c.to_ascii_lowercase()).unwrap_or(0)
        } else {
            0
        };
        if let Some(last) = self.tail.last_mut() {
            *last = byte;
        }
        let from = END.saturating_sub(end.len());
        if self.tail.get(from..) == Some(end.as_bytes()) {
            self.skipping = None;
            self.tail = [0; END];
        }
    }

    fn tag_done(&mut self, tag: &str, out: &mut String) -> Result<(), Error> {
        let body = tag.trim_end_matches('>').trim();
        let (closing, body) = match body.strip_prefix('/') {
            Some(rest) => (true, rest),
            None => (false, body),
        };
        let length = body.bytes().take_while(u8::is_ascii_alphanumeric).count();
        let name = body.get(..length).unwrap_or("");
        let is = |other: &str| name.eq_ignore_ascii_case(other);
        if !closing {
            let end = if is("script") {
                Some("</script>")
            } else if is("style") {
                Some("</style>")
            } else if is("head") {
                Some("</head>")
            } else {
                None
            };
            if end.is_some() && !body.ends_with('/') {
                self.skipping = end;
                self.tail = [0; END];
                return Ok(());
            }
        }
        if is("li") && !closing {
            self.line_break(out, 1)?;
            return self.raw("\u{2022} ", out);
        }
        if BLOCKS.iter().any(|block| is(block)) {
            let lines = if is("br") || is("li") || is("tr") {
                1
            } else {
                2
            };
            self.line_break(out, lines)?;
        }
        Ok(())
    }

    /// Whether `bytes` more still fit the limit; if not, the text is cut.
    fn room(&mut self, bytes: usize) -> bool {
        match self.made.checked_add(bytes) {
            Some(total) if total <= self.limit => {
                self.made = total;
                true
            }
            _ => {
                self.cut = true;
                false
            }
        }
    }

    /// A line break of `lines` (at most two empty lines in a row come out).
    fn line_break(&mut self, out: &mut String, lines: u8) -> Result<(), Error> {
        if out.is_empty() && self.column == 0 {
            return Ok(());
        }
        let have = self.breaks;
        let wanted = lines.min(2);
        for _ in have..wanted {
            if self.room(1) {
                push_char(out, '\n')?;
            }
        }
        self.breaks = self.breaks.max(wanted);
        self.column = 0;
        self.space = false;
        Ok(())
    }

    fn raw(&mut self, text: &str, out: &mut String) -> Result<(), Error> {
        if self.room(text.len()) {
            push_str(out, text)?;
        }
        self.column = self.column.saturating_add(text.chars().count());
        self.breaks = 0;
        self.space = false;
        Ok(())
    }

    /// A character of the text: white space folds into one space.
    fn text(&mut self, c: char, out: &mut String) -> Result<(), Error> {
        if c == '\u{a0}' {
            return self.raw(" ", out);
        }
        if c.is_whitespace() || c == '\u{200b}' {
            if !self.space && self.column > 0 {
                if self.room(1) {
                    push_char(out, ' ')?;
                }
                self.column = self.column.saturating_add(1);
                self.space = true;
            }
            return Ok(());
        }
        if c.is_control() {
            return Ok(());
        }
        if self.room(c.len_utf8()) {
            push_char(out, c)?;
        }
        self.column = self.column.saturating_add(1);
        self.space = false;
        self.breaks = 0;
        Ok(())
    }
}

/// The character of an entity's name (`amp`, `#39`, `#x1F600`).
fn entity_char(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let value = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(value).filter(|c| *c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "bull" => '\u{2022}',
        "euro" => '\u{20ac}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "auml" => '\u{e4}',
        "ouml" => '\u{f6}',
        "uuml" => '\u{fc}',
        "Auml" => '\u{c4}',
        "Ouml" => '\u{d6}',
        "Uuml" => '\u{dc}',
        "szlig" => '\u{df}',
        "eacute" => '\u{e9}',
        "egrave" => '\u{e8}',
        "agrave" => '\u{e0}',
        "ccedil" => '\u{e7}',
        _ => return None,
    })
}

/// The text of a whole `html`, at most [`MAX_TEXT`] bytes.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn to_text(html: &str) -> Result<String, Error> {
    let mut parser = HtmlText::new();
    let mut out = String::new();
    parser.push(html, &mut out)?;
    parser.finish(&mut out)?;
    out.truncate(out.trim_end().len());
    Ok(out)
}
