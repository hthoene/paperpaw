//! Looking a word up, as a provider and as the asker (`docs/abi/document.md` 7).
//!
//! **A provider** declares itself with `Declarations::lookup_provider` and is
//! called as a subscriber of the stage `core.lookup`: the call's payload is one
//! [`Ask`], the reply one entry written with [`answer`]. A provider answers at
//! once. One that must fetch (a network provider) answers [`Entry::pending`]
//! at once, starts its own request and keeps the result; once kept it stages
//! `lookup_ready` (`CommandSender::lookup_ready`), and the asker asks again and
//! is answered from what was kept. An entry may name a screen that shows more
//! of it ([`Entry::opens`], a whole article).
//!
//! **The asker** (the dictionary screen, the reader's selection menu) opens
//! [`Lookup`] and asks for one provider page at a time with [`Lookup::ask`],
//! counting `cursor` up from 0 until `None` comes back. An entry that is
//! [`ProviderEntry::pending`] is answered later: an asker subscribed to
//! `EventKind::LookupReady` hears when, and asks again with the same text and
//! language. Where an entry [`opens`](ProviderEntry::opens) a screen, the asker
//! shows a line for it (Beta 3: *Open Wikipedia ›*) and opens that screen with
//! a hand-over of `word` and `language` and the entry's text.
use super::{
    Error, Host,
    declare::records_wire::{self as wire, RawRecord},
    reply::Reply,
    services::Service,
};
use super::{
    stage_schemas::core_lookup_v1 as stage, wire::services_wire::core_lookup_v1 as schema,
};

/// The longest text asked about.
pub const MAX_TEXT: usize = 1024;
/// The longest context (the words around the selection).
pub const MAX_CONTEXT: usize = 1024;
/// The longest BCP 47 tag.
pub const MAX_LANGUAGE: usize = 35;
/// The longest title of an entry.
pub const MAX_TITLE: usize = 256;
/// The longest text of an entry.
pub const MAX_ENTRY: usize = 2048;
/// The longest id of the screen an entry opens.
pub const MAX_OPENS: usize = 128;
/// The buffer an asker needs for the input of one ask and its reply.
pub const BUFFER: usize = 4096;
const MORE: u32 = 1;
const PENDING: u32 = 2;

/// One ask a provider is called with: the stage payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask<'a>(stage::LookupAsk<'a>);

impl<'a> Ask<'a> {
    /// Reads the payload of the stage call.
    ///
    /// # Errors
    /// `Invalid` for a payload that is not exactly one ask.
    pub fn decode(payload: &'a [u8]) -> Result<Self, Error> {
        stage::LookupAsk::decode(payload)
            .map(Self)
            .map_err(|_| Error::Invalid)
    }

    /// What is looked up.
    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.0.text
    }

    /// The language to look it up in, `None` for any.
    #[must_use]
    pub fn language(&self) -> Option<&'a str> {
        Some(self.0.language).filter(|tag| !tag.is_empty())
    }

    /// The words around the selection, `""` when there are none.
    #[must_use]
    pub const fn context(&self) -> &'a str {
        self.0.context
    }

    /// The page asked for: a provider answers its own pages.
    #[must_use]
    pub const fn cursor(&self) -> u32 {
        self.0.cursor
    }
}

/// What a provider answers one ask with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    title: &'a str,
    text: &'a str,
    flags: u32,
    opens: &'a str,
}

impl<'a> Entry<'a> {
    /// An entry of `title` (the source's name or the headword) and `text`.
    ///
    /// # Errors
    /// `NoSpace` for a title over 256 or a text over 2,048 bytes: cut it first.
    pub fn new(title: &'a str, text: &'a str) -> Result<Self, Error> {
        if title.len() > MAX_TITLE || text.len() > MAX_ENTRY {
            return Err(Error::NoSpace);
        }
        Ok(Self {
            title,
            text,
            flags: 0,
            opens: "",
        })
    }

    /// The answer is fetched and comes later: an entry with an empty text
    /// (`title` may name the source). Once the result is kept, the provider
    /// stages `lookup_ready` for the ask's text and language, and answers the
    /// asker's next ask from what it kept.
    ///
    /// # Errors
    /// `NoSpace` for a title over 256 bytes.
    pub fn pending(title: &'a str) -> Result<Self, Error> {
        let mut entry = Self::new(title, "")?;
        entry.flags |= PENDING;
        Ok(entry)
    }

    /// `screen` shows more of this entry (a whole article): the asker offers a
    /// line for it and opens it with a hand-over of `word`, `language` and the
    /// entry's text (interface 6 `LookupEntry::more`).
    ///
    /// # Errors
    /// `Invalid` for an empty id or one over 128 bytes.
    pub const fn opens(mut self, screen: &'a str) -> Result<Self, Error> {
        if screen.is_empty() || screen.len() > MAX_OPENS {
            return Err(Error::Invalid);
        }
        self.opens = screen;
        Ok(self)
    }

    /// More pages of this provider follow (the asker asks again).
    #[must_use]
    pub const fn more(mut self) -> Self {
        self.flags |= MORE;
        self
    }
}

/// Writes `entry` as the reply of the stage call, straight into the host's
/// buffer after it was measured: a refusal appends no reply bytes and preserves the existing prefix.
///
/// # Errors
/// `NoSpace` where the host's reply buffer is too small.
pub fn answer(reply: &mut Reply<'_>, entry: &Entry<'_>) -> Result<(), Error> {
    let record = stage::Record::LookupEntry(stage::LookupEntry::new(
        entry.title,
        entry.text,
        entry.flags,
        entry.opens,
    ));
    // Encoded in place, after the whole entry was measured: a refusal writes
    // nothing, and no buffer of the entry's size is held on the stack.
    reply.write_with(|out| stage::reply_encode(&[record], out).map_err(Error::from_status))
}

