//! What the native translation plugin decides without host calls: the
//! language DeepL is asked for, the request (its service, the form body, the
//! authorisation that holds the key) and the translation read from the
//! answer. Each part is the interface-6 code made fallible, with a test that
//! both give the same answers.
//!
//! Where it differs from interface 6, on purpose:
//!
//! - a refused allocation is `NoMemory`, never an abort;
//! - the answer is read up to [`MAX_BODY`] (the request's `max_body`); a longer
//!   one is refused with `NoSpace`;
//! - a language code of a setting over 16 bytes is none (the reader's own
//!   language is asked for), and capitals are ASCII ones;
//! - the line that carries the key is a [`Secret`] that wipes itself.
use alloc::string::String;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{json::string_member, text::concat};

pub use crate::declarations::{AUTO, LANGUAGES};

/// The longest text sent at once, in characters.
pub const MAX_CHARS: usize = 3000;

/// The most bytes of an answer that are read: 3,000 characters of any
/// script, translated, with DeepL's own members around them.
pub const MAX_BODY: usize = 24 * 1024;

/// The longest code after `translate.to.` taken from a setting.
const MAX_CODE: usize = 16;

/// DeepL's target language for the setting `to`, with `ui` PaperPaw's
/// language for [`AUTO`] (`en` → `EN-GB`, `de-AT` → `DE`).
///
/// # Errors
/// `NoMemory` when the code cannot be kept.
pub fn target(to: &str, ui: &str) -> Result<String, Error> {
    if let Some(code) = to.strip_prefix("translate.to.")
        && to != AUTO
        && code.len() <= MAX_CODE
    {
        return upper(code);
    }
    let primary = ui.split(['-', '_']).next().unwrap_or("en");
    let code = [
        ("en", "en-gb"),
        ("pt", "pt-pt"),
        ("zh", "zh-hans"),
        ("no", "nb"),
        ("nn", "nb"),
    ]
    .into_iter()
    .find(|(from, _)| primary.eq_ignore_ascii_case(from))
    .map(|(_, to)| to);
    let known = |code: &str| {
        LANGUAGES
            .iter()
            .any(|l| l.strip_prefix("translate.to.") == Some(code))
    };
    let chosen = match code {
        Some(code) => Some(code),
        None => LANGUAGES
            .iter()
            .filter_map(|l| l.strip_prefix("translate.to."))
            .find(|l| primary.eq_ignore_ascii_case(l)),
    };
    match chosen.filter(|code| known(code)) {
        Some(code) => upper(code),
        None => upper("en-gb"),
    }
}

/// `code` in ASCII capitals.
fn upper(code: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(code.len())
        .map_err(|_| Error::NoMemory)?;
    out.extend(code.chars().map(|c| c.to_ascii_uppercase()));
    Ok(out)
}

/// The host DeepL's service for `key` runs on: free keys (ending in `:fx`)
/// go to the free service.
#[must_use]
pub fn host(key: &str) -> &'static str {
    if key.ends_with(":fx") {
        "api-free.deepl.com"
    } else {
        "api.deepl.com"
    }
}

/// The address translating with `key`.
///
/// # Errors
/// `NoMemory` when the address cannot be kept.
pub fn url(key: &str) -> Result<String, Error> {
    concat(&["https://", host(key), "/v2/translate"])
}

/// `text` as a query value: letters, digits and `-_.~` as they are, spaces as
/// `+`, everything else percent-encoded; appended to `out`.
fn encode_into(out: &mut String, text: &str) -> Result<(), Error> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.try_reserve(text.len().saturating_mul(3))
        .map_err(|_| Error::NoMemory)?;
    for byte in text.bytes() {
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
    Ok(())
}

/// The form body translating `text` (its first [`MAX_CHARS`] characters) into
/// `target`.
///
/// # Errors
/// `NoMemory` when the body cannot be kept.
pub fn body(text: &str, target: &str) -> Result<String, Error> {
    let end = text
        .char_indices()
        .nth(MAX_CHARS)
        .map_or(text.len(), |(at, _)| at);
    let text = text.get(..end).unwrap_or(text);
    let mut out = String::new();
    out.try_reserve(text.len().saturating_mul(3) + target.len() * 3 + 24)
        .map_err(|_| Error::NoMemory)?;
    out.push_str("text=");
    encode_into(&mut out, text)?;
    out.push_str("&target_lang=");
    encode_into(&mut out, target)?;
    Ok(out)
}

/// The value of the `Authorization` header for `key`, in a buffer that wipes
/// itself and shows nothing in `Debug`.
///
/// # Errors
/// `NoMemory`.
pub fn authorization(key: &str) -> Result<Secret, Error> {
    const HEAD: &str = "DeepL-Auth-Key ";
    let mut line = String::new();
    line.try_reserve_exact(HEAD.len() + key.len())
        .map_err(|_| Error::NoMemory)?;
    line.push_str(HEAD);
    line.push_str(key);
    Ok(Secret(line))
}

/// A text that wipes itself when it is dropped: the line with the key in it.
pub struct Secret(String);

impl Secret {
    /// A copy of `text` that wipes itself when it is dropped.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(text: &str) -> Result<Self, Error> {
        let mut copy = String::new();
        copy.try_reserve_exact(text.len())
            .map_err(|_| Error::NoMemory)?;
        copy.push_str(text);
        Ok(Self(copy))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        paperpaw_ui_kit::mem::wipe_text(core::mem::take(&mut self.0));
    }
}

/// The translation in DeepL's answer
/// (`{"translations":[{"detected_source_language":"EN","text":"…"}]}`).
///
/// # Errors
/// `NoSpace` for a body over [`MAX_BODY`], `NoMemory`.
pub fn translation(body: &str) -> Result<Option<String>, Error> {
    if body.len() > MAX_BODY {
        return Err(Error::NoSpace);
    }
    let Some(at) = body.find("\"translations\"") else {
        return Ok(None);
    };
    let Some(open) = body.get(at..).and_then(|rest| rest.find('{')) else {
        return Ok(None);
    };
    match body.get(at + open..) {
        Some(object) => string_member(object, "text"),
        None => Ok(None),
    }
}

/// What an answer says.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The translation.
    Translated(String),
    /// DeepL did not accept the key (`translate.bad-key`); a tap asks again.
    BadKey,
    /// The month's characters are used up (`translate.quota`).
    Quota,
    /// The network did not answer (`translate.unreachable`).
    Unreachable,
    /// Anything else (`translate.failed`).
    Failed,
}

/// What the answer with `status` and `body` says.
///
/// # Errors
/// `NoSpace` for a body over [`MAX_BODY`], `NoMemory`.
pub fn outcome(status: u16, body: &str) -> Result<Outcome, Error> {
    Ok(match status {
        200..=299 => match translation(body)? {
            Some(text) => Outcome::Translated(text),
            None => Outcome::Failed,
        },
        401 | 403 => Outcome::BadKey,
        456 => Outcome::Quota,
        0 => Outcome::Unreachable,
        _ => Outcome::Failed,
    })
}
