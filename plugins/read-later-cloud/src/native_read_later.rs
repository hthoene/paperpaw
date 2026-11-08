//! What the Readwise Reader plugin decides without the host: which texts are
//! article ids, the start and end of a book made of an article, what
//! tells the accounts apart, the lists it keeps, when an article counts as
//! read, and the copying of an article's text out of Readwise's answer. The
//! same rules as the interface-6 plugin (`lib.rs`, `html.rs`, `answers.rs`),
//! with fallible allocation, and the answer's bytes given by the caller in
//! pieces instead of read from a storage. Fetching, the files and the set-up
//! screen are the native plugin's (see `PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

/// Reading progress, in millionths (the native event's), from which an
/// article counts as read where its pages are not counted yet: 95 %.
pub const READ_PPM: u32 = 950_000;
/// The end of an article's book; its text goes before.
pub const TAIL: &str = "</body></html>";
/// The most of an article taken, in bytes: more is left off.
pub const MAX_TEXT: usize = 1024 * 1024;
/// The key whose string is the article.
const KEY: &[u8] = b"\"html_content\"";

/// Whether `id` is an id as Readwise gives them (letters and digits, 26 of
/// them): it goes into addresses, lists and names of files.
pub fn is_id(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether a page turn is the article read to the end: on the last page, or
/// near the end where pages are not counted yet (`progress_ppm` as the
/// reading event gives it, from 0 to 1,000,000).
pub fn is_read(page: Option<u32>, pages: Option<u32>, progress_ppm: u32) -> bool {
    match (page, pages) {
        (Some(page), Some(pages)) => page >= pages,
        _ => progress_ppm >= READ_PPM,
    }
}

/// Where Reader keeps what is asked for, by the setting `from`: its
/// `location`.
pub fn location(from: &str) -> &'static str {
    if from == "read-later-cloud.from.inbox" {
        "new"
    } else {
        "later"
    }
}

/// Whose the articles fetched are: a number made of the token (the token
/// itself is kept with the secrets).
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn account(token: &str) -> Result<String, Error> {
    let hash = paperpaw_sdk::digest::fnv1a64(token);
    let mut out = String::new();
    out.try_reserve_exact(16).map_err(|_| Error::NoMemory)?;
    for shift in (0..16).rev() {
        let digit = ((hash >> (shift * 4)) & 15) as u32;
        out.push(char::from_digit(digit, 16).unwrap_or('0'));
    }
    Ok(out)
}

fn append(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

/// `text` as HTML text, appended to `out`.
fn escape_into(out: &mut String, text: &str) -> Result<(), Error> {
    for c in text.chars() {
        match c {
            '&' => append(out, "&amp;")?,
            '<' => append(out, "&lt;")?,
            '>' => append(out, "&gt;")?,
            _ => {
                out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
                out.push(c);
            }
        }
    }
    Ok(())
}

/// The start of an article's book: its title and author. Its text goes
/// after, then [`TAIL`].
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn head(title: &str, author: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(200 + 2 * title.len() + author.len())
        .map_err(|_| Error::NoMemory)?;
    append(
        &mut out,
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>",
    )?;
    escape_into(&mut out, title)?;
    append(&mut out, "</title></head><body><h1>")?;
    escape_into(&mut out, title)?;
    append(&mut out, "</h1>")?;
    if !author.is_empty() {
        append(&mut out, "<p>")?;
        escape_into(&mut out, author)?;
        append(&mut out, "</p>")?;
    }
    Ok(out)
}

/// An article fetched: its id in Reader and its book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    pub id: String,
    pub path: String,
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

fn push<T>(list: &mut Vec<T>, item: T) -> Result<(), Error> {
    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
    list.push(item);
    Ok(())
}

/// The articles fetched, kept as `id<TAB>path` lines (lines of another shape
/// are left out).
///
/// # Errors
/// `NoMemory` when the list cannot be kept.
pub fn articles(text: &str) -> Result<Vec<Article>, Error> {
    let mut list = Vec::new();
    for line in text.lines() {
        if let Some((id, path)) = line.split_once('\t') {
            push(
                &mut list,
                Article {
                    id: copy(id)?,
                    path: copy(path)?,
                },
            )?;
        }
    }
    Ok(list)
}

/// The list of articles as it is kept (`id<TAB>path` lines).
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn articles_text(list: &[Article]) -> Result<String, Error> {
    let size = list.iter().map(|a| a.id.len() + a.path.len() + 2).sum();
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for a in list {
        out.push_str(&a.id);
        out.push('\t');
        out.push_str(&a.path);
        out.push('\n');
    }
    Ok(out)
}

/// The ids of articles read to the end, to archive: one a line.
///
/// # Errors
/// `NoMemory` when the list cannot be kept.
pub fn finished(text: &str) -> Result<Vec<String>, Error> {
    let mut ids = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        push(&mut ids, copy(line)?)?;
    }
    Ok(ids)
}

/// How copying the article ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copied {
    /// The article's string was read to its end (or to [`MAX_TEXT`]).
    Whole,
    /// The answer has no text for it (`null`, or no such key).
    Missing,
    /// The answer ends inside the string: cut off.
    Cut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Looking for the key: how many of its bytes match so far.
    Seek(usize),
    /// After the key, before its colon.
    Colon,
    /// After the colon, before the value.
    Value,
    /// Inside the string.
    Text,
    /// After a backslash.
    Escape,
    /// In a `\u` escape: digits so far and their value.
    Unicode(u8, u32),
    /// The string ended.
    End,
}

