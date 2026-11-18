//! Transfer over the USB cable on interface 7, host-free: a computer at the
//! other end of the cable (the workshop, a page in a browser) installs a
//! plugin or adds books, after the reader allowed it on the device. The
//! requests are those of `docs/usb-transfer.md`; the screen that serves them
//! (`native_plugin/cable.rs`) fills the slot `core.cable` and serves port 0.
//!
//! - `GET /api/hello`, `GET /api/plugins`: no permission needed.
//! - `POST /api/offer`: the body (a file of the data folder, read a line at a
//!   time, at most [`MAX_FILES`] lines) says what the computer means to send.
//!   The device asks the reader with a confirm question and answers *202*
//!   `{"state":"asking"}`; *507* where there is not room for it all; *409*
//!   while it asks about another offer or while the Wi-Fi transfer is sent the
//!   same plugin. `GET /api/offer` says `none`, `asking`, `allowed` or
//!   `refused`. The answer counts for that offer only (the host says which
//!   question it answers).
//! - Once allowed, and only what was offered, each file as offered (its size
//!   and SHA-256): `PUT /api/plugin/<id>/<path>` into the cable's own staging
//!   folder ([`STAGING`], apart from the Wi-Fi's), `POST /api/plugin/<id>`
//!   (installed in one step: the answer comes when the host ended the install,
//!   as `install_done` tells), `PUT /api/book/<name>` (into the library
//!   folder under a free name, never over a book). Anything else is *403*
//!   before the permission and *404* after.
//!
//! The permission holds while the screen is open; the host's *Back* ends it
//! (the screen closes), and so does a while without a request (the screen's
//! timer). All allocation is fallible, nothing a computer sends can fail the
//! plugin, and every answer is an inline body (the cable takes no `file`
//! answers).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    files::{Volume, for_each_line_on},
    text::{concat, copy},
};

mod host;
pub use host::{Handles, HostCable, clean};

use crate::native_install::{Known, Staged};
use crate::native_transfer::{MAX_FILES, MAX_LINE, MAX_OFFER, Offer, Offered};

/// The cable's own staging folders in the data folder (`cable/<id>/<path>`),
/// apart from the Wi-Fi's (`staging/<id>`), which the Wi-Fi screen cleans.
pub const STAGING: &str = "cable";
/// Without a request this long, the screen closes and the permission with it.
pub const IDLE_MS: u64 = 5 * 60 * 1000;
/// The first number of the install requests of the cable (the Wi-Fi's are
/// from `0x1000`, the own network's is `0x7fff_0001`).
const FIRST_INSTALL: u32 = 0x7ffe_0000;

const NOT_ALLOWED: &str = "{\"why\":\"not allowed on the device\"}";
const NOT_FOUND: &str = "{\"why\":\"not found\"}";
const NOT_OFFERED: &str = "{\"why\":\"not offered\"}";
const NOT_AS_OFFERED: &str = "{\"why\":\"not as offered\"}";
const BUSY: &str = "{\"why\":\"busy\"}";

/// Where the reader stands on the offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Permission {
    /// No offer (yet).
    None,
    /// The reader is asked.
    Asking,
    /// The reader allowed what was offered.
    Allowed,
    /// The reader refused, or the question closed without an answer.
    Refused,
}

/// What happened last, to be said on the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Said {
    /// A file of plugin `id` came.
    Receiving(String),
    /// A book was put into the library, under this name.
    Received(String),
    /// The host asks the reader about installing plugin `id`.
    Installing(String),
    /// Plugin `id` was installed; `restart` where its code runs after a
    /// restart.
    Installed { id: String, restart: bool },
    /// Plugin `id` was not installed.
    NotInstalled(String),
}

/// The body of an answer: a text kept in the plugin, or one made for it.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Body {
    Text(&'static str),
    Owned(String),
}

impl Body {
    /// The bytes to send.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Text(text) => text.as_bytes(),
            Self::Owned(text) => text.as_bytes(),
        }
    }
}

/// An answer: always JSON, always inline.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Reply {
    pub status: u16,
    pub body: Body,
}

