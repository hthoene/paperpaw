//! The calls the sync makes: the requests of `core.net` and the password kept
//! with `core.secrets` (`docs/abi/network.md`, `docs/abi/secrets.md`), around
//! the engine of `native_webdav`.
//!
//! - The password is the plugin's own secret `webdav`, labelled `stats.secret`
//!   (interface 6 kept it as `stats.webdav`, which the secret store moves to
//!   the plugin's own name at the first start). Every buffer and string that
//!   held it, or the `Authorization` value made of it, is wiped, and it is
//!   never logged.
//! - The download is saved to a file of the data folder and the upload sent
//!   from one (`save_to`, `send_file`): the statistics' file is never held in
//!   memory. A request ends with one `http_done` event: [`Dav::done`] reads the
//!   answer's ETag while it is delivered.
use crate::native_webdav::{
    Downloaded, Engine, FILE, Facts, INCOMING, Precondition, Say, Step, basic_auth,
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::net::{
    FileRoot, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Reason, Request, RequestId,
};
use paperpaw_sdk::native::secrets::{SecretName, SecretWrite, Secrets, wipe};
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::mem;

/// The plugin's own number of the request, in its `http_done` event.
pub(crate) const REQUEST_TOKEN: u32 = 1;
/// The secret the password is kept as, and the label the list of keys shows.
const SECRET_NAME: &str = "webdav";
const SECRET_LABEL: &str = "stats.secret";
/// The input and the reply a secrets call needs.
const SECRETS_INPUT: usize = 1280;
/// The `User-Agent` of interface 6.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";

/// What keeping the password came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum Kept {
    /// Kept: it is there after a restart.
    Saved,
    /// Nothing typed: the password is removed.
    Removed,
    /// The host cannot keep passwords (`ERR_UNSUPPORTED`): the reader is told
    /// (`core.secrets-unsupported`).
    Unsupported,
}

/// A string that is wiped when it goes.
struct Secret(String);

impl Drop for Secret {
    fn drop(&mut self) {
        mem::wipe_text(core::mem::take(&mut self.0));
    }
}

/// Where a request is in a sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Download,
    Upload,
}

/// What a call of the driver leaves for the caller to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// A request is out; its `http_done` goes on.
    Waiting,
    /// The sync ended (or did not begin) with this message.
    Said(Say),
    /// The sync ended asking whether to remove here what was removed there,
    /// for the file with this ETag.
    Asked { etag: String },
    /// The device is in offline mode: the reader may be asked to go online.
    Offline,
}

/// The sync: the engine, the handles and the request in flight.
#[derive(Debug, Default)]
pub(crate) struct Dav {
    engine: Engine,
    net: Option<Net>,
    secrets: Option<Secrets>,
    pending: Option<(RequestId, Phase)>,
}

impl Dav {
    /// Whether a sync is going.
    pub(crate) const fn running(&self) -> bool {
        self.engine.running()
    }

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