/// What to look up and where to continue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Question<'a> {
    text: &'a str,
    language: &'a str,
    context: &'a str,
    cursor: u32,
}

impl<'a> Question<'a> {
    /// A question about `text`.
    ///
    /// # Errors
    /// `Invalid` for empty text, `NoSpace` for text over 1,024 bytes.
    pub fn new(text: &'a str) -> Result<Self, Error> {
        if text.is_empty() {
            return Err(Error::Invalid);
        }
        if text.len() > MAX_TEXT {
            return Err(Error::NoSpace);
        }
        Ok(Self {
            text,
            language: "",
            context: "",
            cursor: 0,
        })
    }

    /// In `tag` (BCP 47: letters and digits in hyphen-separated subtags of 1 to 8).
    ///
    /// # Errors
    /// `Invalid` for another spelling, `NoSpace` for a tag over 35 bytes.
    pub fn language(mut self, tag: &'a str) -> Result<Self, Error> {
        if tag.len() > MAX_LANGUAGE {
            return Err(Error::NoSpace);
        }
        let fine = tag.is_empty()
            || tag.len() >= 2
                && tag.split('-').all(|part| {
                    (1..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
                });
        if !fine {
            return Err(Error::Invalid);
        }
        self.language = tag;
        Ok(self)
    }

    /// The words around the selection, to help a provider choose a sense.
    ///
    /// # Errors
    /// `NoSpace` for context over 1,024 bytes.
    pub fn context(mut self, context: &'a str) -> Result<Self, Error> {
        if context.len() > MAX_CONTEXT {
            return Err(Error::NoSpace);
        }
        self.context = context;
        Ok(self)
    }

    /// The provider page, counted from 0.
    #[must_use]
    pub const fn cursor(mut self, cursor: u32) -> Self {
        self.cursor = cursor;
        self
    }
}

/// One provider's answer to an ask, borrowed from the reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderEntry<'a>(wire::ServiceLookupEntry<'a>);

impl<'a> ProviderEntry<'a> {
    /// The provider's id.
    #[must_use]
    pub const fn provider(&self) -> &'a str {
        self.0.provider
    }

    /// The entry's title.
    #[must_use]
    pub const fn title(&self) -> &'a str {
        self.0.title
    }

    /// The entry's text.
    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.0.text
    }

    /// More pages of this provider follow.
    #[must_use]
    pub const fn more(&self) -> bool {
        self.0.flags & MORE != 0
    }

    /// The provider fetches and answers later (its text is empty): an asker
    /// subscribed to `EventKind::LookupReady` hears when, and asks again with
    /// the same text and language.
    #[must_use]
    pub const fn pending(&self) -> bool {
        self.0.flags & PENDING != 0
    }

    /// The screen that shows more of this entry, `None` where it names none:
    /// opened with a hand-over of `word`, `language` and the entry's text.
    #[must_use]
    pub fn opens(&self) -> Option<&'a str> {
        Some(self.0.opens).filter(|screen| !screen.is_empty())
    }
}

/// A `core.lookup` handle.
#[derive(Debug)]
#[must_use = "release the lookup handle when finished"]
pub struct Lookup {
    service: Service,
}

impl Lookup {
    /// Opens the service.
    ///
    /// # Errors
    /// `Unsupported` where the shell has no providers, otherwise the host's.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }

    /// Releases the handle.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }

    /// The entry of the provider page `question` names; `None` past the last
    /// provider, which ends the loop. `input` and `out` are [`BUFFER`] bytes.
    ///
    /// The entry borrows `out`, so `out` cannot be reused while it is read:
    ///
    /// ```compile_fail
    /// use paperpaw_sdk::native::{Host, lookup::{Lookup, Question}};
    /// fn ask_entries(l: &Lookup, h: &Host<'_>, q: &Question<'_>, i: &mut [u8], out: &mut [u8]) {
    ///     let entry = l.ask(h, q, i, out).unwrap();
    ///     out[0] = 0;
    ///     drop(entry);
    /// }
    /// ```
    ///
    /// # Errors
    /// `Invalid` for a reply that is not exactly one entry with a provider id and
    /// known flags (as the host checks it); `NoSpace` where `input` or `out` is
    /// too small; otherwise the host's, among them the status a provider that
    /// failed answered.
    pub fn ask<'a>(
        &self,
        host: &Host<'_>,
        question: &Question<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Option<ProviderEntry<'a>>, Error> {
        let operation = self
            .service
            .operation(schema::OP_LOOKUP)
            .ok_or(Error::Unsupported)?;
        let row = wire::ServiceLookupAsk {
            text: question.text,
            language: question.language,
            context: question.context,
            cursor: question.cursor,
        };
        let len = wire::records_encode_counted_of(
            [&row].into_iter(),
            usize::from(operation.input_rows),
            operation.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        let n = match self
            .service
            .call(host, schema::OP_LOOKUP, &input[..len], out)
        {
            Ok(n) => n,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let entry = RawRecord::one(&out[..n])
            .and_then(|record| record.decode::<wire::ServiceLookupEntry<'a>>())
            .map_err(|_| Error::Invalid)?;
        if entry.provider.is_empty() || entry.flags & !(MORE | PENDING) != 0 {
            return Err(Error::Invalid);
        }
        Ok(Some(ProviderEntry(entry)))
    }
}

#[cfg(test)]
mod tests;
