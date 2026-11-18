//! A message written on the device, for the native plugin: the interface-6
//! `compose` made fallible (a refused allocation is [`Error::NoMemory`], never
//! an abort) and free of indexing that could panic. The headers and the text as
//! a mail (RFC 5322, MIME: UTF-8 text, quoted-printable where it is not
//! ASCII), the draft kept between openings, the reply to a letter, the Date
//! header. Tested to give the same answers as the old one (`tests/native_compose.rs`).
use super::codec;
use super::header;
use super::mem::{Error, copy, push, push_char, push_str, string_with};
use super::smtp;
use super::store::{self, Letter};
use alloc::string::String;
use alloc::vec::Vec;

/// The most text of a message written here, in bytes.
pub const MAX_BODY: usize = 16 * 1024;
/// The longest line of a mail, without its line break (RFC 5322).
const MAX_LINE: usize = 998;
/// The most recipients of one message.
pub const MAX_RECIPIENTS: usize = 20;

/// What the reader wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Draft {
    /// The sender's name shown, may be empty.
    pub from_name: String,
    /// The sender's address.
    pub from: String,
    /// Addresses, as typed (comma or semicolon apart, names allowed).
    pub to: String,
    /// The copies.
    pub cc: String,
    /// The subject.
    pub subject: String,
    /// The text.
    pub body: String,
    /// The `Message-ID` of what this answers …
    pub in_reply_to: String,
    /// … and its `References`.
    pub references: String,
}

/// Why a draft is not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// The sender is not an address.
    NoSender,
    /// Nobody to send to.
    NoRecipient,
    /// An address that cannot be mailed to; it is the first one.
    BadAddress,
    /// More than [`MAX_RECIPIENTS`].
    TooManyRecipients,
    /// More text than [`MAX_BODY`].
    TooLong,
}

/// A draft that cannot be checked or sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// It cannot be sent, for this reason.
    Problem(Problem),
    /// An allocation was refused.
    NoMemory,
}

impl From<Error> for Refused {
    fn from(_: Error) -> Self {
        Self::NoMemory
    }
}

impl From<Problem> for Refused {
    fn from(problem: Problem) -> Self {
        Self::Problem(problem)
    }
}

fn line(out: &mut String, name: &str, value: &str) -> Result<(), Error> {
    push_str(out, name)?;
    push_str(out, ": ")?;
    push_str(out, &store::field(value).map_err(|_| Error::NoMemory)?)?;
    push_char(out, '\n')
}

