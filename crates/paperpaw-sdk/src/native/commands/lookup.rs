//! The provider's end of an asynchronous lookup (`docs/abi/document.md` 7):
//! a provider that must fetch answers its stage call at once with a `pending`
//! entry, starts its own request and, when it keeps the result, stages
//! `command.lookup_ready`. The host tells each asker that subscribed to
//! [`EventKind::LookupReady`](crate::native::declare::EventKind) with
//! [`Event::LookupReady`](crate::native::events::Event); the asker then asks
//! again with the same text and language and gets the kept answer.
use super::{CommandSender, Error, wire};

impl CommandSender<'_, '_> {
    /// Tell the askers waiting on this provider that its answer for `text` in
    /// `language` is kept (`command.lookup_ready`). Use the same `text` and
    /// `language` the ask carried; `language` may be empty where the ask's was.
    /// A ready that matches no pending ask is accepted and counted by the host,
    /// never an error.
    ///
    /// # Errors
    /// Returns Invalid for empty text, text over 1,024 bytes or a language over
    /// 35 bytes (nothing is staged), NoSpace for short scratch, or the host's
    /// refusal.
    pub fn lookup_ready(
        &mut self,
        text: impl AsRef<str>,
        language: impl AsRef<str>,
    ) -> Result<(), Error> {
        let (text, language) = (text.as_ref(), language.as_ref());
        if text.is_empty()
            || text.len() > wire::COMMAND_LOOKUP_READY_TEXT_MAX_BYTES
            || language.len() > wire::COMMAND_LOOKUP_READY_LANGUAGE_MAX_BYTES
        {
            return Err(Error::Invalid);
        }
        self.submit(wire::Command::LookupReady { text, language })
    }
}
