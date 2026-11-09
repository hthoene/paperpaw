//! Sending the notes that wait (`waiting.txt`), one book after the other, a
//! request at a time: the same steps and rules as interface 6, over a [`Wire`]
//! (the plugin's data folder, the card with the notes, the network, the clock,
//! the account) so that the decisions are tested without a host.
//!
//! 1. Joplin Cloud and Joplin Server: sign in, unless signed in already.
//! 2. `info.json`, which every sync target has: there is Joplin data at the
//!    address, in the version of the format PaperPaw writes
//!    ([`SYNC_VERSION`]), and not end-to-end encrypted.
//! 3. Its locks: none exclusive, held by a client upgrading the target.
//! 4. The notebook named in the settings: made once, then found again.
//! 5. Each book: its note, if it was sent before, fetched to a file and only its
//!    end read. Its text not edited since, it is written over, staying in the
//!    notebook it was moved to; edited in Joplin or put in the trash, a new
//!    note is made beside it, so nothing written there is lost. The new note
//!    is written to a file first, the body copied piece by piece from the
//!    notes, and sent from there. It is kept as being sent before it goes:
//!    should the answer be lost, Joplin may have it all the same, so the next
//!    sending looks for it and writes it over rather than making it again.
//!
//! What fails stays waiting and goes with the next sending. A wrong password
//! or address stops the automatic tries until the account is changed.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::{files::Volume, json, mem, text};

use crate::native_joplin::{self as joplin, NOTE, NOTEBOOK, SYNC_VERSION, Service};
use crate::native_keep::Secret;
use crate::native_kept::{self as kept, Sent};
use crate::native_locks;
use crate::native_note;

/// The files of a sending, in the plugin's data folder.
pub const INFO: &str = "info.json";
pub const SESSION: &str = "session.json";
pub const LOCKS_FILE: &str = "locks.txt";
pub const FOUND: &str = "found.md";
pub const NOTE_FILE: &str = "note.md";
pub const NOTEBOOK_FILE: &str = "notebook.md";
/// The notebook's name when none is set.
pub const DEFAULT_NOTEBOOK: &str = "PaperPaw";
/// An answer that is read whole (a session, `info.json`) is at most this long.
pub const MAX_SMALL: usize = 8 * 1024;
/// The request body is kept in memory up to this: the sign-in.
const MAX_INLINE: usize = paperpaw_sdk::native::net::MAX_INLINE_BODY;

/// What a request is for. Its number is the request's token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    SignIn = 1,
    Info = 2,
    Locks = 3,
    NotebookFound = 4,
    NotebookMade = 5,
    NoteFound = 6,
    NoteSent = 7,
}

impl Step {
    /// The number the host gives back.
    #[must_use]
    pub const fn token(self) -> u32 {
        self as u32
    }

    /// The step a token stands for.
    #[must_use]
    pub const fn of_token(token: u32) -> Option<Self> {
        Some(match token {
            1 => Self::SignIn,
            2 => Self::Info,
            3 => Self::Locks,
            4 => Self::NotebookFound,
            5 => Self::NotebookMade,
            6 => Self::NoteFound,
            7 => Self::NoteSent,
            _ => return None,
        })
    }
}

/// How a request proves who sends it.
#[derive(Debug, Clone, Copy)]
pub enum Auth<'a> {
    /// Joplin Cloud's or Joplin Server's session.
    Session(&'a str),
    /// The user name and the password, on every request.
    Basic { user: &'a str, password: &'a str },
    /// None: the sign-in itself.
    None,
}

/// What a request carries.
#[derive(Debug, Clone, Copy)]
pub enum Body<'a> {
    None,
    /// A text in memory (the sign-in), at most 2 KiB.
    Text(&'a str),
    /// The file in the data folder.
    File(&'a str),
}

/// A request to make.
#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    pub step: Step,
    pub method: &'static str,
    pub url: &'a str,
    pub auth: Auth<'a>,
    /// The other headers (`Content-Type`, `Depth`).
    pub headers: &'a [(&'static str, &'static str)],
    pub body: Body<'a>,
    /// The file in the data folder the answer is saved to, and the most of it.
    pub save_to: Option<(&'a str, u64)>,
}

/// The account as the settings say it, apart from the password.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Account {
    pub service: Service,
    /// Its address (`https://`, no `/` at the end); empty if none is set.
    pub base: String,
    pub user: String,
    /// The notebook's name, trimmed; [`DEFAULT_NOTEBOOK`] if none is set.
    pub notebook: String,
}

impl Account {
    /// Whether the address and the user name are set.
    #[must_use]
    pub fn is_set(&self) -> bool {
        !self.base.is_empty() && !self.user.is_empty()
    }
}

