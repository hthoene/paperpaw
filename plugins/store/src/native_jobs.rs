//! What the plugin carries out for its screens (`Next`) and the work behind it:
//! fetching the list and the pages, the downloads and installs one plugin at a
//! time, the queue (*Update all*, the first-run offer, plugins other plugins ask
//! for), removing a plugin. The decisions are `native_download`,
//! `native_install`, `native_queue` and `native_refusal`'s; this is the host
//! side of them.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage as _;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, PluginEnabled, RedrawMode},
    events::{Event, InstallDone},
    net::HttpDone,
    ui::Instance,
};
use paperpaw_ui_kit::mem::push;

use crate::native_ask::{Say as _, Words};
use crate::native_download::{Done as FileDone, Failure, Started, Step as DownStep};
use crate::native_glue;
use crate::native_install::{End, Have, Next as Turn, hand_over, next as queue_next};
use crate::native_installs::HostInstalls;
use crate::native_load::{Details, Done, Ended, LIST_TOKEN, PAGE_TOKEN};
use crate::native_plugin::NativeStore;
use crate::native_queue::{Mark, Queue};
use crate::native_screens::Next;
use crate::native_wire::{DownloadWire, HostWire, InstallWire};

/// The plugin store's data folder as the reader opens a file of it.
const DATA_DIR: &str = ".paperpaw/plugins/store/";
/// The first request number of a removal: an install's own count is small.
const REMOVE_REQUEST: u32 = 0x7000_0000;
/// The setup's step, as the plugin's own item.
const SETUP_ITEM: &str = "store.setup";

/// What the device has of the plugin being installed, owned.
struct Present {
    version: Option<String>,
    repairs: bool,
}

/// What a plugin's download or install ended in, for the one who asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Result_ {
    Installed,
    NotInstalled,
}

