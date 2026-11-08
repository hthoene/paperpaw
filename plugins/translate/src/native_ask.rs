//! Asking DeepL and keeping the key: the calls of `core.net` and
//! `core.secrets` the translation plugin makes, with caller-owned buffers
//! (`docs/abi/network.md`, `docs/abi/secrets.md`).
//!
//! - The key is the plugin's own secret `deepl-key`, labelled `translate.key`
//!   (`secrets.md` §7); interface 6 kept it as `translate.deepl-key`, which
//!   the secret store moves to the plugin's own name at the first start.
//!   Every buffer that held it is wiped, and it is never logged.
//! - A request goes to DeepL as `POST` with the key in the `Authorization`
//!   header. A form of up to 2,048 bytes goes inline; a longer one (3,000
//!   characters of any script) is written to `request.form` in the data folder
//!   and sent from there (`network.md` §2.1), then removed.
//! - A request is ended by one `http_done` event: [`Asker::done`] reads the
//!   answer and the body of a 2xx while it is delivered.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Storage,
    native::{
        Error, Host,
        net::{
            BODY_PART, FileRoot, Header, HttpDone, MAX_INLINE_BODY, Net, REPLY, REQUEST_INPUT,
            Reason, Request, RequestId,
        },
        secrets::{SecretName, SecretWrite, Secrets, wipe},
        storage::DataStorage,
    },
};
use paperpaw_ui_kit::mem;

use crate::native_model::{self as model, MAX_BODY, Outcome, Secret};

/// The plugin's own number of the request, in its `http_done` event.
pub const REQUEST_TOKEN: u32 = 1;
/// The secret the key is kept as, and the label the list of keys shows.
pub const KEY_NAME: &str = "deepl-key";
/// The label (a translation key of the plugin).
pub const KEY_LABEL: &str = "translate.key";
/// Where a form too long for the request is written.
const FORM_FILE: &str = "request.form";
/// The input and the reply a secrets call needs.
const SECRETS_INPUT: usize = 1280;
/// The `User-Agent` of interface 6.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";

/// What keeping the key came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kept {
    /// Kept: it is there after a restart.
    Saved,
    /// Nothing typed: nothing changes (as in interface 6).
    Empty,
    /// The host cannot keep passwords (`ERR_UNSUPPORTED`): the key is used for
    /// this time only, and the reader is told (`core.secrets-unsupported`).
    Unsupported,
}

/// The handles and the request in flight.
#[derive(Debug)]
pub struct Asker {
    net: Option<Net>,
    secrets: Option<Secrets>,
    pending: Option<RequestId>,
    /// A form file may be in the data folder.
    staged: bool,
}