/// What the sending needs of the world; the host's in the plugin, a stand-in
/// in tests.
pub trait Wire {
    /// The plugin's data folder.
    fn data(&self) -> &dyn Volume;
    /// The card the notes files are on.
    fn notes(&self) -> &dyn Volume;
    /// The account the settings name.
    ///
    /// # Errors
    /// `NoMemory`; what the settings say.
    fn account(&mut self) -> Result<Account, Error>;
    /// The password kept, if there is one.
    ///
    /// # Errors
    /// `NoMemory`; what the secrets service says, except that there is none.
    fn password(&mut self) -> Result<Option<Secret>, Error>;
    /// Whether the network can be used at all.
    ///
    /// # Errors
    /// `Unsupported` or `Denied` without it.
    fn network(&mut self) -> Result<(), Error>;
    /// Milliseconds since 1970 now, if the device knows the time.
    fn now_ms(&mut self) -> Option<i64>;
    /// Makes the request; its end comes with the call's step as token.
    ///
    /// # Errors
    /// What the network service says.
    fn send(&mut self, call: &Call<'_>) -> Result<RequestId, Error>;
}

/// What started a sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Start {
    /// A book's notes were exported.
    Export,
    /// The action, or the set-up's *Send*.
    Action,
    /// By itself (the work is due): says only what went well.
    Retry,
}

/// What to tell the reader of a sending that ended (the texts of interface 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tell {
    /// This many books' notes are in Joplin (`joplin.sent`).
    Sent(i64),
    /// This many were sent, this many not (`joplin.some-failed`).
    SomeFailed {
        sent: i64,
        failed: i64,
    },
    Nothing,
    NoNetwork,
    SetupNeeded,
    NoTime,
    Unreachable,
    /// Joplin answered with this status (`joplin.failed`).
    Failed(u32),
    BadLogin,
    NoServer,
    NoData,
    Encrypted,
    NewerTarget,
    OlderTarget,
    Upgrading,
    NoMemory,
}

impl Tell {
    /// The key of the text that says it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sent(_) => "joplin.sent",
            Self::SomeFailed { .. } => "joplin.some-failed",
            Self::Nothing => "joplin.nothing",
            Self::NoNetwork => "joplin.no-network",
            Self::SetupNeeded => "joplin.setup-needed",
            Self::NoTime => "joplin.no-time",
            Self::Unreachable => "joplin.unreachable",
            Self::Failed(_) => "joplin.failed",
            Self::BadLogin => "joplin.bad-login",
            Self::NoServer => "joplin.no-server",
            Self::NoData => "joplin.no-data",
            Self::Encrypted => "joplin.encrypted",
            Self::NewerTarget => "joplin.newer-target",
            Self::OlderTarget => "joplin.older-target",
            Self::Upgrading => "joplin.upgrading",
            Self::NoMemory => "joplin.no-memory",
        }
    }
}

/// What a step came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Under way, or nothing to do.
    Quiet,
    /// The sending ended. `quiet`: it ran by itself and went wrong or nowhere:
    /// the reader is not told, the set-up's note says it. `apart`: notes that
    /// were changed in Joplin, so made anew (to be told before the end).
    Ended {
        tell: Tell,
        quiet: bool,
        apart: usize,
    },
}

/// How a sending ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// To be tried again by itself.
    Again,
    /// At the account or address: not tried by itself until they change.
    Stop,
}

/// The end of a request, as far as the sending reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Done {
    pub id: RequestId,
    pub token: u32,
    pub reason: Reason,
    pub status: u32,
}

impl Done {
    /// The end of the request `id`.
    #[must_use]
    pub const fn new(id: RequestId, token: u32, reason: Reason, status: u32) -> Self {
        Self {
            id,
            token,
            reason,
            status,
        }
    }
}

/// The book being sent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Book {
    path: String,
    /// Its note, as sent before.
    note: Option<Sent>,
    /// Its note as sent last or now, while no answer came: Joplin may have it,
    /// or still [`note`](Self::note).
    sending: Option<Sent>,
}

impl Book {
    /// The note looked for in Joplin: the one sent last.
    fn last(&self) -> Option<&Sent> {
        self.sending.as_ref().or(self.note.as_ref())
    }

    /// The note Joplin has under the id looked for, if its text (`md5`) is one
    /// PaperPaw wrote there: none if it was edited in Joplin.
    fn written(&self, md5: &str) -> Option<&Sent> {
        let id = &self.last()?.id;
        [&self.sending, &self.note]
            .into_iter()
            .flatten()
            .find(|note| note.id == *id && note.text == md5)
    }
}