    /// The password kept: `None` when there is none.
    fn password(&mut self, host: &Host<'_>) -> Result<Option<Secret>, Error> {
        let secrets = self.secrets(host)?;
        let mut input = mem::buffer(SECRETS_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let found = match secrets.get(host, SECRET_NAME, &mut input, &mut reply) {
            Ok(value) if value.value().is_empty() => Ok(None),
            Ok(value) => paperpaw_ui_kit::text::copy(value.value()).map(|v| Some(Secret(v))),
            Err(Error::NotFound | Error::Unsupported) => Ok(None),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut reply);
        found
    }

    /// Whether a password is kept.
    ///
    /// # Errors
    /// What the service says; `NoMemory`.
    pub(crate) fn has_password(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        Ok(self.password(host)?.is_some())
    }

    /// Keeps what was typed, as typed (spaces too), as the password, or removes
    /// it where nothing was typed.
    ///
    /// # Errors
    /// What the service says, except `Unsupported` (that is
    /// [`Kept::Unsupported`]); `NoSpace` for a password over 1,024 bytes;
    /// `NoMemory`.
    pub(crate) fn keep_password(&mut self, host: &Host<'_>, typed: &str) -> Result<Kept, Error> {
        let secrets = self.secrets(host)?;
        let mut input = mem::buffer(SECRETS_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let result = if typed.is_empty() {
            SecretName::new(SECRET_NAME)
                .and_then(|name| secrets.remove(host, name, &mut input, &mut reply))
                .map(|()| Kept::Removed)
        } else {
            SecretWrite::new(SecretName::new(SECRET_NAME)?, typed)
                .and_then(|write| write.label(SECRET_LABEL))
                .map(SecretWrite::changeable)
                .and_then(|write| secrets.keep(host, write, &mut input, &mut reply))
                .map(|()| Kept::Saved)
        };
        wipe(&mut input);
        wipe(&mut reply);
        match result {
            Err(Error::Unsupported) => Ok(Kept::Unsupported),
            other => other,
        }
    }

    /// Moves a password kept in the plugin's folder by earlier versions
    /// (`webdav.ini`) to the other passwords and keys. The old file goes only
    /// once the secrets have a password (at a later start): until then it is
    /// the only copy, e.g. where the secrets could not be saved.
    ///
    /// # Errors
    /// What the service says; `NoMemory`.
    pub(crate) fn move_old_password(&mut self, host: &Host<'_>) -> Result<(), Error> {
        const OLD_FILE: &str = "webdav.ini";
        let storage = DataStorage::new(host);
        let Ok(size) = storage.size(OLD_FILE) else {
            return Ok(());
        };
        let mut data = mem::buffer(usize::try_from(size.min(4096)).unwrap_or(4096))?;
        let read = storage.read_at(OLD_FILE, 0, &mut data).unwrap_or(0);
        let old = Self::old_password(data.get(..read).unwrap_or_default());
        mem::wipe(&mut data);
        let old = old?;
        if !old.0.is_empty() && !self.has_password(host)? {
            self.keep_password(host, &old.0)?;
            return Ok(());
        }
        storage.remove(OLD_FILE).unwrap_or(());
        Ok(())
    }

    /// The password in the `webdav.ini` of earlier versions.
    fn old_password(bytes: &[u8]) -> Result<Secret, Error> {
        let text = core::str::from_utf8(bytes).unwrap_or_default();
        for entry in paperpaw_sdk::config::parse(text).filter_map(Result::ok) {
            if entry.key == "password" {
                let value = entry.value.try_as_str().map_err(|_| Error::NoMemory)?;
                let copy = paperpaw_ui_kit::text::copy(&value).map(Secret);
                // An escaped value is a copy of its own: wiped as well.
                if let alloc::borrow::Cow::Owned(owned) = value {
                    mem::wipe_text(owned);
                }
                return copy;
            }
        }
        Ok(Secret(String::new()))
    }

    /// Starts a sync.
    ///
    /// # Errors
    /// `NoMemory`.
    pub(crate) fn start(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        facts: Facts<'_>,
    ) -> Result<Outcome, Error> {
        let started = self.engine.start(storage, facts);
        let step = self.guard(storage, started)?;
        let outcome = self.carry_out(host, storage, step);
        self.guard(storage, outcome)
    }

    /// An error ends the sync: nothing is in flight any more, so the next
    /// sync can start (it would wait for a request that never comes).
    fn guard<T>(&mut self, storage: &dyn Storage, result: Result<T, Error>) -> Result<T, Error> {
        if result.is_err() {
            self.pending = None;
            self.engine.lost(storage);
        }
        result
    }

    /// The reader decided what to do with the file the server lacks much of:
    /// the sync starts again with the answer.
    ///
    /// # Errors
    /// `NoMemory`.
    pub(crate) fn decide(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        facts: Facts<'_>,
        (etag, remove): (&str, bool),
    ) -> Result<Option<Outcome>, Error> {
        let decided = self.engine.decide(storage, facts, (etag, remove));
        match self.guard(storage, decided)? {
            Some(step) => {
                let outcome = self.carry_out(host, storage, step);
                self.guard(storage, outcome).map(Some)
            }
            None => Ok(None),
        }
    }

    fn carry_out(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        step: Step,
    ) -> Result<Outcome, Error> {
        match step {
            Step::Say(say) => Ok(Outcome::Said(say)),
            Step::Ask { etag } => Ok(Outcome::Asked { etag }),
            Step::Download => self.send(host, storage, Phase::Download, None),
            Step::Upload(condition) => self.send(host, storage, Phase::Upload, Some(condition)),
        }
    }

    /// Sends the request of `phase`. A request that cannot be made ends the
    /// sync as an unreachable server does, except for a full heap.
    fn send(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        phase: Phase,
        condition: Option<Precondition>,
    ) -> Result<Outcome, Error> {
        self.net(host)?;
        let password = if self.engine.user().is_empty() {
            None
        } else {
            self.password(host)?
        };
        let made = self.request(host, phase, condition.as_ref(), password.as_ref());
        match made {
            Ok(id) => {
                self.pending = Some((id, phase));
                Ok(Outcome::Waiting)
            }
            Err(Error::NoMemory) => {
                self.engine.lost(storage);
                Err(Error::NoMemory)
            }
            Err(_) => {
                self.engine.lost(storage);
                Ok(Outcome::Said(Say::Status(0)))
            }
        }
    }

    fn request(
        &self,
        host: &Host<'_>,
        phase: Phase,
        condition: Option<&Precondition>,
        password: Option<&Secret>,
    ) -> Result<RequestId, Error> {
        let authorization = match password {
            Some(secret) if !self.engine.user().is_empty() => {
                Some(Secret(basic_auth(self.engine.user(), &secret.0)?))
            }
            None if !self.engine.user().is_empty() => {
                Some(Secret(basic_auth(self.engine.user(), "")?))
            }
            _ => None,
        };
        let mut headers: Vec<Header<'_>> = Vec::new();
        headers.try_reserve_exact(5).map_err(|_| Error::NoMemory)?;
        if let Some(auth) = &authorization {
            headers.push(Header::new("Authorization", &auth.0)?);
        }
        headers.push(Header::new("User-Agent", USER_AGENT)?);
        match condition {
            Some(Precondition::NoFile) => headers.push(Header::new("If-None-Match", "*")?),
            Some(Precondition::Version(etag)) => headers.push(Header::new("If-Match", etag)?),
            Some(Precondition::Unknown) | None => {}
        }
        let request = match phase {
            Phase::Download => Request::new(REQUEST_TOKEN, "GET", self.engine.url())?
                .headers(&headers)?
                .save_to(FileRoot::Data, INCOMING)?,
            Phase::Upload => {
                headers.push(Header::new("Content-Type", "application/octet-stream")?);
                Request::new(REQUEST_TOKEN, "PUT", self.engine.url())?
                    .headers(&headers)?
                    .send_file(FileRoot::Data, FILE)?
            }
        };
        let net = self.net.as_ref().ok_or(Error::State)?;
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let id = net.request(host, &request, &mut input, &mut reply);
        // The input held the authorization header.
        wipe(&mut input);
        wipe(&mut reply);
        id
    }

    /// What the `http_done` event says about the request in flight: `None` for
    /// one that is not it (cancelled, replaced, another id).
    ///
    /// # Errors
    /// `NoMemory`; what the host says while the answer is read.
    pub(crate) fn done(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        facts: Facts<'_>,
        done: &HttpDone,
    ) -> Result<Option<Outcome>, Error> {
        let outcome = self.done_inner(host, storage, facts, done);
        self.guard(storage, outcome)
    }

    fn done_inner(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        facts: Facts<'_>,
        done: &HttpDone,
    ) -> Result<Option<Outcome>, Error> {
        let Some((id, phase)) = self.pending else {
            return Ok(None);
        };
        if done.token() != REQUEST_TOKEN || done.id() != id {
            return Ok(None);
        }
        self.pending = None;
        let status = match done.reason() {
            Reason::Ok => u16::try_from(done.status()).unwrap_or(u16::MAX),
            // The request is gone, and with it the sync: ended without a
            // message, since the one who cancelled knows.
            Reason::Cancelled | Reason::Replaced => {
                self.engine.lost(storage);
                return Ok(None);
            }
            Reason::Offline => {
                self.engine.lost(storage);
                return Ok(Some(Outcome::Offline));
            }
            // Anything else is no answer (interface 6: status 0).
            _ => 0,
        };
        let step = match phase {
            Phase::Download => {
                let etag = self.etag(host, id, status)?;
                let downloaded = Downloaded {
                    status,
                    etag: etag.as_deref(),
                };
                self.engine.downloaded(storage, downloaded)?
            }
            Phase::Upload => self.engine.uploaded(storage, facts, status)?,
        };
        match step {
            Some(step) => self.carry_out(host, storage, step).map(Some),
            None => Ok(None),
        }
    }

    /// The ETag of the answer `id`, if it has one.
    fn etag(&self, host: &Host<'_>, id: RequestId, status: u16) -> Result<Option<String>, Error> {
        if status == 0 {
            return Ok(None);
        }
        let net = self.net.as_ref().ok_or(Error::State)?;
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let answer = net.answer(host, id, &mut input, &mut reply)?;
        match answer.etag() {
            "" => Ok(None),
            etag => paperpaw_ui_kit::text::copy(etag).map(Some),
        }
    }
}