impl Reply {
    #[must_use]
    pub fn text(status: u16, body: &'static str) -> Self {
        Self {
            status,
            body: Body::Text(body),
        }
    }

    #[must_use]
    pub fn owned(status: u16, body: String) -> Self {
        Self {
            status,
            body: Body::Owned(body),
        }
    }
}

/// What a request gets: its answer, or one that comes later (an install the
/// host asks the reader about).
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Answer {
    Now(Reply),
    Later,
}

/// A request of the computer's.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Call<'a> {
    /// The host's number of the request: its answer carries it.
    pub number: u32,
    pub method: &'a str,
    pub target: &'a str,
    /// The file of the data folder its body was saved in, and the body's size.
    pub body: Option<&'a str>,
    pub body_len: u64,
}

/// What a book put into the library came to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Imported {
    /// In the library, at this path.
    Taken(String),
    /// The body's SHA-256 is not the one offered.
    Damaged,
}

/// What the host does for the cable's requests.
pub trait CableHost {
    /// The `hello` answer: the device and its PaperPaw, as JSON.
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn hello(&mut self) -> Result<String, Error>;
    /// The installed plugins from the `skip`th on, as JSON that fits an inline
    /// body (with `"next"` where it was cut).
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn listing(&mut self, skip: usize) -> Result<String, Error>;
    /// The offer in the body file `body`: `None` where it is not one.
    ///
    /// # Errors
    /// `NoMemory`.
    fn read_offer(&mut self, body: &str) -> Result<Option<Offer>, Error>;
    /// The free room on the card, `None` where it is not known.
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn free_space(&mut self) -> Result<Option<u64>, Error>;
    /// Whether the Wi-Fi transfer is being sent plugin `id` now.
    fn wifi_is_sent(&mut self, id: &str) -> bool;
    /// Asks the reader about `offer`: the request that names the question.
    ///
    /// # Errors
    /// `Full` or `Busy` where no question can be asked now; `NoMemory`.
    fn ask(&mut self, offer: &Offer) -> Result<u32, Error>;
    /// Removes what was staged for plugin `id`.
    fn forget(&mut self, id: &str);
    /// Takes the body file `body` as file `path` of plugin `id`, if its
    /// SHA-256 is `sha256`; `body` is gone afterwards.
    ///
    /// # Errors
    /// `NoMemory`, or a refusal that is not the file's.
    fn stage(
        &mut self,
        id: &str,
        path: &str,
        body: &str,
        sha256: &[u8; 32],
    ) -> Result<Staged, Error>;
    /// The paths of the files staged for `id` (below its folders too), at most
    /// `limit` of them.
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn staged_files(&mut self, id: &str, limit: usize) -> Result<Vec<String>, Error>;
    /// Whether the staged file `path` of `id` has this size and SHA-256.
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn staged_matches(
        &mut self,
        id: &str,
        path: &str,
        size: u64,
        sha256: &[u8; 32],
    ) -> Result<bool, Error>;
    /// How the registry knows `id`.
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn known(&mut self, id: &str) -> Result<Known, Error>;
    /// Asks the host to install the folder staged for `id` as request `request`.
    ///
    /// # Errors
    /// `Denied`, `Unsupported`, `Full` or `Busy` from the host; `NoMemory`.
    fn install(&mut self, request: u32, id: &str, update: bool) -> Result<(), Error>;
    /// Puts the body file `body` into the library as `name` under a free name,
    /// if its SHA-256 is `sha256`.
    ///
    /// # Errors
    /// The library's refusals (`Full`, `Busy`, ...); `NoMemory`.
    fn import(&mut self, body: &str, name: &str, sha256: &[u8; 32]) -> Result<Imported, Error>;
}