impl NativeStore {
    /// Carries out what screen `instance` asked for.
    pub(crate) fn carry(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        next: Next,
    ) -> Result<(), Error> {
        match next {
            Next::Open { screen, params } => {
                let ui = self.ui(host)?;
                native_glue::open(host, ui, screen, &params)
            }
            Next::Close => back(host),
            Next::FetchList => self.fetch_list(host),
            Next::FetchPage(id) => self.fetch_page(host, &id),
            Next::Install(id) | Next::Update(id) => self.begin(host, &id),
            Next::Remove(id) => self.remove(host, &id),
            Next::Stop(id) => self.stop(host, &id),
            Next::UpdateAll => self.update_all(host),
            Next::Notify(key) => self.say(host, key),
            Next::CloseNotify(key) => {
                back(host)?;
                self.say(host, key)
            }
            Next::Said { key, name } => self.say_with(host, key, &name),
            Next::Again { by, ids } => self.again(host, &by, &ids),
            Next::Restart => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).restart()
            }
            Next::Redraw => redraw(host),
            Next::Ask { tag, ask } => self.start_ask(host, instance, tag, &ask),
            Next::CloseWith { key, value } => {
                self.keep_return(key, &value)?;
                back(host)
            }
            Next::Read(file) => self.read(host, &file),
            Next::SwitchOn(id) => self.switch_on(host, &id),
            Next::Offered {
                switch_on,
                queued,
                notify,
                close,
            } => {
                for id in switch_on {
                    self.switch_on(host, id)?;
                }
                if queued {
                    self.go_on(host)?;
                }
                if !notify.is_empty() {
                    self.say(host, notify)?;
                }
                if close { back(host) } else { Ok(()) }
            }
            Next::None => Ok(()),
        }
    }

    fn fetch_list(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut wire = HostWire::new(host, &mut self.handles);
        self.loader.start_list(&mut wire)?;
        redraw(host)
    }

    fn fetch_page(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        let mut wire = HostWire::new(host, &mut self.handles);
        self.loader.start_page(&mut wire, id)
    }

    /// The end of a request of the list or of a page.
    #[inline(never)]
    pub(crate) fn http_done(
        &mut self,
        host: &Host<'_>,
        done: &HttpDone,
        event: &Event<'_>,
    ) -> Result<(), Error> {
        match done.token() {
            LIST_TOKEN | PAGE_TOKEN => {
                let ended = Done::new(done.id(), done.token(), done.reason(), done.status());
                let mut wire = HostWire::new(host, &mut self.handles);
                let result = self.loader.ended(&mut wire, &ended)?;
                if result == Ended::ListChanged {
                    self.generation = self.generation.wrapping_add(1);
                }
                if result == Ended::Page {
                    self.page_arrived(host)?;
                }
            }
            crate::native_download::FILE_REQUEST => self.file_done(host, done)?,
            _ => {}
        }
        self.forward(host, event)
    }

    /// The page asked for came, or did not: the install it was asked for goes on.
    #[inline(never)]
    fn page_arrived(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(id) = self.want.clone() else {
            return Ok(());
        };
        match self.loader.details() {
            (Some(of), Details::Here) if of == id => {
                self.want = None;
                self.begin(host, &id)
            }
            (Some(of), Details::Offline | Details::Failed) if of == id => {
                self.want_failed(host, &id)
            }
            // A page screen took the slot for another plugin: ask for ours
            // again, once; a second time the plugin counts as not installed
            // (the queue never waits for a page that is not coming).
            (Some(of), _) if of != id => {
                if self.want_retried {
                    return self.want_failed(host, &id);
                }
                self.want_retried = true;
                match self.fetch_page(host, &id) {
                    Ok(()) => Ok(()),
                    Err(Error::NoMemory) => Err(Error::NoMemory),
                    Err(_) => self.want_failed(host, &id),
                }
            }
            _ => Ok(()),
        }
    }

    /// The page of plugin `id`, which the queue waits for, is not coming: the
    /// plugin is not installed and the queue goes on.
    fn want_failed(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        self.want = None;
        self.want_retried = false;
        self.not_installed(host, id, "")?;
        self.go_on(host)
    }

    /// Asks for the page of plugin `id` to install it; a request the host
    /// refuses is the plugin not installed.
    #[inline(never)]
    fn want_page(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        self.want = Some(crate::native_glue::own(id)?);
        self.want_retried = false;
        match self.fetch_page(host, id) {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => {
                self.want = None;
                Err(Error::NoMemory)
            }
            Err(_) => self.want_failed(host, id),
        }
    }

    /// Installs plugin `id`, once its files are known: its page is fetched
    /// first where it is not here.
    #[inline(never)]
    fn begin(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        if self.download_busy() {
            let name = self.busy_name(host)?;
            return self.say_with(host, "store.busy", &name);
        }
        let Some(page) = self.loader.page(id) else {
            return self.want_page(host, id);
        };
        // What the install needs of the page outlives it.
        let kept = paperpaw_ui_kit::mem::boxed(lean(page)?)?;
        let name = crate::native_glue::own(&page.entry.name)?;
        let storage = crate::native_folders::StoreStorage::new(host);
        let started = {
            let mut wire = DownloadWire::new(host, &mut self.handles);
            self.download.start(&mut wire, page)
        }?;
        let _ = storage;
        match started {
            Started::Downloading => {
                self.installing_version = Some(crate::native_glue::own(&kept.entry.version)?);
                self.activity.set_other_job(id, &name)?;
                self.activity.clear_no_room(id);
                self.installing = Some(kept);
                self.sync_step(id);
                redraw(host)
            }
            Started::NoNetwork => {
                self.requests_failed(host)?;
                self.say(host, "store.offline")
            }
            Started::Invalid | Started::Card => {
                self.not_installed(host, id, &name)?;
                self.go_on(host)
            }
        }
    }

    fn download_busy(&self) -> bool {
        matches!(
            self.download.step(),
            DownStep::Downloading { .. } | DownStep::Ready | DownStep::Installing
        )
    }

    /// The name of the plugin that is being downloaded.
    fn busy_name(&self, host: &Host<'_>) -> Result<String, Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        match crate::native_download::Download::progress(&storage)? {
            Some(job) => Ok(job.name),
            None => Ok(String::new()),
        }
    }

    /// A file of the download came, or did not.
    #[inline(never)]
    fn file_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let saved = done.reason() == paperpaw_sdk::native::net::Reason::Ok
            && (200..300).contains(&done.status());
        let ended = FileDone::new(done.id(), done.token(), done.reason(), saved);
        let step = {
            let mut wire = DownloadWire::new(host, &mut self.handles);
            self.download.ended(&mut wire, &ended)?
        };
        match step {
            DownStep::Ready => self.finish(host),
            DownStep::Failed(failure) => self.download_failed(host, failure),
            _ => {
                self.sync_current();
                Ok(())
            }
        }
    }

    /// All the files are here: the host installs them.
    #[inline(never)]
    fn finish(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        let Some(staged) = self.download.take_ready(&storage)? else {
            return Ok(());
        };
        let Some(page) = self.installing.take() else {
            // Nothing is known of what the files are for: they go.
            self.abandon(host, &staged);
            return self.go_on(host);
        };
        let (name, id, installed) = match self.prepare_finish(host, &staged, &page) {
            Ok(found) => found,
            Err(error) => {
                // The staged files and the busy mark must not outlive the
                // install that could not start.
                self.abandon(host, &staged);
                let _ = self.go_on(host);
                return Err(error);
            }
        };
        let have = Have {
            version: installed.version.as_deref(),
            repairs: installed.repairs,
        };
        let step = {
            let mut wire = InstallWire::new(host);
            if let Err(Error::NoMemory) = hand_over(&mut wire, &[]) {
                self.abandon(host, &staged);
                let _ = self.go_on(host);
                return Err(Error::NoMemory);
            }
            self.installer
                .finish(&mut wire, &mut self.download, staged, &page, have)?
        };
        self.sync_step(&id);
        if matches!(step, DownStep::Failed(_)) {
            self.installing_version = None;
            self.not_installed(host, &id, &name)?;
            return self.go_on(host);
        }
        redraw(host)
    }

    /// What an install needs to be sent: the names and what is installed of the
    /// plugin now.
    #[inline(never)]
    fn prepare_finish(
        &mut self,
        host: &Host<'_>,
        staged: &crate::native_download::Staged,
        page: &crate::native_list::Page,
    ) -> Result<(String, String, Present), Error> {
        self.ensure_snapshot(host)?;
        let snapshot = self.snapshot.as_ref().ok_or(Error::State)?;
        let installed = snapshot.have(&page.entry.id);
        let found = Present {
            version: installed
                .as_ref()
                .map(|i| crate::native_glue::own(i.version))
                .transpose()?,
            repairs: installed
                .as_ref()
                .is_some_and(|i| crate::native_page::repairs(&page.entry, i)),
        };
        Ok((
            crate::native_glue::own(&staged.name)?,
            crate::native_glue::own(&staged.plugin)?,
            found,
        ))
    }

    /// Lets go of an install that cannot go on: its staged files, its busy
    /// mark, what was kept for it.
    fn abandon(&mut self, host: &Host<'_>, staged: &crate::native_download::Staged) {
        let storage = crate::native_folders::StoreStorage::new(host);
        let _ = storage.remove_dir_all(&staged.folder);
        self.download.set_step(DownStep::Idle);
        self.installing = None;
        self.installing_version = None;
        self.sync_step(&staged.plugin);
    }

    /// The host's end of an install (`install_done`).
    #[inline(never)]
    pub(crate) fn install_done(
        &mut self,
        host: &Host<'_>,
        done: &InstallDone<'_>,
        event: &Event<'_>,
    ) -> Result<(), Error> {
        let Some(request) = done.request() else {
            return Ok(());
        };
        let request = request.get();
        if request >= REMOVE_REQUEST {
            self.removed(host, done)?;
            return self.forward(host, event);
        }
        let reason = reason_of(done);
        let flags = u32::from(done.data_kept()) | (u32::from(done.at_next_start()) << 1);
        let end = End::new(request, reason, flags);
        let release = self.loader_release(done.id()).unwrap_or_default();
        let outcome = {
            let mut wire = InstallWire::new(host);
            self.installer.done(
                &mut wire,
                &mut self.download,
                &end,
                &release,
                self.activity.flash_hint,
            )?
        };
        self.forget_snapshot();
        self.installing_version = None;
        self.sync_step(done.id());
        // A page the queue or the offer installed from is not kept for nobody.
        if !self.has_page_screen() {
            self.loader.forget_page();
        }
        // No room (the code room, the card) and a withdrawn key are what the
        // page of the plugin says of it afterwards.
        match reason_of(done) {
            7 => self.activity.note_revoked(done.id())?,
            13 | 17 => self.activity.note_no_room(done.id())?,
            _ => {}
        }
        use crate::native_install::Outcome;
        let said = match outcome {
            Outcome::Installed {
                plugin,
                name,
                updated,
                restart,
            } => {
                let key = match (restart, updated) {
                    (true, true) => "store.updated-code",
                    (true, false) => "store.installed-code",
                    (false, true) => "store.updated",
                    (false, false) => "store.installed",
                };
                self.noted(host, &plugin, &name, key, Result_::Installed)?;
                true
            }
            Outcome::Refused { name, key } => {
                self.noted(host, done.id(), &name, key, Result_::NotInstalled)?;
                true
            }
            Outcome::Silent => {
                self.noted(host, done.id(), "", "", Result_::NotInstalled)?;
                true
            }
            _ => false,
        };
        if said {
            self.go_on(host)?;
        }
        self.forward(host, event)
    }

    /// The release of the page the plugin was installed from.
    fn loader_release(&self, id: &str) -> Option<String> {
        // The version that was downloaded, whatever page is held now.
        self.installing_version
            .clone()
            .or_else(|| self.loader.page(id).map(|page| page.entry.version.clone()))
    }

    /// Says what became of plugin `id`, or notes it for the end of a quiet
    /// queue or the request that asked for it.
    #[inline(never)]
    fn noted(
        &mut self,
        host: &Host<'_>,
        id: &str,
        name: &str,
        key: &str,
        result: Result_,
    ) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        let queue = Queue::read(&storage)?;
        let done = result == Result_::Installed;
        if queue.requested() {
            let _ = crate::native_request::outcome(&storage, id, done);
        } else if queue.quiet() {
            let _ = crate::native_queue::note(&storage, name, done);
        } else if !key.is_empty() {
            self.say_with(host, key, name)?;
        }
        Ok(())
    }

    /// A plugin was not installed: said where nobody else is told.
    fn not_installed(&mut self, host: &Host<'_>, id: &str, name: &str) -> Result<(), Error> {
        self.noted(host, id, name, "store.failed", Result_::NotInstalled)
    }

    #[inline(never)]
    fn download_failed(&mut self, host: &Host<'_>, _why: Failure) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        let (id, name) = match crate::native_download::Download::progress(&storage)? {
            Some(job) => (job.plugin, job.name),
            None => (String::new(), String::new()),
        };
        self.installing = None;
        self.installing_version = None;
        self.sync_step(&id);
        self.not_installed(host, &id, &name)?;
        self.go_on(host)
    }

    /// The queue goes on: the next plugin gets its turn.
    pub(crate) fn go_on(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let result = self.go_on_next(host);
        // What the screens read of the queue follows it while they are open.
        if !self.screens_empty() {
            let _ = self.refresh_activity(host);
        }
        result
    }

    fn go_on_next(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.download_busy() || self.want.is_some() {
            return Ok(());
        }
        let storage = crate::native_folders::StoreStorage::new(host);
        let mut queue = Queue::read(&storage)?;
        match queue_next(&mut queue, &storage)? {
            Turn::FetchPage(id) => self.want_page(host, &id),
            _ => {
                self.queue_ended(host, &queue)?;
                redraw(host)
            }
        }
    }

    /// The queue is empty: what the plugins put in quietly became, and what a
    /// request's plugins became, is said.
    #[inline(never)]
    fn queue_ended(&mut self, host: &Host<'_>, queue: &Queue) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        if let Some(done) = crate::native_request::finished(&storage)? {
            self.requests_finished(host, &done)?;
        }
        if let Some(report) = crate::native_queue::report(&storage, queue)? {
            let mut words = Words::new(host)?;
            if !report.done.is_empty() {
                let text = words.format(
                    "store.offer.done",
                    &[Arg::Text {
                        name: "names",
                        value: &report.done,
                    }],
                )?;
                native_glue::notify(host, &text)?;
            }
            if !report.not.is_empty() {
                let text = words.format(
                    "store.offer.not-done",
                    &[Arg::Text {
                        name: "names",
                        value: &report.not,
                    }],
                )?;
                native_glue::notify(host, &text)?;
            }
        }
        Ok(())
    }

    /// *Update all*: every plugin with a newer version is queued.
    #[inline(never)]
    fn update_all(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure_snapshot(host)?;
        let storage = crate::native_folders::StoreStorage::new(host);
        let ids = self.updates(&storage)?;
        let mut queue = Queue::read(&storage)?;
        queue.add(ids.iter().map(String::as_str), Mark::Said)?;
        queue.keep(&storage)?;
        self.go_on(host)
    }

    /// Fetch again the plugins `ids` that plugin `by` asked for.
    #[inline(never)]
    fn again(&mut self, host: &Host<'_>, by: &str, ids: &[String]) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        crate::native_request::Request::again(&storage, by, ids)?;
        let mut queue = Queue::read(&storage)?;
        queue.add(ids.iter().map(String::as_str), Mark::Requested)?;
        queue.keep(&storage)?;
        self.go_on(host)
    }

    /// The ids of the plugins that have a newer version in the library's list.
    #[inline(never)]
    fn updates(
        &self,
        storage: &crate::native_folders::StoreStorage<'_, '_>,
    ) -> Result<Vec<String>, Error> {
        let mut ids = Vec::new();
        let (Some(index), Some(snapshot)) = (self.loader.index(), self.snapshot.as_ref()) else {
            return Ok(ids);
        };
        let betas = self.activity.takes_betas;
        index.scan(storage, &mut |_, entry| {
            if let Some(installed) = snapshot.have(&entry.id)
                && crate::native_page::newer(&entry, &installed, betas)
            {
                push(&mut ids, crate::native_glue::own(&entry.id)?)?;
            }
            Ok(())
        })?;
        Ok(ids)
    }

    /// Stops the install of plugin `id`; the next one in the queue goes on.
    #[inline(never)]
    fn stop(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        {
            let mut wire = DownloadWire::new(host, &mut self.handles);
            self.download.cancel(&mut wire);
        }
        {
            let mut wire = InstallWire::new(host);
            self.installer.forget(&mut wire);
        }
        self.download.set_step(DownStep::Idle);
        self.installing = None;
        self.installing_version = None;
        self.want = None;
        self.want_retried = false;
        self.sync_step(id);
        self.go_on(host)?;
        redraw(host)
    }

    /// Removes plugin `id`.
    #[inline(never)]
    fn remove(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        self.remove_count = self.remove_count.wrapping_add(1) & 0xffff;
        let request = REMOVE_REQUEST | self.remove_count.max(1);
        {
            let mut scratch = [0; 256];
            CommandSender::new(host, &mut scratch).remove_plugin(request, id)?;
        }
        // The name the reader knows it by, kept until the host answers.
        let name = match self.loader.page(id) {
            Some(page) => crate::native_glue::own(&page.entry.name)?,
            None => crate::native_glue::own(id)?,
        };
        self.removing = Some(paperpaw_ui_kit::mem::boxed((request, name))?);
        self.forget_snapshot();
        Ok(())
    }

    /// The host's end of a removal: said, and the record of the install let go,
    /// only where it was removed (declined or dismissed, nothing changed).
    #[inline(never)]
    fn removed(&mut self, host: &Host<'_>, done: &InstallDone<'_>) -> Result<(), Error> {
        use crate::native_refusal::{Outcome as Said, outcome};
        let name = match self.removing.take() {
            Some(removing) => removing.1,
            None => crate::native_glue::own(done.id())?,
        };
        self.forget_snapshot();
        match outcome(reason_of(done), self.activity.flash_hint) {
            Said::Done => {
                let id = crate::native_glue::own(done.id())?;
                let storage = crate::native_folders::StoreStorage::new(host);
                let _ = crate::native_install::Installs::forget(&storage, &id);
                self.say_with(host, "store.removed", &name)
            }
            Said::Said(key) => self.say_with(host, key, &name),
            _ => Ok(()),
        }
    }

    fn switch_on(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).set_plugin_enabled(id, PluginEnabled::On)?;
        self.forget_snapshot();
        redraw(host)
    }

    /// Shows a text file of the data folder in the reader.
    fn read(&mut self, host: &Host<'_>, file: &str) -> Result<(), Error> {
        let path = paperpaw_ui_kit::text::concat(&[DATA_DIR, file])?;
        let ui = self.ui(host)?;
        native_glue::open_reader(host, ui, &path)
    }

    fn say(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let mut words = Words::new(host)?;
        let text = words.text(key)?;
        native_glue::notify(host, &text)
    }

    fn say_with(&mut self, host: &Host<'_>, key: &str, name: &str) -> Result<(), Error> {
        let mut words = Words::new(host)?;
        let text = words.format(
            key,
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )?;
        native_glue::notify(host, &text)
    }

    /// Keeps the value a screen hands to the one below it.
    fn keep_return(&mut self, key: &str, value: &str) -> Result<(), Error> {
        self.returned = Some(paperpaw_ui_kit::mem::boxed((
            crate::native_glue::own(key)?,
            crate::native_glue::own(value)?,
        ))?);
        Ok(())
    }

    /// The plugin has started: a download cut off by the restart is begun anew
    /// from its page, or first when the queue goes on; the plugins the setup
    /// put in quietly go on without the store being opened; the setup's card
    /// says whether it applies.
    #[inline(never)]
    pub(crate) fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        let mut queue = Queue::read(&storage)?;
        queue.interrupted();
        queue.keep(&storage)?;
        {
            let mut wire = DownloadWire::new(host, &mut self.handles);
            self.download.cancel(&mut wire);
        }
        let quiet = queue.has_quiet();
        drop(queue);
        let state = self.setup_state(host);
        if quiet {
            self.go_on(host)?;
        }
        self.release_idle();
        state
    }

    /// Whether the setup's step applies, and what it says of the plugins chosen
    /// (`command.set_item_state`).
    #[inline(never)]
    pub(crate) fn setup_state(&mut self, host: &Host<'_>) -> Result<(), Error> {
        use crate::native_ask::{Badge, CHOSEN, OFFERED, badge, waiting_offered};
        use paperpaw_sdk::native::commands::ItemState;
        let storage = crate::native_folders::StoreStorage::new(host);
        // The start reads the rows of the plugins the offer names, not what the
        // device has, and none of it is kept.
        let present = crate::native_installs::Snapshot::read_some(host, &OFFERED)?;
        self.activity.read_facts(host, &storage, Some(&present))?;
        let queue = Queue::read(&storage)?;
        let installs = HostInstalls::new(&present, &self.activity);
        let applies = crate::native_screen_offer::applies(&installs);
        let how = badge(waiting_offered(&queue), storage.exists(CHOSEN));
        let mut scratch = [0; 256];
        let mut args = [0; 64];
        let mut sender = CommandSender::new(host, &mut scratch);
        let count = [Arg::Count(match how {
            Badge::BeingAdded(count) => i64::try_from(count).unwrap_or(i64::MAX),
            _ => 0,
        })];
        let state = if !applies {
            ItemState::new().hidden()
        } else {
            match how {
                Badge::BeingAdded(_) => ItemState::new().badge("store.offer.being-added", &count),
                Badge::Added => ItemState::new().badge("store.offer.added", &[]),
                _ => ItemState::new(),
            }
        };
        sender.set_item_state(SETUP_ITEM, &state, &mut args)
    }

    /// Nothing of the device or of the queue is kept while no screen of the
    /// store is open and no install goes on (#248).
    pub(crate) fn release_idle(&mut self) {
        if !self.screens_empty() {
            return;
        }
        self.forget_snapshot();
        if self.download_busy() || self.want.is_some() {
            return;
        }
        self.activity.queue = Queue::default();
        self.activity.step = Vec::new();
        self.activity.actions = Vec::new();
        self.activity.restart_needed = Vec::new();
    }

    /// Where the downloads and the installs stand, for the screens.
    pub(crate) fn refresh_activity(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let storage = crate::native_folders::StoreStorage::new(host);
        self.activity.queue = Queue::read(&storage)?;
        Ok(())
    }

    /// Tells the screens how plugin `id`'s install stands.
    fn sync_step(&mut self, id: &str) {
        use crate::native_screens::Step;
        let step = match self.download.step() {
            DownStep::Idle => Step::Idle,
            DownStep::Downloading { done, total } => Step::Downloading {
                done: u32::try_from(done).unwrap_or(u32::MAX),
                total: u32::try_from(total).unwrap_or(u32::MAX),
            },
            DownStep::Ready | DownStep::Installing => Step::Installing,
            DownStep::Installed => Step::Installed,
            DownStep::Failed(_) => Step::Failed(1),
        };
        if !self.download_busy() {
            self.activity.clear_other_job();
        }
        self.activity.step.retain(|(known, _)| known != id);
        if step != Step::Idle {
            let _ = push(
                &mut self.activity.step,
                (crate::native_glue::own(id).unwrap_or_default(), step),
            );
        }
    }

    fn sync_current(&mut self) {
        if let Some(id) = self.installing.as_ref().map(|p| p.entry.id.clone()) {
            self.sync_step(&id);
        }
    }
}

