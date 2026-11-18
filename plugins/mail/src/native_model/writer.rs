//! What the compose window shows and does, host-free: its rows, what each says,
//! what a typed answer changes, why a draft is refused, a fresh draft with the
//! signature, and the part of a message's id that is not the domain. Everything
//! allocated is asked for.
use super::browser::Words;
use super::compose::{Draft, Problem};
use super::mem::{Error, copy, push_char, push_str, string_with};
use alloc::string::String;
use alloc::vec::Vec;

/// A row of the compose window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Part {
    /// Which account sends (only where there are several).
    From,
    /// To whom.
    To,
    /// The copies.
    Cc,
    /// The subject.
    Subject,
    /// The text.
    Body,
    /// Send it.
    Send,
}

impl Part {
    /// The catalog key of the row's title (and of the question typed for it).
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Part::From => "mail.from",
            Part::To => "mail.to",
            Part::Cc => "mail.cc",
            Part::Subject => "mail.subject",
            Part::Body => "mail.body",
            Part::Send => "mail.send",
        }
    }

    /// Whether choosing the row asks for text.
    #[must_use]
    pub const fn typed(self) -> bool {
        matches!(self, Part::To | Part::Cc | Part::Subject | Part::Body)
    }
}

/// The rows, for `accounts` accounts to send from.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn parts(accounts: usize) -> Result<Vec<Part>, Error> {
    let mut parts = Vec::new();
    parts.try_reserve_exact(6).map_err(|_| Error::NoMemory)?;
    if accounts > 1 {
        parts.push(Part::From);
    }
    parts.extend([Part::To, Part::Cc, Part::Subject, Part::Body, Part::Send]);
    Ok(parts)
}

/// What a row says at the right: the sender's address, what was typed (of the
/// text, its first line with something in it), nothing for *Send*.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn value(
    part: Part,
    draft: &Draft,
    sender: &str,
    words: &mut dyn Words,
) -> Result<String, Error> {
    match part {
        Part::From => copy(sender),
        Part::To => copy(&draft.to),
        Part::Cc => copy(&draft.cc),
        Part::Subject => copy(&draft.subject),
        Part::Body => {
            let first = draft
                .body
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            if first.is_empty() {
                words.tr("mail.body.empty")
            } else {
                copy(first)
            }
        }
        _ => Ok(String::new()),
    }
}

/// What the reader typed for `part`: addresses and the subject are trimmed, the
/// text is kept as it is.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn typed(draft: &mut Draft, part: Part, text: &str) -> Result<(), Error> {
    match part {
        Part::To => draft.to = copy(text.trim())?,
        Part::Cc => draft.cc = copy(text.trim())?,
        Part::Subject => draft.subject = copy(text.trim())?,
        Part::Body => draft.body = copy(text)?,
        _ => {}
    }
    Ok(())
}

/// The text a row's question starts with.
#[must_use]
pub fn current(draft: &Draft, part: Part) -> &str {
    match part {
        Part::To => &draft.to,
        Part::Cc => &draft.cc,
        Part::Subject => &draft.subject,
        _ => &draft.body,
    }
}

/// The catalog key of what a refused draft says.
#[must_use]
pub const fn problem_key(problem: Problem) -> &'static str {
    match problem {
        Problem::NoSender => "mail.problem.sender",
        Problem::NoRecipient => "mail.problem.recipient",
        Problem::BadAddress => "mail.problem.address",
        Problem::TooManyRecipients => "mail.problem.many",
        _ => "mail.problem.long",
    }
}

/// The text of a mail written from nothing: the signature (a blank line, `-- `
/// and the signature) or no text.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn fresh_body(signature: &str) -> Result<String, Error> {
    let signature = signature.trim();
    if signature.is_empty() {
        return Ok(String::new());
    }
    let mut out = string_with(signature.len() + 8)?;
    push_str(&mut out, "\n\n-- \n")?;
    push_str(&mut out, signature)?;
    push_char(&mut out, '\n')?;
    Ok(out)
}

/// The name of a sender, else its address: `Jane Doe <jane@x>` is `Jane Doe`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn header_name(from: &str) -> Result<String, Error> {
    match super::header::addresses(from)?.first() {
        Some(a) => copy(a.shown()),
        None => copy(from),
    }
}

/// The part of a message's id before the `@`: the random bytes as hex digits,
/// or the time where the device has none.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn message_id(random: Option<&[u8]>, now: i64) -> Result<String, Error> {
    let mut out = string_with(24)?;
    match random {
        Some(bytes) if !bytes.is_empty() => {
            for byte in bytes {
                for digit in [byte >> 4, byte & 15] {
                    push_char(
                        &mut out,
                        char::from_digit(u32::from(digit), 16).unwrap_or('0'),
                    )?;
                }
            }
        }
        _ => push_str(
            &mut out,
            &paperpaw_ui_kit::text::format(format_args!("{now}"))?,
        )?,
    }
    Ok(out)
}