/// A sending under way.
#[derive(Debug)]
struct Run {
    quiet: bool,
    account: Account,
    password: Secret,
    /// Signed in anew in this sending, after the session had ended.
    renewed: bool,
    queue: Vec<String>,
    book: Option<Book>,
    notebook: String,
    /// The id the notebook is looked for or made with.
    pending: String,
    flight: Option<(RequestId, Step)>,
    sent: usize,
    failed: usize,
    /// Notes changed in Joplin, so made anew beside them.
    apart: usize,
}

/// The sending: at most one at a time; what is kept between sendings.
#[derive(Debug, Default)]
pub struct Sender {
    run: Option<Run>,
    /// Joplin Cloud's or Joplin Server's session, kept while PaperPaw runs.
    session: Option<Secret>,
    /// The last sending stopped at the account or address.
    blocked: bool,
}

/// The JSON text of the sign-in, wiped when dropped.
pub(crate) fn sign_in_body(email: &str, password: &str) -> Result<Secret, Error> {
    let mut out = String::new();
    out.try_reserve(email.len() + password.len() + 32)
        .map_err(|_| Error::NoMemory)?;
    out.push_str("{\"email\":");
    escape_into(&mut out, email)?;
    out.push_str(",\"password\":");
    escape_into(&mut out, password)?;
    out.push('}');
    if out.len() > MAX_INLINE {
        mem::wipe_text(out);
        return Err(Error::NoSpace);
    }
    let secret = Secret::copy(&out);
    mem::wipe_text(out);
    secret
}

/// `text` as a JSON string literal.
fn escape_into(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len() * 2 + 2)
        .map_err(|_| Error::NoMemory)?;
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let n = c as u32;
                out.push_str("\\u00");
                out.push(char::from_digit(n >> 4, 16).unwrap_or('0'));
                out.push(char::from_digit(n & 15, 16).unwrap_or('0'));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    Ok(())
}

/// What the sync target's `info.json`, just fetched, says: the version of its
/// format (0 if none) and whether its notes are end-to-end encrypted.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn target_info(v: &dyn Volume) -> Result<(u32, bool), Error> {
    use paperpaw_sdk::json::stream::{Event as Json, Parser};
    let mut parser = Parser::default();
    let (mut version, mut encrypted) = (0, false);
    let mut visit = |event: Json<'_>| {
        let Json::Value(at, value) = event else {
            return;
        };
        match at {
            [key] if key == "version" => version = value.parse().unwrap_or(0),
            [key, value_key] if key == "e2ee" && value_key == "value" => {
                encrypted = value == "true";
            }
            _ => {}
        }
    };
    let mut piece = [0u8; 512];
    let mut at = 0u64;
    loop {
        let n = match v.read_at(INFO, at, &mut piece) {
            Ok(n) => n,
            Err(Error::NotFound) => break,
            Err(error) => return Err(error),
        };
        if n == 0 {
            break;
        }
        at += n as u64;
        parser
            .try_feed(piece.get(..n).unwrap_or(&[]), &mut visit)
            .map_err(|_| Error::NoMemory)?;
    }
    parser.try_finish(&mut visit).map_err(|_| Error::NoMemory)?;
    Ok((version, encrypted))
}

