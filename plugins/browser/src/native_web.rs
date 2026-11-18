//! What the browser decides about addresses, answers and pages on interface
//! 7: the same rules as `web.rs`, host-free, with every allocation fallible
//! (a refusal is [`Error::NoMemory`], never an abort) and sizes bounded in
//! code. The interface-6 module goes at activation.
//!
//! Bounds: an address is at most [`MAX_URL`] bytes (the longest `core.net`
//! takes; a longer one is [`Error::NoSpace`], not sent), a page is read
//! [`HEAD`] bytes deep for its title and copied in [`PIECE`]-byte pieces, a
//! title is what fits [`HEAD`].

use alloc::string::String;

use paperpaw_sdk::native::{Error, net::MAX_URL};
use paperpaw_sdk::web::try_file_name;
use paperpaw_sdk::xml::{Token, Tokenizer, local_name, try_decode};
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_ui_kit::{mem, text};

/// Where searches go: a search engine whose results work without
/// JavaScript.
const SEARCH: &str = "https://html.duckduckgo.com/html/?q=";

/// How deep into a page its `<title>` is looked for.
pub const HEAD: usize = 16 * 1024;
/// The size of the pieces a downloaded page is copied in.
pub const PIECE: usize = 4 * 1024;

/// What an answer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// A web page, shown in the reader.
    Page,
    /// A book (EPUB, text), saved in the library.
    Book,
    /// A font (TrueType, OpenType), saved with the fonts.
    Font,
    /// Anything else, saved as a download.
    Other,
}

/// Why a page could not be kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeepError {
    /// A buffer could not be allocated.
    NoMemory,
    /// The storage refused.
    Storage(StorageError),
}

impl From<StorageError> for KeepError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

fn reserve(text: &mut String, more: usize) -> Result<(), Error> {
    text.try_reserve_exact(more).map_err(|_| Error::NoMemory)
}

/// How many bytes `input` takes encoded for a query (spaces are one `+`).
fn encoded_len(input: &str) -> usize {
    input
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~ ".contains(&b) {
                1
            } else {
                3
            }
        })
        .sum()
}

/// `input` for a query: letters, digits and `-_.~` as they are, spaces as
/// `+`, everything else percent-encoded (as `paperpaw_sdk::web::encode`).
fn push_encoded(out: &mut String, input: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in input.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            byte => {
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
    }
}

