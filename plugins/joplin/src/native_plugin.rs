//! The interface-7 Joplin plugin: a book's exported notes sent to Joplin over
//! `core.net` (`native_send`), the account set up on a screen of `core.ui`
//! rows with the password kept by `core.secrets`, and the retry of what waits
//! as declared work (`core.work`). The decisions are in `native_send`; the
//! screen's rows in `native_setup`; this file is the host's side of them.
//!
//! The export itself arrives through [`NativeJoplin::exported`], which the
//! reader's export stage calls once that stage is defined (`PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    declare::EventKind,
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    formatting::Arg,
    net::{FileRoot, Header, Net, REPLY, REQUEST_INPUT, Request, RequestId},
    registrations,
    reply::Reply,
    secrets::Secrets,
    settings::{self, SettingValue},
    snapshot::capture_snapshot,
    storage::{self, Root},
    ui::{Notify, Open, Question, RequestId as AskId, TextMode, TextWrite, Ui},
    work::{Moment, Work, WorkSpec},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    files::Volume,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    read_event,
    text::copy,
};

use crate::declarations::{HTTP_TOKEN, OPEN_TOKEN, SCREEN, SEND_TOKEN, STARTED_TOKEN, WORK_TOKEN};
use crate::native_joplin::{CLOUD, Service};
use crate::native_keep::{self as keep, PASSWORD, Secret};
use crate::native_kept as kept;
use crate::native_send::{
    Account, Auth, Body, Call, DEFAULT_NOTEBOOK, Done, Outcome, Sender, Start, Tell, Wire,
};
use crate::native_setup::Row;

mod image {
    include!(concat!(env!("OUT_DIR"), "/joplin_declarations.rs"));
}

/// The settings of the account.
const SERVICE: &str = "joplin.service";
const ADDRESS: &str = "joplin.address";
const USER: &str = "joplin.user";
const NOTEBOOK: &str = "joplin.notebook";
/// The key of the work that sends what waits, by itself.
const RETRY_KEY: &str = "joplin.retry";
/// The key of the work the action *Send to Joplin* declares.
const SEND_KEY: &str = "joplin.send";
/// What waits is tried again within half an hour of the network being there.
const RETRY_WITHIN_MS: u32 = 30 * 60 * 1000;
/// What a kept password is shown as.
const DOTS: &str = "••••••";

/// The set-up screen: its rows and what to say below them.
#[derive(Debug, Default)]
struct View {
    list: ItemList,
    rows: Vec<Row>,
}

/// The interface-7 Joplin plugin.
#[derive(Debug, Default)]
pub struct NativeJoplin {
    sender: Sender,
    handles: Handles,
    screens: Screens<View>,
    /// The row being typed, and its question.
    typing: Option<(AskId, Row)>,
    /// How the last sending went, in words; empty until one has.
    message: String,
}

/// The handles of the services, opened when first needed.
#[derive(Debug, Default)]
struct Handles {
    net: Option<Net>,
    secrets: Option<Secrets>,
    work: Option<Work>,
    ui: Option<Ui>,
}

impl Handles {
    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    fn secrets(&mut self, host: &Host<'_>) -> Result<&Secrets, Error> {
        if self.secrets.is_none() {
            self.secrets = Some(Secrets::open(host)?);
        }
        self.secrets.as_ref().ok_or(Error::State)
    }

    fn work(&mut self, host: &Host<'_>) -> Result<&Work, Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        self.work.as_ref().ok_or(Error::State)
    }

    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// Declares work `key`: `at_once`, or when the network is there.
    fn declare(&mut self, host: &Host<'_>, key: &str, at_once: bool) -> Result<(), Error> {
        let spec = if at_once {
            WorkSpec::new(key, WORK_TOKEN)?.network()
        } else {
            WorkSpec::new(key, WORK_TOKEN)?
                .network()
                .within_ms(RETRY_WITHIN_MS)
                .at_moment(Moment::Online)
        };
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        self.work(host)?.declare(host, &spec, &mut input, &mut out)
    }
}

