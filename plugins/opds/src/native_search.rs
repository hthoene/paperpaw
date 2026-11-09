//! Searching a catalog: the address an OpenSearch description gives for
//! searches in OPDS (`{searchTerms}` in it), and the address of a search for
//! some words. Both allocate fallibly; the description is read as it streams.
use alloc::string::String;

use paperpaw_sdk::{
    native::Error,
    xml::{Token, Tokenizer, local_name},
};

use paperpaw_ui_kit::mem::buffer;

use crate::native_model::{CHUNK, MAX_URL, Source, join_url};

/// A description larger than this is not read.
const MAX_DESCRIPTION: u64 = 64 * 1024;

/// The address the OpenSearch description `source` holds (from `base`) gives
/// for searches in OPDS (an `application/atom+xml` one), with `{searchTerms}`
/// in it; `None` where it has none, or one too long.
///
/// # Errors
/// `NoMemory`; the source's error when it cannot be read.
pub fn template(source: &mut impl Source, base: &str) -> Result<Option<String>, Error> {
    let mut found: Option<String> = None;
    let mut failed = false;
    let mut tokenizer = Tokenizer::new();
    let mut buf = buffer(CHUNK)?;
    let mut offset = 0u64;
    let mut emit = |token: Token<'_>| {
        if failed || found.is_some() {
            return;
        }
        let Token::Start { name, attrs, .. } = token else {
            return;
        };
        if local_name(name) != "Url" {
            return;
        }
        let made = (|| {
            let kind = attrs.try_get("type").map_err(|_| Error::NoMemory)?;
            if !kind.is_some_and(|t| t.contains("atom+xml")) {
                return Ok(None);
            }
            let Some(url) = attrs.try_get("template").map_err(|_| Error::NoMemory)? else {
                return Ok(None);
            };
            if url.len() > MAX_URL {
                return Ok(None);
            }
            join_url(base, &url).map(Some)
        })();
        match made {
            Ok(url) => found = url.filter(|url| url.len() <= MAX_URL),
            Err(_) => failed = true,
        }
    };
    while offset < MAX_DESCRIPTION {
        let n = source.read_at(offset, &mut buf)?;
        if n == 0 {
            break;
        }
        let piece = buf.get(..n).ok_or(Error::Invalid)?;
        // Out of memory: no address is an answer, but not a refusal passed off.
        if tokenizer.try_feed(piece, &mut emit).is_err() {
            return Err(Error::NoMemory);
        }
        offset += n as u64;
    }
    tokenizer.finish(&mut emit);
    if failed {
        return Err(Error::NoMemory);
    }
    Ok(found)
}

/// The address of a search for `terms` with `template`: the terms in place of
/// `{searchTerms}`, optional parameters (`{startPage?}`) left out.
///
/// # Errors
/// `NoMemory`; `NoSpace` where the address would pass [`MAX_URL`] bytes.
pub fn url(template: &str, terms: &str) -> Result<String, Error> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        put(&mut out, rest.get(..start).unwrap_or_default())?;
        let Some(end) = rest.get(start..).and_then(|r| r.find('}')) else {
            break;
        };
        let name = rest.get(start + 1..start + end).unwrap_or_default();
        if name == "searchTerms" {
            encode(&mut out, terms)?;
        }
        rest = rest.get(start + end + 1..).unwrap_or_default();
    }
    put(&mut out, rest)?;
    if out.len() > MAX_URL {
        return Err(Error::NoSpace);
    }
    Ok(out)
}

fn put(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

/// `terms` for a query: letters, digits and `-_.~` as they are, spaces as `+`,
/// everything else percent-encoded.
fn encode(out: &mut String, terms: &str) -> Result<(), Error> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in terms.bytes() {
        out.try_reserve(3).map_err(|_| Error::NoMemory)?;
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            byte => {
                out.push('%');
                out.push(char::from(
                    HEX.get(usize::from(byte >> 4)).copied().unwrap_or(b'0'),
                ));
                out.push(char::from(
                    HEX.get(usize::from(byte & 15)).copied().unwrap_or(b'0'),
                ));
            }
        }
        if out.len() > 2 * MAX_URL {
            return Err(Error::NoSpace);
        }
    }
    Ok(())
}