/// The address to load for what was typed: a web address as it is,
/// another (a name with a dot or a port, as `example.org:8080`) with
/// `https://`, else a search for the words.
///
/// # Errors
/// [`Error::NoSpace`] when the address would be longer than [`MAX_URL`];
/// [`Error::NoMemory`].
pub fn address(input: &str) -> Result<String, Error> {
    let typed = input.trim();
    let starts = |prefix: &[u8]| {
        typed
            .as_bytes()
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    if starts(b"http://") || starts(b"https://") {
        return bounded(typed.len(), |out| out.push_str(typed));
    }
    let host = typed.split(['/', '?', '#']).next().unwrap_or_default();
    let port = host.rsplit_once(':').is_some_and(|(name, port)| {
        !name.is_empty() && !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
    });
    let looks_like_address = !typed.contains(char::is_whitespace)
        && (typed.contains('.') || port)
        && !typed.starts_with('.')
        && !typed.ends_with('.');
    if looks_like_address {
        bounded("https://".len() + typed.len(), |out| {
            out.push_str("https://");
            out.push_str(typed);
        })
    } else {
        bounded(SEARCH.len() + encoded_len(typed), |out| {
            out.push_str(SEARCH);
            push_encoded(out, typed);
        })
    }
}

/// A string of exactly `len` bytes made by `fill`, if that is a usable
/// address.
fn bounded(len: usize, fill: impl FnOnce(&mut String)) -> Result<String, Error> {
    if len > MAX_URL {
        return Err(Error::NoSpace);
    }
    let mut out = String::new();
    reserve(&mut out, len)?;
    fill(&mut out);
    Ok(out)
}

/// Whether `mime` is any of `names`, in any case.
fn is_any(mime: &str, names: &[&str]) -> bool {
    names.iter().any(|name| mime.eq_ignore_ascii_case(name))
}

/// What an answer is, by its content type, else its address, else its
/// first bytes (`head`).
///
/// # Errors
/// [`Error::NoMemory`] where the file name in the address cannot be made.
pub fn kind(content_type: Option<&str>, url: &str, head: &[u8]) -> Result<Kind, Error> {
    let mime = content_type
        .and_then(|t| t.split(';').next())
        .map_or("", str::trim);
    if is_any(mime, &["text/html", "application/xhtml+xml"]) {
        return Ok(Kind::Page);
    }
    if is_any(mime, &["application/epub+zip", "text/plain"]) {
        return Ok(Kind::Book);
    }
    if is_any(
        mime,
        &[
            "font/ttf",
            "font/otf",
            "font/sfnt",
            "font/collection",
            "application/font-sfnt",
            "application/x-font-ttf",
            "application/x-font-otf",
            "application/vnd.ms-opentype",
        ],
    ) {
        return Ok(Kind::Font);
    }
    let name = try_file_name(url).map_err(|_| Error::NoMemory)?;
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    if is_any(extension, &["epub", "txt"]) {
        return Ok(Kind::Book);
    }
    if is_any(extension, &["ttf", "otf", "ttc"]) {
        return Ok(Kind::Font);
    }
    if is_any(extension, &["html", "htm", "xhtml"]) {
        return Ok(Kind::Page);
    }
    if head.starts_with(b"OTTO")
        || head.starts_with(&[0, 1, 0, 0])
        || head.starts_with(b"true")
        || head.starts_with(b"ttcf")
    {
        return Ok(Kind::Font);
    }
    // An EPUB's first entry is its media type, stored.
    if head.starts_with(b"PK") && head.windows(20).any(|w| w == b"application/epub+zip") {
        return Ok(Kind::Book);
    }
    let body = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let start = body.iter().position(|b| !b.is_ascii_whitespace());
    if mime.is_empty() && start.is_some_and(|i| body[i] == b'<') {
        return Ok(Kind::Page);
    }
    Ok(Kind::Other)
}

/// The title of the page in storage at `path`, from its `<title>` within the
/// first [`HEAD`] bytes; `None` where there is none, or the page cannot be
/// read.
///
/// # Errors
/// [`Error::NoMemory`]; the screen then names the page by its host, as the
/// interface-6 browser did where memory was short.
pub fn title(storage: &dyn Storage, path: &str) -> Result<Option<String>, Error> {
    let mut head = mem::buffer(HEAD)?;
    let Ok(n) = storage.read_at(path, 0, &mut head) else {
        return Ok(None);
    };
    let mut title: Option<String> = None;
    let mut done = false;
    let mut refused = false;
    let mut tokenizer = Tokenizer::new();
    let mut emit = |token: Token<'_>| match token {
        Token::Start { name, .. } if !done && local_name(name).eq_ignore_ascii_case("title") => {
            title = Some(String::new());
        }
        Token::End { name } if local_name(name).eq_ignore_ascii_case("title") => done = true,
        Token::Text(piece) if !done && !refused => {
            if let Some(title) = title.as_mut() {
                match try_decode(&piece) {
                    Ok(decoded) => {
                        refused = title.try_reserve(decoded.len()).is_err();
                        if !refused {
                            title.push_str(&decoded);
                        }
                    }
                    Err(_) => refused = true,
                }
            }
        }
        _ => {}
    };
    tokenizer
        .try_feed(&head[..n], &mut emit)
        .map_err(|_| Error::NoMemory)?;
    tokenizer.finish(&mut emit);
    if refused {
        return Err(Error::NoMemory);
    }
    let Some(title) = title else {
        return Ok(None);
    };
    // The words of the title, one space between them.
    let mut clean = String::new();
    reserve(&mut clean, title.len())?;
    for word in title.split_whitespace() {
        if !clean.is_empty() {
            clean.push(' ');
        }
        clean.push_str(word);
    }
    Ok((!clean.is_empty()).then_some(clean))
}

/// `<base href="…">` and a line break, with `url` escaped for the attribute.
fn base_line(url: &str) -> Result<String, Error> {
    const OPEN: &str = "<base href=\"";
    const CLOSE: &str = "\">\n";
    let extra: usize = url
        .bytes()
        .map(|b| match b {
            b'&' => 4,
            b'"' => 5,
            _ => 0,
        })
        .sum();
    let mut line = String::new();
    reserve(&mut line, OPEN.len() + url.len() + extra + CLOSE.len())?;
    line.push_str(OPEN);
    for c in url.chars() {
        match c {
            '&' => line.push_str("&amp;"),
            '"' => line.push_str("&quot;"),
            c => line.push(c),
        }
    }
    line.push_str(CLOSE);
    Ok(line)
}

/// Writes the page downloaded to `from` to `to`, preceded by its address
/// as `<base href>`, so its links lead where they should; `from` is removed.
///
/// # Errors
/// [`KeepError::NoMemory`]; [`KeepError::Storage`] where the storage refuses.
pub fn keep_page(storage: &dyn Storage, from: &str, to: &str, url: &str) -> Result<(), KeepError> {
    let line = base_line(url).map_err(|_| KeepError::NoMemory)?;
    let mut chunk = mem::buffer(PIECE).map_err(|_| KeepError::NoMemory)?;
    storage.write(to, line.as_bytes())?;
    let mut at = 0u64;
    loop {
        let n = storage.read_at(from, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        storage.append(to, &chunk[..n])?;
        at += n as u64;
    }
    storage.remove(from)?;
    Ok(())
}

/// A stable name for the page at `url`, so a page shown again opens
/// where it was left.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn page_name(url: &str) -> Result<String, Error> {
    let hash = paperpaw_sdk::digest::fnv1a64(url);
    text::format(format_args!("{hash:016x}.html"))
}
