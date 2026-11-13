//! The lookup provider beside the dictionaries (`docs/abi/document.md` 7):
//! the dictionary panel asks, the provider answers *pending* at once, fetches
//! the summary through `core.net` as itself and keeps it; once kept it stages
//! `lookup_ready` with the ask's text and language, and the panel's next ask
//! is answered from what was kept, with the summary screen to open for the
//! whole of it (Beta 3: *Open Wikipedia ›*).
//!
//! As in Beta 3, an answer that is not a summary (no article, the network
//! not reached, an unexpected answer) is an entry that says why. One ask is
//! fetched and one answer kept at a time: a new ask replaces both.
use crate::{
    native_model::{self, Outcome},
    native_view::{SCREEN, read_body, request_summary},
};
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    i18n,
    lookup::{self, Ask, Entry},
    net::{HttpDone, Net, Reason, RequestId},
    reply::Reply,
};
use paperpaw_ui_kit::{Messages, mem::buffer, text};

/// The ask being fetched.
#[derive(Debug)]
struct Asked {
    text: String,
    language: String,
    request: RequestId,
}

/// What a fetch came to.
#[derive(Debug)]
enum Answer {
    /// The summary, with the article's title where the answer had one.
    Summary { title: Option<String>, text: String },
    /// Why there is none: the key of the text.
    Message(&'static str),
}

/// An answer kept for the ask of `text` in `language`.
#[derive(Debug)]
struct Kept {
    text: String,
    language: String,
    answer: Answer,
}

/// The provider's state: the ask in flight and the last answer kept.
#[derive(Debug, Default)]
pub struct Provider {
    asked: Option<Asked>,
    kept: Option<Kept>,
}

impl Provider {
    /// Answers the stage call of an ask (`payload`): from what was kept for
    /// it, *pending* while it is fetched, else *pending* after asking for it.
    /// Without a network service, or where the request is refused, the
    /// answer says so at once.
    ///
    /// # Errors
    /// `Invalid` for a payload that is not one ask; `NoMemory`; the host's
    /// refusals; `NoSpace` for a reply buffer too small for the entry.
    pub fn ask(
        &mut self,
        host: &Host<'_>,
        net: Option<&Net>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let ask = Ask::decode(payload)?;
        let language = ask.language().unwrap_or("");
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "wikipedia.title")?;
        if let Some(kept) = self
            .kept
            .as_ref()
            .filter(|kept| kept.text == ask.text() && kept.language == language)
        {
            return answer(host, &mut messages, &title, &kept.answer, reply);
        }
        let fetching = self
            .asked
            .as_ref()
            .is_some_and(|asked| asked.text == ask.text() && asked.language == language);
        if fetching {
            return lookup::answer(reply, &Entry::pending(&title)?);
        }
        let failed = match (net, ask.text().trim().is_empty()) {
            (_, true) => Some(Answer::Message("wikipedia.not-found")),
            // No network service at all: as Beta 3's request that never
            // reached the network.
            (None, false) => Some(Answer::Message("wikipedia.unreachable")),
            (Some(net), false) => match self.fetch(host, net, &ask, language) {
                Ok(()) => None,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => Some(Answer::Message("wikipedia.failed")),
            },
        };
        match failed {
            Some(why) => answer(host, &mut messages, &title, &why, reply),
            None => lookup::answer(reply, &Entry::pending(&title)?),
        }
    }

    /// Asks `core.net` for the summary of `ask` in `language` (the
    /// interface's where the ask has none).
    fn fetch(
        &mut self,
        host: &Host<'_>,
        net: &Net,
        ask: &Ask<'_>,
        language: &str,
    ) -> Result<(), Error> {
        let (text, kept_language) = (text::copy(ask.text())?, text::copy(language)?);
        let mut scratch = [0_u8; 64];
        let wiki_language = if language.is_empty() {
            i18n::language(host, &mut scratch).unwrap_or("en")
        } else {
            language
        };
        let request = request_summary(host, net, wiki_language, ask.text())?;
        self.asked = Some(Asked {
            text,
            language: kept_language,
            request,
        });
        Ok(())
    }

