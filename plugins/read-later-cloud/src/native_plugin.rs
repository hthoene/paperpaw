//! The interface-7 Readwise Reader plugin: the articles saved to read later,
//! fetched over `core.net` into books of the library (`core.library`), the
//! access token asked for on the keyboard and kept with `core.secrets`, and
//! the set-up screen of `core.ui` rows. The decisions are in `native_fetch`
//! (the fetch over a [`crate::native_fetch::Wire`] and a [`crate::native_fetch::Shelf`]) and `native_lists`; this file is
//! the host's side of them. The reader's page turns note the articles read to
//! the end, and a fetch a rule runs is declared work (`core.work`).
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{RefCell, RefMut};

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    formatting::Arg,
    library::{Existing, Import, Library},
    net::{FileRoot, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Request, RequestId as NetId},
    registrations,
    reply::Reply,
    secrets::{SecretName, SecretWrite, Secrets},
    settings::{self, SettingValue},
    storage::{self, Root},
    ui::{Notify, Open, Question, RequestId as AskId, Ui},
    work::{Work, WorkSpec},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    files::Volume,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    read_event,
    text::copy,
};

use crate::declarations::{
    FETCH_TOKEN, FROM_LATER, HTTP_TOKEN, OPEN_TOKEN, PAGE_TOKEN, SCREEN, STOP_TOKEN, WORK_TOKEN,
};
use crate::native_fetch::{Call, Done, Ended, Fetcher, REQUEST_TOKEN, Shelf, Started, Stop, Wire};
use crate::native_lists::{self as lists, FOLDER};
use crate::native_secret::{Secret, authorization};

mod image {
    include!(concat!(
        env!("OUT_DIR"),
        "/read_later_cloud_declarations.rs"
    ));
}

/// Where Readwise Reader is.
const SERVER: &str = "https://readwise.io";
/// The setting for where the articles come from.
const FROM: &str = "read-later-cloud.from";
/// The secret the token is kept as, and the label the list of secrets shows.
const TOKEN_NAME: &str = "token";
const TOKEN_LABEL: &str = "read-later-cloud.secret";
/// What tells the accounts apart, kept beside the token and not changed by
/// the user.
const ACCOUNT_NAME: &str = "account";
const ACCOUNT_LABEL: &str = "read-later-cloud.secret.account";
/// The key of the work a rule's fetch is declared as.
const WORK_KEY: &str = "read-later-cloud.fetch";
/// The `User-Agent` of interface 6 where the kernel is not listed.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";
/// What an answer that is not saved is read to, at most.
const MAX_PLAIN: u32 = 256;
/// What a secret is shown as.
const DOTS: &str = "••••••";

/// The rows the set-up shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Token,
    Fetch,
}

const SHOWN: [Row; 2] = [Row::Token, Row::Fetch];

impl Row {
    const fn title(self) -> &'static str {
        match self {
            Row::Token => "read-later-cloud.token",
            Row::Fetch => "read-later-cloud.fetch",
        }
    }
}

/// The set-up screen: its rows and the note below them.
#[derive(Debug, Default)]
struct View {
    list: ItemList,
    note: String,
}

/// The handles of the services, opened when first needed.
#[derive(Debug, Default)]
struct Handles {
    ui: Option<Ui>,
    net: Option<Net>,
    work: Option<Work>,
    library: Option<Library>,
    /// The secrets service; `None` once it is known the device keeps none.
    secrets: Option<Secrets>,
    secrets_tried: bool,
}

/// The interface-7 Readwise Reader plugin.
#[derive(Debug, Default)]
pub struct NativeReadLaterCloud {
    fetcher: Fetcher,
    /// Shared by the wire and the shelf of a fetch, which call it in turn.
    handles: RefCell<Handles>,
    screens: Screens<View>,
    /// The token being asked for, on the keyboard.
    typing: Option<AskId>,
}

