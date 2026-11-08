//! What the Readwise plugin decides without the host: it reads the Markdown
//! the reader writes for a book's notes, a line at a time, and writes the
//! JSON Readwise takes for each highlight. The same rules as the
//! interface-6 plugin (`lib.rs`), with fallible allocation. Sending, the
//! files, the token and the retries are the native plugin's (see
//! `PORTING-7.md`).
use alloc::string::String;
use paperpaw_sdk::native::Error;

/// Longest text and note of a highlight, title and author sent, in bytes
/// (Readwise takes no more characters).
pub const MAX_TEXT: usize = 8191;
pub const MAX_TITLE: usize = 511;
pub const MAX_AUTHOR: usize = 1024;

/// A highlight as the Markdown has it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Highlight {
    pub text: String,
    /// Where in the book, in percent.
    pub percent: u32,
    pub note: String,
}

/// Where a [`Parser`] is in the notes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The title and author, before the first heading of a part.
    #[default]
    Head,
    /// The highlights, under the first heading.
    Highlights,
    /// Later parts (bookmarks), left out.
    Rest,
}

/// Reads the Markdown the reader writes, a line at a time: `# Title`, the
/// author on the next line, then under the first `##` heading each highlight
/// as a quote (`> text`) ending in its place (`> 12 %`), followed by the note
/// on it, if any; later headings (bookmarks) are left out. Only the
/// highlight being read is held, cut to what Readwise takes.
#[derive(Debug, Default)]
pub struct Parser {
    part: Part,
    title: String,
    author: String,
    current: Option<Highlight>,
    quoting: bool,
}

impl Parser {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    /// Reads the next line (without its line break): the highlight it ends,
    /// if any.
    ///
    /// # Errors
    /// `NoMemory` when the line cannot be kept: the highlight it would have
    /// started is not started, and a title or author is left as far as it
    /// was read.
    pub fn line(&mut self, line: &str) -> Result<Option<Highlight>, Error> {
        match self.part {
            Part::Head => {
                if line.starts_with("## ") {
                    self.part = Part::Highlights;
                } else if let Some(title) = line.strip_prefix("# ") {
                    self.title.clear();
                    push_bounded(&mut self.title, title.trim(), MAX_TITLE)?;
                } else if !line.trim().is_empty()
                    && self.author.is_empty()
                    && !self.title.is_empty()
                {
                    push_bounded(&mut self.author, line.trim(), MAX_AUTHOR)?;
                }
                Ok(None)
            }
            Part::Highlights if line.starts_with("## ") => {
                let last = self.finish();
                self.part = Part::Rest;
                Ok(last)
            }
            Part::Highlights => self.highlight_line(line),
            Part::Rest => Ok(None),
        }
    }

    fn highlight_line(&mut self, line: &str) -> Result<Option<Highlight>, Error> {
        if let Some(quote) = line.strip_prefix('>') {
            let quote = quote.trim();
            // A quote after other lines starts the next highlight. It is
            // made first, so that one that cannot be kept ends nothing.
            let mut fresh = Highlight::default();
            let starting = !self.quoting;
            let h = if starting {
                &mut fresh
            } else {
                self.current.get_or_insert_with(Highlight::default)
            };
            if let Some(percent) = quote.strip_suffix('%').and_then(|p| p.trim().parse().ok()) {
                h.percent = percent;
            } else if !quote.is_empty() {
                push_line(&mut h.text, quote)?;
            }
            let ended = if starting {
                self.quoting = true;
                self.current.replace(fresh)
            } else {
                None
            };
            return Ok(ended.filter(|h| !h.text.is_empty()));
        }
        self.quoting = false;
        if let Some(h) = self.current.as_mut()
            && !line.trim().is_empty()
        {
            push_line(&mut h.note, line.trim())?;
        }
        Ok(None)
    }

    /// The end of the notes: the highlight read last, if any.
    pub fn finish(&mut self) -> Option<Highlight> {
        self.quoting = false;
        self.current.take().filter(|h| !h.text.is_empty())
    }

    /// The highlight `h` of these notes as Readwise takes it, in JSON.
    ///
    /// # Errors
    /// `NoMemory` when the text cannot be kept.
    pub fn json(&self, h: &Highlight) -> Result<String, Error> {
        let mut out = String::new();
        // The members, with room for the escapes of a usual text.
        let size = 160
            + h.text.len()
            + self.title.len()
            + self.author.len()
            + h.note.len()
            + (h.text.len() + h.note.len()) / 16;
        out.try_reserve(size).map_err(|_| Error::NoMemory)?;
        append(&mut out, "{\"text\":")?;
        quote(&mut out, &h.text)?;
        append(&mut out, ",\"title\":")?;
        quote(&mut out, &self.title)?;
        append(
            &mut out,
            ",\"source_type\":\"paperpaw\",\"category\":\"books\",\"location\":",
        )?;
        number(&mut out, h.percent)?;
        append(&mut out, ",\"location_type\":\"order\"")?;
        if !self.author.is_empty() {
            append(&mut out, ",\"author\":")?;
            quote(&mut out, &self.author)?;
        }
        if !h.note.is_empty() {
            append(&mut out, ",\"note\":")?;
            quote(&mut out, &h.note)?;
        }
        append(&mut out, "}")?;
        Ok(out)
    }
}

fn append(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

fn number(out: &mut String, value: u32) -> Result<(), Error> {
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    let mut rest = value;
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    append(out, core::str::from_utf8(&digits[at..]).unwrap_or("0"))
}

/// `text` as a JSON string literal, as `paperpaw_sdk::json::string` writes
/// it.
fn quote(out: &mut String, text: &str) -> Result<(), Error> {
    append(out, "\"")?;
    for c in text.chars() {
        match c {
            '"' => append(out, "\\\"")?,
            '\\' => append(out, "\\\\")?,
            '\n' => append(out, "\\n")?,
            '\r' => append(out, "\\r")?,
            '\t' => append(out, "\\t")?,
            c if (c as u32) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = c as usize;
                let digits = [b'\\', b'u', b'0', b'0', HEX[code >> 4], HEX[code & 15]];
                append(out, core::str::from_utf8(&digits).unwrap_or(""))?;
            }
            c => {
                out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
                out.push(c);
            }
        }
    }
    append(out, "\"")
}

/// Adds `text` to `field` on a line of its own, as much as fits in
/// [`MAX_TEXT`] bytes.
fn push_line(field: &mut String, text: &str) -> Result<(), Error> {
    let before = field.len();
    let mut end = text
        .len()
        .min(MAX_TEXT.saturating_sub(before + usize::from(before > 0)));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    // Nothing of it fits: the field stays as it is.
    if before > 0 && end == 0 {
        return Ok(());
    }
    field
        .try_reserve(usize::from(before > 0) + end)
        .map_err(|_| Error::NoMemory)?;
    if before > 0 {
        field.push('\n');
    }
    field.push_str(&text[..end]);
    Ok(())
}

/// Adds `text` to `field`, as much as fits in `max` bytes, whole characters
/// only.
fn push_bounded(field: &mut String, text: &str, max: usize) -> Result<(), Error> {
    let mut end = text.len().min(max.saturating_sub(field.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    field.try_reserve(end).map_err(|_| Error::NoMemory)?;
    field.push_str(&text[..end]);
    Ok(())
}