    /// A request ended: where it is this provider's, the answer is kept and
    /// the askers are told (`lookup_ready`). `false` for another request.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn done(&mut self, host: &Host<'_>, net: &Net, done: &HttpDone) -> Result<bool, Error> {
        if self
            .asked
            .as_ref()
            .is_none_or(|asked| asked.request != done.id())
        {
            return Ok(false);
        }
        let answer = match done.reason() {
            Reason::Ok => match read_body(host, net, done) {
                Ok(body) => {
                    outcome(done.status(), &body).unwrap_or(Answer::Message("wikipedia.failed"))
                }
                // An answer that cannot be read is no answer, never a hang:
                // the askers are told all the same.
                Err(_) => Answer::Message("wikipedia.failed"),
            },
            // Beta 3 said the network was not reached for every request that
            // did not get through (its status 0).
            Reason::Offline
            | Reason::Saving
            | Reason::NetworkOff
            | Reason::NoNetwork
            | Reason::Dns
            | Reason::Tls
            | Reason::Timeout
            | Reason::Refused => Answer::Message("wikipedia.unreachable"),
            _ => Answer::Message("wikipedia.failed"),
        };
        let Some(asked) = self.asked.take() else {
            return Ok(false);
        };
        let mut scratch = buffer(READY_SCRATCH)?;
        // `ready` is never an error to the provider (`document.md` 7): where
        // the host refuses it, the answer is kept all the same and the
        // asker's next ask gets it.
        let _ = CommandSender::new(host, &mut scratch).lookup_ready(&asked.text, &asked.language);
        self.kept = Some(Kept {
            text: asked.text,
            language: asked.language,
            answer,
        });
        Ok(true)
    }

    /// The whole summary kept for `word` (the screen an entry opens shows it
    /// without fetching it again): its title and text.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn summary_for(&self, word: &str) -> Result<Option<(Option<String>, String)>, Error> {
        let Some(Kept {
            answer: Answer::Summary { title, text },
            ..
        }) = self.kept.as_ref().filter(|kept| kept.text == word)
        else {
            return Ok(None);
        };
        let title = title.as_deref().map(text::copy).transpose()?;
        Ok(Some((title, text::copy(text)?)))
    }
}

/// The entry for `kept`: the summary (cut to an entry's 2,048 bytes) that
/// opens the summary screen, or why there is none.
fn answer(
    host: &Host<'_>,
    messages: &mut Messages,
    title: &str,
    kept: &Answer,
    reply: &mut Reply<'_>,
) -> Result<(), Error> {
    match kept {
        Answer::Summary { text, .. } => {
            let entry = Entry::new(title, cut(text, lookup::MAX_ENTRY))?.opens(SCREEN)?;
            lookup::answer(reply, &entry)
        }
        Answer::Message(key) => {
            let why = messages.copy(host, key)?;
            lookup::answer(reply, &Entry::new(title, cut(&why, lookup::MAX_ENTRY))?)
        }
    }
}

/// What the answer with `status` and `body` came to.
fn outcome(status: u32, body: &str) -> Result<Answer, Error> {
    let status = u16::try_from(status).unwrap_or(u16::MAX);
    Ok(match native_model::outcome(status, body) {
        Ok(Outcome::Found { title, text }) => Answer::Summary { title, text },
        Ok(Outcome::NotFound) => Answer::Message("wikipedia.not-found"),
        Ok(Outcome::Unreachable) => Answer::Message("wikipedia.unreachable"),
        Ok(_) | Err(Error::NoSpace | Error::Invalid) => Answer::Message("wikipedia.failed"),
        Err(error) => return Err(error),
    })
}

/// `text` cut at a character to at most `max` bytes.
fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// The scratch of `lookup_ready`: the ask's text (1,024) and language (35)
/// with the command's framing.
const READY_SCRATCH: usize = lookup::MAX_TEXT + lookup::MAX_LANGUAGE + 64;

#[cfg(test)]
mod tests {
    use super::cut;

    #[test]
    fn a_text_is_cut_at_a_character() {
        assert_eq!(cut("äbc", 1), "");
        assert_eq!(cut("äbc", 2), "ä");
        assert_eq!(cut("abc", 8), "abc");
    }
}