/// What an install needs of a page: the plugin's id, name and version, the
/// flags of its build and the library's word of its author; none of what the page
/// shows and none of the files (the download holds those on the card).
fn lean(page: &crate::native_list::Page) -> Result<crate::native_list::Page, Error> {
    let mut rotations = Vec::new();
    rotations
        .try_reserve_exact(page.rotations.len())
        .map_err(|_| Error::NoMemory)?;
    rotations.extend(page.rotations.iter().copied());
    Ok(crate::native_list::Page {
        entry: crate::native_model::Entry {
            id: crate::native_glue::own(&page.entry.id)?,
            name: crate::native_glue::own(&page.entry.name)?,
            version: crate::native_glue::own(&page.entry.version)?,
            code: page.entry.code,
            needs_newer: page.entry.needs_newer,
            waits_author: page.entry.waits_author,
            ..crate::native_model::Entry::default()
        },
        files: Vec::new(),
        code: page.code,
        author_key: page.author_key,
        recovery_key: page.recovery_key,
        rotations,
        more: crate::native_item::More::default(),
    })
}

fn back(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).back()
}

pub(crate) fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

/// The host's reason of an `install_done` as `device.md` 5.5 numbers it.
fn reason_of(done: &InstallDone<'_>) -> u32 {
    use paperpaw_sdk::native::device::DeviceOutcome as O;
    match done.outcome() {
        O::Ok => {
            if done.status().is_ok() {
                0
            } else {
                1
            }
        }
        O::Declined => 2,
        O::Dismissed => 3,
        O::Damaged => 4,
        O::OtherAuthor => 5,
        O::KeyChanged => 6,
        O::Revoked => 7,
        O::Unsigned => 8,
        O::UnknownKey => 9,
        O::NewerInterface => 10,
        O::OlderInterface => 11,
        O::OtherChip => 12,
        O::NoRoom => 13,
        O::NotNewer => 14,
        O::IdTaken => 15,
        O::CardError => 16,
        O::CardFull => 17,
        O::ContentChanged => 18,
        O::Unsupported => 19,
        O::Cancelled => 20,
        O::RestartPending => 21,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::lean;
    use crate::native_install::Proof;
    use crate::native_item::{More, Rotation};
    use crate::native_list::Page;
    use crate::native_model::Entry;

    #[test]
    fn what_an_install_keeps_of_a_page_is_the_librarys_word_of_its_author() {
        let page = Page {
            entry: Entry {
                id: "airy-theme".into(),
                ..Entry::default()
            },
            files: Vec::new(),
            code: true,
            author_key: Some([7; 32]),
            recovery_key: Some([8; 32]),
            rotations: vec![Rotation {
                to: [1; 32],
                signature: [2; 64],
            }],
            more: More::default(),
        };
        let kept = lean(&page).unwrap();
        let proof = Proof::of(&kept).unwrap();
        assert_eq!(proof.author, Some([7; 32]));
        assert_eq!(proof.recovery, Some([8; 32]));
        assert_eq!(proof.rotations.len(), 1);
    }
}
