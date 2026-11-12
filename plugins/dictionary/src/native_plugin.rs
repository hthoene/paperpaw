//! The interface-7 dictionary (slices 1 and 2): the folder setting and the
//! StarDict dictionaries as a lookup provider. The provider answers by the
//! model's pages from the dictionaries of the folder, read through the host's
//! files; a dictionary whose indexes are not made is indexed in the
//! background (`core.work`), a step at a time, and answers once it is ready.
//! The panel is a later slice (`PORTING-7.md`).
use crate::{
    declarations::{POPUP_TOKEN, PROVIDER_TOKEN, WORK_KEY, WORK_TOKEN},
    native_disk::{HostRoot, Routed, failed_file},
    native_folders::{self, Folders},
    native_model::{self, Source},
    native_popup::View,
    native_source::{Dictionaries, Pending},
};
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        events::{Event, Incoming},
        lookup::{self, Ask, Entry},
        registrations,
        reply::Reply,
        ui::{Instance, Ui},
        work::{Work, WorkSpec},
    },
};
use paperpaw_ui_kit::Screens;

/// Bytes of an index read per step of the background work.
const STEP_BYTES: usize = 64 * 1024;

/// The block indexes being made, and how far they are.
#[derive(Debug, Default)]
struct Index {
    pending: Vec<Pending>,
    /// Bytes read, and in all, since the first of them started.
    done: u64,
    total: u64,
}

impl Index {
    /// Takes the indexes of dictionaries that are not being made yet; whether
    /// any was new.
    fn adopt(&mut self, more: Vec<Pending>) -> Result<bool, Error> {
        if self.pending.is_empty() {
            (self.done, self.total) = (0, 0);
        }
        let mut added = false;
        for next in more {
            if self.pending.iter().any(|have| have.cache == next.cache) {
                continue;
            }
            self.pending.try_reserve(1).map_err(|_| Error::NoMemory)?;
            self.total = self.total.saturating_add(next.builder.progress().1);
            self.pending.push(next);
            added = true;
        }
        Ok(added)
    }

    /// How far the indexes are, in percent.
    fn percent(&self) -> u32 {
        u32::try_from(self.done.saturating_mul(100) / self.total.max(1)).unwrap_or(100)
    }

    fn clear(&mut self) {
        self.pending = Vec::new();
    }
}

mod image {
    include!(concat!(env!("OUT_DIR"), "/dictionary_declarations.rs"));
}