impl Draft {
    /// An empty draft from `address`, shown as `name` where there is one.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn with_sender(name: &str, address: &str) -> Result<Self, Error> {
        Ok(Self {
            from_name: copy(name)?,
            from: copy(address)?,
            ..Self::default()
        })
    }

    /// The draft as kept between two openings: the headers a line each, a
    /// blank line, the text.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn to_text(&self) -> Result<String, Error> {
        let mut out = string_with(self.body.len() + 256)?;
        line(&mut out, "To", &self.to)?;
        line(&mut out, "Cc", &self.cc)?;
        line(&mut out, "Subject", &self.subject)?;
        line(&mut out, "In-Reply-To", &self.in_reply_to)?;
        line(&mut out, "References", &self.references)?;
        push_char(&mut out, '\n')?;
        push_str(&mut out, &self.body)?;
        Ok(out)
    }

    /// A draft kept by [`Draft::to_text`].
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn from_text(text: &str) -> Result<Self, Error> {
        let (head, body) = text.split_once("\n\n").unwrap_or((text, ""));
        let mut draft = Self {
            body: copy(body)?,
            ..Self::default()
        };
        for line in head.lines() {
            let Some((name, value)) = line.split_once(": ") else {
                continue;
            };
            let value = copy(value)?;
            match name {
                "To" => draft.to = value,
                "Cc" => draft.cc = value,
                "Subject" => draft.subject = value,
                "In-Reply-To" => draft.in_reply_to = value,
                "References" => draft.references = value,
                _ => {}
            }
        }
        Ok(draft)
    }

    /// The addresses to send to: to and cc, each valid, no repeats.
    ///
    /// # Errors
    /// [`Refused`].
    pub fn recipients(&self) -> Result<Vec<String>, Refused> {
        let mut out: Vec<String> = Vec::new();
        for list in [&self.to, &self.cc] {
            for field in split_list(list)? {
                let Some(found) = header::addresses(&field)?.into_iter().next() else {
                    return Err(Problem::BadAddress.into());
                };
                if !smtp::valid_address(&found.address) {
                    return Err(Problem::BadAddress.into());
                }
                if !out.iter().any(|a| a.eq_ignore_ascii_case(&found.address)) {
                    push(&mut out, found.address)?;
                }
            }
        }
        if out.is_empty() {
            return Err(Problem::NoRecipient.into());
        }
        if out.len() > MAX_RECIPIENTS {
            return Err(Problem::TooManyRecipients.into());
        }
        Ok(out)
    }

    /// Whether it can be sent: the addresses it goes to.
    ///
    /// # Errors
    /// [`Refused`].
    pub fn check(&self) -> Result<Vec<String>, Refused> {
        if !smtp::valid_address(self.from.trim()) {
            return Err(Problem::NoSender.into());
        }
        if self.body.len() > MAX_BODY {
            return Err(Problem::TooLong.into());
        }
        self.recipients()
    }

    /// The message with its headers, lines ended by CRLF. `date` is the Date
    /// header's value, `id` the part of the Message-ID before the `@`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn message(&self, date: &str, id: &str) -> Result<String, Error> {
        let domain = self.from.rsplit_once('@').map_or("localhost", |(_, d)| d);
        let mut out = string_with(self.body.len() + 600)?;
        let mut header_line = |name: &str, value: &str| -> Result<(), Error> {
            push_str(&mut out, name)?;
            push_str(&mut out, ": ")?;
            push_str(&mut out, &one_line(value)?)?;
            push_str(&mut out, "\r\n")
        };
        header_line("Date", date)?;
        let from = if self.from_name.trim().is_empty() {
            copy(self.from.trim())?
        } else {
            let mut from = display_name(&self.from_name)?;
            push_str(&mut from, " <")?;
            push_str(&mut from, self.from.trim())?;
            push_char(&mut from, '>')?;
            from
        };
        header_line("From", &from)?;
        header_line("To", &list_header(&self.to)?)?;
        if !self.cc.trim().is_empty() {
            header_line("Cc", &list_header(&self.cc)?)?;
        }
        header_line("Subject", &codec::encode_words(&one_line(&self.subject)?)?)?;
        let mut message_id = string_with(id.len() + domain.len() + 3)?;
        push_char(&mut message_id, '<')?;
        push_str(&mut message_id, id)?;
        push_char(&mut message_id, '@')?;
        push_str(&mut message_id, domain)?;
        push_char(&mut message_id, '>')?;
        header_line("Message-ID", &message_id)?;
        if !self.in_reply_to.trim().is_empty() {
            header_line("In-Reply-To", self.in_reply_to.trim())?;
            let mut references = copy(self.references.trim())?;
            if !references.is_empty() {
                push_char(&mut references, ' ')?;
            }
            push_str(&mut references, self.in_reply_to.trim())?;
            header_line("References", &references)?;
        }
        header_line("MIME-Version", "1.0")?;
        header_line("Content-Type", "text/plain; charset=utf-8")?;
        // Plain where it is ASCII and no line is longer than mail allows
        // (998); else quoted-printable, which breaks long lines.
        let ascii = self.body.is_ascii()
            && self
                .body
                .split(['\n', '\r'])
                .all(|line| line.len() <= MAX_LINE);
        header_line(
            "Content-Transfer-Encoding",
            if ascii { "7bit" } else { "quoted-printable" },
        )?;
        header_line("X-Mailer", "PaperPaw")?;
        push_str(&mut out, "\r\n")?;
        if ascii {
            plain_body(&mut out, &self.body)?;
        } else {
            push_str(&mut out, &codec::encode_qp(&self.body)?)?;
        }
        Ok(out)
    }
}

/// `body` with CRLF line ends (a final line break is one, not two).
fn plain_body(out: &mut String, body: &str) -> Result<(), Error> {
    for line in body.split('\n') {
        push_str(out, line.strip_suffix('\r').unwrap_or(line))?;
        push_str(out, "\r\n")?;
    }
    // `split` of a text ending in a line break gave one empty line more.
    if body.ends_with('\n') {
        out.truncate(out.len().saturating_sub(2));
    }
    Ok(())
}

/// `text` on one line: line breaks and control characters become spaces.
fn one_line(text: &str) -> Result<String, Error> {
    let mut out = string_with(text.len())?;
    for c in text.chars() {
        push_char(&mut out, if c.is_control() { ' ' } else { c })?;
    }
    copy(out.trim())
}

/// A name for the From header: quoted where it needs, encoded where it is not
/// ASCII.
fn display_name(name: &str) -> Result<String, Error> {
    let name = one_line(name)?;
    if name.is_ascii() {
        let plain = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' || c == '\'');
        if plain {
            return Ok(name);
        }
        let mut quoted = string_with(name.len() + 2)?;
        push_char(&mut quoted, '"')?;
        for c in name.chars() {
            if c == '"' || c == '\\' {
                push_char(&mut quoted, '\\')?;
            }
            push_char(&mut quoted, c)?;
        }
        push_char(&mut quoted, '"')?;
        return Ok(quoted);
    }
    codec::encode_words(&name)
}

