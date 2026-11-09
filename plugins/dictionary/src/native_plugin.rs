//! The interface-7 dictionary (slices 1 and 2): the folder setting and the
//! StarDict dictionaries as a lookup provider. The provider answers by the
//! model's pages from the dictionaries of the folder, read through the host's
//! files; a dictionary whose indexes are not made is indexed in the
//! background (`core.work`), a step at a time, and answers once it is ready.
//! The panel is a later slice (`PORTING-7.md`).
use crate::{
    declarations::{DEFAULT_FOLDER, PROVIDER_TOKEN, WORK_KEY, WORK_TOKEN},
    native_disk::{HostRoot, Routed},
    native_folders::Folders,
    native_model::{self, Source},
    native_source::Dictionaries,
};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming},
    lookup::{self, Ask, Entry},
    registrations,
    reply::Reply,
    settings::{self, SettingValue},
    work::{Work, WorkSpec},
};
use paperpaw_stardict::Builder;

/// The setting of the folder.
const FOLDER_KEY: &str = "dictionary.folder";
/// Bytes of an index read per step of the background work.
const STEP_BYTES: usize = 64 * 1024;

mod image {
    include!(concat!(env!("OUT_DIR"), "/dictionary_declarations.rs"));
}

/// The interface-7 dictionary.
#[derive(Debug, Default)]
pub struct NativeDictionary {
    /// The indexes the background work is making.
    pending: Vec<Builder>,
    /// An index could not be made: not tried again until the next start.
    gave_up: bool,
}

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

    /// Answers an ask from the files, and starts the work that makes the
    /// indexes still missing.
    fn ask(&mut self, host: &Host<'_>, ask: &Ask<'_>, reply: &mut Reply<'_>) -> Result<(), Error> {
        let mut scratch = [0u8; 300];
        let folder = match settings::read_setting(host, FOLDER_KEY, &mut scratch)? {
            SettingValue::Text(text) if !text.trim_matches('/').is_empty() => text,
            _ => DEFAULT_FOLDER,
        };
        let (files, data) = (HostRoot::files(host), HostRoot::data(host));
        let storage = Routed {
            files: &files,
            data: &data,
        };
        let mut folders = Folders::open(host)?;
        let outcome = self.ask_in(host, &mut folders, &storage, folder, ask, reply);
        let released = folders.finish();
        outcome.and(released)
    }

    fn ask_in(
        &mut self,
        host: &Host<'_>,
        folders: &mut Folders<'_, '_>,
        storage: &Routed<'_>,
        folder: &str,
        ask: &Ask<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let mut source = Dictionaries::new(folders, storage, folder)?;
        let answered = Self::answer(&mut source, ask, reply);
        if self.pending.is_empty() && !self.gave_up {
            self.pending = source.builders()?;
            if !self.pending.is_empty() {
                Self::schedule(host)?;
            }
        }
        answered
    }

    /// Declares the work that does the next step of the indexing.
    fn schedule(host: &Host<'_>) -> Result<(), Error> {
        let work = Work::open(host)?;
        let mut input = [0u8; 256];
        let declared = WorkSpec::new(WORK_KEY, WORK_TOKEN)
            .and_then(|spec| work.declare(host, &spec, &mut input, &mut []));
        let released = work.release(host);
        declared.and(released)
    }

    /// One step of the indexing: the first index a while, then the rest.
    fn index(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (files, data) = (HostRoot::files(host), HostRoot::data(host));
        let storage = Routed {
            files: &files,
            data: &data,
        };
        if let Some(builder) = self.pending.first_mut() {
            match builder.step(&storage, STEP_BYTES) {
                Ok(false) => {}
                Ok(true) => {
                    self.pending.remove(0);
                }
                Err(_) => {
                    // Tried again at the next start, not at every ask.
                    self.pending = Vec::new();
                    self.gave_up = true;
                }
            }
        }
        if self.pending.is_empty() {
            Ok(())
        } else {
            Self::schedule(host)
        }
    }
}

impl Plugin for NativeDictionary {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::PROVIDERS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let event = incoming.decode()?;
        match event {
            Event::StageCall { stage, payload, .. }
                if token == PROVIDER_TOKEN && stage == "core.lookup" =>
            {
                let ask = Ask::decode(payload)?;
                self.ask(host, &ask, reply)
            }
            Event::WorkDue(_) if token == WORK_TOKEN => self.index(host),
            _ => Ok(()),
        }
    }
}
