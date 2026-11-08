//! A message (RFC 5322 and MIME) as text for the screen, for the native
//! plugin: the interface-6 `render` made fallible and bounded. The text part
//! (plain, else HTML made text) is decoded to UTF-8 and the attachments are
//! named. The message is read from storage a piece at a time and the text
//! written to storage, so a message of any size takes the same little memory;
//! what does not fit the limit is cut, and said.
//!
//! What differs from interface 6, all of it where that one would have
//! panicked, aborted or grown without end:
//!
//! - a refused allocation is [`Fail::NoMemory`], never an abort;
//! - the HTML reader is made with [`HtmlText::with_limit`] for the room the
//!   text has left, so a mail of HTML that makes more text than fits never
//!   builds it (interface 6 made all of it and cut afterwards);
//! - no slicing that could panic.
use super::codec::{self, Base64, Charset, Decoder};
use super::fail::Fail;
use super::header::{self, ContentType, Headers};
use super::html::HtmlText;
use super::mem::{push, push_bytes, push_char, push_str, string_with, vec_with};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;

/// The most of a header block that is read (a longer one is cut).
const MAX_HEADERS: usize = 16 * 1024;
/// The longest line taken whole; a longer one comes as several.
const MAX_LINE: usize = 8 * 1024;
/// Parts inside parts, at most.
const MAX_DEPTH: usize = 8;
/// The most parts looked at.
const MAX_PARTS: usize = 256;
/// The most attachments named.
const MAX_ATTACHMENTS: usize = 32;
/// The pieces a message is read in.
const CHUNK: usize = 1024;

/// What the headers and the structure of a message say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Rendered {
    /// The senders.
    pub from: Vec<header::Address>,
    /// The recipients.
    pub to: Vec<header::Address>,
    /// The copies.
    pub cc: Vec<header::Address>,
    /// Where a reply goes.
    pub reply_to: Vec<header::Address>,
    /// The subject, cut to [`header::MAX_SUBJECT`].
    pub subject: String,
    /// Seconds since 1970, where the Date header could be read.
    pub date: Option<i64>,
    /// The message's id.
    pub message_id: String,
    /// The ids it refers to (the last that fit).
    pub references: String,
    /// `name (type)` of each attachment.
    pub attachments: Vec<String>,
    /// Whether the text was cut at the limit.
    pub truncated: bool,
    /// Bytes of text written.
    pub text_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Identity,
    Qp,
    Base64,
}

#[derive(Debug)]
struct Frame {
    /// `--boundary`
    boundary: Vec<u8>,
    alternative: bool,
    plain_seen: bool,
}

#[derive(Debug)]
struct Body {
    html: Option<HtmlText>,
    encoding: Encoding,
    base64: Base64,
    decoder: Decoder,
}

#[derive(Debug)]
enum Mode {
    /// The headers of a part, as far as they came.
    Headers(Vec<u8>),
    /// Before the first boundary of a multipart, and after the last.
    Skip,
    Body(Body),
}

#[derive(Debug, Clone, Copy)]
enum Boundary {
    Part,
    End,
}

/// The walk through a message, line by line.
#[derive(Debug)]
pub struct Walker {
    mode: Mode,
    frames: Vec<Frame>,
    top: bool,
    parts: usize,
    limit: usize,
    /// The text so far, not yet taken.
    text: String,
    written: usize,
    /// Whether text of a part came already (a second is set apart).
    any: bool,
    info: Rendered,
}