/// Takes the article's text out of Readwise's answer, which comes in pieces:
/// the answer is a JSON file with the article's HTML as one long string
/// (`html_content`), found by its key, unescaped as it goes. What is taken
/// is held until the caller writes it to the book and clears it, so an
/// article of any length needs the same few kilobytes.
#[derive(Debug)]
pub struct Copier {
    state: State,
    /// The first half of a surrogate pair, waiting for its second.
    high: Option<u32>,
    out: Vec<u8>,
    taken: usize,
    /// The string has begun (the answer was not just `null`).
    text: bool,
}

impl Default for Copier {
    fn default() -> Self {
        Self {
            state: State::Seek(0),
            high: None,
            out: Vec::new(),
            taken: 0,
            text: false,
        }
    }
}

impl Copier {
    /// The text taken since it was last cleared.
    pub fn taken(&self) -> &[u8] {
        &self.out
    }

    /// Lets go of the text taken, once written.
    pub fn clear(&mut self) {
        self.out.clear();
    }

    /// Whether the string has ended (or reached [`MAX_TEXT`]): the rest of
    /// the answer is not needed.
    pub fn is_done(&self) -> bool {
        self.state == State::End
    }

    /// How it ended, once the answer's last piece was fed.
    pub fn outcome(&self) -> Copied {
        match self.state {
            State::End => Copied::Whole,
            _ if self.text => Copied::Cut,
            _ => Copied::Missing,
        }
    }

    /// Takes the next piece of the answer.
    ///
    /// # Errors
    /// `NoMemory` when the text taken cannot be kept: write and clear it
    /// more often (the text of one piece is at most four times as long).
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        for &b in piece {
            self.byte(b)?;
            self.text |= matches!(self.state, State::Text);
            if self.state == State::End {
                break;
            }
        }
        Ok(())
    }

    fn put(&mut self, c: char) -> Result<(), Error> {
        let mut buf = [0u8; 4];
        let bytes = c.encode_utf8(&mut buf).as_bytes();
        self.push(bytes)
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.taken + bytes.len() > MAX_TEXT {
            self.state = State::End;
            return Ok(());
        }
        self.out
            .try_reserve(bytes.len())
            .map_err(|_| Error::NoMemory)?;
        self.taken += bytes.len();
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    /// A lone half of a pair stands for the replacement character.
    fn flush_high(&mut self) -> Result<(), Error> {
        if self.high.take().is_some() {
            self.put('\u{fffd}')?;
        }
        Ok(())
    }

    fn unit(&mut self, unit: u32) -> Result<(), Error> {
        match (self.high.take(), unit) {
            (Some(high), 0xdc00..=0xdfff) => {
                let code = 0x10000 + ((high - 0xd800) << 10) + (unit - 0xdc00);
                self.put(char::from_u32(code).unwrap_or('\u{fffd}'))
            }
            (Some(_), _) => {
                self.put('\u{fffd}')?;
                self.unit(unit)
            }
            (None, 0xd800..=0xdbff) => {
                self.high = Some(unit);
                Ok(())
            }
            (None, 0xdc00..=0xdfff) => self.put('\u{fffd}'),
            (None, 0) => Ok(()),
            (None, _) => self.put(char::from_u32(unit).unwrap_or('\u{fffd}')),
        }
    }

    fn byte(&mut self, b: u8) -> Result<(), Error> {
        match self.state {
            State::Seek(at) => {
                self.state = if b == KEY[at] {
                    if at + 1 == KEY.len() {
                        State::Colon
                    } else {
                        State::Seek(at + 1)
                    }
                } else {
                    // The key's two quotes are its only ones: a quote that
                    // is not its next byte begins it anew.
                    State::Seek(usize::from(b == b'"'))
                };
            }
            State::Colon => match b {
                b' ' | b'\t' | b'\r' | b'\n' => {}
                b':' => self.state = State::Value,
                // A string that only looked like the key.
                _ => self.state = State::Seek(usize::from(b == b'"')),
            },
            State::Value => match b {
                b' ' | b'\t' | b'\r' | b'\n' => {}
                b'"' => self.state = State::Text,
                _ => self.state = State::Seek(0),
            },
            State::Text => match b {
                b'"' => {
                    self.flush_high()?;
                    self.state = State::End;
                }
                b'\\' => self.state = State::Escape,
                _ => {
                    self.flush_high()?;
                    self.push(&[b])?;
                }
            },
            State::Escape => return self.escape(b),
            State::Unicode(digits, value) => {
                let Some(digit) = char::from(b).to_digit(16) else {
                    // Not a digit: the escape is broken; go on as text.
                    self.put('\u{fffd}')?;
                    self.state = State::Text;
                    return self.byte(b);
                };
                let value = value * 16 + digit;
                if digits == 3 {
                    self.state = State::Text;
                    self.unit(value)?;
                } else {
                    self.state = State::Unicode(digits + 1, value);
                }
            }
            State::End => {}
        }
        Ok(())
    }

    fn escape(&mut self, b: u8) -> Result<(), Error> {
        let plain = match b {
            b'n' => Some('\n'),
            b't' => Some('\t'),
            b'r' => Some('\r'),
            b'b' => Some('\u{8}'),
            b'f' => Some('\u{c}'),
            b'u' => {
                self.state = State::Unicode(0, 0);
                return Ok(());
            }
            // `"`, `\`, `/` and anything else stand for themselves.
            _ => None,
        };
        self.flush_high()?;
        match plain {
            Some(c) => self.put(c)?,
            None => self.push(&[b])?,
        }
        if self.state == State::Escape {
            self.state = State::Text;
        }
        Ok(())
    }
}