/// The host as the fetch uses it.
struct HostWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a RefCell<Handles>,
    volume: DataVolume<'a, 'h>,
}

/// The handles, for the length of one call.
fn held(handles: &RefCell<Handles>) -> Result<RefMut<'_, Handles>, Error> {
    handles.try_borrow_mut().map_err(|_| Error::State)
}

impl<'a, 'h> HostWire<'a, 'h> {
    fn new(host: &'a Host<'h>, handles: &'a RefCell<Handles>) -> Self {
        Self {
            host,
            handles,
            volume: DataVolume(host),
        }
    }
}

impl Handles {
    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    fn work(&mut self, host: &Host<'_>) -> Result<&Work, Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        self.work.as_ref().ok_or(Error::State)
    }

    fn library(&mut self, host: &Host<'_>) -> Result<&Library, Error> {
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        self.library.as_ref().ok_or(Error::State)
    }

    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// The secrets service, or `None` for a device that keeps none.
    fn secrets(&mut self, host: &Host<'_>) -> Result<Option<&Secrets>, Error> {
        if !self.secrets_tried {
            match Secrets::open(host) {
                Ok(secrets) => self.secrets = Some(secrets),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
            self.secrets_tried = true;
        }
        Ok(self.secrets.as_ref())
    }

    /// The secret `name`: `None` where there is none or an empty one.
    fn secret(&mut self, host: &Host<'_>, name: &str) -> Result<Option<Secret>, Error> {
        let Some(secrets) = self.secrets(host)? else {
            return Ok(None);
        };
        let (mut input, mut reply) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let found = match secrets.get(host, name, &mut input, &mut reply) {
            Ok(value) if value.value().is_empty() => Ok(None),
            Ok(value) => Secret::new(value.value()).map(Some),
            Err(Error::NotFound | Error::Unsupported | Error::Denied) => Ok(None),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut reply);
        found
    }

    /// Keeps `value` as the secret `name`, called `label`.
    fn keep(
        &mut self,
        host: &Host<'_>,
        (name, label): (&str, &str),
        value: &str,
        changeable: bool,
    ) -> Result<(), Error> {
        let Some(secrets) = self.secrets(host)? else {
            return Err(Error::Unsupported);
        };
        let (mut input, mut reply) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let result = SecretName::new(name)
            .and_then(|name| SecretWrite::new(name, value))
            .and_then(|write| write.label(label))
            .map(|write| {
                if changeable {
                    write.changeable()
                } else {
                    write
                }
            })
            .and_then(|write| secrets.keep(host, write, &mut input, &mut reply));
        wipe(&mut input);
        wipe(&mut reply);
        result
    }
}

impl Wire for HostWire<'_, '_> {
    fn volume(&self) -> &dyn Volume {
        &self.volume
    }

    fn network(&mut self) -> Result<(), Error> {
        let host = self.host;
        held(self.handles)?.net(host).map(|_| ())
    }

    fn token(&mut self) -> Result<Option<Secret>, Error> {
        let host = self.host;
        held(self.handles)?.secret(host, TOKEN_NAME)
    }

    fn account(&mut self) -> Result<Option<String>, Error> {
        let host = self.host;
        match held(self.handles)?.secret(host, ACCOUNT_NAME)? {
            Some(account) => copy(account.as_str()).map(Some),
            None => Ok(None),
        }
    }

    fn keep_account(&mut self, account: &str) -> Result<(), Error> {
        let host = self.host;
        let mut say = Messages::new()?;
        let label = say.copy(host, ACCOUNT_LABEL)?;
        held(self.handles)?.keep(host, (ACCOUNT_NAME, &label), account, false)
    }

