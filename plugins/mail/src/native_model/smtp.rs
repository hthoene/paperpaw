//! SMTP (RFC 5321) as text, for the native plugin: the interface-6 `smtp`
//! made fallible. A server's replies, the extensions it offers, the commands
//! to send, and the message ready for the DATA command. A refused allocation
//! is [`Error::NoMemory`], never an abort; a reply is bounded as before (a
//! line of 1,000 bytes, a reply of 4,096 bytes and 64 lines).
//!
//! The one change of behaviour: the line `AUTH PLAIN <credentials>` comes in a
//! [`Secret`] that wipes itself, as the credentials it was made from do.
use super::codec;
use super::mem::{
    Error, Secret, copy, ends_with_ignore_case, lossy, push, push_byte, push_bytes, push_char,
    push_str, starts_with_ignore_case, string_with, vec_with, wipe_text,
};
use alloc::string::String;
use alloc::vec::Vec;

/// A reply, all its lines together.
#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub code: u16,
    /// The text of each line, without the code.
    pub lines: Vec<String>,
}

impl Reply {
    /// 2xx: done.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.code)
    }

    /// 4xx: try again later.
    #[must_use]
    pub fn is_temporary(&self) -> bool {
        (400..500).contains(&self.code)
    }

    /// The first line's text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.lines.first().map_or("", String::as_str)
    }
}

/// Lines of replies, put together: `250-a`, `250 b` is one reply.
#[derive(Debug, Default)]
pub struct ReplyParser {
    code: u16,
    lines: Vec<String>,
    /// Bytes of the reply so far.
    total: usize,
}

/// The longest line of a reply, and the longest reply, in bytes (RFC 5321
/// has lines of 512): a longer one is no server's.
const MAX_REPLY_LINE: usize = 1000;
const MAX_REPLY: usize = 4096;
/// The most lines of a reply kept.
const MAX_REPLY_LINES: usize = 64;

/// A reply that is none: code 0 and a note.
fn broken(note: &str) -> Result<Reply, Error> {
    let mut lines = Vec::new();
    push(&mut lines, copy(note)?)?;
    Ok(Reply { code: 0, lines })
}

impl ReplyParser {
    /// One line of the server (without its line break): the reply, once
    /// its last line came. A line that is no reply line ends the reply
    /// with code 0 (a server that does not speak SMTP).
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the reply so far is then dropped.
    pub fn line(&mut self, line: &[u8]) -> Result<Option<Reply>, Error> {
        self.total = self.total.saturating_add(line.len());
        if line.len() > MAX_REPLY_LINE || self.total > MAX_REPLY {
            self.lines.clear();
            self.total = 0;
            return broken("reply too long").map(Some);
        }
        let text = lossy(line)?;
        let bytes = text.as_bytes();
        let code = bytes
            .get(..3)
            .filter(|d| d.iter().all(u8::is_ascii_digit))
            .and_then(|d| core::str::from_utf8(d).ok())
            .and_then(|d| d.parse::<u16>().ok());
        let (Some(code), Some(sep)) = (code, bytes.get(3).copied().or(Some(b' '))) else {
            self.lines.clear();
            self.total = 0;
            return broken(text.trim()).map(Some);
        };
        if !self.lines.is_empty() && code != self.code {
            // A reply that changes its code half way is no reply.
            self.lines.clear();
        }
        self.code = code;
        if self.lines.len() < MAX_REPLY_LINES {
            push(&mut self.lines, copy(text.get(4..).unwrap_or("").trim())?)?;
        }
        if sep == b'-' {
            return Ok(None);
        }
        self.total = 0;
        Ok(Some(Reply {
            code,
            lines: core::mem::take(&mut self.lines),
        }))
    }
}

/// What a server offers in answer to EHLO.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extensions {
    pub starttls: bool,
    pub auth_plain: bool,
    pub auth_login: bool,
    /// The largest message, in bytes, 0 where it says none.
    pub size: u64,
}

