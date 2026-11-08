//! The interface-7 Vocabulary: so far its settings and the recovery at start
//! (a word list or a deck left half written by a power cut is put right). The
//! words looked up, the screens, the flash cards' review and the rest follow
//! with their host pieces; see `PORTING-7.md`.
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Incoming, kinds},
    registrations,
    reply::Reply,
    storage::DataStorage,
};

use crate::files::DataFolder;
use crate::{cards, store};

mod image {
    include!(concat!(env!("OUT_DIR"), "/vocabulary_declarations.rs"));
}

/// The plugin's callback token (see `declarations.rs`).
const EVENTS: u32 = crate::declarations::EVENTS_TOKEN;

/// Vocabulary's interface-7 state: none yet.
#[derive(Debug, Default)]
pub struct NativeVocabulary;

impl Plugin for NativeVocabulary {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Incoming<'_>,
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token == EVENTS && event.decode_as::<kinds::Started>()?.is_some() {
            recover(&DataStorage::new(host));
        }
        Ok(())
    }
}

/// Puts right what a power cut left: the word list, then the decks. (Best
/// effort, as on interface 6: what cannot be put right now is tried at the
/// next start.)
pub(crate) fn recover(storage: &dyn paperpaw_sdk::Storage) {
    let folder = DataFolder(storage);
    store::recover(&folder);
    cards::recover(&folder);
}

#[cfg(test)]
mod tests;