    fn send(&mut self, token: &Secret, call: &Call<'_>) -> Result<NetId, Error> {
        let host = self.host;
        let url = paperpaw_ui_kit::text::concat(&[SERVER, call.path])?;
        let authorization = authorization(token)?;
        let mut input = buffer(REQUEST_INPUT)?;
        let mut out = buffer(REPLY)?;
        let mut headers = Vec::new();
        headers.try_reserve_exact(3).map_err(|_| Error::NoMemory)?;
        headers.push(Header::new("Authorization", authorization.as_str())?);
        headers.push(Header::new("User-Agent", USER_AGENT)?);
        if call.body.is_some() {
            headers.push(Header::new("Content-Type", "application/json")?);
        }
        let mut request = Request::new(REQUEST_TOKEN, call.method, &url)?.headers(&headers)?;
        if let Some(body) = call.body {
            request = request.body(body.as_bytes())?;
        }
        request = match call.save {
            Some(save) => request
                .save_to(FileRoot::Data, save.file)?
                .max_file(u64::from(save.max))?,
            None => request.max_body(MAX_PLAIN),
        };
        let sent = held(self.handles)?
            .net(host)?
            .request(host, &request, &mut input, &mut out);
        // The input held the token in the header.
        wipe(&mut input);
        wipe(&mut out);
        sent
    }

    fn cancel(&mut self) {
        let host = self.host;
        if let (Ok(mut input), Ok(mut out)) = (buffer(REQUEST_INPUT), buffer(REPLY))
            && let Ok(mut handles) = held(self.handles)
            && let Ok(net) = handles.net(host)
        {
            net.cancel(host, REQUEST_TOKEN, &mut input, &mut out).ok();
        }
    }
}

/// The library as the fetch uses it: books in (`Library::import`), books out
/// (`Library::delete`, never with the reader's marks).
struct HostShelf<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a RefCell<Handles>,
}

impl Shelf for HostShelf<'_, '_> {
    fn put(&mut self, from: &str, file: &str, free_name: bool) -> Result<String, Error> {
        let host = self.host;
        let (mut input, mut out) = (buffer(2048)?, buffer(4096)?);
        let existing = if free_name {
            Existing::FreeName
        } else {
            Existing::Refuse
        };
        let import = Import::new(from, file).folder(FOLDER).existing(existing);
        let path = held(self.handles)?
            .library(host)?
            .import(host, &import, &mut input, &mut out)?;
        copy(path)
    }

    fn remove(&mut self, path: &str) -> Result<(), Error> {
        let host = self.host;
        let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
        let mut handles = held(self.handles)?;
        let library = match handles.library(host) {
            Ok(library) => library,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(()),
        };
        // `with_notes` off: a book with the reader's marks answers `State` and
        // stays, as one that cannot be told about does.
        match library.delete(host, &[path], false, &mut input, &mut out) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Ok(_) | Err(_) => Ok(()),
        }
    }
}

/// The storage calls on the plugin's data folder.
struct DataVolume<'a, 'h>(&'a Host<'h>);

impl Volume for DataVolume<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, Root::Data, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.0, Root::Data, name, at, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.0, Root::Data, name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.0, Root::Data, name, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        storage::rename(self.0, Root::Data, from, to)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.0, Root::Data, name)
    }
    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.0)
    }
}