/// The interface-7 dictionary.
#[derive(Debug, Default)]
pub struct NativeDictionary {
    /// The indexes the background work is making.
    index: Index,
    /// An index could not be made for a reason that may pass: not tried again
    /// until the next start. (A damaged dictionary is kept in a file by its
    /// cache, [`crate::native_disk::failed_file`].)
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
        if !self.gave_up && self.index.pending.is_empty() {
            let pending = source.pending()?;
            self.start(host, pending)?;
        }
        answered
    }

    /// Takes the indexes still to be made and declares the work that makes
    /// them, if none runs. Not scheduled: not kept either, or the next ask
    /// would not try again.
    fn start(&mut self, host: &Host<'_>, pending: Vec<Pending>) -> Result<(), Error> {
        let running = !self.index.pending.is_empty();
        if !self.index.adopt(pending)? || running {
            return Ok(());
        }
        if let Err(error) = Self::schedule(host) {
            self.index.clear();
            return Err(error);
        }
        Ok(())
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

    /// One step of the indexing: the first index a while, then the rest. A
    /// damaged dictionary is skipped now and until its files change; any other
    /// failure, a refused allocation too, ends the indexing for this session
    /// (tried again at the next start, not at every ask).
    fn index(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (files, data) = (HostRoot::files(host), HostRoot::data(host));
        let storage = Routed {
            files: &files,
            data: &data,
        };
        if let Some(first) = self.index.pending.first_mut() {
            let before = first.builder.progress().0;
            let stepped = first.builder.step(&storage, STEP_BYTES);
            let after = first.builder.progress().0;
            self.index.done = self.index.done.saturating_add(after.saturating_sub(before));
            match stepped {
                Ok(false) => {}
                Ok(true) => {
                    self.index.pending.remove(0);
                }
                Err(paperpaw_stardict::Error::Malformed(_)) => self.skip_damaged(&storage),
                Err(_) => {
                    self.index.clear();
                    self.gave_up = true;
                }
            }
        }
        self.tell_panels(host);
        if self.index.pending.is_empty() {
            Ok(())
        } else {
            Self::schedule(host)
        }
    }

    /// Whether a panel waits for the indexes once they were started: not where
    /// the work could not be declared (what is ready is what there is), and a
    /// refused allocation is an error.
    fn waits_for(started: Result<(), Error>) -> Result<bool, Error> {
        match started {
            Ok(()) => Ok(true),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }

    /// Forgets a closed panel. Beta 3 stopped the indexing with the last panel
    /// that waited for it.
    fn close_screen(&mut self, instance: Instance) {
        let waiting = self.screens.all().any(|view| view.preparing());
        self.screens.close(instance);
        if waiting && !self.screens.all().any(|view| view.preparing()) {
            self.index.clear();
        }
    }

    /// The dictionary of the first index is left out, and a file in its cache
    /// says it could not be indexed from these files.
    fn skip_damaged(&mut self, storage: &Routed<'_>) {
        let Some(first) = self.index.pending.first() else {
            return;
        };
        // Best effort: without the file it is tried again at the next start.
        if let Ok(path) = failed_file(&first.cache) {
            storage.write(&path, first.stamp.as_bytes()).ok();
        }
        let cache = first.cache.clone();
        let rest: u64 = self
            .index
            .pending
            .iter()
            .filter(|p| p.cache == cache)
            .map(|p| {
                p.builder
                    .progress()
                    .1
                    .saturating_sub(p.builder.progress().0)
            })
            .sum();
        self.index.total = self.index.total.saturating_sub(rest);
        self.index.pending.retain(|p| p.cache != cache);
    }

    /// Tells the open panels how far the indexes are, or that they are made.
    fn tell_panels(&mut self, host: &Host<'_>) {
        let percent = if self.index.pending.is_empty() {
            None
        } else {
            Some(self.index.percent())
        };
        for view in self.screens.all() {
            if view.preparing() && view.indexed(host, percent).is_err() {
                // What could not be read is not worth failing the work for:
                // the panel stops waiting and says nothing was found.
                view.give_up();
            }
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
                // Where indexing was given up, what is ready is what there is.
                let (mut view, pending) = View::open(host, ui, handover, !self.gave_up)?;
                if !pending.is_empty() {
                    if Self::waits_for(self.start(host, pending))? {
                        view.prepare(self.index.percent());
                    } else {
                        view.indexed(host, None)?;
                    }
                }
                self.screens.open(instance, view)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.screens.shown(POPUP_TOKEN, instance),
            Event::ScreenClosed { instance, .. } => {
                self.close_screen(instance);
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// The instance number `n` the host issues: read from an event as the
    /// plugin reads it.
    fn instance(n: u32) -> Instance {
        use paperpaw_core::native_host::registrations::{
            EventScreenOpened, Record, records_encode,
        };
        let record = Record::EventScreenOpened(EventScreenOpened {
            now_ms: 0,
            screen: "dictionary.panel",
            instance: n,
            slot: "",
            flags: 0,
            handover: 0,
            params: 0,
            text_bytes: 0,
        });
        let mut bytes = [0_u8; 256];
        let len = records_encode(core::slice::from_ref(&record), &mut bytes).unwrap();
        match Incoming::new(&bytes[..len]).unwrap().decode().unwrap() {
            Event::ScreenOpened { instance, .. } => instance,
            other => panic!("{other:?}"),
        }
    }

    /// One index still to be made: a dictionary on a card, never indexed.
    fn pending() -> Vec<Pending> {
        use crate::{
            native_disk::Routed,
            native_model::{Item, Listing},
            native_source::Dictionaries,
        };
        use paperpaw_sdk::MemoryStorage;
        struct Tree;
        impl Listing for Tree {
            fn list(&mut self, _: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
                for name in ["a.ifo", "a.idx", "a.dict"] {
                    each(Item {
                        name,
                        is_dir: false,
                    });
                }
                Ok(())
            }
        }
        let (files, data) = (MemoryStorage::new(), MemoryStorage::new());
        let mut idx = b"cat\0".to_vec();
        idx.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        let ifo = format!(
            "StarDict's dict ifo file\nversion=2.4.2\nwordcount=1\nidxfilesize={}\nbookname=A\nsametypesequence=m\n",
            idx.len()
        );
        files.write("d/a.ifo", ifo.as_bytes()).unwrap();
        files.write("d/a.idx", &idx).unwrap();
        files.write("d/a.dict", b"x").unwrap();
        let storage = Routed {
            files: &files,
            data: &data,
        };
        let mut tree = Tree;
        Dictionaries::new(&mut tree, &storage, "d")
            .unwrap()
            .pending()
            .unwrap()
    }

    #[test]
    fn work_that_cannot_be_declared_leaves_the_panel_with_what_is_ready() {
        assert_eq!(NativeDictionary::waits_for(Ok(())), Ok(true));
        for refused in [Error::State, Error::Invalid, Error::NotFound] {
            assert_eq!(
                NativeDictionary::waits_for(Err(refused)),
                Ok(false),
                "no waiting for work that will not run: {refused:?}"
            );
        }
        assert_eq!(
            NativeDictionary::waits_for(Err(Error::NoMemory)),
            Err(Error::NoMemory),
            "a refused allocation still fails"
        );
    }

    #[test]
    fn the_indexing_stops_when_the_last_panel_that_waits_for_it_closes() {
        let (first, second) = (instance(1), instance(2));
        let mut plugin = NativeDictionary::default();
        assert!(plugin.index.adopt(pending()).unwrap());
        for instance in [first, second] {
            let mut view = View::default();
            view.prepare(10);
            plugin.screens.open(instance, view).unwrap();
        }
        plugin.close_screen(first);
        assert!(
            !plugin.index.pending.is_empty(),
            "another panel still waits"
        );
        plugin.close_screen(second);
        assert!(plugin.index.pending.is_empty(), "no panel waits: stopped");
        // A panel that waited for nothing does not stop what an ask started.
        assert!(plugin.index.adopt(pending()).unwrap());
        plugin.screens.open(first, View::default()).unwrap();
        plugin.close_screen(first);
        assert!(!plugin.index.pending.is_empty());
    }

    #[test]
    fn the_percent_of_the_indexes_is_what_is_read_of_what_there_is() {
        let mut index = Index::default();
        assert_eq!(
            index.percent(),
            0,
            "nothing to read is not a division by zero"
        );
        (index.done, index.total) = (50, 200);
        assert_eq!(index.percent(), 25);
        index.done = 200;
        assert_eq!(index.percent(), 100);
        index.clear();
        assert!(index.pending.is_empty());
    }
}
