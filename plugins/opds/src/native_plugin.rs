//! The interface-7 catalogs: the list of catalogs, the editor of one (its
//! password among the plugin's secrets), browsing a catalog with its downloads
//! and covers, and the sync of the catalogs marked for it. The rules are the
//! engines' (`native_catalogs`, `native_browse`, `native_sync`: host-free or
//! driven through scripted services in the tests); this file feeds them what
//! comes (the screens' input, the events of the network, of the work service
//! and of the images) and shows what they say. `PORTING-7.md` lists what is
//! not ported.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    device::{Device, DeviceCapability, DeviceClass},
    document::{Documents, FormatCursor},
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    library::Library,
    registrations,
    reply::Reply,
    secrets::Secrets,
    settings::{self, SettingValue},
    ui::{Notify, Open, Question, RequestId as AskId, TextMode, TextWrite, Ui},
    work::Work,
};
use paperpaw_ui_kit::{
    Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    read_event,
    text::copy,
};

use crate::declarations::{
    BROWSE_SCREEN, CATALOGS_SCREEN, EDIT_SCREEN, HTTP_TOKEN, IMAGE_TOKEN, OPEN_TOKEN,
    PROGRESS_TOKEN, STOP_TOKEN, SYNC_TOKEN, WORK_TOKEN,
};
use crate::native_catalogs::{self as catalogs, Catalog, Vault};
use crate::native_files::DataFiles;
use crate::native_keep::{self as keep, Secret};
use crate::native_sync::Sync;
use crate::native_web::Web;

mod browse;
mod edit;
mod list;
mod sync;

mod image {
    include!(concat!(env!("OUT_DIR"), "/opds_declarations.rs"));
}

/// The library folder, where books go below.
const LIBRARY_FOLDER: &str = "core.library-folder";
/// What a kept password is shown as.
const DOTS: &str = "••••••";

/// What the next screen to open is for (the screens take no parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// Browse the catalog at this place in the list.
    Browse(usize),
    /// Edit it.
    Edit(usize),
}

/// A question the plugin asked, and what its answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// The address of a new catalog.
    Address,
    /// The text of a field of the catalog being edited.
    Field(edit::Field),
    /// What to do with the catalog at this place in the list.
    Offer(usize),
    /// The words to search for.
    Terms,
    /// Whether to read the book just saved.
    Read,
}

/// The state of one open screen.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "a view is made fallibly and kept in place; boxing would take an infallible allocation"
)]
enum View {
    List(list::ListView),
    Edit(edit::EditView),
    Browse(browse::BrowseView),
}

/// The state of the catalogs: made at the first need ([`NativeOpds`]).
#[derive(Debug, Default)]
pub(crate) struct Opds {
    ui: Option<Ui>,
    secrets: Option<Secrets>,
    library: Option<Library>,
    work: Option<Work>,
    web: Web,
    sync: Sync,
    /// Whether the services were looked for (one that is not there is not
    /// asked for again).
    opened: bool,
    /// What the device is, read when the first screen opens.
    small: bool,
    touch: bool,
    folder: String,
    extensions: Vec<String>,
    screens: Screens<View>,
    pending: Option<Pending>,
    /// The path of the book just saved, while the choice to read it is open.
    saved_path: Option<String>,
    asked: Option<(AskId, Ask)>,
    /// The catalogs, as the list last said; `unreadable` when the list could
    /// not be read (it is left as it is, and changes to it are not kept).
    catalogs: Vec<Catalog>,
    unreadable: bool,
    /// What the list's last line says, until something else happens.
    list_note: Option<String>,
}

/// The interface-7 catalogs. Idle, only the pointer to the state is held; the
/// state (the services' handles, the screens, the web and the sync engines) is
/// made by the first screen or sync and kept.
#[derive(Debug, Default)]
pub struct NativeOpds {
    /// None before the first need; at most one.
    state: Vec<Opds>,
}

impl NativeOpds {
    fn state(&mut self) -> Result<&mut Opds, Error> {
        if self.state.is_empty() {
            self.state
                .try_reserve_exact(1)
                .map_err(|_| Error::NoMemory)?;
            // Made in place: built as a value first, it would be on the stack
            // as large as it is, deep in a callback.
            self.state.resize_with(1, Opds::default);
        }
        self.state.first_mut().ok_or(Error::State)
    }
}

impl Plugin for NativeOpds {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if self.state.is_empty() {
            // Before the first screen or sync there is nothing to answer but
            // the action that opens the list.
            return match token {
                OPEN_TOKEN => open_list(host),
                SYNC_TOKEN | CATALOGS_SCREEN | EDIT_SCREEN | BROWSE_SCREEN => {
                    self.state()?.event(host, token, incoming, reply)
                }
                _ => Ok(()),
            };
        }
        self.state()?.event(host, token, incoming, reply)
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        self.state
            .first_mut()
            .ok_or(Error::State)?
            .draw(host, token, frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.state.first_mut() {
            Some(state) => state.input(host, token, input),
            None => Ok(Handled::No),
        }
    }
}

/// Asks for the list of catalogs.
fn open_list(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch).open(Open::screen("opds.catalogs")?)
}