impl Plugin for NativeReadLaterCloud {
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
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ActionRun,
            kinds::Answered,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::HttpDone,
            kinds::PageTurned,
            kinds::WorkDue,
            kinds::Stopping
        ) else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("read-later-cloud.setup")?)
            }
            // The action (also a rule's): the fetch is declared work that runs
            // at once, where the host lets background work run.
            Event::ActionRun { .. } if token == FETCH_TOKEN => self.declare(host),
            Event::WorkDue(_) if token == WORK_TOKEN => self.fetch(host),
            Event::HttpDone(done) if token == HTTP_TOKEN => self.ended(host, &done),
            Event::PageTurned(reading) if token == PAGE_TOKEN => lists::page_turned(
                &DataVolume(host),
                reading.path(),
                (reading.page(), reading.pages(), reading.progress_ppm()),
            ),
            Event::Stopping { .. } if token == STOP_TOKEN => {
                let mut wire = HostWire::new(host, &self.handles);
                self.fetcher.cancel(&mut wire);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened { instance, .. } if token == SCREEN => {
                let mut view = View::default();
                self.build(host, &mut view)?;
                self.screens.open(instance, view)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = say.copy(host, "read-later-cloud.title")?;
        let footer = if view.note.is_empty() {
            Footer::None
        } else {
            Footer::Note(&view.note)
        };
        view.list.draw_with(host, &mut frame, &title, true, footer)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(row) = SHOWN.get(index).copied()
        {
            match row {
                Row::Fetch => self.fetch(host)?,
                Row::Token => self.ask(host)?,
            }
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeReadLaterCloud {
    /// Where the articles come from, as the setting says.
    fn from(host: &Host<'_>) -> Result<String, Error> {
        let mut scratch = [0; 256];
        match settings::read_setting(host, FROM, &mut scratch)? {
            SettingValue::Text(text) => copy(text),
            _ => copy(FROM_LATER),
        }
    }

    /// The rows and the note below them, as the account is now.
    fn build(&mut self, host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let kept = self.handles.get_mut().secret(host, TOKEN_NAME)?.is_some();
        let mut items = Vec::new();
        for row in SHOWN {
            let value = match row {
                Row::Token => copy(if kept { DOTS } else { "" })?,
                Row::Fetch => copy("›")?,
            };
            let title = say.copy(host, row.title())?;
            push(&mut items, Item::row(&title, &value)?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        if view.note.is_empty() {
            view.note = say.copy(host, "read-later-cloud.help")?;
        }
        Ok(())
    }

    /// Rebuilds the rows of the open screen and asks for a redraw.
    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut view = View::default();
        let Some(top) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        view.list.set_focus(top.list.focus());
        view.note = copy(&top.note)?;
        self.build(host, &mut view)?;
        if let Some(top) = self.screens.top(SCREEN) {
            *top = view;
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// Asks for the token on the keyboard, masked.
    fn ask(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(host, Row::Token.title())?;
        let question = Question::text(&title, "")?.secret()?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let asked = self
            .handles
            .get_mut()
            .ui(host)?
            .ask(host, question, &mut input, &mut out);
        wipe(&mut input);
        wipe(&mut out);
        self.typing = Some(asked?);
        Ok(())
    }

    /// An answer to the question the plugin asked.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some(request) = self.typing else {
            return Ok(());
        };
        if request != answer.request() {
            return Ok(());
        }
        self.typing = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let read = self.handles.get_mut().ui(host)?.text_read(
            host,
            answer.request().text(),
            0,
            &mut input,
            &mut out,
        );
        let text = match read {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::new(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        match text {
            Ok(text) => self.apply(host, text.as_str()),
            // Over the longest token: it cannot be one.
            Err(Error::NoSpace) => self.notice_key(host, "read-later-cloud.not-kept"),
            Err(error) => Err(error),
        }
    }

    /// Keeps what was typed, an empty answer not: a confirm by mistake on the
    /// empty field (a token is never shown in it) loses nothing.
    fn apply(&mut self, host: &Host<'_>, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        if text.is_empty() {
            return Ok(());
        }
        let mut say = Messages::new()?;
        let label = say.copy(host, TOKEN_LABEL)?;
        let key = match self
            .handles
            .get_mut()
            .keep(host, (TOKEN_NAME, &label), text, true)
        {
            Ok(()) => None,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // The shared text where the device keeps no secrets; any other
            // refusal (a value too long, a full store) says it was not kept.
            Err(Error::Unsupported) => Some("core.secrets-unsupported"),
            Err(_) => Some("read-later-cloud.not-kept"),
        };
        if let Some(key) = key {
            return self.notice_key(host, key);
        }
        // On to the *Fetch* row.
        if let Some(top) = self.screens.top(SCREEN) {
            top.list.set_focus(SHOWN.len() - 1);
        }
        self.refresh(host)
    }

    /// Declares the fetch as work that runs at once (a rule's, or the action).
    fn declare(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let spec = WorkSpec::new(WORK_KEY, WORK_TOKEN)?.network();
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        match self
            .handles
            .get_mut()
            .work(host)?
            .declare(host, &spec, &mut input, &mut out)
        {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // A work that cannot be declared (a full table, the network turned
            // off) costs this one fetch.
            _ => Ok(()),
        }
    }

    /// Starts a fetch and says how it went.
    fn fetch(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let from = Self::from(host)?;
        let mut wire = HostWire::new(host, &self.handles);
        match self.fetcher.start(&mut wire, &from)? {
            Started::Fetching => self.notice_key(host, "read-later-cloud.fetching"),
            Started::Running => Ok(()),
            Started::NoNetwork => self.finish_key(host, "read-later-cloud.no-network"),
            Started::SetupNeeded => self.finish_key(host, "read-later-cloud.setup-needed"),
            Started::Unreadable => self.finish_key(host, "read-later-cloud.unreadable"),
            Started::Unreachable => self.finish_key(host, "read-later-cloud.unreachable"),
        }
    }

    /// The end of a request of the fetch.
    fn ended(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let end = Done::new(done.id(), done.token(), done.reason(), done.status());
        let ended = {
            let mut wire = HostWire::new(host, &self.handles);
            // The fetch calls the wire and the shelf in turn, never together.
            let mut shelf = HostShelf {
                host,
                handles: &self.handles,
            };
            self.fetcher.ended(&mut wire, &mut shelf, &end)
        };
        match ended {
            Ended::Ignored | Ended::Next => Ok(()),
            Ended::Memory => self.finish_key(host, "read-later-cloud.no-memory"),
            Ended::Stopped(Stop::NoNetwork) => self.finish_key(host, "read-later-cloud.no-network"),
            Ended::Stopped(Stop::Unreachable) => {
                self.finish_key(host, "read-later-cloud.unreachable")
            }
            Ended::Stopped(Stop::BadToken) => self.finish_key(host, "read-later-cloud.bad-token"),
            Ended::Stopped(Stop::Wait) => self.finish_key(host, "read-later-cloud.wait"),
            Ended::Stopped(Stop::TooBig) => self.finish_key(host, "read-later-cloud.too-big"),
            Ended::Finished {
                fetched,
                archived,
                failed,
            } => {
                let key = if failed > 0 {
                    "read-later-cloud.done-failed"
                } else {
                    "read-later-cloud.done"
                };
                let mut say = Messages::new()?;
                let text = say.format(
                    host,
                    key,
                    &[
                        Arg::Int {
                            name: "fetched",
                            value: i64::from(fetched),
                        },
                        Arg::Int {
                            name: "archived",
                            value: i64::from(archived),
                        },
                        Arg::Int {
                            name: "failed",
                            value: i64::from(failed),
                        },
                    ],
                )?;
                self.finish_text(host, text)
            }
        }
    }

    /// Says how a fetch ended: a notice, and the note of the set-up screen.
    fn finish_key(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        self.finish_text(host, text)
    }

    fn finish_text(&mut self, host: &Host<'_>, text: String) -> Result<(), Error> {
        self.notice(host, &text)?;
        if let Some(top) = self.screens.top(SCREEN) {
            top.note = text;
            self.refresh(host)?;
        }
        Ok(())
    }

    fn notice_key(&self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        self.notice(host, &text)
    }

    /// A notice, where it can be shown: one that cannot be changes nothing else.
    fn notice(&self, host: &Host<'_>, text: &str) -> Result<(), Error> {
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch)
            .notify(Notify::new(text)?)
            .ok();
        Ok(())
    }
}
