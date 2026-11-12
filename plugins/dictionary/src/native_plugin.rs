//! The interface-7 dictionary (slices 1 and 2): the folder setting and the
//! StarDict dictionaries as a lookup provider. The provider answers by the
//! model's pages from the dictionaries of the folder, read through the host's
//! files; a dictionary whose indexes are not made is indexed in the
//! background (`core.work`), a step at a time, and answers once it is ready.
//! The panel is a later slice (`PORTING-7.md`).
use crate::{
    declarations::{POPUP_TOKEN, PROVIDER_TOKEN, WORK_KEY, WORK_TOKEN},
    native_disk::{HostRoot, Routed},
    native_folders::{self, Folders},
    native_model::{self, Source},
    native_popup::View,
    native_source::Dictionaries,
};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        events::{Event, Incoming},
        lookup::{self, Ask, Entry},
        registrations,
        reply::Reply,
        ui::Ui,
        work::{Work, WorkSpec},
    },
};
use paperpaw_stardict::Builder;
use paperpaw_ui_kit::Screens;

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
    ui: Option<Ui>,
    screens: Screens<View>,
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
        let folder = native_folders::folder_setting(host, &mut scratch)?;
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
                // Not scheduled: not kept either, or the next ask would not try again.
                if let Err(error) = Self::schedule(host) {
                    self.pending = Vec::new();
                    return Err(error);
                }
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
                    // Any failure, a refused allocation too, ends the indexing for this
                    // session: tried again at the next start, not at every ask.
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
        for batch in image::SETTINGS
            .iter()
            .chain(image::PROVIDERS)
            .chain(image::SCREENS)
        {
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
            Event::ScreenOpened {
                instance, handover, ..
            } => {
                if self.ui.is_none() {
                    self.ui = Some(Ui::open(host)?);
                }
                let ui = self.ui.as_ref().ok_or(Error::State)?;
                self.screens.open(instance, View::open(host, ui, handover)?)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.screens.shown(POPUP_TOKEN, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        view.draw(host, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.screens.top(token) {
            Some(view) => view.input(host, &input),
            None => Ok(Handled::No),
        }
    }
}
