//! The page the phone opens: the form sent from it and the pages served, as
//! the interface-6 screen (`phone.rs`) made them, with fallible allocation and
//! the words handed in by the caller instead of read from an environment.
use alloc::string::String;
use core::fmt::Write;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::copy;

use crate::native_policy::Password;

/// The page around what it shows.
const PAGE: &str = include_str!("../phone.html");

/// A text that grows by fallible allocation only.
struct Out(String);

impl Write for Out {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        self.0
            .try_reserve(text.len())
            .map_err(|_| core::fmt::Error)?;
        self.0.push_str(text);
        Ok(())
    }
}

fn done(result: core::fmt::Result) -> Result<(), Error> {
    result.map_err(|_| Error::NoMemory)
}

/// `value` of a form field's text: `+` is a space and `%XX` a byte (a byte
/// sequence that is no UTF-8 is replaced).
fn decode(value: &str) -> Result<String, Error> {
    let bytes = value.as_bytes();
    let mut out = alloc::vec::Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| Error::NoMemory)?;
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push(high << 4 | low);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    match String::from_utf8(out) {
        Ok(text) => Ok(text),
        Err(error) => {
            // Rare: replace what is no UTF-8, in a copy made with room.
            let lossy = String::from_utf8_lossy(error.as_bytes());
            copy(&lossy)
        }
    }
}

/// Field `name` of a form sent, decoded.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn field(form: &str, name: &str) -> Result<Option<String>, Error> {
    form.split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| decode(value))
        .transpose()
}

/// The network and password sent from the page: a network listed, or one
/// named in the field below them. A name of 1 to 32 bytes.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn chosen(form: &str) -> Result<Option<(String, Password)>, Error> {
    let other = field(form, "other")?.unwrap_or_default();
    let ssid = if other.trim().is_empty() {
        match field(form, "ssid")? {
            Some(ssid) => ssid,
            None => return Ok(None),
        }
    } else {
        copy(other.trim())?
    };
    if ssid.is_empty() || ssid.len() > 32 {
        return Ok(None);
    }
    // The decoded field is wiped once the password has its own copy.
    let typed = field(form, "password")?.unwrap_or_default();
    let kept = Password::new(&typed);
    paperpaw_ui_kit::mem::wipe_text(typed);
    Ok(Some((ssid, kept?)))
}

/// `text` safe in HTML, written to `out`.
fn escape_to(out: &mut Out, text: &str) -> Result<(), Error> {
    for c in text.chars() {
        match c {
            '&' => done(out.write_str("&amp;"))?,
            '<' => done(out.write_str("&lt;"))?,
            '>' => done(out.write_str("&gt;"))?,
            '"' => done(out.write_str("&quot;"))?,
            c => done(out.write_char(c))?,
        }
    }
    Ok(())
}

/// `text` safe in HTML.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn escape(text: &str) -> Result<String, Error> {
    let mut out = Out(String::new());
    escape_to(&mut out, text)?;
    Ok(out.0)
}

/// The words of the pages, in the user's language.
#[derive(Debug, Clone, Copy)]
pub struct Texts<'a> {
    pub language: &'a str,
    pub title: &'a str,
    pub choose_one: &'a str,
    pub other: &'a str,
    pub password: &'a str,
    pub join: &'a str,
    pub note: &'a str,
    /// Beside a network that asks for no password.
    pub open: &'a str,
}

/// A network found.
#[derive(Debug, Clone, Copy)]
pub struct Found<'a> {
    pub ssid: &'a str,
    pub secured: bool,
}

/// The page with `body` (already HTML) in it.
fn wrap(texts: &Texts<'_>, body: &str) -> Result<String, Error> {
    let mut out = Out(String::new());
    let mut rest = PAGE;
    while let Some(at) = rest.find('{') {
        done(out.write_str(&rest[..at]))?;
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("{lang}") {
            done(out.write_str(texts.language))?;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("{title}") {
            escape_to(&mut out, texts.title)?;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("{body}") {
            done(out.write_str(body))?;
            rest = after;
        } else {
            // A brace of the style sheet.
            done(out.write_char('{'))?;
            rest = &rest[1..];
        }
    }
    done(out.write_str(rest))?;
    Ok(out.0)
}

/// The page to choose a network: those found, in the order given (the
/// strongest first), a field for another one, the password. `again` after a
/// form without a network.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn choose_page(
    texts: &Texts<'_>,
    networks: &[Found<'_>],
    again: bool,
    token: &str,
) -> Result<String, Error> {
    let mut body = Out(String::new());
    done(body.write_str("<h1>"))?;
    escape_to(&mut body, texts.title)?;
    done(body.write_str("</h1>"))?;
    if again {
        done(body.write_str("<p class=\"note\">"))?;
        escape_to(&mut body, texts.choose_one)?;
        done(body.write_str("</p>"))?;
    }
    done(body.write_str(
        "<form method=\"post\" action=\"/join\"><input type=\"hidden\" name=\"t\" value=\"",
    ))?;
    escape_to(&mut body, token)?;
    done(body.write_str("\">"))?;
    for network in networks {
        done(
            body.write_str("<label class=\"network\"><input type=\"radio\" name=\"ssid\" value=\""),
        )?;
        escape_to(&mut body, network.ssid)?;
        done(body.write_str("\"> "))?;
        escape_to(&mut body, network.ssid)?;
        done(body.write_str("<small>"))?;
        if !network.secured {
            escape_to(&mut body, texts.open)?;
            done(body.write_str(" ·"))?;
        }
        done(body.write_str("</small></label>"))?;
    }
    done(body.write_str("<p><label>"))?;
    escape_to(&mut body, texts.other)?;
    done(body.write_str(
        "<input type=\"text\" name=\"other\" autocomplete=\"off\"></label></p><p><label>",
    ))?;
    escape_to(&mut body, texts.password)?;
    done(body.write_str(
        "<input type=\"password\" name=\"password\" autocomplete=\"off\"></label></p><button type=\"submit\">",
    ))?;
    escape_to(&mut body, texts.join)?;
    done(body.write_str("</button></form><p class=\"note\">"))?;
    escape_to(&mut body, texts.note)?;
    done(body.write_str("</p>"))?;
    wrap(texts, &body.0)
}

/// The page that says `line` (what is being joined), as a heading.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn line_page(texts: &Texts<'_>, line: &str) -> Result<String, Error> {
    let mut body = Out(String::new());
    done(body.write_str("<h1>"))?;
    escape_to(&mut body, line)?;
    done(body.write_str("</h1>"))?;
    wrap(texts, &body.0)
}

/// The page for what is not served.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn not_found_page(texts: &Texts<'_>) -> Result<String, Error> {
    wrap(texts, "<h1>404</h1>")
}
