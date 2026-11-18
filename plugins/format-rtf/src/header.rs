//! What the first bytes of an RTF file tell: that it is one (`{\rtf`), the
//! code page (`\ansicpgN`), the default language (`\deflangN`) and the title,
//! author and subject of the document's `\info` group. Only the first
//! [`HEAD`] bytes are read: what the head does not hold is not known.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{DocumentError, Metadata};

use crate::charset::Charset;
use crate::mem::{self, Refused};

/// Bytes of the head read.
const HEAD: usize = 16 * 1024;
/// The longest file read, in bytes: source offsets are 32 bits, and a
/// document with pictures is mostly their hexadecimal text.
pub(crate) const MOST_SIZE: u64 = 192 * 1024 * 1024;
/// Longest title, author and subject kept, in characters.
const MOST_NAME: usize = 300;

/// What the head tells.
#[derive(Debug, Clone)]
pub(crate) struct Header {
    /// The file's size: the length of its one section.
    pub size: u32,
    pub charset: Charset,
    pub metadata: Metadata,
}

fn malformed(why: &'static str) -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed(why))
}

/// Whether the first bytes of a file are an RTF document's.
pub(crate) fn is_rtf(head: &[u8]) -> bool {
    head.starts_with(b"{\\rtf")
}

/// The language tag of a Windows locale's low byte, for the common ones.
fn locale_language(locale: u32) -> Option<&'static str> {
    Some(match locale & 0xff {
        9 => "en",
        7 => "de",
        12 => "fr",
        10 => "es",
        16 => "it",
        19 => "nl",
        22 => "pt",
        25 => "ru",
        29 => "sv",
        21 => "pl",
        14 => "hu",
        5 => "cs",
        8 => "el",
        6 => "da",
        11 => "fi",
        31 => "tr",
        34 => "uk",
        4 => "zh",
        17 => "ja",
        18 => "ko",
        _ => return None,
    })
}

/// The number after the control word `word` (`\ansicpg1252`) in `head`.
fn number_of(head: &[u8], word: &[u8]) -> Option<u32> {
    let at = head.windows(word.len()).position(|w| w == word)? + word.len();
    let digits = head.get(at..)?;
    let end = digits.iter().position(|b| !b.is_ascii_digit())?;
    core::str::from_utf8(&digits[..end]).ok()?.parse().ok()
}

/// The text of the group `{\word …}` in `head`, as far as it can be read
/// without the converter: letters, `\'hh`, `\uN` with its fallback skipped.
fn group_text(head: &[u8], word: &[u8], charset: Charset) -> Result<Option<String>, Refused> {
    let mut key: Vec<u8> = Vec::new();
    key.try_reserve_exact(word.len() + 2).map_err(|_| Refused)?;
    key.extend_from_slice(b"{\\");
    key.extend_from_slice(word);
    let Some(at) = head.windows(key.len()).position(|w| *w == key[..]) else {
        return Ok(None);
    };
    let mut at = at + key.len();
    // The word must end there (`\title`, not `\titlepg`).
    match head.get(at) {
        Some(b' ' | b'\r' | b'\n') => at += 1,
        Some(b'\\' | b'{') => {}
        _ => return Ok(None),
    }
    let mut out = String::new();
    let mut skip = 0u32;
    let mut count = 0;
    while let Some(&byte) = head.get(at) {
        at += 1;
        match byte {
            b'}' => {
                let text = mem::string(out.trim())?;
                return Ok((!text.is_empty()).then_some(text));
            }
            b'{' | b'\r' | b'\n' => {}
            b'\\' => match head.get(at) {
                Some(b'\'') => {
                    let value = head
                        .get(at + 1..at + 3)
                        .and_then(|h| core::str::from_utf8(h).ok())
                        .and_then(|h| u8::from_str_radix(h, 16).ok());
                    at += 3;
                    if let Some(value) = value.filter(|_| skip == 0) {
                        mem::push_char(&mut out, charset.decode(value))?;
                        count += 1;
                    }
                    skip = skip.saturating_sub(1);
                }
                Some(&c @ (b'\\' | b'{' | b'}')) => {
                    at += 1;
                    mem::push_char(&mut out, char::from(c))?;
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    let end = head
                        .get(at..)
                        .and_then(|rest| rest.iter().position(|b| !b.is_ascii_alphabetic()))
                        .map_or(head.len(), |n| at + n);
                    let name = &head[at..end];
                    let digits = head
                        .get(end..)
                        .map(|rest| {
                            let minus = usize::from(rest.first() == Some(&b'-'));
                            minus
                                + rest[minus..]
                                    .iter()
                                    .take_while(|b| b.is_ascii_digit())
                                    .count()
                        })
                        .unwrap_or(0);
                    let param = core::str::from_utf8(&head[end..end + digits])
                        .ok()
                        .and_then(|d| d.parse::<i32>().ok());
                    at = end + digits;
                    if head.get(at) == Some(&b' ') {
                        at += 1;
                    }
                    if name == b"u" {
                        let unit = param.unwrap_or(0) as u16;
                        if let Some(c) = char::from_u32(u32::from(unit)) {
                            mem::push_char(&mut out, c)?;
                            count += 1;
                        }
                        skip = 1;
                    }
                }
                _ => at += 1,
            },
            _ if skip > 0 => skip -= 1,
            _ => {
                mem::push_char(&mut out, charset.decode(byte))?;
                count += 1;
            }
        }
        if count >= MOST_NAME {
            let text = mem::string(out.trim())?;
            return Ok((!text.is_empty()).then_some(text));
        }
    }
    Ok(None)
}