/// A book's note, or a notebook, a status away from being sent.
impl Sender {
    /// A new one, with nothing under way.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            run: None,
            session: None,
            blocked: false,
        }
    }

    /// Whether a sending is under way.
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.run.is_some()
    }

    /// The account was changed: signed in anew, tried again by itself.
    pub fn account_changed(&mut self) {
        self.session = None;
        self.blocked = false;
    }

    /// A notes file was exported: it waits, and the sending starts.
    ///
    /// # Errors
    /// `NoMemory`; what the card says while the file is put on the list (then
    /// nothing is sent).
    pub fn exported(&mut self, wire: &mut dyn Wire, path: &str) -> Result<Outcome, Error> {
        kept::add_waiting(wire.data(), path)?;
        self.start(wire, Start::Export)
    }

    /// Sends the notes that wait, or says why it cannot.
    ///
    /// # Errors
    /// `NoMemory` (the sending ends first, the notes keep waiting).
    pub fn start(&mut self, wire: &mut dyn Wire, start: Start) -> Result<Outcome, Error> {
        let waiting = match kept::waiting(wire.data()) {
            Ok(waiting) => waiting,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A list that cannot be read is tried again later.
            Err(_) => return Ok(Outcome::Quiet),
        };
        if let Some(run) = self.run.as_mut() {
            // Those exported meanwhile come along.
            let current = run.book.as_ref().map(|b| b.path.as_str());
            for path in waiting {
                if !run.queue.contains(&path) && current != Some(path.as_str()) {
                    run.queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    run.queue.push(path);
                }
            }
            return Ok(Outcome::Quiet);
        }
        if start == Start::Retry && (self.blocked || waiting.is_empty()) {
            return Ok(Outcome::Quiet);
        }
        let quiet = start == Start::Retry;
        if waiting.is_empty() {
            return Ok(self.ended(quiet, Tell::Nothing, End::Again, 0));
        }
        if let Err(Error::Unsupported | Error::Denied) = wire.network() {
            return Ok(self.ended(quiet, Tell::NoNetwork, End::Again, 0));
        }
        let account = wire.account()?;
        let password = wire.password()?;
        let Some(password) = password.filter(|_| account.is_set()) else {
            return Ok(self.ended(quiet, Tell::SetupNeeded, End::Stop, 0));
        };
        if wire.now_ms().is_none() {
            return Ok(self.ended(quiet, Tell::NoTime, End::Again, 0));
        }
        self.run = Some(Run {
            quiet,
            notebook: String::new(),
            pending: String::new(),
            account,
            password,
            renewed: false,
            queue: waiting,
            book: None,
            flight: None,
            sent: 0,
            failed: 0,
            apart: 0,
        });
        self.guard(wire, Self::sign_in_or_check)
    }

    /// Runs `step`; a refused allocation ends the sending (the notes keep
    /// waiting) and says so.
    fn guard(
        &mut self,
        wire: &mut dyn Wire,
        step: fn(&mut Self, &mut dyn Wire) -> Result<Outcome, Error>,
    ) -> Result<Outcome, Error> {
        match step(self, wire) {
            Err(Error::NoMemory) => Ok(self.end_run(wire, Tell::NoMemory, End::Again)),
            other => other,
        }
    }

    /// The request for `step` has been refused or failed to start: how the
    /// sending goes on.
    fn refused(&mut self, wire: &mut dyn Wire, error: Error) -> Result<Outcome, Error> {
        match error {
            Error::Unsupported | Error::Denied => {
                Ok(self.end_run(wire, Tell::NoNetwork, End::Again))
            }
            Error::NoMemory => Err(Error::NoMemory),
            // Too long an address, a full table: this one cannot go.
            _ => match self.run.as_ref().is_some_and(|r| r.book.is_some()) {
                true => self.book_failed(wire),
                false => Ok(self.end_run(wire, Tell::Unreachable, End::Again)),
            },
        }
    }

    /// What the request just made came to: kept in flight, or refused.
    fn flown(
        &mut self,
        wire: &mut dyn Wire,
        step: Step,
        sent: Result<RequestId, Error>,
    ) -> Result<Outcome, Error> {
        match sent {
            Ok(id) => {
                if let Some(run) = self.run.as_mut() {
                    run.flight = Some((id, step));
                }
                Ok(Outcome::Quiet)
            }
            Err(error) => self.refused(wire, error),
        }
    }

    /// How requests prove who sends them: the session where there is one.
    fn auth<'a>(session: Option<&'a Secret>, run: &'a Run) -> Auth<'a> {
        match session {
            Some(session) if run.account.service.signs_in() => Auth::Session(session.as_str()),
            _ => Auth::Basic {
                user: &run.account.user,
                password: run.password.as_str(),
            },
        }
    }

    /// Requests the file `name` of the sync target with `method`.
    fn call(
        &mut self,
        wire: &mut dyn Wire,
        step: Step,
        method: &'static str,
        name: &str,
        body: (Body<'_>, &[(&'static str, &'static str)]),
        save_to: Option<(&str, u64)>,
    ) -> Result<Outcome, Error> {
        let Some(run) = self.run.as_ref() else {
            return Ok(Outcome::Quiet);
        };
        let url = run.account.service.url(&run.account.base, name)?;
        let call = Call {
            step,
            method,
            url: &url,
            auth: Self::auth(self.session.as_ref(), run),
            headers: body.1,
            body: body.0,
            save_to,
        };
        {
            let (step, sent) = (call.step, wire.send(&call));
            self.flown(wire, step, sent)
        }
    }

    fn sign_in_or_check(&mut self, wire: &mut dyn Wire) -> Result<Outcome, Error> {
        let Some(run) = self.run.as_ref() else {
            return Ok(Outcome::Quiet);
        };
        if run.account.service.signs_in() && self.session.is_none() {
            let body = sign_in_body(&run.account.user, run.password.as_str());
            let body = match body {
                Ok(body) => body,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // Too long to be an account's.
                Err(_) => return Ok(self.end_run(wire, Tell::BadLogin, End::Stop)),
            };
            let url = text::format(format_args!("{}/api/sessions", run.account.base))?;
            let call = Call {
                step: Step::SignIn,
                method: "POST",
                url: &url,
                auth: Auth::None,
                headers: &[("Content-Type", "application/json")],
                body: Body::Text(body.as_str()),
                save_to: Some((SESSION, MAX_SMALL as u64)),
            };
            return {
                let (step, sent) = (call.step, wire.send(&call));
                self.flown(wire, step, sent)
            };
        }
        let save = Some((INFO, MAX_SMALL as u64));
        self.call(wire, Step::Info, "GET", INFO, (Body::None, &[]), save)
    }

    /// The end of a request: the next step, or the end.
    ///
    /// # Errors
    /// `NoMemory` only where nothing could be done about it (the sending has
    /// ended; the notes keep waiting).
    pub fn done(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Outcome, Error> {
        let Some(step) = Step::of_token(done.token) else {
            return Ok(Outcome::Quiet);
        };
        let Some(run) = self.run.as_mut() else {
            return Ok(Outcome::Quiet);
        };
        if run.flight != Some((done.id, step)) {
            return Ok(Outcome::Quiet);
        }
        run.flight = None;
        let status = match done.reason {
            // By a newer request: nothing to learn.
            Reason::Cancelled | Reason::Replaced => {
                self.run = None;
                clean(wire.data());
                return Ok(Outcome::Quiet);
            }
            // The run's time is over: no request is to follow outside it.
            Reason::WorkEnded => return Ok(self.end_run(wire, Tell::Unreachable, End::Again)),
            Reason::Ok => done.status,
            // No answer came (interface 6: status 0).
            _ => 0,
        };
        let result = self.step(wire, step, status);
        match result {
            Err(Error::NoMemory) => Ok(self.end_run(wire, Tell::NoMemory, End::Again)),
            other => other,
        }
    }

    fn step(&mut self, wire: &mut dyn Wire, step: Step, status: u32) -> Result<Outcome, Error> {
        match step {
            Step::SignIn => self.signed_in(wire, status),
            Step::Info => self.checked(wire, status),
            Step::Locks => self.locks_listed(wire, status),
            Step::NotebookFound => self.notebook_found(wire, status),
            Step::NotebookMade => self.notebook_made(wire, status),
            Step::NoteFound => self.note_found(wire, status),
            Step::NoteSent => self.note_sent(wire, status),
        }
    }

    /// Answers that are no success, the same for every step.
    fn trouble(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        match status {
            401 | 403 => self.unauthorised(wire),
            0 => Ok(self.end_run(wire, Tell::Unreachable, End::Again)),
            status => Ok(self.end_run(wire, Tell::Failed(status), End::Again)),
        }
    }

    /// Not let in: the session ended (signed in anew, once), or the user name
    /// or password is wrong.
    fn unauthorised(&mut self, wire: &mut dyn Wire) -> Result<Outcome, Error> {
        let had_session = self.session.take().is_some();
        if let Some(run) = self.run.as_mut()
            && had_session
            && !run.renewed
        {
            run.renewed = true;
            if let Some(book) = run.book.take() {
                run.queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
                run.queue.insert(0, book.path);
            }
            return self.sign_in_or_check(wire);
        }
        Ok(self.end_run(wire, Tell::BadLogin, End::Stop))
    }

    fn signed_in(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        match status {
            200..=299 => {
                let id = Self::session_id(wire.data())?;
                let _ = wire.data().remove(SESSION);
                match id {
                    Some(id) => {
                        self.session = Some(id);
                        self.sign_in_or_check(wire)
                    }
                    None => Ok(self.end_run(wire, Tell::NoServer, End::Stop)),
                }
            }
            400 | 401 | 403 => Ok(self.end_run(wire, Tell::BadLogin, End::Stop)),
            404 | 405 => Ok(self.end_run(wire, Tell::NoServer, End::Stop)),
            _ => self.trouble(wire, status),
        }
    }

    /// The session's id in the sign-in's answer; none if there is none.
    fn session_id(v: &dyn Volume) -> Result<Option<Secret>, Error> {
        let Some(bytes) = paperpaw_ui_kit::files::read_all_on(v, SESSION, MAX_SMALL)? else {
            return Ok(None);
        };
        let body = String::from_utf8_lossy(&bytes);
        let found = json::string_member(&body, "id")?;
        match found {
            Some(id) if !id.is_empty() => Secret::copy(&id).map(Some),
            _ => Ok(None),
        }
    }

    fn checked(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        match status {
            200..=299 => {
                // Items are written as Joplin writes them for this version of
                // the format only; Joplin itself syncs with no other.
                let tell = match target_info(wire.data())? {
                    (version, _) if version > SYNC_VERSION => Tell::NewerTarget,
                    (version, _) if version < SYNC_VERSION => Tell::OlderTarget,
                    (_, true) => Tell::Encrypted,
                    _ => return self.list_locks(wire),
                };
                Ok(self.end_run(wire, tell, End::Stop))
            }
            404 => Ok(self.end_run(wire, Tell::NoData, End::Stop)),
            _ => self.trouble(wire, status),
        }
    }

    fn list_locks(&mut self, wire: &mut dyn Wire) -> Result<Outcome, Error> {
        let Some(run) = self.run.as_ref() else {
            return Ok(Outcome::Quiet);
        };
        let service = run.account.service;
        let url = service.locks_url(&run.account.base)?;
        let webdav = service == Service::WebDav;
        // WebDAV lists a folder: its own request, asking when each file changed.
        let (method, headers, body): (_, &[(&'static str, &'static str)], _) = if webdav {
            (
                "PROPFIND",
                &[("Depth", "1"), ("Content-Type", "application/xml")],
                Body::Text(native_locks::PROPFIND),
            )
        } else {
            ("GET", &[], Body::None)
        };
        let call = Call {
            step: Step::Locks,
            method,
            url: &url,
            auth: Self::auth(self.session.as_ref(), run),
            headers,
            body,
            save_to: Some((LOCKS_FILE, MAX_SMALL as u64)),
        };
        let sent = wire.send(&call);
        self.flown(wire, Step::Locks, sent)
    }

    /// The sync target's locks are listed: nothing is written while a client
    /// upgrades it.
    fn locks_listed(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        match status {
            200..=299 => {
                let now = wire.now_ms().unwrap_or_default();
                let service = self
                    .run
                    .as_ref()
                    .map_or(Service::Cloud, |r| r.account.service);
                let upgrading = native_locks::upgrading(service, wire.data(), LOCKS_FILE, now);
                let _ = wire.data().remove(LOCKS_FILE);
                match upgrading {
                    Ok(false) => self.find_notebook(wire),
                    Ok(true) => Ok(self.end_run(wire, Tell::Upgrading, End::Again)),
                    // Not known: nothing is written to a target that may be
                    // upgrading.
                    Err(_) => Ok(self.end_run(wire, Tell::NoMemory, End::Again)),
                }
            }
            // No locks: no folder for them (WebDAV), no lock list (a Joplin
            // Server before locks).
            404 => self.find_notebook(wire),
            _ => self.trouble(wire, status),
        }
    }

    /// The notebook made before, or a new one.
    fn find_notebook(&mut self, wire: &mut dyn Wire) -> Result<Outcome, Error> {
        let name = match self.run.as_ref() {
            Some(run) => text::copy(&run.account.notebook)?,
            None => return Ok(Outcome::Quiet),
        };
        match kept::notebook(wire.data()) {
            Ok(Some((kept_name, id))) if kept_name == name && joplin::is_id(&id) => {
                let file = joplin::file_name(&id)?;
                if let Some(run) = self.run.as_mut() {
                    run.pending = id;
                }
                let save = Some((FOUND, MAX_SMALL as u64));
                self.call(
                    wire,
                    Step::NotebookFound,
                    "GET",
                    &file,
                    (Body::None, &[]),
                    save,
                )
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => self.make_notebook(wire),
        }
    }

    fn notebook_found(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        match status {
            200..=299 => {
                let props = match native_note::read_props(wire.data(), FOUND) {
                    Ok(props) => props,
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => joplin::Props::default(),
                };
                let _ = wire.data().remove(FOUND);
                if props.kind == NOTEBOOK && !props.deleted {
                    if let Some(run) = self.run.as_mut() {
                        run.notebook = core::mem::take(&mut run.pending);
                    }
                    self.next_book(wire)
                } else {
                    self.make_notebook(wire)
                }
            }
            404 => self.make_notebook(wire),
            _ => self.trouble(wire, status),
        }
    }

    fn make_notebook(&mut self, wire: &mut dyn Wire) -> Result<Outcome, Error> {
        let now = wire.now_ms().unwrap_or_default();
        let name = match self.run.as_ref() {
            Some(run) => text::copy(&run.account.notebook)?,
            None => return Ok(Outcome::Quiet),
        };
        let old = kept::notebook(wire.data()).ok().flatten().map(|(_, id)| id);
        let seed = text::format(format_args!("paperpaw notebook {name} {now}"))?;
        let id = joplin::new_id(&seed, old.as_deref())?;
        let item = joplin::notebook(&id, &name, &joplin::iso8601(now)?)?;
        if wire.data().write(NOTEBOOK_FILE, item.as_bytes()).is_err() {
            return Ok(self.end_run(wire, Tell::Unreachable, End::Again));
        }
        let file = joplin::file_name(&id)?;
        if let Some(run) = self.run.as_mut() {
            run.pending = id;
        }
        let put = (
            Body::File(NOTEBOOK_FILE),
            &[("Content-Type", "application/octet-stream")][..],
        );
        self.call(wire, Step::NotebookMade, "PUT", &file, put, None)
    }

    fn notebook_made(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        if !(200..=299).contains(&status) {
            return self.trouble(wire, status);
        }
        let _ = wire.data().remove(NOTEBOOK_FILE);
        let Some(run) = self.run.as_mut() else {
            return Ok(Outcome::Quiet);
        };
        let id = core::mem::take(&mut run.pending);
        let _ = kept::keep_notebook(wire.data(), &run.account.notebook, &id);
        run.notebook = id;
        self.next_book(wire)
    }

    /// The next book: its note fetched if it was sent before, else made.
    fn next_book(&mut self, wire: &mut dyn Wire) -> Result<Outcome, Error> {
        loop {
            let Some(run) = self.run.as_mut() else {
                return Ok(Outcome::Quiet);
            };
            if run.queue.is_empty() {
                return Ok(self.finish(wire));
            }
            let path = run.queue.remove(0);
            if let Err(Error::NotFound) = wire.notes().size(&path) {
                // Its notes were deleted since.
                let _ = kept::remove_waiting(wire.data(), &path);
                let _ = kept::remove_sending(wire.data(), &path);
                continue;
            }
            let book = Book {
                note: kept::sent(wire.data(), &path).or_else(Self::lenient)?,
                sending: kept::sending(wire.data(), &path).or_else(Self::lenient)?,
                path,
            };
            let found = book.last().map(|note| joplin::file_name(&note.id));
            if let Some(run) = self.run.as_mut() {
                run.book = Some(book);
            }
            let Some(found) = found else {
                return self.write_note(wire, None);
            };
            let save = Some((FOUND, MAX_SMALL as u64));
            return self.call(
                wire,
                Step::NoteFound,
                "GET",
                &found?,
                (Body::None, &[]),
                save,
            );
        }
    }

    /// A list that cannot be read counts as empty, as in interface 6; only a
    /// refused allocation is an error.
    fn lenient(error: Error) -> Result<Option<Sent>, Error> {
        match error {
            Error::NoMemory => Err(Error::NoMemory),
            _ => Ok(None),
        }
    }

    fn note_found(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        match status {
            200..=299 => {
                let props = match native_note::read_props(wire.data(), FOUND) {
                    Ok(props) => props,
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => joplin::Props::default(),
                };
                // Edited in Joplin since, if its text is not what PaperPaw
                // wrote (its times tell less: moving it to another notebook
                // changes them too, some edits do not).
                let text = native_note::text_hash(wire.data(), FOUND, props.text_end);
                let text = match text {
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    other => other.ok(),
                };
                let _ = wire.data().remove(FOUND);
                let Some(run) = self.run.as_mut() else {
                    return Ok(Outcome::Quiet);
                };
                let Some(book) = run.book.as_mut() else {
                    return Ok(Outcome::Quiet);
                };
                let written = text.and_then(|md5| book.written(&md5).cloned());
                match written {
                    Some(note) if props.kind == NOTE && !props.deleted => {
                        // Joplin has it as sent, also if the answer was lost.
                        if book.note.as_ref() != Some(&note) {
                            let _ = kept::keep_sent(wire.data(), &book.path, &note);
                            book.note = Some(note);
                        }
                        self.write_note(wire, Some(props))
                    }
                    _ => {
                        if !props.deleted {
                            run.apart += 1;
                        }
                        self.write_note(wire, None)
                    }
                }
            }
            // Deleted in Joplin: made anew.
            404 => self.write_note(wire, None),
            401 | 403 | 0 => self.trouble(wire, status),
            _ => self.book_failed(wire),
        }
    }

    /// Writes the book's note and sends it: over `found` (its note as Joplin
    /// has it), or as a new note.
    fn write_note(
        &mut self,
        wire: &mut dyn Wire,
        found: Option<joplin::Props>,
    ) -> Result<Outcome, Error> {
        let now = wire.now_ms().unwrap_or_default();
        let Some(run) = self.run.as_mut() else {
            return Ok(Outcome::Quiet);
        };
        let Some(mut book) = run.book.take() else {
            return Ok(Outcome::Quiet);
        };
        // Later than the note found, or Joplin keeps that one (also if this
        // device's clock is behind).
        let at = found
            .as_ref()
            .map_or(now, |p| now.max(p.updated.saturating_add(1)));
        let updated = joplin::iso8601(at)?;
        let old = book.last().map(|n| n.id.clone());
        let id = match (&found, old) {
            (Some(_), Some(old)) => old,
            (_, old) => {
                let seed = text::format(format_args!("paperpaw note {} {now}", book.path))?;
                joplin::new_id(&seed, old.as_deref())?
            }
        };
        let head = match native_note::head(wire.notes(), &book.path) {
            Ok(head) => head,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => joplin::Head::default(),
        };
        let title = match head.title.as_str() {
            "" => file_title(&book.path),
            title => title,
        };
        // Written over, it keeps where it is and when it was made.
        let keep = |field: fn(&joplin::Props) -> &str, new: &str| -> Result<String, Error> {
            match found.as_ref().map(field) {
                Some(value) if !value.is_empty() => text::copy(value),
                _ => text::copy(new),
            }
        };
        let parent = keep(|p| &p.parent, &run.notebook)?;
        let created = keep(|p| &p.created, &updated)?;
        let user_created = keep(|p| &p.user_created, &created)?;
        let note = joplin::Note {
            id: &id,
            parent: &parent,
            title,
            author: &head.author,
            created: &created,
            user_created: &user_created,
            updated: &updated,
            order: found.as_ref().map_or(now, |p| p.order),
        };
        let written =
            native_note::write_note(&note, wire.notes(), &book.path, wire.data(), NOTE_FILE);
        let text = match written {
            Ok(text) => text,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                run.book = Some(book);
                return self.book_failed(wire);
            }
        };
        let sending = Sent { id, text };
        // Kept before it goes, for when its answer is lost.
        match kept::keep_sending(wire.data(), &book.path, &sending) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                run.book = Some(book);
                return self.book_failed(wire);
            }
        }
        let file = joplin::file_name(&sending.id)?;
        book.sending = Some(sending);
        run.book = Some(book);
        let put = (
            Body::File(NOTE_FILE),
            &[("Content-Type", "application/octet-stream")][..],
        );
        self.call(wire, Step::NoteSent, "PUT", &file, put, None)
    }

    fn note_sent(&mut self, wire: &mut dyn Wire, status: u32) -> Result<Outcome, Error> {
        if !(200..=299).contains(&status) {
            return match status {
                401 | 403 | 0 => self.trouble(wire, status),
                _ => self.book_failed(wire),
            };
        }
        let _ = wire.data().remove(NOTE_FILE);
        let Some(run) = self.run.as_mut() else {
            return Ok(Outcome::Quiet);
        };
        if let Some(book) = run.book.take() {
            if let Some(note) = &book.sending
                && kept::keep_sent(wire.data(), &book.path, note).is_ok()
            {
                let _ = kept::remove_sending(wire.data(), &book.path);
            }
            let _ = kept::remove_waiting(wire.data(), &book.path);
        }
        run.sent += 1;
        self.next_book(wire)
    }

    /// This book could not be sent: it waits, the others go on.
    fn book_failed(&mut self, wire: &mut dyn Wire) -> Result<Outcome, Error> {
        if let Some(run) = self.run.as_mut() {
            run.book = None;
            run.failed += 1;
        }
        self.next_book(wire)
    }

    /// All books were tried: says how it went.
    fn finish(&mut self, wire: &mut dyn Wire) -> Outcome {
        let Some(run) = self.run.as_mut() else {
            return Outcome::Quiet;
        };
        let (sent, failed) = (
            i64::try_from(run.sent).unwrap_or(i64::MAX),
            i64::try_from(run.failed).unwrap_or(i64::MAX),
        );
        let apart = if run.quiet { 0 } else { run.apart };
        if failed > 0 {
            let tell = Tell::SomeFailed { sent, failed };
            return self.end_with(wire, tell, End::Again, apart);
        }
        if sent == 0 {
            // The notes that waited were deleted.
            return self.end_with(wire, Tell::Nothing, End::Again, apart);
        }
        run.quiet = false;
        self.end_with(wire, Tell::Sent(sent), End::Again, apart)
    }

    /// Ends the sending with `tell`.
    fn end_run(&mut self, wire: &mut dyn Wire, tell: Tell, end: End) -> Outcome {
        self.end_with(wire, tell, end, 0)
    }

    fn end_with(&mut self, wire: &mut dyn Wire, tell: Tell, end: End, apart: usize) -> Outcome {
        clean(wire.data());
        let quiet = self.run.take().is_none_or(|run| run.quiet);
        self.ended(quiet, tell, end, apart)
    }

    fn ended(&mut self, quiet: bool, tell: Tell, end: End, apart: usize) -> Outcome {
        self.blocked = end == End::Stop;
        Outcome::Ended { tell, quiet, apart }
    }
}

/// The files of a sending are taken off the data folder.
fn clean(data: &dyn Volume) {
    for name in [INFO, SESSION, LOCKS_FILE, FOUND, NOTE_FILE, NOTEBOOK_FILE] {
        let _ = data.remove(name);
    }
}

/// The name of the notes file `path` without its folder and its `.md`: the
/// title of a book whose notes name none.
fn file_title(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.trim_end_matches(".md")
}
