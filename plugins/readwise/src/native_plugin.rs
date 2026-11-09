//! The interface-7 Readwise plugin: the highlights of an exported book sent to
//! Readwise over `core.net`, the access token asked for on the keyboard
//! (`core.ui`, a masked question) and kept with `core.secrets`, and the retry
//! of what waits as declared work (`core.work`). The decisions are in
//! `native_send` and `native_readwise`; this file is the host's side of them.
//!
//! The export itself arrives as the reader's stage `reader.notes-exported@1`
//! ([`crate::native_export`]), which calls [`NativeReadwise::exported`].
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    commands::CommandSender,
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    formatting::Arg,
    net::{FileRoot, Header, Net, REPLY, REQUEST_INPUT, Request, RequestId as NetId},
    registrations,
    reply::Reply,
    secrets::{SecretName, SecretWrite, Secrets, wipe},
    storage::{self, Root},
    ui::{Notify, Question, RequestId as AskId, Ui},
    work::{Moment, Work, WorkSpec},
};
use paperpaw_ui_kit::{Messages, files::Volume, mem, read_event, text};

use crate::declarations::{EXPORT_TOKEN, HTTP_TOKEN, STARTED_TOKEN, WORK_TOKEN};
use crate::native_export;
use crate::native_send::{
    BODY, Done, MAX_TOKEN, REQUEST_TOKEN, Secret, Sender, Start, Step, Tell, Wire, authorization,
    waiting,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/readwise_declarations.rs"));
}

/// Where the highlights go.
const URL: &str = "https://readwise.io/api/v2/highlights/";
/// What Readwise answers with is not read: the request asks for no more of it
/// than this (it lists every highlight it took).
const MAX_ANSWER: u32 = 256;
/// The secret the token is kept as, and the label the list of secrets shows.
const TOKEN_NAME: &str = "token";
const TOKEN_LABEL: &str = "readwise.token";
/// The key of the work that sends what waits.
const WORK_KEY: &str = "readwise.retry";
/// What waits is tried again within half an hour of the network being there.
const RETRY_WITHIN_MS: u32 = 30 * 60 * 1000;
/// The input and the reply a secrets or UI call needs.
const SERVICE_BUFFER: usize = 1280;

/// The interface-7 Readwise plugin.
#[derive(Debug, Default)]
pub struct NativeReadwise {
    sender: Sender,
    handles: Handles,
    /// The token being asked for, on the keyboard.
    asking: Option<AskId>,
}

/// The handles of the services, opened when first needed.
#[derive(Debug, Default)]
struct Handles {
    net: Option<Net>,
    secrets: Option<Secrets>,
    work: Option<Work>,
    ui: Option<Ui>,
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

    fn net(&mut self) -> Result<&Net, Error> {
        if self.handles.net.is_none() {
            self.handles.net = Some(Net::open(self.host)?);
        }
        self.handles.net.as_ref().ok_or(Error::State)
    }

    fn secrets(&mut self) -> Result<&Secrets, Error> {
        if self.handles.secrets.is_none() {
            self.handles.secrets = Some(Secrets::open(self.host)?);
        }
        self.handles.secrets.as_ref().ok_or(Error::State)
    }

    fn work(&mut self) -> Result<&Work, Error> {
        if self.handles.work.is_none() {
            self.handles.work = Some(Work::open(self.host)?);
        }
        self.handles.work.as_ref().ok_or(Error::State)
    }

    fn ui(&mut self) -> Result<&Ui, Error> {
        if self.handles.ui.is_none() {
            self.handles.ui = Some(Ui::open(self.host)?);
        }
        self.handles.ui.as_ref().ok_or(Error::State)
    }

    /// Declares the retry of what waits: when the network is there, within
    /// half an hour at the latest.
    fn declare_retry(&mut self) -> Result<(), Error> {
        let spec = WorkSpec::new(WORK_KEY, WORK_TOKEN)?
            .network()
            .within_ms(RETRY_WITHIN_MS)
            .at_moment(Moment::Online);
        let mut input = mem::buffer(256)?;
        let mut out = mem::buffer(64)?;
        let host = self.host;
        self.work()?.declare(host, &spec, &mut input, &mut out)
    }
}

impl Wire for HostWire<'_, '_> {
    fn data(&self) -> &dyn Volume {
        &self.data
    }

    fn notes(&self) -> &dyn Volume {
        &self.card
    }