impl Header {
    /// Reads the head of the file at `path`.
    ///
    /// # Errors
    /// A file that is no RTF document, or too large; what the card or
    /// memory says.
    pub(crate) fn read(storage: &dyn Storage, path: &str) -> Result<Header, DocumentError> {
        let size = storage.size(path)?;
        if size > MOST_SIZE {
            return Err(malformed("the document is too large"));
        }
        let want = size.min(HEAD as u64) as usize;
        let mut head: Vec<u8> = Vec::new();
        head.try_reserve_exact(want).map_err(|_| Refused)?;
        head.resize(want, 0);
        let n = storage.read_at(path, 0, &mut head)?;
        head.truncate(n);
        if !is_rtf(&head) {
            return Err(malformed("this is not an RTF document"));
        }
        let charset = Charset::of_page(number_of(&head, b"\\ansicpg").unwrap_or(1252));
        let mut metadata = Metadata::default();
        metadata.title = group_text(&head, b"title", charset)?;
        if let Some(author) = group_text(&head, b"author", charset)? {
            mem::push(&mut metadata.authors, author)?;
        }
        if let Some(subject) = group_text(&head, b"subject", charset)? {
            mem::push(&mut metadata.subjects, subject)?;
        }
        if let Some(language) = number_of(&head, b"\\deflang").and_then(locale_language) {
            metadata.language = Some(mem::string(language)?);
        }
        Ok(Header {
            size: u32::try_from(size).map_err(|_| malformed("the document is too large"))?,
            charset,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_head_says() {
        let head = b"{\\rtf1\\ansi\\ansicpg1252\\deflang1031{\\info{\\title Caf\\'e9 \\u8364?}{\\author A. B.}}";
        assert!(is_rtf(head));
        assert_eq!(number_of(head, b"\\ansicpg"), Some(1252));
        assert_eq!(
            group_text(head, b"title", Charset::Windows1252)
                .unwrap()
                .as_deref(),
            Some("Café €")
        );
        assert_eq!(
            group_text(head, b"author", Charset::Windows1252)
                .unwrap()
                .as_deref(),
            Some("A. B.")
        );
        assert_eq!(
            group_text(head, b"subject", Charset::Windows1252).unwrap(),
            None
        );
        assert_eq!(locale_language(1031), Some("de"));
        assert!(!is_rtf(b"{\\rt"));
    }
}