impl Walker {
    /// A walk that makes at most `limit` bytes of text.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            mode: Mode::Headers(Vec::new()),
            frames: Vec::new(),
            top: true,
            parts: 0,
            limit,
            text: String::new(),
            written: 0,
            any: false,
            info: Rendered::default(),
        }
    }

    /// What the headers and the parts said so far.
    #[must_use]
    pub fn info(&self) -> &Rendered {
        &self.info
    }

    /// Bytes of text made so far.
    #[must_use]
    pub fn written(&self) -> usize {
        self.written
    }

    /// The text made since the last call.
    pub fn take_text(&mut self) -> String {
        core::mem::take(&mut self.text)
    }

    /// Whether the text reached the limit: the rest is not needed.
    #[must_use]
    pub fn full(&self) -> bool {
        self.info.truncated
    }

    /// One line of the message, without its line break.
    ///
    /// # Errors
    /// [`Fail::NoMemory`]; the walk is then in no state to go on.
    pub fn line(&mut self, line: &[u8]) -> Result<(), Fail> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some((frame, kind)) = self.boundary_of(line) {
            self.end_part()?;
            match kind {
                Boundary::Part => {
                    self.frames.truncate(frame + 1);
                    self.mode = Mode::Headers(Vec::new());
                }
                Boundary::End => {
                    self.frames.truncate(frame);
                    self.mode = Mode::Skip;
                }
            }
            return Ok(());
        }
        match &mut self.mode {
            Mode::Skip => {}
            Mode::Headers(raw) => {
                if line.is_empty() {
                    let raw = core::mem::take(raw);
                    self.begin_part(&raw)?;
                } else if raw.len() < MAX_HEADERS {
                    push_bytes(raw, line)?;
                    push(raw, b'\n')?;
                }
            }
            Mode::Body(_) => self.body_line(line)?,
        }
        Ok(())
    }

    /// The message ends: what is open is closed.
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn finish(&mut self) -> Result<(), Fail> {
        // A message of only headers (no empty line after them).
        if let Mode::Headers(raw) = &mut self.mode {
            let raw = core::mem::take(raw);
            if !raw.is_empty() {
                self.begin_part(&raw)?;
            }
        }
        self.end_part()?;
        self.mode = Mode::Skip;
        Ok(())
    }

    /// Which multipart's boundary `line` is (the innermost that has it),
    /// and whether it starts a part or ends the multipart.
    fn boundary_of(&self, line: &[u8]) -> Option<(usize, Boundary)> {
        if !line.starts_with(b"--") {
            return None;
        }
        let (index, frame) = self
            .frames
            .iter()
            .enumerate()
            .rev()
            .find(|(_, f)| line.starts_with(&f.boundary))?;
        let rest = line.get(frame.boundary.len()..)?;
        if rest.starts_with(b"--") {
            Some((index, Boundary::End))
        } else if rest.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r')) {
            Some((index, Boundary::Part))
        } else {
            None
        }
    }

    /// The headers of a part are complete: what the part is.
    fn begin_part(&mut self, raw: &[u8]) -> Result<(), Fail> {
        let headers = Headers::parse(raw)?;
        let top = core::mem::replace(&mut self.top, false);
        if top {
            self.top_headers(&headers)?;
        }
        self.parts += 1;
        if self.parts > MAX_PARTS {
            self.mode = Mode::Skip;
            return Ok(());
        }
        let kind = ContentType::parse(headers.get("content-type"))?;
        if kind.is_multipart() {
            let boundary = kind.param("boundary").filter(|b| !b.is_empty());
            match boundary {
                Some(boundary) if self.frames.len() < MAX_DEPTH => {
                    let mut line = vec_with(boundary.len() + 2)?;
                    push_bytes(&mut line, b"--")?;
                    push_bytes(&mut line, boundary.as_bytes())?;
                    push(
                        &mut self.frames,
                        Frame {
                            boundary: line,
                            alternative: kind.kind == "multipart/alternative",
                            plain_seen: false,
                        },
                    )?;
                    self.mode = Mode::Skip;
                }
                _ => self.mode = Mode::Skip,
            }
            return Ok(());
        }
        let disposition = headers.get("content-disposition");
        let attached = disposition.is_some_and(|d| {
            d.get(.."attachment".len())
                .is_some_and(|head| head.eq_ignore_ascii_case("attachment"))
        });
        let is_text = kind.kind == "text/plain" || kind.kind == "text/html";
        if !is_text || attached {
            self.attachment(&kind, disposition)?;
            self.mode = Mode::Skip;
            return Ok(());
        }
        let html = kind.kind == "text/html";
        // Of a pair of the same text, the plain one is read.
        if let Some(frame) = self.frames.last_mut()
            && frame.alternative
        {
            if html && frame.plain_seen {
                self.mode = Mode::Skip;
                return Ok(());
            }
            if !html {
                frame.plain_seen = true;
            }
        }
        let encoding = match headers.get("content-transfer-encoding").map(str::trim) {
            Some(e) if e.eq_ignore_ascii_case("quoted-printable") => Encoding::Qp,
            Some(e) if e.eq_ignore_ascii_case("base64") => Encoding::Base64,
            _ => Encoding::Identity,
        };
        let charset = kind
            .param("charset")
            .and_then(Charset::of)
            .unwrap_or(Charset::Utf8);
        if self.any {
            self.emit("\n\n")?;
        }
        self.any = true;
        let room = self.limit.saturating_sub(self.written);
        self.mode = Mode::Body(Body {
            html: html.then(|| HtmlText::with_limit(room)),
            encoding,
            base64: Base64::default(),
            decoder: Decoder::new(charset),
        });
        Ok(())
    }

    fn top_headers(&mut self, headers: &Headers) -> Result<(), Fail> {
        let info = &mut self.info;
        let list = |name: &str| header::addresses(headers.get(name).unwrap_or(""));
        info.from = list("from")?;
        info.to = list("to")?;
        info.cc = list("cc")?;
        info.reply_to = list("reply-to")?;
        info.subject = header::capped(
            headers.get("subject").unwrap_or("").trim(),
            header::MAX_SUBJECT,
        )?;
        info.date = headers.get("date").and_then(header::date_seconds);
        info.message_id =
            header::capped(headers.get("message-id").unwrap_or("").trim(), header::MAX_ID)?;
        info.references = header::last_ids(
            headers.get("references").unwrap_or(""),
            header::MAX_REFERENCES,
        )?;
        Ok(())
    }

    fn attachment(&mut self, kind: &ContentType, disposition: Option<&str>) -> Result<(), Fail> {
        let from_disposition = match disposition {
            Some(d) => {
                let params = d.split_once(';').map_or("", |(_, p)| p);
                let mut value = string_with(4 + params.len())?;
                push_str(&mut value, "x/x;")?;
                push_str(&mut value, params)?;
                Some(ContentType::parse(Some(&value))?)
            }
            None => None,
        };
        let name = from_disposition
            .as_ref()
            .and_then(|d| d.param("filename"))
            .or_else(|| kind.param("name"))
            .unwrap_or("");
        let mut label = string_with(name.len() + kind.kind.len() + 16)?;
        push_str(&mut label, if name.is_empty() { "(no name)" } else { name })?;
        push_str(&mut label, " (")?;
        push_str(&mut label, &kind.kind)?;
        push_char(&mut label, ')')?;
        if self.info.attachments.len() < MAX_ATTACHMENTS {
            let label = header::capped(&label, header::MAX_LABEL)?;
            push(&mut self.info.attachments, label)?;
        }
        Ok(())
    }

    fn body_line(&mut self, line: &[u8]) -> Result<(), Fail> {
        let Mode::Body(body) = &mut self.mode else {
            return Ok(());
        };
        let mut out = String::new();
        let mut cut = false;
        match body.encoding {
            Encoding::Identity => {
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                body.decoder.push(line, &mut out)?;
                body.decoder.push(b"\n", &mut out)?;
            }
            Encoding::Qp => {
                let mut with_break = vec_with(line.len() + 1)?;
                push_bytes(&mut with_break, line.strip_suffix(b"\r").unwrap_or(line))?;
                push(&mut with_break, b'\n')?;
                let bytes = codec::decode_qp(&with_break, false)?;
                body.decoder.push(&bytes, &mut out)?;
            }
            Encoding::Base64 => {
                let mut bytes = Vec::new();
                body.base64.push(line, &mut bytes)?;
                body.decoder.push(&bytes, &mut out)?;
            }
        }
        if let Some(html) = &mut body.html {
            let mut text = String::new();
            html.push(&out, &mut text)?;
            out = text;
            cut = html.is_cut();
        }
        self.emit(&out)?;
        // After the emit: it ignores text once the walk is truncated.
        self.info.truncated |= cut;
        Ok(())
    }

    /// A part ends: what the decoders held back comes out.
    fn end_part(&mut self) -> Result<(), Fail> {
        let Mode::Body(body) = &mut self.mode else {
            return Ok(());
        };
        let mut out = String::new();
        let mut cut = false;
        if body.encoding == Encoding::Base64 {
            let mut bytes = Vec::new();
            body.base64.finish(&mut bytes)?;
            body.decoder.push(&bytes, &mut out)?;
        }
        body.decoder.finish(&mut out)?;
        if let Some(html) = &mut body.html {
            let mut text = String::new();
            html.push(&out, &mut text)?;
            html.finish(&mut text)?;
            out = text;
            cut = html.is_cut();
        }
        self.mode = Mode::Skip;
        self.emit(&out)?;
        self.info.truncated |= cut;
        Ok(())
    }

    /// Adds `text`, up to the limit.
    fn emit(&mut self, text: &str) -> Result<(), Fail> {
        if self.info.truncated || text.is_empty() {
            return Ok(());
        }
        let room = self.limit.saturating_sub(self.written);
        if text.len() <= room {
            push_str(&mut self.text, text)?;
            self.written += text.len();
            return Ok(());
        }
        let cut = header::cut(text, room);
        push_str(&mut self.text, cut)?;
        self.written += cut.len();
        self.info.truncated = true;
        Ok(())
    }
}