    fn token(&mut self) -> Result<Option<Secret>, Error> {
        let host = self.host;
        let secrets = match self.secrets() {
            Ok(secrets) => secrets,
            // A device that keeps no secrets has no token.
            Err(Error::Unsupported | Error::Denied) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut input = mem::buffer(SERVICE_BUFFER)?;
        let mut reply = mem::buffer(REPLY)?;
        let found = match secrets.get(host, TOKEN_NAME, &mut input, &mut reply) {
            Ok(value) if value.value().is_empty() => Ok(None),
            Ok(value) => Secret::new(value.value()).map(Some),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut reply);
        found
    }

    fn network(&mut self) -> Result<(), Error> {
        self.net().map(|_| ())
    }

    fn post(&mut self, token: &Secret) -> Result<NetId, Error> {
        let host = self.host;
        let authorization = authorization(token)?;
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let headers = [
            Header::new("Authorization", authorization.as_str())?,
            Header::new("Content-Type", "application/json")?,
        ];
        let request = Request::new(REQUEST_TOKEN, "POST", URL)?
            .headers(&headers)?
            .send_file(FileRoot::Data, BODY)?
            .max_body(MAX_ANSWER);
        let sent = self.net()?.request(host, &request, &mut input, &mut out);
        // The input held the token in the header.
        wipe(&mut input);
        wipe(&mut out);
        sent
    }

    fn cancel(&mut self) {
        let host = self.host;
        if let (Ok(mut input), Ok(mut out)) = (mem::buffer(REQUEST_INPUT), mem::buffer(REPLY))
            && let Ok(net) = self.net()
        {
            net.cancel(host, REQUEST_TOKEN, &mut input, &mut out).ok();
        }
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

impl Plugin for NativeReadwise {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::EVENTS {
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
        let Some(event) = read_event!(
            incoming;
            kinds::StageCall,
            kinds::Started,
            kinds::WorkDue,
            kinds::HttpDone,
            kinds::Answered
        ) else {
            return Ok(());
        };
        match event {
            Event::StageCall { stage, payload, .. } if token == EXPORT_TOKEN => {
                let Some(path) = native_export::read(stage, payload)? else {
                    return Ok(());
                };
                // The reader ignores what is answered: the run is done either way.
                let sent = self.exported(host, path);
                native_export::done(reply, sent.is_ok())
            }
            Event::Started { .. } if token == STARTED_TOKEN => self.retry_later(host),
            Event::WorkDue(_) if token == WORK_TOKEN => {
                let step = self.sender.start(
                    &mut HostWire::new(host, &mut self.handles),
                    Start::Retry,
                    None,
                );
                self.stepped(host, step)
            }
            Event::HttpDone(done) if token == HTTP_TOKEN => {
                let done = Done::new(done.id(), done.token(), done.reason(), done.status());
                let step = self
                    .sender
                    .done(&mut HostWire::new(host, &mut self.handles), &done);
                self.stepped(host, step)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }
}

impl NativeReadwise {
    /// A book's notes were exported to `path` (on the card): they wait, and
    /// are sent.
    ///
    /// # Errors
    /// `NoMemory`; what the callback's own commands refuse.
    pub fn exported(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let step = self
            .sender
            .exported(&mut HostWire::new(host, &mut self.handles), path);
        self.stepped(host, step)
    }

    /// What a step came to: the reader is told, the token asked for, and what
    /// still waits is declared for later.
    fn stepped(&mut self, host: &Host<'_>, step: Result<Step, Error>) -> Result<(), Error> {
        let told = match step {
            Ok(Step::Quiet) => Ok(()),
            Ok(Step::Told(tell)) => self.tell(host, tell),
            Ok(Step::NeedsToken) => {
                notice(host, "readwise.needs-token")?;
                self.ask_token(host)
            }
            Err(Error::NoMemory) => notice(host, "readwise.no-memory"),
            // A step the host refused costs this sending; the files keep waiting.
            Err(_) => Ok(()),
        };
        self.retry_later(host)?;
        told
    }

    fn tell(&mut self, host: &Host<'_>, tell: Tell) -> Result<(), Error> {
        match tell {
            Tell::Sent(count) => {
                let message =
                    Messages::new()?.format(host, "readwise.sent", &[Arg::Count(count)])?;
                say(host, &message)
            }
            Tell::Nothing => notice(host, "readwise.nothing"),
            Tell::NoNetwork => notice(host, "readwise.no-network"),
            Tell::Unreachable => notice(host, "readwise.unreachable"),
            Tell::BadToken => notice(host, "readwise.bad-token"),
            Tell::Failed => notice(host, "readwise.failed"),
        }
    }

    /// Declares the next try while exports wait.
    fn retry_later(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut wire = HostWire::new(host, &mut self.handles);
        match waiting(wire.data()) {
            Ok(waiting) if waiting.is_empty() => Ok(()),
            Ok(_) => match wire.declare_retry() {
                Err(Error::NoMemory) => Err(Error::NoMemory),
                // A work that cannot be declared (a full table, the network
                // turned off) costs that retry: the next export tries again.
                _ => Ok(()),
            },
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(()),
        }
    }

    /// Asks for the token on the keyboard, typed masked.
    fn ask_token(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.asking.is_some() {
            return Ok(());
        }
        // The catalog's words; the key itself where the host has none.
        let title = match Messages::new()?.copy(host, TOKEN_LABEL) {
            Ok(title) => title,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => text::copy(TOKEN_LABEL)?,
        };
        let question = Question::text(&title, "")?.secret()?;
        let (mut input, mut out) = (
            mem::buffer(mem::SERVICE_BUFFER)?,
            mem::buffer(mem::SERVICE_BUFFER)?,
        );
        let asked = HostWire::new(host, &mut self.handles)
            .ui()
            .and_then(|ui| ui.ask(host, question, &mut input, &mut out));
        wipe(&mut input);
        wipe(&mut out);
        self.asking = Some(asked?);
        Ok(())
    }

    /// The token was typed: kept with the secrets and sent with at once.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if self.asking != Some(answer.request()) || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asking = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let Some(typed) = self.read_token(host, answer.request())? else {
            return Ok(());
        };
        // Not kept (no secrets, the card, the list is full): sent with all the
        // same; only a heap failure stops it.
        if let Err(Error::NoMemory) = self.keep_token(host, &typed) {
            return Err(Error::NoMemory);
        }
        let step = self.sender.start(
            &mut HostWire::new(host, &mut self.handles),
            Start::Export,
            Some(typed),
        );
        self.stepped(host, step)
    }

    /// The token typed, trimmed, read with buffers that are wiped; `None` for
    /// none, or one longer than a token can be.
    fn read_token(&mut self, host: &Host<'_>, request: AskId) -> Result<Option<Secret>, Error> {
        let (mut input, mut out) = (
            mem::buffer(mem::SERVICE_BUFFER)?,
            mem::buffer(mem::SERVICE_BUFFER)?,
        );
        let read = HostWire::new(host, &mut self.handles).ui().and_then(|ui| {
            let part = ui.text_read(host, request.text(), 0, &mut input, &mut out)?;
            let total = usize::try_from(part.total()).map_err(|_| Error::NoSpace)?;
            if total > MAX_TOKEN + 64 {
                return Ok(None);
            }
            if part.data().len() != total {
                return Err(Error::Invalid);
            }
            let token = part.data().trim();
            if token.is_empty() || token.len() > MAX_TOKEN {
                return Ok(None);
            }
            Secret::new(token).map(Some)
        });
        wipe(&mut input);
        wipe(&mut out);
        read
    }

    /// Keeps the token with the secrets, to be changed or removed in the list
    /// of passwords and keys.
    fn keep_token(&mut self, host: &Host<'_>, token: &Secret) -> Result<(), Error> {
        let mut wire = HostWire::new(host, &mut self.handles);
        let secrets = wire.secrets()?;
        let mut input = mem::buffer(SERVICE_BUFFER)?;
        let mut reply = mem::buffer(REPLY)?;
        let result = SecretWrite::new(SecretName::new(TOKEN_NAME)?, token.as_str())
            .and_then(|write| write.label(TOKEN_LABEL))
            .map(SecretWrite::changeable)
            .and_then(|write| secrets.keep(host, write, &mut input, &mut reply));
        wipe(&mut input);
        wipe(&mut reply);
        result
    }
}

/// The notice `key`, shown to the reader.
fn notice(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let message = Messages::new()?.copy(host, key)?;
    say(host, &message)
}

/// A courtesy: a notice that cannot be shown (no room, the host refuses)
/// changes nothing else.
fn say(host: &Host<'_>, message: &str) -> Result<(), Error> {
    let mut scratch = [0; 512];
    CommandSender::new(host, &mut scratch)
        .notify(Notify::new(message)?)
        .ok();
    Ok(())
}
