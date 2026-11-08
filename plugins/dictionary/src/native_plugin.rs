//! The interface-7 dictionary (slice 1): the folder setting and the StarDict
//! dictionaries as a lookup provider. The provider answers by the model's
//! pages; what it reads dictionaries from (the host's files) and the panel
//! are later slices, so until then every ask is answered "no entry"
//! (`PORTING-7.md`).
use crate::{
    declarations::PROVIDER_TOKEN,
    native_model::{self, Found, Source},
};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::Event,
    lookup::{self, Ask, Entry},
    registrations,
    reply::Reply,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/dictionary_declarations.rs"));
}

/// The dictionaries a provider answers from: none are readable until the
/// host's files reach plugins (`core.files`, #339/#384).
#[derive(Debug, Default)]
struct Unreadable;

impl Source for Unreadable {
    fn ready(&mut self) -> Result<usize, Error> {
        Ok(0)
    }

    fn definitions(&mut self, _: usize, _: &str) -> Result<Vec<Found>, Error> {
        Err(Error::Unsupported)
    }
}

/// The interface-7 dictionary.
#[derive(Debug, Default)]
pub struct NativeDictionary;

impl NativeDictionary {
    /// Answers one ask of the `core.lookup` stage with the entry its cursor
    /// names, or `NotFound` where the dictionaries have none.
    fn answer(source: &mut impl Source, ask: &Ask<'_>, reply: &mut Reply<'_>) -> Result<(), Error> {
        let cursor = usize::try_from(ask.cursor()).map_err(|_| Error::Invalid)?;
        let page = native_model::page_at(source, ask.text(), cursor)?.ok_or(Error::NotFound)?;
        let (title, text) = native_model::fit(&page.found);
        let entry = Entry::new(title, text)?;
        lookup::answer(reply, &if page.more { entry.more() } else { entry })
    }
}

impl Plugin for NativeDictionary {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::PROVIDERS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }

    fn event(
        &mut self,
        _: &Host<'_>,
        token: u32,
        event: Event<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::StageCall { stage, payload, .. }
                if token == PROVIDER_TOKEN && stage == "core.lookup" =>
            {
                let ask = Ask::decode(payload)?;
                Self::answer(&mut Unreadable, &ask, reply)
            }
            _ => Ok(()),
        }
    }
}