/// The message in `source` as text in `out` (replaced), at most `limit`
/// bytes of it, and what its headers say. The message is read and the text
/// written in pieces.
///
/// # Errors
/// [`Fail::Storage`] where the card says no, [`Fail::NoMemory`].
pub fn render(
    storage: &dyn Storage,
    source: &str,
    out: &str,
    limit: usize,
) -> Result<Rendered, Fail> {
    let mut walker = Walker::new(limit);
    storage.write(out, b"")?;
    let mut offset = 0u64;
    let mut line: Vec<u8> = Vec::new();
    let mut buffer = [0u8; CHUNK];
    loop {
        let n = storage.read_at(source, offset, &mut buffer)?;
        if n == 0 {
            break;
        }
        offset += n as u64;
        for &byte in buffer.get(..n).unwrap_or(&[]) {
            if byte == b'\n' {
                walker.line(&line)?;
                line.clear();
            } else if line.len() < MAX_LINE {
                push(&mut line, byte)?;
            } else {
                walker.line(&line)?;
                line.clear();
                push(&mut line, byte)?;
            }
        }
        flush(storage, out, &mut walker)?;
        if walker.full() {
            break;
        }
    }
    if !line.is_empty() {
        walker.line(&line)?;
    }
    walker.finish()?;
    flush(storage, out, &mut walker)?;
    let written = walker.written();
    let mut info = walker.info;
    info.text_len = written;
    Ok(info)
}

fn flush(storage: &dyn Storage, out: &str, walker: &mut Walker) -> Result<(), Fail> {
    let text = walker.take_text();
    if !text.is_empty() {
        storage.append(out, text.as_bytes())?;
    }
    Ok(())
}
