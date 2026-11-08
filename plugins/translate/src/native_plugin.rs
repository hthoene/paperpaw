//! The interface-7 translation plugin (slice 2): the panel's screen (its offer
//! for the slot `core.translate` and its subscription to the end of a request
//! are in the image but not submitted) (the
//! language setting follows with draft 29, which lets a choice have 33
//! values), asking DeepL over `core.net` and the key kept with `core.secrets`
//! (`native_ask`). Showing the translation needs the screen hosts (slice 3).
//! What the plugin decides without a host is in `native_model`, tested without
//! one (`PORTING-7.md`).
use crate::{declarations::HTTP_TOKEN, native_ask::Asker, native_model::Outcome};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
};

#[expect(
    dead_code,
    reason = "the slot offer and the subscription are in the image, submitted once the host admits them"
)]
mod image {
    include!(concat!(env!("OUT_DIR"), "/translate_declarations.rs"));
}

/// The interface-7 translation plugin.
#[derive(Debug, Default)]
pub struct NativeTranslate {
    asker: Asker,
    outcome: Option<Outcome>,
}

impl NativeTranslate {
    /// The asking side: the key and the request.
    pub const fn asker(&mut self) -> &mut Asker {
        &mut self.asker
    }

    /// What the last request came to, once; `None` while it is in flight.
    pub const fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }
}

impl Plugin for NativeTranslate {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        // `image::EVENTS` (the subscription to the end of a request) is submitted
        // when the kernel's host admits event declarations (#944): it refuses
        // the plugin's construction until then, and a `make` that fails keeps
        // the whole plugin from being admitted. Without the subscription a
        // request to `core.net` is refused as `State`; nothing starts one yet.
        for batch in image::SCREEN {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token != HTTP_TOKEN {
            return Ok(());
        }
        // Only the end of a request is read, so only its decoder is linked.
        if let Some(Event::HttpDone(done)) = incoming.decode_as::<kinds::HttpDone>()?
            && let Some(outcome) = self.asker.done(host, &done)?
        {
            self.outcome = Some(outcome);
        }
        Ok(())
    }
}