/// The text setting `key`.
fn text_setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut scratch = [0; 1100];
    match settings::read_setting(host, key, &mut scratch)? {
        SettingValue::Text(text) => copy(text),
        _ => Ok(String::new()),
    }
}

/// The account the settings name.
fn account(host: &Host<'_>) -> Result<Account, Error> {
    let service = Service::of_key(&text_setting(host, SERVICE)?);
    let base = if service == Service::Cloud {
        copy(CLOUD)?
    } else {
        paperpaw_sdk::web::try_base_url(&text_setting(host, ADDRESS)?)
            .map_err(|_| Error::NoMemory)?
    };
    let notebook = text_setting(host, NOTEBOOK)?;
    let notebook = match notebook.trim() {
        "" => DEFAULT_NOTEBOOK,
        name => name,
    };
    Ok(Account {
        service,
        base,
        user: copy(text_setting(host, USER)?.trim())?,
        notebook: copy(notebook)?,
    })
}

/// The host as the sending uses it.
struct HostWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a mut Handles,
    data: RootVolume<'a, 'h>,
    card: RootVolume<'a, 'h>,
}

impl<'a, 'h> HostWire<'a, 'h> {
    fn new(host: &'a Host<'h>, handles: &'a mut Handles) -> Self {
        Self {
            host,
            handles,
            data: RootVolume(host, Root::Data),
            card: RootVolume(host, Root::Files),
        }
    }
}

impl Wire for HostWire<'_, '_> {
    fn data(&self) -> &dyn Volume {
        &self.data
    }

    fn notes(&self) -> &dyn Volume {
        &self.card
    }

    fn account(&mut self) -> Result<Account, Error> {
        account(self.host)
    }

    fn password(&mut self) -> Result<Option<Secret>, Error> {
        let host = self.host;
        match self.handles.secrets(host) {
            Ok(secrets) => keep::read(secrets, host, keep::PASSWORD),
            // A device that keeps no secrets has no password.
            Err(Error::Unsupported | Error::Denied) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn network(&mut self) -> Result<(), Error> {
        self.handles.net(self.host).map(|_| ())
    }

    fn now_ms(&mut self) -> Option<i64> {
        // To the second: the clock the snapshot gives.
        let seconds = capture_snapshot(self.host).ok()?.unix_seconds()?;
        seconds.checked_mul(1000)
    }

    fn send(&mut self, call: &Call<'_>) -> Result<RequestId, Error> {
        let host = self.host;
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let sent = self.make(call, &mut input, &mut out);
        // The input held the password or the session in a header.
        wipe(&mut input);
        wipe(&mut out);
        let _ = host;
        sent
    }
}

impl HostWire<'_, '_> {
    /// The request for `call`; its headers carry the session or the password.
    fn make(&mut self, call: &Call<'_>, input: &mut [u8], out: &mut [u8]) -> Result<RequestId, Error> {
        let host = self.host;
        let proof = match call.auth {
            Auth::Session(id) => Some(("X-API-AUTH", copy(id)?)),
            Auth::Basic { user, password } => Some((
                "Authorization",
                copy(&paperpaw_sdk::web::basic_auth(user, password))?,
            )),
            Auth::None => None,
        };
        let mut headers: Vec<Header<'_>> = Vec::new();
        headers
            .try_reserve(call.headers.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        if let Some((name, value)) = &proof {
            headers.push(Header::new(name, value)?);
        }
        for (name, value) in call.headers {
            headers.push(Header::new(name, value)?);
        }
        let mut request = Request::new(call.step.token(), call.method, call.url)?
            .headers(&headers)?
            .max_body(256);
        request = match call.body {
            Body::None => request,
            Body::Text(body) => request.body(body.as_bytes())?,
            Body::File(path) => request.send_file(FileRoot::Data, path)?,
        };
        if let Some((path, most)) = call.save_to {
            request = request.save_to(FileRoot::Data, path)?.max_file(most)?;
        }
        let sent = self.handles.net(host)?.request(host, &request, input, out);
        if let Some((_, value)) = proof {
            paperpaw_ui_kit::mem::wipe_text(value);
        }
        sent
    }
}

/// The storage calls on one root.
struct RootVolume<'a, 'h>(&'a Host<'h>, Root);

impl Volume for RootVolume<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, self.1, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.0, self.1, name, at, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.0, self.1, name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.0, self.1, name, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        storage::rename(self.0, self.1, from, to)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.0, self.1, name)
    }
    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.0)
    }
}

