//! What the native Wikipedia lookup decides without host calls: the wiki of
//! a language, the addresses of a summary and of its article, and the summary
//! read from the answer. Each part is the interface-6 code made fallible,
//! with a test that both give the same answers.
//!
//! The answer is read like interface 6 reads it (the members `title` and
//! `extract` of its object), the texts unescaped into a fallible buffer and
//! kept whole, as long as the answer's body (`MAX_BODY`).
use alloc::string::String;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{json::string_member, text::concat};

/// The most bytes of an answer that are read: the request's `max_body`
/// (a summary is a paragraph or two). A longer text is refused with `NoSpace`.
pub const MAX_BODY: usize = 16 * 1024;

/// The wiki for `language` (`en-GB` → `en`), `en` where there is no letter.
///
/// # Errors
/// `NoMemory` when the code cannot be kept.
pub fn wiki(language: &str) -> Result<String, Error> {
    let primary = language.split(['-', '_']).next().unwrap_or("en");
    let mut code = String::new();
    code.try_reserve(primary.len())
        .map_err(|_| Error::NoMemory)?;
    code.extend(
        primary
            .chars()
            .filter(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_lowercase()),
    );
    if code.is_empty() {
        code.try_reserve(2).map_err(|_| Error::NoMemory)?;
        code.push_str("en");
    }
    Ok(code)
}

/// `title` as a path segment of an article: letters, digits and `-_.~` as
/// they are, spaces as `_`, every other byte percent-encoded.
fn segment(title: &str) -> Result<String, Error> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let title = title.trim();
    let mut out = String::new();
    out.try_reserve(title.len().saturating_mul(3))
        .map_err(|_| Error::NoMemory)?;
    for byte in title.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('_'),
            byte => {
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
    }
    Ok(out)
}

/// The address of the summary of `word`'s article.
///
/// # Errors
/// `NoMemory` when the address cannot be kept.
pub fn summary_url(language: &str, word: &str) -> Result<String, Error> {
    concat(&[
        "https://",
        &wiki(language)?,
        ".wikipedia.org/api/rest_v1/page/summary/",
        &segment(word)?,
    ])
}

/// The address of the article `title` as its source is named: without
/// `https://`, short enough to read.
///
/// # Errors
/// `NoMemory` when the address cannot be kept.
pub fn article_url(language: &str, title: &str) -> Result<String, Error> {
    concat(&[&wiki(language)?, ".wikipedia.org/wiki/", &segment(title)?])
}

/// What an answer says.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The summary, with the article's title when the answer has one.
    Found {
        /// The article's title.
        title: Option<String>,
        /// The summary.
        text: String,
    },
    /// No article, or nothing in it to show (`wikipedia.not-found`).
    NotFound,
    /// The network did not answer (`wikipedia.unreachable`).
    Unreachable,
    /// Any other failure (`wikipedia.failed`).
    Failed,
}

/// What the answer with `status` and `body` says: a summary for a status of
/// 200 to 299 whose `extract` is not blank, nothing found for 404 or an empty
/// `extract`, the network gone for status 0.
///
/// # Errors
/// `NoSpace` for a body longer than [`MAX_BODY`], `NoMemory` when a text
/// cannot be kept.
pub fn outcome(status: u16, body: &str) -> Result<Outcome, Error> {
    match status {
        200..=299 => {
            if body.len() > MAX_BODY {
                return Err(Error::NoSpace);
            }
            let (title, text) = members(body)?;
            Ok(match text.filter(|t| !t.trim().is_empty()) {
                Some(text) => Outcome::Found { title, text },
                None => Outcome::NotFound,
            })
        }
        404 => Ok(Outcome::NotFound),
        0 => Ok(Outcome::Unreachable),
        _ => Ok(Outcome::Failed),
    }
}

/// The `title` and the `extract` of the answer's object.
fn members(body: &str) -> Result<(Option<String>, Option<String>), Error> {
    Ok((
        string_member(body, "title")?,
        string_member(body, "extract")?,
    ))
}