/// The addresses of a typed list as one header value.
fn list_header(list: &str) -> Result<String, Error> {
    let mut out = String::new();
    for field in split_list(list)? {
        if !out.is_empty() {
            push_str(&mut out, ", ")?;
        }
        match header::addresses(&field)?.into_iter().next() {
            Some(a) if a.name.is_empty() => push_str(&mut out, &a.address)?,
            Some(a) => {
                push_str(&mut out, &display_name(&a.name)?)?;
                push_str(&mut out, " <")?;
                push_str(&mut out, &a.address)?;
                push_char(&mut out, '>')?;
            }
            None => push_str(&mut out, &one_line(&field)?)?,
        }
    }
    Ok(out)
}

/// The fields of a list typed with commas or semicolons (a comma in quotes is
/// none).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn split_list(list: &str) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let (mut quoted, mut angle) = (false, false);
    let mut current = String::new();
    for c in list.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                push_char(&mut current, c)?;
            }
            '<' if !quoted => {
                angle = true;
                push_char(&mut current, c)?;
            }
            '>' if !quoted => {
                angle = false;
                push_char(&mut current, c)?;
            }
            ',' | ';' | '\n' if !quoted && !angle => {
                if !current.trim().is_empty() {
                    push(&mut out, copy(current.trim())?)?;
                }
                current.clear();
            }
            c => push_char(&mut current, c)?,
        }
    }
    if !current.trim().is_empty() {
        push(&mut out, copy(current.trim())?)?;
    }
    Ok(out)
}

/// The draft that answers `letter` from the address `own`: to whom it was from
/// (or its `Reply-To`), with everyone else who got it in Cc for `all`; `Re: `
/// before the subject unless it has one; the original quoted after `intro`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn reply_to(letter: &Letter, own: &str, all: bool, intro: &str) -> Result<Draft, Error> {
    let own_address = |field: &str| -> Result<bool, Error> {
        Ok(header::addresses(field)?
            .first()
            .is_some_and(|a| a.address.eq_ignore_ascii_case(own)))
    };
    let known = |field: &str, list: &str| -> Result<bool, Error> {
        let found = header::addresses(field)?.into_iter().next();
        let Some(found) = found else {
            return Ok(false);
        };
        Ok(header::addresses(list)?
            .iter()
            .any(|a| a.address.eq_ignore_ascii_case(&found.address)))
    };
    let to = if letter.reply_to.trim().is_empty() {
        &letter.from
    } else {
        &letter.reply_to
    };
    let to = copy(to.trim())?;
    let mut cc: Vec<String> = Vec::new();
    if all {
        let mut fields = split_list(&letter.to)?;
        for field in split_list(&letter.cc)? {
            push(&mut fields, field)?;
        }
        for field in fields {
            let mut seen = known(&field, &to)?;
            for c in &cc {
                seen = seen || known(&field, c)?;
            }
            if !own_address(&field)? && !seen {
                push(&mut cc, field)?;
            }
        }
    }
    let mut references = copy(letter.references.trim())?;
    if !references.is_empty() {
        push_char(&mut references, ' ')?;
    }
    push_str(&mut references, letter.message_id.trim())?;
    let mut body = String::new();
    push_str(&mut body, "\n\n")?;
    push_str(&mut body, &quote(&letter.text, intro, 40)?)?;
    let mut joined = String::new();
    for (i, c) in cc.iter().enumerate() {
        if i > 0 {
            push_str(&mut joined, ", ")?;
        }
        push_str(&mut joined, c)?;
    }
    Ok(Draft {
        from: copy(own)?,
        to,
        cc: joined,
        subject: header::reply_subject(letter.subject.trim())?,
        body,
        in_reply_to: copy(letter.message_id.trim())?,
        references: header::last_ids(&references, header::MAX_REFERENCES)?,
        ..Draft::default()
    })
}

/// Date header value for `seconds` since 1970 (UTC).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn date_header(seconds: i64) -> Result<String, Error> {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = header::civil_from_days(days);
    let weekday = usize::try_from(days.rem_euclid(7)).unwrap_or(0);
    let month = usize::try_from((month - 1).clamp(0, 11)).unwrap_or(0);
    paperpaw_ui_kit::text::format(format_args!(
        "{}, {day} {} {year} {:02}:{:02}:{:02} +0000",
        DAYS.get(weekday).copied().unwrap_or("Thu"),
        MONTHS.get(month).copied().unwrap_or("Jan"),
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    ))
}

/// The text of a reply: the original quoted, each line after `> `, with
/// `intro` (a line saying who wrote it, in the reader's language) before.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn quote(original: &str, intro: &str, max_lines: usize) -> Result<String, Error> {
    let mut out = String::new();
    if !intro.is_empty() {
        push_str(&mut out, intro)?;
        push_char(&mut out, '\n')?;
    }
    for line in original.lines().take(max_lines) {
        push_str(&mut out, "> ")?;
        push_str(&mut out, line)?;
        push_char(&mut out, '\n')?;
    }
    Ok(out)
}