/// The offer in the file `path` of `volume`, read a line at a time (so an offer
/// of any length costs little memory): `None` unless it is whole, understood
/// and names only what may be written.
///
/// # Errors
/// `NoMemory`.
pub fn read_offer(volume: &(impl Volume + ?Sized), path: &str) -> Result<Option<Offer>, Error> {
    match volume.size(path) {
        Ok(size) if size <= MAX_OFFER => {}
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        _ => return Ok(None),
    }
    let mut offer = Offer::default();
    let mut lines = 0usize;
    let mut understood = true;
    let read = for_each_line_on(volume, path, &mut |line| {
        let line = line.trim();
        if line.is_empty() {
            return Ok(core::ops::ControlFlow::Continue(()));
        }
        lines += 1;
        if lines > MAX_FILES || line.len() > MAX_LINE || offer.take(line)?.is_none() {
            understood = false;
            return Ok(core::ops::ControlFlow::Break(()));
        }
        Ok(core::ops::ControlFlow::Continue(()))
    });
    match read {
        Ok(()) => Ok(if understood { offer.finish() } else { None }),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}

/// Whether the files staged for plugin `id` are those `offered`, each with the
/// size and SHA-256 offered, and no others: what the reader allowed.
///
/// # Errors
/// What the host refuses; `NoMemory`.
pub fn staged_as_offered(
    host: &mut dyn CableHost,
    id: &str,
    offered: &[Offered],
) -> Result<bool, Error> {
    let staged = host.staged_files(id, offered.len().saturating_add(1))?;
    if staged.len() != offered.len() {
        return Ok(false);
    }
    for (path, size, sha256) in offered {
        if !staged.contains(path) || !host.staged_matches(id, path, *size, sha256)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether the `sha256` a request's query names, if it names one, is `offered`.
#[must_use]
pub fn same_digest(query: &str, offered: &[u8; 32]) -> bool {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("sha256="))
        .is_none_or(|hex| {
            paperpaw_sdk::digest::hex::decode_array::<32>(hex).as_ref() == Some(offered)
        })
}

/// The number `skip` of a query, 0 where there is none.
fn skipped(query: &str) -> usize {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("skip="))
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// The answer to what the host refused while a request was served.
fn failure(error: Error) -> Reply {
    match error {
        Error::Busy => Reply::text(503, BUSY),
        Error::Full | Error::NoSpace => Reply::text(507, "{\"why\":\"not enough room\"}"),
        Error::Unsupported => Reply::text(501, "{\"why\":\"not supported\"}"),
        Error::Denied => Reply::text(403, "{\"why\":\"not allowed\"}"),
        // A name the library does not take (it is stricter than the offer's).
        Error::Invalid => Reply::text(422, "{\"why\":\"name\"}"),
        _ => Reply::text(500, "{\"why\":\"storage\"}"),
    }
}

/// A plugin that was not installed: *422* `{"state":"refused","why":…}`.
fn refused(why: &str) -> Result<Reply, Error> {
    Ok(Reply::owned(
        422,
        concat(&["{\"state\":\"refused\",\"why\":\"", why, "\"}"])?,
    ))
}

/// An install the host was asked for, and the request waiting for its end.
#[derive(Debug)]
struct Installing {
    /// The request number the host's `install_done` carries.
    request: u32,
    /// The computer's request that waits for the answer.
    reply: u32,
    id: String,
}

/// The cable's state while its screen is open: what the computer offered, how
/// the reader stands on it, and what was asked of the host.
#[derive(Debug)]
pub struct Cable {
    permission: Permission,
    offer: Option<Offer>,
    /// The question asked about the offer (the host's request number).
    asked: Option<u32>,
    installing: Option<Installing>,
    said: Option<Said>,
    sent: u32,
    /// Counts what the screen shows changing (the permission, what was said).
    shown: u32,
}

impl Default for Cable {
    fn default() -> Self {
        Self::new()
    }
}

impl Cable {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            permission: Permission::None,
            offer: None,
            asked: None,
            installing: None,
            said: None,
            sent: 0,
            shown: 0,
        }
    }

    /// A number that changes whenever what the screen shows does.
    #[must_use]
    pub const fn shown(&self) -> u32 {
        self.shown
    }

    fn set(&mut self, permission: Permission) {
        self.permission = permission;
        self.shown = self.shown.wrapping_add(1);
    }

    fn say(&mut self, said: Said) {
        self.said = Some(said);
        self.shown = self.shown.wrapping_add(1);
    }

    #[must_use]
    pub const fn permission(&self) -> Permission {
        self.permission
    }

    /// What happened last.
    #[must_use]
    pub const fn said(&self) -> Option<&Said> {
        self.said.as_ref()
    }

    /// Whether the reader is asked, or the host asks about a plugin: the screen
    /// does not close while it waits for an answer.
    #[must_use]
    pub const fn waiting(&self) -> bool {
        self.asked.is_some() || self.installing.is_some()
    }

    /// Answers a request of the computer's. A refusal of the host that is not
    /// the computer's to hear is answered 5xx; only `NoMemory` is an error.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn handle(&mut self, host: &mut dyn CableHost, call: &Call<'_>) -> Result<Answer, Error> {
        match self.route(host, call) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => Ok(Answer::Now(failure(error))),
            Ok(answer) => Ok(answer),
        }
    }

    fn route(&mut self, host: &mut dyn CableHost, call: &Call<'_>) -> Result<Answer, Error> {
        let (path, query) = call.target.split_once('?').unwrap_or((call.target, ""));
        let reply = match (call.method, path) {
            ("GET", "/api/hello") => Reply::owned(200, host.hello()?),
            ("GET", "/api/plugins") => Reply::owned(200, host.listing(skipped(query))?),
            ("POST", "/api/offer") => self.offered(host, call)?,
            ("GET", "/api/offer") => self.state(),
            _ if self.permission != Permission::Allowed => Reply::text(403, NOT_ALLOWED),
            ("PUT", path) if path.starts_with("/api/plugin/") => {
                self.plugin_file(host, call, path, query)?
            }
            ("POST", path) if path.starts_with("/api/plugin/") => {
                return self.install(host, call, path);
            }
            ("PUT", path) if path.starts_with("/api/book/") => {
                self.book(host, call, path, query)?
            }
            _ => Reply::text(404, NOT_FOUND),
        };
        Ok(Answer::Now(reply))
    }

    fn state(&self) -> Reply {
        Reply::text(
            200,
            match self.permission {
                Permission::None => "{\"state\":\"none\"}",
                Permission::Asking => "{\"state\":\"asking\"}",
                Permission::Allowed => "{\"state\":\"allowed\"}",
                Permission::Refused => "{\"state\":\"refused\"}",
            },
        )
    }

    /// An offer came: the reader is asked.
    fn offered(&mut self, host: &mut dyn CableHost, call: &Call<'_>) -> Result<Reply, Error> {
        // One question at a time: an answer is never taken for another.
        if self.permission == Permission::Asking {
            return Ok(Reply::text(409, "{\"why\":\"asking about another offer\"}"));
        }
        // An install is running for the offer before: a new offer would forget
        // what is being installed.
        if self.installing.is_some() {
            return Ok(Reply::text(409, BUSY));
        }
        // A new offer ends the one before, what was sent for it too.
        self.forget_offer(host);
        self.set(Permission::None);
        let offer = match call.body {
            Some(body) => host.read_offer(body)?,
            None => None,
        };
        let Some(offer) = offer else {
            return Ok(Reply::text(400, "{\"why\":\"not an offer\"}"));
        };
        let id = offer.plugin.as_ref().map(|(id, _)| id.as_str());
        // The Wi-Fi transfer is sent this plugin now: not both at once.
        if id.is_some_and(|id| host.wifi_is_sent(id)) {
            return Ok(Reply::text(409, "{\"why\":\"being sent over Wi-Fi\"}"));
        }
        let room = host.free_space()?;
        match offer.size() {
            Some(size) if room.is_none_or(|free| size <= free) => {}
            _ => return Ok(Reply::text(507, "{\"why\":\"not enough room\"}")),
        }
        // Nothing sent before counts: a plugin's files are taken anew.
        if let Some(id) = id {
            host.forget(id);
        }
        let request = match host.ask(&offer) {
            Ok(request) => request,
            Err(Error::Full | Error::Busy) => {
                return Ok(Reply::text(503, "{\"why\":\"cannot ask now\"}"));
            }
            Err(error) => return Err(error),
        };
        self.asked = Some(request);
        self.offer = Some(offer);
        self.set(Permission::Asking);
        Ok(Reply::text(202, "{\"state\":\"asking\"}"))
    }

    /// The reader answered question `request`: `yes` allows the offer it asked
    /// about, and anything else (no, or the dialog closed without an answer)
    /// refuses it. An answer to another question changes nothing. `true` where
    /// the state changed.
    pub fn answered(&mut self, host: &mut dyn CableHost, request: u32, yes: bool) -> bool {
        if self.permission != Permission::Asking || self.asked != Some(request) {
            return false;
        }
        self.asked = None;
        if yes {
            self.set(Permission::Allowed);
        } else {
            self.forget_offer(host);
            self.set(Permission::Refused);
        }
        true
    }

    /// Lets go of the offer, and of what was sent for its plugin.
    fn forget_offer(&mut self, host: &mut dyn CableHost) {
        self.asked = None;
        if let Some((id, _)) = self.offer.take().and_then(|offer| offer.plugin) {
            host.forget(&id);
        }
    }

    /// The screen closed (or its time ran out): nothing is allowed any more,
    /// and what was staged goes.
    pub fn close(&mut self, host: &mut dyn CableHost) {
        self.forget_offer(host);
        if let Some(installing) = self.installing.take() {
            host.forget(&installing.id);
        }
        self.set(Permission::None);
    }

    fn plugin_file(
        &mut self,
        host: &mut dyn CableHost,
        call: &Call<'_>,
        path: &str,
        query: &str,
    ) -> Result<Reply, Error> {
        let rest = path.strip_prefix("/api/plugin/").unwrap_or_default();
        let Some((id, file)) = rest.split_once('/') else {
            return Ok(Reply::text(404, NOT_FOUND));
        };
        let offered = self
            .offer
            .as_ref()
            .and_then(|offer| offer.plugin_file(id, file));
        let (Some((size, sha256)), Some(body)) = (offered, call.body) else {
            return Ok(Reply::text(403, NOT_OFFERED));
        };
        if call.body_len != size || !same_digest(query, &sha256) {
            return Ok(Reply::text(422, NOT_AS_OFFERED));
        }
        let said = Said::Receiving(copy(id)?);
        match host.stage(id, file, body, &sha256)? {
            Staged::Taken => {
                self.say(said);
                Ok(Reply::text(200, "{\"ok\":true}"))
            }
            Staged::Damaged => refused("damaged"),
            _ => refused("storage"),
        }
    }

    /// Installs the plugin from the files sent, in one step: the host asks the
    /// reader, and the answer comes with its `install_done`.
    fn install(
        &mut self,
        host: &mut dyn CableHost,
        call: &Call<'_>,
        path: &str,
    ) -> Result<Answer, Error> {
        let id = path.strip_prefix("/api/plugin/").unwrap_or_default();
        if self.installing.is_some() {
            return Ok(Answer::Now(Reply::text(409, BUSY)));
        }
        let files = match self.offer.as_ref().and_then(|offer| offer.plugin.as_ref()) {
            Some((offered, files)) if offered == id => files,
            _ => return Ok(Answer::Now(Reply::text(403, NOT_OFFERED))),
        };
        // The reader allowed what was offered: what is staged must be it,
        // file by file, its code too, checked now (not only as it came).
        if !staged_as_offered(host, id, files)? {
            return Ok(Answer::Now(Reply::text(422, NOT_AS_OFFERED)));
        }
        let known = host.known(id)?;
        if known == Known::BuiltIn {
            return self.not_installed(host, id, "built in");
        }
        let (installing_id, said) = (copy(id)?, Said::Installing(copy(id)?));
        self.sent = self.sent.wrapping_add(1);
        let request = FIRST_INSTALL.wrapping_add(self.sent & 0xffff);
        match host.install(request, id, known == Known::Installed) {
            Ok(()) => {
                self.installing = Some(Installing {
                    request,
                    reply: call.number,
                    id: installing_id,
                });
                self.say(said);
                Ok(Answer::Later)
            }
            Err(Error::Denied) => self.not_installed(host, id, "not allowed"),
            Err(Error::Unsupported) => self.not_installed(host, id, "not supported"),
            // Four requests wait already: the files stay for another try.
            Err(Error::Full | Error::Busy) => Ok(Answer::Now(Reply::text(409, BUSY))),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => self.not_installed(host, id, "refused"),
        }
    }

    /// Plugin `id` was not installed, and why: what was sent goes.
    fn not_installed(
        &mut self,
        host: &mut dyn CableHost,
        id: &str,
        why: &str,
    ) -> Result<Answer, Error> {
        let reply = refused(why)?;
        let said = Said::NotInstalled(copy(id)?);
        host.forget(id);
        if let Some(offer) = &mut self.offer {
            offer.plugin = None;
        }
        self.say(said);
        Ok(Answer::Now(reply))
    }

    /// The host ended install `request`: the computer's request waiting for it
    /// and its answer. `None` where it is not the cable's install.
    ///
    /// # Errors
    /// `NoMemory`: the end stays to be told again.
    pub fn install_done(
        &mut self,
        host: &mut dyn CableHost,
        request: u32,
        result: Result<bool, &'static str>,
    ) -> Result<Option<(u32, Reply)>, Error> {
        let Some(installing) = self.installing.as_ref().filter(|i| i.request == request) else {
            return Ok(None);
        };
        let (reply, said) = match result {
            Ok(restart) => (
                Reply::owned(
                    200,
                    concat(&[
                        "{\"state\":\"installed\",\"restart\":",
                        if restart { "true" } else { "false" },
                        "}",
                    ])?,
                ),
                Said::Installed {
                    id: copy(&installing.id)?,
                    restart,
                },
            ),
            Err(why) => (refused(why)?, Said::NotInstalled(copy(&installing.id)?)),
        };
        let Some(installing) = self.installing.take() else {
            return Ok(None);
        };
        // The host copied the folder; what is left is let go.
        host.forget(&installing.id);
        if let Some(offer) = &mut self.offer {
            offer.plugin = None;
        }
        self.say(said);
        Ok(Some((installing.reply, reply)))
    }

    /// A book offered, put into the library once.
    fn book(
        &mut self,
        host: &mut dyn CableHost,
        call: &Call<'_>,
        path: &str,
        query: &str,
    ) -> Result<Reply, Error> {
        let raw = path.strip_prefix("/api/book/").unwrap_or_default();
        let name = paperpaw_sdk::web::try_percent_decode(raw).map_err(|_| Error::NoMemory)?;
        let offered = self.offer.as_ref().and_then(|offer| offer.book(&name));
        let (Some((size, sha256)), Some(body)) = (offered, call.body) else {
            return Ok(Reply::text(403, NOT_OFFERED));
        };
        if call.body_len != size || !same_digest(query, &sha256) {
            return Ok(Reply::text(422, NOT_AS_OFFERED));
        }
        // Everything that can fail for want of memory is made before the book
        // goes in: once it is in the library the answer is 200.
        let said = Said::Received(copy(&name)?);
        let Imported::Taken(at) = host.import(body, &name, &sha256)? else {
            return Ok(Reply::text(422, NOT_AS_OFFERED));
        };
        // Offered once, taken once.
        if let Some(offer) = &mut self.offer {
            let key = crate::native_transfer::name_key(&name);
            offer.books.retain(|(k, ..)| *k != key);
        }
        self.say(said);
        Ok(match path_json(&at) {
            Ok(json) => Reply::owned(200, json),
            Err(_) => Reply::text(200, "{\"ok\":true}"),
        })
    }
}

/// `{"path": …}`.
fn path_json(path: &str) -> Result<String, Error> {
    let mut json = String::new();
    json.try_reserve(16 + path.len())
        .map_err(|_| Error::NoMemory)?;
    json.push_str("{\"path\":");
    crate::native_web::push_json(&mut json, path)?;
    json.push('}');
    Ok(json)
}

#[cfg(test)]
mod tests;