impl Asker {
    /// A new one, with no handle opened yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            net: None,
            secrets: None,
            pending: None,
            // A form a cut-off run left behind is taken away with the first one.
            staged: true,
        }
    }

    /// Whether a request is in flight.
    #[must_use]
    pub const fn waiting(&self) -> bool {
        self.pending.is_some()
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

    /// The key kept: `None` when there is none (or it is empty).
    ///
    /// # Errors
    /// What the service says; `NoMemory`.
    pub fn key(&mut self, host: &Host<'_>) -> Result<Option<Secret>, Error> {
        let secrets = self.secrets(host)?;
        let mut input = mem::buffer(SECRETS_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let found = match secrets.get(host, KEY_NAME, &mut input, &mut reply) {
            Ok(value) if value.value().is_empty() => Ok(None),
            Ok(value) => Secret::new(value.value()).map(Some),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut reply);
        found
    }

    /// Keeps what was typed (trimmed) as the key.
    ///
    /// # Errors
    /// What the service says, except `Unsupported` (that is [`Kept::Unsupported`]);
    /// `Invalid` for a key the secrets cannot hold; `NoMemory`.
    pub fn keep_key(&mut self, host: &Host<'_>, typed: &str) -> Result<Kept, Error> {
        let key = typed.trim();
        if key.is_empty() {
            return Ok(Kept::Empty);
        }
        let secrets = self.secrets(host)?;
        let mut input = mem::buffer(SECRETS_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let result = SecretWrite::new(SecretName::new(KEY_NAME)?, key)
            .and_then(|write| write.label(KEY_LABEL))
            .map(SecretWrite::changeable)
            .and_then(|write| secrets.keep(host, write, &mut input, &mut reply));
        wipe(&mut input);
        wipe(&mut reply);
        match result {
            Ok(()) => Ok(Kept::Saved),
            Err(Error::Unsupported) => Ok(Kept::Unsupported),
            Err(error) => Err(error),
        }
    }

    /// Sends `text` off to DeepL, translated into `target` with `key`. A
    /// request still in flight is cancelled first.
    ///
    /// # Errors
    /// `Busy` at 4 requests of the plugin (wait for an `http_done`), `State`
    /// without the subscription, `Denied` without the network feature,
    /// `Invalid` for a key the headers cannot hold, `NoMemory`; otherwise the
    /// host's.
    pub fn ask(
        &mut self,
        host: &Host<'_>,
        key: &str,
        text: &str,
        target: &str,
    ) -> Result<(), Error> {
        self.net(host)?;
        let url = model::url(key)?;
        let authorization = model::authorization(key)?;
        let form = model::body(text, target)?;
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let result = self.start(
            host,
            &url,
            &authorization,
            form.as_bytes(),
            &mut input,
            &mut reply,
        );
        // The input held the key in the header.
        wipe(&mut input);
        wipe(&mut reply);
        let id = result?;
        self.pending = Some(id);
        Ok(())
    }

    fn start(
        &mut self,
        host: &Host<'_>,
        url: &str,
        authorization: &Secret,
        form: &[u8],
        input: &mut [u8],
        reply: &mut [u8],
    ) -> Result<RequestId, Error> {
        let headers = [
            Header::new("Authorization", authorization.as_str())?,
            Header::new("Content-Type", "application/x-www-form-urlencoded")?,
            Header::new("User-Agent", USER_AGENT)?,
        ];
        // A request still in flight ends with its own `http_done`, which `done`
        // ignores (another id); it goes first, so that the form it sends is not
        // replaced under it.
        if self.pending.take().is_some()
            && let Some(net) = self.net.as_ref()
        {
            net.cancel(host, REQUEST_TOKEN, input, reply).ok();
        }
        self.remove_form(host);
        let mut request = Request::new(REQUEST_TOKEN, "POST", url)?
            .headers(&headers)?
            .max_body(u32::try_from(MAX_BODY).map_err(|_| Error::Invalid)?);
        if form.len() <= MAX_INLINE_BODY {
            request = request.body(form)?;
        } else {
            DataStorage::new(host)
                .write(FORM_FILE, form)
                .map_err(|_| Error::Io)?;
            self.staged = true;
            request = request.send_file(FileRoot::Data, FORM_FILE)?;
        }
        let net = self.net.as_ref().ok_or(Error::State)?;
        let started = net.request(host, &request, input, reply);
        if started.is_err() {
            self.remove_form(host);
        }
        started
    }

    /// Takes the form file away.
    fn remove_form(&mut self, host: &Host<'_>) {
        if core::mem::take(&mut self.staged) {
            DataStorage::new(host).remove(FORM_FILE).ok();
        }
    }

    /// Cancels the request in flight; its `http_done` follows and is ignored.
    pub fn cancel(&mut self, host: &Host<'_>) {
        if self.pending.take().is_some()
            && let (Some(net), Ok(mut input), Ok(mut reply)) = (
                self.net.as_ref(),
                mem::buffer(REQUEST_INPUT),
                mem::buffer(REPLY),
            )
        {
            net.cancel(host, REQUEST_TOKEN, &mut input, &mut reply).ok();
        }
        self.remove_form(host);
    }

    /// What the `http_done` event says about the request in flight: `None`
    /// for one that is not it (cancelled, another token or id).
    ///
    /// # Errors
    /// `NoMemory`; what the host says while the answer is read.
    pub fn done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<Option<Outcome>, Error> {
        if done.token() != REQUEST_TOKEN || self.pending != Some(done.id()) {
            return Ok(None);
        }
        self.pending = None;
        self.remove_form(host);
        let status = match done.reason() {
            Reason::Ok => u16::try_from(done.status()).unwrap_or(u16::MAX),
            Reason::Cancelled | Reason::Replaced => return Ok(None),
            // Anything else is no answer (interface 6: status 0).
            _ => 0,
        };
        let net = self.net.as_ref().ok_or(Error::State)?;
        if !(200..=299).contains(&status) {
            return model::outcome(status, "").map(Some);
        }
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut reply = mem::buffer(REPLY)?;
        let body = read_body(net, host, done, &mut input, &mut reply);
        wipe(&mut input);
        wipe(&mut reply);
        let body = body?;
        match core::str::from_utf8(&body) {
            Ok(text) => model::outcome(status, text).map(Some),
            Err(_) => Ok(Some(Outcome::Failed)),
        }
    }
}

/// The memory body of the request that ended, read in pieces and kept at
/// most [`MAX_BODY`] bytes.
fn read_body(
    net: &Net,
    host: &Host<'_>,
    done: &HttpDone,
    input: &mut [u8],
    reply: &mut [u8],
) -> Result<Vec<u8>, Error> {
    let length = usize::try_from(done.body_len()).map_err(|_| Error::NoSpace)?;
    if length > MAX_BODY {
        return Err(Error::NoSpace);
    }
    let mut body = Vec::new();
    body.try_reserve_exact(length)
        .map_err(|_| Error::NoMemory)?;
    while body.len() < length {
        let offset = u32::try_from(body.len()).map_err(|_| Error::NoSpace)?;
        let part = net.body(host, done.id(), offset, input, reply)?;
        if part.is_empty() {
            break;
        }
        let take = part.len().min(BODY_PART).min(length - body.len());
        body.extend_from_slice(part.get(..take).ok_or(Error::Invalid)?);
    }
    Ok(body)
}

/// Whether `form` goes inline in the request.
#[must_use]
pub const fn inline(form: &str) -> bool {
    form.len() <= MAX_INLINE_BODY
}

impl Default for Asker {
    fn default() -> Self {
        Self::new()
    }
}
