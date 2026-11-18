//! Typing on a phone for the keyboard (slot `core.remote-keyboard`), host-free:
//! the page the phone gets, and the answer to its requests. The keyboard asks
//! (`core.ui` `ask` with the slot) and the host opens this plugin's phone
//! screen as the dialog for the request; what the phone sends comes back as the
//! dialog's answer. The address carries a key drawn anew each time, guarded as
//! the transfer's.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, ui::RequestId};
use paperpaw_sdk::served::Guard;
use paperpaw_ui_kit::mem::wipe_text;
use paperpaw_ui_kit::text::copy;

use crate::native_web::{Answer, Body, Request, gate, push_json};

/// Longest text taken, in bytes (the server takes no body longer).
pub const MAX_TEXT: usize = 4096;
/// The page, in the data folder, for `core.net` `respond` to send.
pub const PAGE_FILE: &str = "phone.html";

const PAGE: &str = include_str!("../data/phone.html");
const PLACEHOLDER: &str = "/*STRINGS*/{}";

/// The question the keyboard asked, as this screen shows it.
#[derive(Debug)]
pub struct Dialog {
    pub request: RequestId,
    /// Shown masked on the phone (a password); the text is not offered.
    pub secret: bool,
    pub title: String,
    /// The text to start from (empty for a secret one).
    pub text: String,
    /// The most bytes the answer may hold.
    pub limit: usize,
}

impl Dialog {
    /// Wipes what the reader typed or was asked for.
    pub fn wipe(&mut self) {
        wipe_text(core::mem::take(&mut self.text));
    }
}

/// The words of the page: its button, what it says after sending and where
/// sending failed, and the checkbox that shows a password.
#[derive(Debug, Clone, Copy)]
pub struct Words<'a> {
    pub send: &'a str,
    pub sent: &'a str,
    pub failed: &'a str,
    pub show: &'a str,
}

/// The page with its words, the title and the text to start from (`password`:
/// a masked field, never given the text).
///
/// # Errors
/// `NoMemory`.
pub fn page(words: &Words<'_>, title: &str, text: &str, password: bool) -> Result<Vec<u8>, Error> {
    let mut json = String::new();
    json.try_reserve(512 + text.len() * 2)
        .map_err(|_| Error::NoMemory)?;
    json.push('{');
    for (name, value) in [
        ("send", words.send),
        ("sent", words.sent),
        ("failed", words.failed),
        ("show", words.show),
        ("title", title),
    ] {
        json.push_str(name);
        json.push(':');
        push_json(&mut json, value)?;
        json.push(',');
    }
    json.push_str("text:");
    push_json(&mut json, if password { "" } else { text })?;
    json.push_str(if password {
        ",password:true}"
    } else {
        ",password:false}"
    });
    let at = PAGE.find(PLACEHOLDER).ok_or(Error::Invalid)?;
    let (head, tail) = (&PAGE[..at], &PAGE[at + PLACEHOLDER.len()..]);
    let mut out = Vec::new();
    out.try_reserve_exact(head.len() + json.len() + tail.len())
        .map_err(|_| Error::NoMemory)?;
    out.extend_from_slice(head.as_bytes());
    out.extend_from_slice(json.as_bytes());
    out.extend_from_slice(tail.as_bytes());
    wipe_text(json);
    Ok(out)
}

/// The body a request saved (a file of the data folder).
pub trait Texts {
    /// The text in `body`, whole, up to `limit` bytes.
    ///
    /// # Errors
    /// `NoSpace` where it is longer, `Utf8` where it is no text, `NoMemory`,
    /// the host's refusals.
    fn read(&mut self, body: &str, limit: usize) -> Result<String, Error>;
}

/// The answer to `request`, if `guard` lets it in, and the text it sent. A text
/// too long is refused (*413*), so the page keeps it and says so.
pub fn answer(
    texts: &mut dyn Texts,
    guard: &mut Guard,
    request: &Request<'_>,
    limit: usize,
) -> (Answer, Option<String>) {
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((request.target, ""));
    if path == "/favicon.ico" {
        return (plain(404, "not found"), None);
    }
    if let Err(refused) = gate(guard, query, request.at) {
        return (refused, None);
    }
    match (request.method, path) {
        ("GET", "/") => (
            Answer {
                status: 200,
                kind: "text/html; charset=utf-8",
                body: Body::Data(PAGE_FILE),
            },
            None,
        ),
        ("POST", "/send") => send(texts, request, limit),
        _ => (plain(404, "not found"), None),
    }
}

fn send(texts: &mut dyn Texts, request: &Request<'_>, limit: usize) -> (Answer, Option<String>) {
    let text = match request.body {
        Some(_) if request.body_len > limit as u64 => Err(Error::NoSpace),
        // Nothing sent is an empty text.
        None => copy(""),
        Some(body) => texts.read(body, limit),
    };
    match text {
        Ok(text) => (
            Answer {
                status: 200,
                kind: "application/json",
                body: Body::Text("{\"ok\":true}"),
            },
            Some(text),
        ),
        Err(Error::NoSpace) => (
            plain(
                413,
                "The text is too long for the device: send a shorter one.",
            ),
            None,
        ),
        Err(Error::Utf8) => (plain(400, "The text is not valid text."), None),
        Err(_) => (plain(500, "The text could not be read."), None),
    }
}

fn plain(status: u16, words: &'static str) -> Answer {
    Answer {
        status,
        kind: "text/plain; charset=utf-8",
        body: Body::Text(words),
    }
}

#[cfg(test)]
mod tests;