impl Plugin for NativeJoplin {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
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
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ActionRun,
            kinds::Answered,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Started,
            kinds::WorkDue,
            kinds::HttpDone,
            kinds::SettingChanged
        ) else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("joplin.setup")?)
            }
            // The action (also a rule's): the work that sends is declared and
            // runs at once.
            Event::ActionRun { .. } if token == SEND_TOKEN => self.declared(
                host,
                SEND_KEY,
                true,
            ),
            Event::Started { .. } if token == STARTED_TOKEN => self.retry_later(host),
            Event::WorkDue(due) if token == WORK_TOKEN => {
                let start = if due.key() == SEND_KEY {
                    Start::Action
                } else {
                    Start::Retry
                };
                self.start(host, start)
            }
            Event::HttpDone(done) if token == HTTP_TOKEN => {
                let done = Done::new(done.id(), done.token(), done.reason(), done.status());
                let step = self
                    .sender
                    .done(&mut HostWire::new(host, &mut self.handles), &done);
                self.stepped(host, step)
            }
            Event::SettingChanged { key, .. } => {
                if [SERVICE, ADDRESS, USER].contains(&key) || key.is_empty() {
                    self.sender.account_changed();
                }
                self.rebuild(host)
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
        let title = say.copy(host, "joplin.title")?;
        let service = Service::of_key(&text_setting(host, SERVICE)?);
        let help = match view.rows.get(view.list.focus()) {
            Some(row) => say.copy(host, row.help())?,
            None => String::new(),
        };
        let _ = service;
        let footer = if help.is_empty() {
            Footer::None
        } else {
            Footer::Note(&help)
        };
        view.list.draw_with(host, &mut frame, &title, true, footer)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(row) = view.rows.get(index).copied()
        {
            self.activate(host, row)?;
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeJoplin {
    /// A book's notes were exported to `path` (on the card): they wait, and
    /// are sent.
    ///
    /// # Errors
    /// `NoMemory`; what the callback's own commands refuse.
    pub fn exported(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let step = self
            .sender
            .exported(&mut HostWire::new(host, &mut self.handles), path);
        let open = matches!(
            step,
            Ok(Outcome::Ended {
                tell: Tell::SetupNeeded,
                ..
            })
        );
        self.stepped(host, step)?;
        if open {
            let mut scratch = [0; 128];
            CommandSender::new(host, &mut scratch).open(Open::screen("joplin.setup")?)?;
        }
        Ok(())
    }

    fn declared(&mut self, host: &Host<'_>, key: &str, at_once: bool) -> Result<(), Error> {
        match self.handles.declare(host, key, at_once) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // A work that cannot be declared (a full table, the network turned
            // off) costs this one sending; what waits is tried again.
            _ => Ok(()),
        }
    }

    fn start(&mut self, host: &Host<'_>, start: Start) -> Result<(), Error> {
        let step = self
            .sender
            .start(&mut HostWire::new(host, &mut self.handles), start);
        self.stepped(host, step)
    }

    /// What a step came to: the reader is told, the screen's note is kept, and
    /// what still waits is declared for later.
    fn stepped(&mut self, host: &Host<'_>, step: Result<Outcome, Error>) -> Result<(), Error> {
        let told = match step {
            Ok(Outcome::Quiet) => Ok(()),
            Ok(Outcome::Ended { tell, quiet, apart }) => self.tell(host, tell, quiet, apart),
            Err(Error::NoMemory) => self.tell(host, Tell::NoMemory, false, 0),
            // A step the host refused costs this sending; the notes keep waiting.
            Err(_) => Ok(()),
        };
        self.retry_later(host)?;
        told
    }

    /// Says how a sending ended (a sending that runs by itself tells only what
    /// went well; the set-up screen says it either way).
    fn tell(&mut self, host: &Host<'_>, tell: Tell, quiet: bool, apart: usize) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let text = match tell {
            Tell::Sent(count) => say.format(host, tell.key(), &[Arg::Count(count)])?,
            Tell::SomeFailed { sent, failed } => say.format(
                host,
                tell.key(),
                &[
                    Arg::Int {
                        name: "sent",
                        value: sent,
                    },
                    Arg::Int {
                        name: "failed",
                        value: failed,
                    },
                ],
            )?,
            Tell::Failed(status) => say.format(
                host,
                tell.key(),
                &[Arg::Int {
                    name: "status",
                    value: i64::from(status),
                }],
            )?,
            _ => say.copy(host, tell.key())?,
        };
        if !quiet {
            if apart > 0 {
                let count = i64::try_from(apart).unwrap_or(i64::MAX);
                let note = say.format(host, "joplin.apart", &[Arg::Count(count)])?;
                notify(host, &note);
            }
            notify(host, &text);
        }
        self.message = text;
        self.rebuild(host)
    }

    /// Declares the next try while notes wait.
    fn retry_later(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let waiting = kept::waiting(&RootVolume(host, Root::Data));
        match waiting {
            Ok(waiting) if waiting.is_empty() => Ok(()),
            Ok(_) => self.declared(host, RETRY_KEY, false),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(()),
        }
    }

    /// How many books wait, in words, or what happens to notes.
    fn waiting_text(host: &Host<'_>, say: &mut Messages) -> Result<String, Error> {
        let count = kept::waiting(&RootVolume(host, Root::Data)).map_or(0, |list| list.len());
        if count == 0 {
            return say.copy(host, "joplin.idle");
        }
        let count = i64::try_from(count).unwrap_or(i64::MAX);
        say.format(host, "joplin.waiting", &[Arg::Count(count)])
    }

    /// Whether the password is kept (a device without secrets keeps none).
    fn password_kept(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        let secrets = match self.handles.secrets(host) {
            Ok(secrets) => secrets,
            Err(Error::Unsupported | Error::Denied) => return Ok(false),
            Err(error) => return Err(error),
        };
        keep::present(secrets, host, PASSWORD)
    }

    /// The rows, as the account is now.
    fn build(&mut self, host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let service = Service::of_key(&text_setting(host, SERVICE)?);
        let rows = Row::of(service);
        let mut items = Vec::new();
        let mut shown = Vec::new();
        for row in rows {
            let value = match row {
                Row::Service => say.copy(host, service.key())?,
                Row::Address => {
                    let url = paperpaw_sdk::web::try_base_url(&text_setting(host, ADDRESS)?)
                        .map_err(|_| Error::NoMemory)?;
                    copy(url.strip_prefix("https://").unwrap_or(&url))?
                }
                Row::User => text_setting(host, USER)?,
                Row::Password if self.password_kept(host)? => copy(DOTS)?,
                Row::Password => String::new(),
                Row::Notebook => account(host)?.notebook,
                Row::Send => copy("›")?,
            };
            let title = say.copy(host, row.title(service))?;
            let item = if *row == Row::Send {
                // How the last sending went, or how many books wait.
                let note = if self.message.is_empty() {
                    Self::waiting_text(host, &mut say)?
                } else {
                    copy(&self.message)?
                };
                Item::noted(&title, &value, &note)?
            } else {
                Item::row(&title, &value)?
            };
            push(&mut items, item)?;
            push(&mut shown, *row)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus.min(shown.len().saturating_sub(1)));
        view.rows = shown;
        Ok(())
    }

    /// Rebuilds the rows of the open screen and asks for a redraw.
    fn rebuild(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(top) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let mut view = View::default();
        view.list.set_focus(top.list.focus());
        self.build(host, &mut view)?;
        if let Some(top) = self.screens.top(SCREEN) {
            *top = view;
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// A row was chosen: the service goes on to the next, *Send* sends, the
    /// others ask for their text.
    fn activate(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        match row {
            Row::Service => {
                let next = Service::of_key(&text_setting(host, SERVICE)?).next();
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch)
                    .set_setting(SERVICE, SettingValue::Text(next.key()))?;
                Ok(())
            }
            Row::Send => self.start(host, Start::Action),
            _ => self.ask(host, row),
        }
    }

    /// Asks for the text of `row` (a password masked, the address as one).
    fn ask(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        if self.typing.is_some() {
            return Ok(());
        }
        let mut say = Messages::new()?;
        let service = Service::of_key(&text_setting(host, SERVICE)?);
        let title = say.copy(host, row.title(service))?;
        let initial = match row.setting() {
            Some(key) => Some(text_setting(host, key)?),
            None => None,
        };
        let question = match row {
            Row::Password => Question::text(&title, "")?.secret()?,
            Row::Address => Question::text(&title, "")?.mode(TextMode::Url)?,
            _ => Question::text(&title, "")?,
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.handles.ui(host)?;
        let asked = ui.ask(host, question, &mut input, &mut out);
        // The question stays open when the filling fails (the field is then
        // empty); it is tracked either way so its answer is still taken.
        if let (Ok(request), Some(initial)) = (&asked, initial.as_deref().filter(|i| !i.is_empty()))
        {
            let _ = ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut out,
            );
        }
        wipe(&mut input);
        wipe(&mut out);
        self.typing = Some((asked?, row));
        Ok(())
    }

    /// An answer to a question the plugin asked.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, row)) = self.typing else {
            return Ok(());
        };
        if request != answer.request() {
            return Ok(());
        }
        self.typing = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let ui = self.handles.ui(host)?;
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
        let text = text?;
        self.apply(host, row, text.as_str())
    }

    /// Keeps what was typed for `row`: a setting, or the password. An empty
    /// answer for the password keeps what was kept: a confirm by mistake on the
    /// empty field (a password is never shown in it) loses nothing.
    fn apply(&mut self, host: &Host<'_>, row: Row, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        if let Some(key) = row.setting() {
            let mut scratch = [0; 1200];
            CommandSender::new(host, &mut scratch).set_setting(key, SettingValue::Text(text))?;
        } else if row == Row::Password {
            if text.is_empty() {
                return Ok(());
            }
            self.keep_password(host, text)?;
            self.sender.account_changed();
        } else {
            return Ok(());
        }
        // On to the next row.
        if let Some(top) = self.screens.top(SCREEN) {
            let at = top.rows.iter().position(|r| *r == row).unwrap_or(0);
            let last = top.rows.len().saturating_sub(1);
            top.list.set_focus((at + 1).min(last));
        }
        self.rebuild(host)
    }

    /// Keeps the password; said with the shared text where the device keeps
    /// no secrets.
    fn keep_password(&mut self, host: &Host<'_>, value: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let label = say.copy(host, "joplin.secret")?;
        let kept = match self.handles.secrets(host) {
            Ok(secrets) => keep::keep(secrets, host, PASSWORD, &label, value),
            Err(error) => Err(error),
        };
        let key = match kept {
            Ok(()) => return Ok(()),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(Error::Unsupported | Error::Denied) => "core.secrets-unsupported",
            // A value too long, a full store: it was not kept.
            Err(_) => "joplin.not-kept",
        };
        let text = say.copy(host, key)?;
        notify(host, &text);
        Ok(())
    }
}

/// A courtesy: a notice that cannot be shown (no room, the host refuses)
/// changes nothing else.
fn notify(host: &Host<'_>, message: &str) {
    let mut scratch = [0; 512];
    if let Ok(notice) = Notify::new(message) {
        let _ = CommandSender::new(host, &mut scratch).notify(notice);
    }
}

/// The kinds of event the plugin reads (checked against the declarations).
#[cfg(test)]
pub(crate) const READ: [EventKind; 4] = [
    EventKind::Started,
    EventKind::HttpDone,
    EventKind::WorkDue,
    EventKind::SettingChanged,
];