impl Opds {
    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::HttpDone, kinds::HttpProgress, kinds::WorkDue, kinds::ImageReady, kinds::Stopping)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => open_list(host),
            Event::ActionRun { background, .. } if token == SYNC_TOKEN => {
                self.sync_action(host, background)
            }
            Event::ScreenOpened { instance, .. } if token == CATALOGS_SCREEN => {
                self.open(host)?;
                self.opened_list(host, instance)
            }
            Event::ScreenOpened { instance, .. } if token == EDIT_SCREEN => {
                self.open(host)?;
                self.opened_edit(host, instance)
            }
            Event::ScreenOpened { instance, .. } if token == BROWSE_SCREEN => {
                self.open(host)?;
                self.opened_browse(host, instance)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if matches!(token, CATALOGS_SCREEN | EDIT_SCREEN | BROWSE_SCREEN) => {
                self.screens.shown(token, instance)?;
                if token == CATALOGS_SCREEN {
                    // Back from editing a catalog.
                    self.reload(host)?;
                    self.refresh(host)?;
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                self.closed(host, instance);
                Ok(())
            }
            Event::HttpDone(done) if token == HTTP_TOKEN => self.http_done(host, &done),
            Event::HttpProgress(progress) if token == PROGRESS_TOKEN => {
                self.http_progress(host, &progress)
            }
            Event::WorkDue(due) if token == WORK_TOKEN => self.work_due(host, due.key()),
            Event::ImageReady(ready) if token == IMAGE_TOKEN => self.image_ready(host, &ready),
            Event::Stopping { .. } if token == STOP_TOKEN => {
                self.stopping(host);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        match view {
            View::List(view) => view.draw(host, frame),
            View::Edit(view) => view.draw(host, frame),
            View::Browse(view) => view.draw(host, frame),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        match view {
            View::List(_) => self.list_input(host, token, &input),
            View::Edit(_) => self.edit_input(host, token, &input),
            View::Browse(_) => self.browse_input(host, token, &input),
        }
    }
}

/// The device's secrets as the engines' vault: where the device keeps none,
/// nothing is kept (`Unsupported`).
struct HostVault<'a, 'cb> {
    secrets: Option<&'a Secrets>,
    host: &'a Host<'cb>,
}

impl Vault for HostVault<'_, '_> {
    fn read(&self, name: &str) -> Result<Option<Secret>, Error> {
        match self.secrets {
            Some(secrets) => keep::read(secrets, self.host, name),
            None => Err(Error::Unsupported),
        }
    }

    fn keep(&self, name: &str, label: &str, value: &str) -> Result<(), Error> {
        match self.secrets {
            Some(secrets) => keep::keep(secrets, self.host, name, label, value),
            None => Err(Error::Unsupported),
        }
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        match self.secrets {
            Some(secrets) => keep::remove(secrets, self.host, name),
            None => Err(Error::Unsupported),
        }
    }

    fn others(&self, wanted: &[String]) -> Result<Vec<String>, Error> {
        match self.secrets {
            Some(secrets) => keep::others(secrets, self.host, wanted),
            None => Err(Error::Unsupported),
        }
    }
}

impl Opds {
    /// Opens the services once (the first callback has a live frame) and
    /// learns what the device is. A service that is not there is `None`: what
    /// needs it says so.
    fn open(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if self.opened {
            return Ok(());
        }
        self.opened = true;
        // What a run cut short left (a power cut): the answers' files go.
        let files = DataFiles::new(host);
        for name in crate::native_web::FILES {
            crate::native_files::remove(&files, name);
        }
        self.secrets = Self::service(Secrets::open(host))?;
        self.library = Self::service(Library::open(host))?;
        self.work = Self::service(Work::open(host))?;
        if let Err(error) = self.learn(host) {
            self.opened = false;
            return Err(error);
        }
        Ok(())
    }

    /// A service that is not there is `None`; a refused allocation is not an
    /// answer.
    fn service<T>(opened: Result<T, Error>) -> Result<Option<T>, Error> {
        match opened {
            Ok(service) => Ok(Some(service)),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(None),
        }
    }

    /// The device's class (a small one, the X4, keeps less), its touch screen,
    /// the library folder and the extensions of the formats it reads.
    fn learn(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.small = true;
        if let Ok(device) = Device::open(host) {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            if let Ok(row) = device.device(host, &mut input, &mut out) {
                self.small = !matches!(row.class(), DeviceClass::Medium | DeviceClass::Large);
                self.touch = row.supports(DeviceCapability::Touch);
            }
            // A handle that cannot be released is no reason to lose the facts.
            device.release(host).ok();
        }
        let mut scratch = [0; 256];
        self.folder = match settings::read_setting(host, LIBRARY_FOLDER, &mut scratch) {
            Ok(SettingValue::Text(text)) => copy(text.trim_matches('/'))?,
            _ => copy("books")?,
        };
        self.extensions = Vec::new();
        if let Ok(documents) = Documents::open(host) {
            let found = self.read_extensions(host, &documents);
            documents.release(host).ok();
            found?;
        }
        Ok(())
    }

    /// The extensions of every format, a page at a time.
    fn read_extensions(&mut self, host: &Host<'_>, documents: &Documents) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = FormatCursor::start();
        let mut restarted = false;
        loop {
            let page = match documents.formats(host, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::State) if !restarted => {
                    restarted = true;
                    self.extensions.clear();
                    cursor = FormatCursor::start();
                    continue;
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(()),
            };
            for format in page.rows() {
                for extension in format.extensions() {
                    push(&mut self.extensions, copy(extension)?)?;
                }
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => return Ok(()),
            }
        }
    }

    /// The setting `key` as a number, `default` where it is not there.
    fn number_setting(host: &Host<'_>, key: &str, default: i64) -> i64 {
        let mut scratch = [0; 64];
        match settings::read_setting(host, key, &mut scratch) {
            Ok(SettingValue::Int(value)) => value,
            _ => default,
        }
    }

    /// The setting `key` as a switch, `default` where it is not there.
    fn switch_setting(host: &Host<'_>, key: &str, default: bool) -> bool {
        let mut scratch = [0; 64];
        match settings::read_setting(host, key, &mut scratch) {
            Ok(SettingValue::Bool(value)) => value,
            _ => default,
        }
    }

    /// The secrets as the engines' vault.
    fn vault<'a, 'cb>(&'a self, host: &'a Host<'cb>) -> HostVault<'a, 'cb> {
        HostVault {
            secrets: self.secrets.as_ref(),
            host,
        }
    }

    /// The list of catalogs, read again from the file.
    fn reload(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let files = DataFiles::new(host);
        match catalogs::load(&files, catalogs::max_catalogs(self.small)) {
            Ok(list) => {
                self.catalogs = list;
                self.unreadable = false;
                Ok(())
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // There, but it cannot be read: left as it is.
            Err(_) => {
                self.catalogs = Vec::new();
                self.unreadable = true;
                Ok(())
            }
        }
    }

    /// Redraws what is on show, partly.
    fn redraw(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// A notice, where it can be shown: best effort, the work it reports is
    /// done all the same.
    fn notice(host: &Host<'_>, text: &str) -> Result<(), Error> {
        let mut scratch = [0; 512];
        match CommandSender::new(host, &mut scratch).notify(Notify::new(text)?) {
            Ok(()) | Err(_) => Ok(()),
        }
    }

    /// A notice of the catalog text `key`.
    fn notice_key(host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        Self::notice(host, &text)
    }

    /// Asks a question and tracks its answer for `ask`. The question stays
    /// open when the filling of its field fails (the field is then empty); it
    /// is tracked either way so its answer is still taken.
    fn ask(
        &mut self,
        host: &Host<'_>,
        question: Question<'_>,
        initial: Option<&str>,
        ask: Ask,
    ) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let asked = ui.ask(host, question, &mut input, &mut out);
        if let (Ok(request), Some(initial)) = (&asked, initial.filter(|i| !i.is_empty())) {
            match ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut out,
            ) {
                Ok(()) | Err(_) => {}
            }
        }
        wipe(&mut input);
        wipe(&mut out);
        self.asked = Some((asked?, ask));
        Ok(())
    }

    /// A question for text: its title, and the keyboard it comes with.
    fn ask_text(
        &mut self,
        host: &Host<'_>,
        title: &str,
        initial: Option<&str>,
        mode: Option<TextMode>,
        secret: bool,
        ask: Ask,
    ) -> Result<(), Error> {
        let mut question = Question::text(title, "")?;
        if secret {
            question = question.secret()?;
        }
        if let Some(mode) = mode {
            question = question.mode(mode)?;
        }
        self.ask(host, question, initial, ask)
    }

    /// An answer to a question the plugin asked.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, ask)) = self.asked else {
            return Ok(());
        };
        if request != answer.request() {
            return Ok(());
        }
        self.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match ask {
            Ask::Offer(index) => self.offered(host, index, answer.choice()),
            Ask::Read => {
                if answer.choice() == 0 {
                    self.read_saved(host)?;
                }
                Ok(())
            }
            Ask::Address | Ask::Field(_) | Ask::Terms => {
                let text = self.typed(host, &answer)?;
                match ask {
                    Ask::Address => self.address_typed(host, text.as_str()),
                    Ask::Field(field) => self.field_typed(host, field, text.as_str()),
                    _ => self.terms_typed(host, text.as_str()),
                }
            }
        }
    }

    /// What was typed, whole; the buffers that held it are wiped.
    fn typed(&self, host: &Host<'_>, answer: &Answered) -> Result<Secret, Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let read = ui.text_read(host, answer.request().text(), 0, &mut input, &mut out);
        let text = match read {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::copy(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        text
    }

    /// The screen closed: what it held goes, and a download or a load under
    /// way for it is not waited for.
    fn closed(&mut self, host: &Host<'_>, instance: paperpaw_sdk::native::ui::Instance) {
        self.close_browse(host, instance);
        self.screens.close(instance);
    }

    /// The plugin stops: what is under way is cancelled and its files go.
    fn stopping(&mut self, host: &Host<'_>) {
        self.stop_sync(host);
        self.stop_all(host);
    }
}