impl Extensions {
    /// From the lines of the reply to EHLO (the first is the greeting).
    #[must_use]
    pub fn of(reply: &Reply) -> Self {
        let mut out = Self::default();
        for line in reply.lines.iter().skip(1) {
            let mut words = line.split_whitespace();
            let Some(first) = words.next() else {
                continue;
            };
            if first.eq_ignore_ascii_case("STARTTLS") {
                out.starttls = true;
            } else if first.eq_ignore_ascii_case("SIZE") {
                out.size = words.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            } else if first.eq_ignore_ascii_case("AUTH") {
                for mechanism in words {
                    if mechanism.eq_ignore_ascii_case("PLAIN") {
                        out.auth_plain = true;
                    } else if mechanism.eq_ignore_ascii_case("LOGIN") {
                        out.auth_login = true;
                    }
                }
            } else if starts_with_ignore_case(first, "AUTH=") {
                // `AUTH=PLAIN` of old servers.
                out.auth_plain |= ends_with_ignore_case(first, "PLAIN");
                out.auth_login |= ends_with_ignore_case(first, "LOGIN");
            }
        }
        out
    }
}

/// `AUTH PLAIN` with the credentials in it, in a buffer that wipes itself.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn auth_plain(user: &str, password: &str) -> Result<Secret, Error> {
    let data = codec::plain_credentials(user, password)?;
    let encoded = codec::base64_line(data.as_slice())?;
    let line = Secret::with_capacity("AUTH PLAIN ".len() + encoded.len() + 2);
    let line = line.and_then(|mut line| {
        line.push_str("AUTH PLAIN ")?;
        line.push_str(&encoded)?;
        line.push_str("\r\n")?;
        Ok(line)
    });
    wipe_text(encoded);
    line
}

/// `MAIL FROM:<address>`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn mail_from(address: &str) -> Result<String, Error> {
    command("MAIL FROM:<", address)
}

/// `RCPT TO:<address>`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn rcpt_to(address: &str) -> Result<String, Error> {
    command("RCPT TO:<", address)
}

/// `head`, the address with nothing in it that would end the command, `>`
/// and a line break.
fn command(head: &str, address: &str) -> Result<String, Error> {
    let mut line = string_with(head.len() + address.len() + 3)?;
    push_str(&mut line, head)?;
    for c in address.chars() {
        if !c.is_control() && !matches!(c, '<' | '>' | ' ') {
            push_char(&mut line, c)?;
        }
    }
    push_str(&mut line, ">\r\n")?;
    Ok(line)
}

/// Whether `address` can be mailed to: one `@`, a local part and a domain
/// with a dot or a bracket, no white space, nothing that ends a command.
#[must_use]
pub fn valid_address(address: &str) -> bool {
    let Some((local, domain)) = address.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && address.len() <= 254
        && !address.contains(|c: char| {
            c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',' | ';')
        })
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'))
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
}

/// A message's bytes as the DATA command sends them: lines end in CRLF, a
/// line that starts with a dot gets another, and the end is `.` on a line.
/// The caller bounds the message (it holds it whole).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn data(message: &str) -> Result<Vec<u8>, Error> {
    let mut out = vec_with(message.len() + message.len() / 40 + 8)?;
    let mut at_line_start = true;
    let bytes = message.as_bytes();
    for (i, &byte) in bytes.iter().enumerate() {
        // Bare LF and bare CR are line breaks too (CR LF is one).
        match byte {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {}
            b'\r' | b'\n' => {
                push_bytes(&mut out, b"\r\n")?;
                at_line_start = true;
            }
            b => {
                if at_line_start && b == b'.' {
                    push_byte(&mut out, b'.')?;
                }
                push_byte(&mut out, b)?;
                at_line_start = false;
            }
        }
    }
    if !(out.ends_with(b"\r\n") || out.is_empty()) {
        push_bytes(&mut out, b"\r\n")?;
    }
    push_bytes(&mut out, b".\r\n")?;
    Ok(out)
}
