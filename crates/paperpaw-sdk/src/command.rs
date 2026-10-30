use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::id::Id;
use crate::params::Params;
use crate::settings::SettingValue;
use paperpaw_platform::RefreshMode;

/// An HTTP request of a plugin. When it is answered the kernel emits
/// `reply` with `params` plus `status` (the HTTP status; `0` when the
/// request failed, e.g. without network) and `body` (the response as text).
///
/// Made with [`get`](Self::get) or [`new`](Self::new) and the methods
/// that set the rest; every request is answered once, also when it is
/// refused or taken back ([`HttpReply::refused`](crate::HttpReply)).
///
/// Printed for debugging without what may be secret: the address without
/// its query, the headers' names without their values, the body's length
/// without the body, the params' names without their values.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpRequest {
    /// `GET`, `PUT`, `POST`, …
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    /// A file in storage to send as the body instead, e.g. a database to
    /// upload.
    body_from: Option<String>,
    /// A file in storage to write the response's body to (also large
    /// ones, e.g. a download); the reply then has no `body`.
    save_to: Option<String>,
    reply: Id,
    params: Params,
    /// What few requests set, kept apart so that commands stay small
    /// ([`options`](Self::options)).
    options: Option<alloc::boxed::Box<HttpOptions>>,
}

impl core::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let address = self.url.split(['?', '#']).next().unwrap_or_default();
        let headers: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        let params: Vec<&str> = self.params.iter().map(|(name, _)| name).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &address)
            .field("headers", &headers)
            .field("body", &self.body.as_ref().map(String::len))
            .field("body_from", &self.body_from)
            .field("save_to", &self.save_to)
            .field("reply", &self.reply)
            .field("params", &params)
            .field("options", &self.options)
            .finish()
    }
}

/// An [`HttpRequest`]'s parts, for the kernel to take over.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[derive(Debug)]
#[non_exhaustive]
pub struct HttpParts {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub body_from: Option<String>,
    pub save_to: Option<String>,
    pub reply: Id,
    pub params: Params,
    pub options: HttpOptions,
}

/// What few requests set ([`HttpRequest::options`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpOptions {
    /// How long it may take in all, the connection and TLS counted; for
    /// a download to a file ([`HttpRequest::save_to`]) or an upload from
    /// one, how long it may go without a byte, however long it takes in
    /// all. `None`: the default, 30 s, or a minute without a byte.
    pub timeout: Option<core::time::Duration>,
    /// Headers of the answer to pass back in
    /// [`HttpReply::headers`](crate::HttpReply), by name (any case).
    /// `ETag` and `Content-Type` always come back.
    pub want_headers: Vec<String>,
    /// The longest body kept in memory, in bytes: a longer one is cut
    /// there. `None`: by the device's memory (64 KiB on small devices,
    /// 1 MiB on others); at most 1 MiB. Large answers go to a file
    /// ([`HttpRequest::save_to`]).
    pub max_body: Option<usize>,
}

/// The options of a request that sets none.
static NO_OPTIONS: HttpOptions = HttpOptions {
    timeout: None,
    want_headers: Vec::new(),
    max_body: None,
};

impl HttpRequest {
    /// A request without headers or body; its answer comes as `reply`, an
    /// [`Event<HttpReply>`](crate::replies::HttpReply) (or an id).
    pub fn new(method: &str, url: &str, reply: impl Into<Id>) -> Self {
        Self {
            method: String::from(method),
            url: String::from(url),
            headers: Vec::new(),
            body: None,
            body_from: None,
            save_to: None,
            reply: reply.into(),
            params: Params::new(),
            options: None,
        }
    }

    /// Its timeout, the headers it wants back and the longest body.
    pub fn options(&self) -> &HttpOptions {
        self.options.as_deref().unwrap_or(&NO_OPTIONS)
    }

    /// Its method, e.g. `GET`.
    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// The body it sends, as text ([`body`](Self::body)).
    pub fn body_text(&self) -> Option<&str> {
        self.body.as_deref()
    }

    /// The file it sends as its body ([`body_from`](Self::body_from)).
    pub fn body_file(&self) -> Option<&str> {
        self.body_from.as_deref()
    }

    /// Where its answer's body is saved ([`save_to`](Self::save_to)).
    pub fn save_path(&self) -> Option<&str> {
        self.save_to.as_deref()
    }

    /// The event its answer comes as.
    pub fn reply(&self) -> &Id {
        &self.reply
    }

    /// The parameters passed back with its answer.
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Its parts, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_parts(self) -> HttpParts {
        HttpParts {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            body_from: self.body_from,
            save_to: self.save_to,
            reply: self.reply,
            params: self.params,
            options: self.options.map(|o| *o).unwrap_or_default(),
        }
    }

    fn options_mut(&mut self) -> &mut HttpOptions {
        self.options.get_or_insert_with(Default::default)
    }

    /// A `GET` of `url`.
    pub fn get(url: &str, reply: impl Into<Id>) -> Self {
        Self::new("GET", url, reply)
    }

    /// With this header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// With this body, as text.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// With the file at this storage path as its body.
    pub fn body_from(mut self, path: impl Into<String>) -> Self {
        self.body_from = Some(path.into());
        self
    }

    /// Its answer's body written to the file at this storage path.
    pub fn save_to(mut self, path: impl Into<String>) -> Self {
        self.save_to = Some(path.into());
        self
    }

    /// With these parameters, passed back with the reply.
    pub fn with(mut self, params: Params) -> Self {
        let progress = self.params.get(http_params::PROGRESS).map(String::from);
        self.params = params;
        if let Some(progress) = progress {
            self.params.set(http_params::PROGRESS, progress);
        }
        self
    }

    /// With `event` while its answer's body comes (a download, see
    /// [`save_to`](Self::save_to)): how much came and, where the server
    /// said, how much there is in all ([`HttpProgress`](crate::HttpProgress),
    /// with this request's parameters). The kernel passes it on at most
    /// every 5 % or every 2 seconds, and never after the reply: a screen
    /// redraws its progress no more often than that.
    pub fn progress(mut self, event: &crate::event::Event<crate::HttpProgress>) -> Self {
        self.params
            .set(http_params::PROGRESS, String::from(event.id().as_str()));
        self
    }

    /// With this parameter too, passed back with the reply.
    pub fn param(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.params.set(key, value);
        self
    }

    /// Taking at most this long in all (a file's download or upload: at
    /// most this long without a byte).
    pub fn timeout(mut self, timeout: core::time::Duration) -> Self {
        self.options_mut().timeout = Some(timeout);
        self
    }

    /// Passing back these headers of the answer.
    pub fn want_headers(mut self, names: &[&str]) -> Self {
        self.options_mut().want_headers = names.iter().map(|n| String::from(*n)).collect();
        self
    }

    /// Keeping at most this many bytes of the answer's body in memory.
    pub fn max_body(mut self, bytes: usize) -> Self {
        self.options_mut().max_body = Some(bytes);
        self
    }
}

/// The answer to a request a plugin's server received (see
/// [`Command::Serve`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServerResponse {
    /// The request's number ([`ServerNotice::Request`](crate::ServerNotice::Request)).
    request: u32,
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// A file in storage to send as the body instead.
    body_from: Option<String>,
}

/// A [`ServerResponse`]'s parts, for the kernel to take over.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[derive(Debug)]
#[non_exhaustive]
pub struct ResponseParts {
    pub request: u32,
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub body_from: Option<String>,
}

impl ServerResponse {
    /// An answer with `body` of `content_type`.
    pub fn new(request: u32, status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            request,
            status,
            content_type: String::from(content_type),
            headers: Vec::new(),
            body,
            body_from: None,
        }
    }

    /// With this header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// With the file at this storage path as its body.
    pub fn body_from(mut self, path: impl Into<String>) -> Self {
        self.body_from = Some(path.into());
        self
    }

    /// The number of the request it answers.
    pub fn request(&self) -> u32 {
        self.request
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The file it sends as its body ([`body_from`](Self::body_from)).
    pub fn body_file(&self) -> Option<&str> {
        self.body_from.as_deref()
    }

    /// Its parts, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_parts(self) -> ResponseParts {
        ResponseParts {
            request: self.request,
            status: self.status,
            content_type: self.content_type,
            headers: self.headers,
            body: self.body,
            body_from: self.body_from,
        }
    }
}

/// UDP datagrams to the local network (see [`Command::Broadcast`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Broadcast {
    id: Id,
    ports: Vec<u16>,
    data: Vec<u8>,
    wait_ms: u32,
    events: Id,
}

impl Broadcast {
    /// `data` to `ports`, answers within `wait_ms` heard as `events` with
    /// `id`.
    pub fn new(id: Id, ports: Vec<u16>, data: Vec<u8>, wait_ms: u32, events: Id) -> Self {
        Self {
            id,
            ports,
            data,
            wait_ms,
            events,
        }
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// How long answers are waited for, in milliseconds.
    pub fn wait_ms(&self) -> u32 {
        self.wait_ms
    }

    /// The event the answers come as.
    pub fn events(&self) -> &Id {
        &self.events
    }

    /// Its data, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
}

/// Parameters of a server's events (see [`Command::Serve`]).
pub mod server_params {
    /// What happened: [`STARTED`], [`REQUEST`], [`FAILED`] or [`STOPPED`].
    pub const KIND: &str = "kind";
    pub const STARTED: &str = "started";
    pub const REQUEST: &str = "request";
    pub const FAILED: &str = "failed";
    pub const STOPPED: &str = "stopped";
    /// Where the server is reached, e.g. `192.168.1.20:8080`.
    pub const ADDRESS: &str = "address";
    /// Why it failed.
    pub const REASON: &str = "reason";
    /// A request's method (`GET`, `PUT`, …) and target (path and query).
    pub const METHOD: &str = "method";
    pub const TARGET: &str = "target";
    /// Where a request's body was saved, if it had one; it is removed
    /// once the request is answered, unless moved.
    pub const BODY_FILE: &str = "body-file";
}

/// What a socket is to read next (see [`Command::SocketRead`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SocketRead {
    /// What has come, up to `max` bytes, as text (UTF-8; invalid bytes
    /// replaced): the notice [`SocketNotice::Received`](crate::SocketNotice::Received) with it.
    Text { max: u32 },
    /// Exactly `len` bytes, written to `path` in storage as they come (a
    /// file there is replaced): the notice [`SocketNotice::Saved`](crate::SocketNotice::Saved) once
    /// they all came, so files of any size never need to fit in memory.
    File { path: String, len: u64 },
}

/// Parameters of a socket's events (see [`Command::Connect`]) and of a
/// broadcast's (see [`Command::Broadcast`]).
pub mod socket_params {
    /// What happened: [`CONNECTED`], [`RECEIVED`], [`SAVED`], [`CLOSED`]
    /// or [`FAILED`]; for broadcasts [`ANSWER`] and [`DONE`].
    pub const KIND: &str = "kind";
    pub const CONNECTED: &str = "connected";
    /// Text came, in [`TEXT`]; empty when the other side closed.
    pub const RECEIVED: &str = "received";
    /// A file read is complete; its [`PATH`].
    pub const SAVED: &str = "saved";
    pub const CLOSED: &str = "closed";
    pub const FAILED: &str = "failed";
    /// An answer to a broadcast: its [`TEXT`] and where it came [`FROM`].
    pub const ANSWER: &str = "answer";
    /// No more answers to a broadcast will come.
    pub const DONE: &str = "done";
    pub const TEXT: &str = "text";
    pub const PATH: &str = "path";
    /// The address an answer came from, e.g. `192.168.1.20`.
    pub const FROM: &str = "from";
    /// Why it failed or closed.
    pub const REASON: &str = "reason";
}

/// Parameters of an HTTP reply event (see [`HttpRequest`]).
pub mod http_params {
    pub const STATUS: &str = "status";
    pub const BODY: &str = "body";
    /// The response's `ETag` header, if it had one.
    pub const ETAG: &str = "etag";
    /// The response's `Content-Type` header, if it had one, e.g.
    /// `text/html; charset=utf-8`.
    pub const CONTENT_TYPE: &str = "content-type";
    /// Where the answer came from, if redirected elsewhere (named apart
    /// from a request's own parameters, which come back too).
    pub const FINAL_URL: &str = "final-url";
    /// Before a header's name in lower case, for the headers asked for
    /// ([`HttpOptions::want_headers`](crate::HttpOptions::want_headers)), e.g. `header:last-modified`.
    pub const HEADER: &str = "header:";
    /// Why there was no answer, as [`HttpError::as_str`](crate::HttpError::as_str)
    /// writes it.
    pub const ERROR: &str = "error";
    /// `true` where the body was cut to the most the request takes.
    pub const TRUNCATED: &str = "truncated";
    /// In a request's parameters, the event its progress comes as
    /// ([`HttpRequest::progress`](crate::HttpRequest::progress)); the
    /// kernel takes it out.
    pub const PROGRESS: &str = "http-progress";
    /// How many bytes of the body came so far
    /// ([`HttpProgress`](crate::HttpProgress)).
    pub const RECEIVED: &str = "received";
    /// How many bytes the body has in all, where the server said.
    pub const TOTAL: &str = "total";
}

/// Text to speak aloud (see [`Command::Speak`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Speech {
    text: String,
    /// The language of the text, e.g. `de` or `en-GB`, for the voice;
    /// empty for the system's.
    language: String,
    /// How fast, in percent of the voice's normal speed.
    rate: u16,
    /// Emitted once the text has been spoken, or could not be: a
    /// [`Spoken`](crate::replies::Spoken) with `params`.
    reply: Id,
    params: Params,
    /// Spoken after the texts given before instead of replacing them: the
    /// next sentence handed over while one is spoken follows without a
    /// pause.
    queue: bool,
}

impl Speech {
    /// `text` in the system's language at normal speed.
    pub fn new(text: impl Into<String>, reply: impl Into<Id>) -> Self {
        Self {
            text: text.into(),
            language: String::new(),
            rate: 100,
            reply: reply.into(),
            params: Params::new(),
            queue: false,
        }
    }

    /// In the language with this tag (empty: the system's).
    pub fn in_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// At `rate` percent of the normal speed.
    pub fn at_rate(mut self, rate: u16) -> Self {
        self.rate = rate;
        self
    }

    /// With `params` passed back in the reply.
    pub fn with(mut self, params: Params) -> Self {
        self.params = params;
        self
    }

    /// After what is being spoken, instead of in its place.
    pub fn queued(mut self, queue: bool) -> Self {
        self.queue = queue;
        self
    }

    /// The text to speak.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The language's tag, e.g. `de`; empty for the system's.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// How fast, in percent of the voice's normal speed.
    pub fn rate(&self) -> u16 {
        self.rate
    }

    /// The event its reply comes as.
    pub fn reply(&self) -> &Id {
        &self.reply
    }

    /// The parameters passed back with its reply.
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Whether it is spoken after what is being spoken.
    pub fn is_queued(&self) -> bool {
        self.queue
    }

    /// Its text, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_text(self) -> String {
        self.text
    }

    /// Its reply: it was `spoken`, or could not be (a plugin's voice
    /// answers so, see [`voices`](crate::voices)).
    pub fn answer(&self, spoken: bool) -> Command {
        use crate::event::EventData;
        let reply = crate::replies::Spoken::new(spoken, self.params.clone());
        Command::Emit(self.reply.clone(), reply.into_params())
    }
}

/// Parameters of the reply to [`Command::Speak`].
pub mod speech_params {
    /// Present (`1`) when the text could not be spoken, e.g. without a
    /// voice for it.
    pub const FAILED: &str = "failed";
}

/// A request from a plugin to the kernel.
///
/// Plugins never mutate kernel state directly; commands are queued and
/// applied by the kernel after the plugin returns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Shows the screen bound to a slot, passing `Params` to it. If that
    /// slot is already open, the screens above it are closed.
    OpenSlot(Id, Params),
    /// Opens a specific screen on top of the current one.
    OpenScreen(Id, Params),
    /// Closes the current screen (no-op on the root screen).
    Back,
    /// Closes the open screen `.0` wherever it is in the stack, leaving
    /// the screens above it (see [`Cx::close`](crate::Cx::close)); none
    /// where it is the only one.
    Close(crate::ScreenKey),
    /// Opens screen `.0` with `.1` in place of the one on top, in one
    /// step: the screen below it is not shown between (the next step of
    /// a guide).
    Replace(Id, Params),
    /// Closes the screens above the topmost open screen `.0`, or the
    /// screen of slot `.0`: back to it in one step. Nothing where none is
    /// open.
    BackTo(Id),
    /// Opens a book with the reading app chosen for it, in
    /// [`slots::READER`](crate::builtin::slots::READER): its path in
    /// `params::PATH`, and the readers that could not open it. See
    /// [`readers::open`](crate::readers::open) and
    /// [`readers::cannot_open`](crate::readers::cannot_open), which make
    /// it, and [`readers::resolve`](crate::readers::resolve), which says
    /// what it does.
    OpenBook(Params),
    /// Opens screen `.1` for slot `.0` with `.2` as [`Command::OpenSlot`]
    /// opens the slot's own: shown again where the slot is open with it
    /// and these params, else in place of what the slot shows (one
    /// launcher of several, chosen once).
    OpenInSlot(Id, Id, Params),
    /// Runs action `.0`'s own handler with params `.1`, past the handlers
    /// put in its place and the screens: for a handler put in its place
    /// that does more around it. Only from such a handler of action `.0`
    /// (its commands then come where it asked for them); from anywhere
    /// else it is refused. Seen by no observer.
    RunOriginal(Id, Params),
    /// Raises the input `.0` as written in key maps (`button.menu`,
    /// `double-tap.center`, a plugin's `input.<id>`): it runs what the
    /// layers shown bind it to, as if it had come (switch access, chords
    /// of one's own).
    Input(String),
    /// Puts the key map layer `.1` in front of the slot's and the global
    /// layer for open screen `.0`, e.g. a book's own bindings
    /// (`[keymap.<layer>]`): it applies while that screen is on top and
    /// goes with it; not below a screen that guards the device. `None`
    /// takes it away. A screen sends it with
    /// [`Cx::set_key_layer`](crate::Cx::set_key_layer); sent from anywhere
    /// else (a handler, a rule), it is refused.
    SetKeyLayer(crate::ScreenKey, Option<Id>),
    /// Shows task `.0`, a tab of tabs: the screens open now are kept as
    /// the task shown before (by their ids and params; dropped from memory
    /// and made anew when it is shown again), and those of task `.0` open
    /// again, or the first time the screen of slot `.0` (or screen `.0`).
    /// Each task keeps its own way back; its first screen is the first of
    /// [`Env::stack`](crate::Env::stack). At start the task is the home
    /// slot's.
    SwitchTask(Id),
    /// Binds slot `.0` to screen `.1` for the user, or back to what the
    /// plugins bind it to (`None`): at once and kept in `paperpaw.ini`
    /// (`[slots]`), e.g. "use this as the home screen?".
    BindSlot(Id, Option<Id>),
    /// Shows the row of hints for a few seconds on a screen that has it
    /// only on a press ([`HintVisibility::OnPress`](crate::HintVisibility)).
    ShowHints,
    /// Changes a setting; invalid values are rejected with a warning.
    SetSetting(Id, SettingValue),
    /// Forgets the value set for a setting: its default applies again (in
    /// the active scope, where changes go there: the scope's value, the
    /// global one applies again). See `Settings::reset`.
    ResetSetting(Id),
    /// Shows settings with these values for a while without keeping them
    /// (`None`: as if not set), e.g. a layout before it is chosen: every
    /// screen drawn from them shows them, until the next
    /// `PreviewSettings` (empty: the values kept show again) or until the
    /// screen that asked closes; the event
    /// [`PREVIEW_CHANGED`](crate::builtin::events::PREVIEW_CHANGED)
    /// follows each change. What the kernel does when a setting changes
    /// (the language, the turn of the screen) waits for the value kept.
    /// See `Settings::set_preview`.
    PreviewSettings(Vec<(Id, Option<SettingValue>)>),
    /// Redraws the current screen completely; with
    /// [`RefreshMode::Full`] the whole screen, flashing.
    Redraw(RefreshMode),
    /// Keeps the device from going to sleep by itself for a while (see
    /// [`WakeHold`](crate::power::WakeHold)); the same id again renews
    /// it.
    KeepAwake(crate::power::WakeHold),
    /// Lets go of the hold of this id ([`Command::KeepAwake`]).
    LetSleep(Id),
    /// A command that comes after this release, by its key: what PaperPaw
    /// adds later reaches plugins built now without a new plugin
    /// interface. A kernel that does not know the key leaves it.
    #[non_exhaustive]
    Extended { key: Id, params: Params },
    /// Notifies all subscribers of an event, then every open screen.
    Emit(Id, Params),
    /// Runs action `.0` as if the user had triggered it (e.g. from a
    /// menu), with params `.1`: its handler gets them, and so does a
    /// screen that takes it ([`Cx::params`](crate::Cx::params)), e.g. a
    /// rule applying the profile it names. [`Command::run`] runs it
    /// without.
    RunAction(Id, Params),
    /// Switches a plugin on or off in `paperpaw.ini`; applies after a
    /// restart. The kernel refuses plugins that cannot be switched off
    /// (see [`PluginInfo::can_switch_off`](crate::PluginInfo)).
    SetPluginEnabled(Id, bool),
    /// Loads the data plugin in this folder on the storage (e.g.
    /// `plugins/font-fredoka`, just downloaded) at once, as if PaperPaw
    /// had found it at start: its fonts can be chosen, its translations,
    /// key bindings and presets apply. The event
    /// [`PLUGINS_CHANGED`](crate::builtin::events::PLUGINS_CHANGED)
    /// follows; problems are warnings, as at start.
    LoadPlugin(String),
    /// Removes a data plugin: its folder is deleted and its fonts and
    /// presets go at once (a font chosen from it gives way to the
    /// default); what else it brought (translations, key bindings,
    /// styles, defaults) is gone after the next start. The event
    /// [`PLUGINS_CHANGED`](crate::builtin::events::PLUGINS_CHANGED)
    /// follows. Plugins with code are not removed this way.
    RemovePlugin(Id),
    /// Installs the data plugin `Id` downloaded into this folder (one
    /// PaperPaw does not load, e.g. `plugins/.airy.part`, see
    /// [`install`](crate::install)): it takes the place of every version
    /// of that plugin on the storage (loaded, switched off or refused at
    /// start), becomes the folder named as it beside the one it was
    /// downloaded into, and is loaded at once. A plugin switched off stays
    /// off; what was chosen from the version before (e.g. its font for
    /// reading) stays chosen where the new one offers it too. The event
    /// [`PLUGINS_CHANGED`](crate::builtin::events::PLUGINS_CHANGED)
    /// follows. A folder that holds another plugin, or none that loads,
    /// is deleted with a warning, and the version before stays.
    InstallPlugin(Id, String),
    /// Makes an HTTP request (see [`HttpRequest`]); the answer comes as
    /// the request's reply event. Check [`Capabilities::network`](crate::Capabilities) first.
    /// Made with [`Command::http`].
    Http(alloc::boxed::Box<HttpRequest>),
    /// Uses the settings stored in the file at this storage path for
    /// [scoped](crate::SettingSpec::scoped) settings, e.g. those of the
    /// book being read, while the screen that sent it stays open; `None`
    /// ends the scope.
    SetScope(Option<String>),
    /// Redefines a menu with exactly these entries (actions, settings or
    /// submenus), saved in `[menus]` of `paperpaw.ini`; `None` returns to
    /// the entries plugins contribute.
    SetMenu(Id, Option<Vec<Id>>),
    /// Binds `input` (as in `paperpaw.ini`, e.g. `button.menu` or
    /// `tap.center`) in key map `layer` (a screen or slot id, or
    /// `global`) to `action`, saved in the user's `paperpaw.ini` and in
    /// effect at once; `None` returns it to what plugins and presets bind.
    /// Bind [`actions::NOTHING`](crate::builtin::actions::NOTHING) to make
    /// an input do nothing.
    #[non_exhaustive]
    Bind {
        layer: String,
        input: String,
        action: Option<Id>,
    },
    /// Shows a short message for a few seconds, e.g. "Sync done", by the
    /// screen bound to [`slots::NOTICE`](crate::builtin::slots::NOTICE),
    /// over whatever is open and without taking any input. A newer
    /// message replaces it. Only what the reader is doing shows one:
    /// background work's (declared work, events, actions run by rules)
    /// becomes the plugin's note instead (see [`work`](crate::work)).
    Notify(String),
    /// Shows a message that must reach the reader also from background
    /// work, as [`Notify`](Self::Notify) does: one they set up themself
    /// (a rule's, "the battery is at 80 %: unplug"), or one about the
    /// device itself (safe mode, plugins not installed at start, an
    /// update installed). Never for results of syncs, progress or offers:
    /// those belong on the plugin's own page.
    Announce(String),
    /// Restarts PaperPaw, e.g. to apply switched plugins. The shell builds
    /// a new kernel; the reading position is already saved.
    Restart,
    /// Saves what the screen shows as a gray PNG at this storage path,
    /// once the frame is drawn (so a menu that ran this is closed by
    /// then); a message tells where.
    Screenshot(String),
    /// Keeps or removes a password or key (see
    /// [`secrets`](crate::secrets)), saved at once.
    SetSecret(alloc::boxed::Box<crate::secrets::SecretChange>),
    /// Bluetooth, for plugins declaring [`Uses::BLUETOOTH`](crate::Uses):
    /// written with the functions of [`bluetooth`](crate::bluetooth).
    Bluetooth(alloc::boxed::Box<crate::bluetooth::BtRequest>),
    /// The developer link over USB, for plugins declaring
    /// [`Uses::DEVELOPER`](crate::Uses): written with the functions of
    /// [`developer`](crate::developer). Reserved: answered as not
    /// available in this release.
    Developer(alloc::boxed::Box<crate::developer::DevRequest>),
    /// Tells the radio what to do, where PaperPaw manages Wi-Fi
    /// ([`Env::wifi`](crate::Env)); see [`wifi`](crate::wifi). Ignored
    /// elsewhere.
    Wifi(crate::wifi::WifiCommand),
    /// No network is coming for now (none saved, none in reach): the
    /// requests, servers and connections waiting for one fail at once, as
    /// they would after [`NETWORK_WAIT_MS`](crate::wifi::NETWORK_WAIT_MS),
    /// so their screens say so instead of waiting. The plugin that runs
    /// Wi-Fi sends it.
    NoNetwork,
    /// Declares work for the kernel to run at the cheapest moment (see
    /// [`work`](crate::work)): the same id replaces what was declared.
    Work(alloc::boxed::Box<crate::work::Work>),
    /// Takes back the work declared with this id and key (none: every
    /// key of the id), if it has not run; requests made already are taken
    /// back with [`CancelHttp`](Self::CancelHttp).
    CancelWork(Id, Option<String>),
    /// Takes back the plugin's requests whose reply is this event; each
    /// is answered with the refusal `Cancelled`.
    CancelHttp(Id),
    /// The reader chose *Go online* where the mode is Offline (see
    /// [`power`](crate::power)): the network for the screen sending it
    /// until it closes; work held back runs on the same connection.
    GoOnline,
    /// Wears a power mode: its level for plugins and declared work, its
    /// values over the reader's own (see [`power`](crate::power)). Sent
    /// by the plugin that chooses the mode.
    Wear(crate::power::Wear),
    /// Installs a firmware update or restarts, where PaperPaw is the
    /// device's firmware ([`Env::firmware`](crate::Env)); see
    /// [`firmware`](crate::firmware). Ignored elsewhere.
    Firmware(crate::firmware::FirmwareCommand),
    /// Asks the shell to sleep once the frame is shown, where it can
    /// ([`Capabilities::sleep`](crate::Capabilities)); the kernel is told when it wakes.
    Sleep,
    /// Puts text on the clipboard, on devices with one also the system's,
    /// for other apps. Long text is cut at [`MAX_CLIPBOARD`] bytes.
    SetClipboard(String),
    /// Serves HTTP to the local network on `port` (shells that cannot say
    /// so with a failure). Everything about the server comes as the event
    /// `events`: that it started (with its address), each request (answer
    /// with [`Command::Respond`]), that it failed or stopped: a
    /// [`ServerNotice`](crate::ServerNotice). Serve only while the user wants it: a server
    /// also stops when the screen showing as it started is closed.
    #[non_exhaustive]
    Serve { server: Id, port: u16, events: Id },
    /// Stops serving.
    StopServing(Id),
    /// Opens a TCP connection to `host` and `port` (see
    /// [ADR 0014](https://github.com/hthoene/paperpaw/blob/main/docs/adr/0014-sockets-for-plugins.md)).
    /// Everything about it comes as the event `events`, with `id` the
    /// socket ([`params::ID`](crate::builtin::params::ID)) and what
    /// happened, a [`SocketNotice`](crate::SocketNotice): connected,
    /// received, saved, closed, failed. Nothing is read unasked ([`Command::SocketRead`]). A socket
    /// also closes when the screen showing as it was opened is closed.
    #[non_exhaustive]
    Connect {
        socket: Id,
        host: String,
        port: u16,
        events: Id,
    },
    /// Sends bytes on a socket.
    #[non_exhaustive]
    SocketSend { socket: Id, data: Vec<u8> },
    /// Asks a socket for what comes next (one read at a time).
    #[non_exhaustive]
    SocketRead { socket: Id, read: SocketRead },
    /// Closes a socket.
    Disconnect(Id),
    /// Sends `data` as a UDP datagram to the local network on each of
    /// `ports`, e.g. to find a service. Each answer within `wait_ms` comes
    /// as the event `events` ([`SocketNotice::Answer`](crate::SocketNotice::Answer), with `id`), then
    /// [`SocketNotice::Done`](crate::SocketNotice::Done). Made with [`Command::broadcast`].
    Broadcast(alloc::boxed::Box<Broadcast>),
    /// Answers a request of a server. Made with [`Command::respond`].
    Respond(alloc::boxed::Box<ServerResponse>),
    /// Asks for the text on the clipboard (see
    /// [`Env::clipboard`](crate::Env)). It comes as the event `.0` with
    /// the parameters `.1` plus `text` ([`params::TEXT`](crate::builtin::params::TEXT)),
    /// empty if there is none: a [`Typed`](crate::replies::Typed). Read it only when the user asks to paste:
    /// some systems tell the user every time an app reads the clipboard.
    ReadClipboard(Id, Params),
    /// Speaks text aloud where the device can
    /// ([`Capabilities::speech`](crate::Capabilities)), instead of what is being spoken or
    /// after it (`Speech::queue`). Without speech the reply comes at
    /// once, failed. Speech stops when the screen that started it is
    /// closed. Made with [`Command::speak`].
    Speak(alloc::boxed::Box<Speech>),
    /// Stops speaking; no reply comes for the texts cut off.
    StopSpeaking,
    /// Switches the device off once the frame is shown, where PaperPaw
    /// does so ([`Capabilities::power_off`](crate::Capabilities)): plugins
    /// hear [`events::SUSPEND`](crate::builtin::events::SUSPEND) first, and
    /// the next start is a cold one. Ignored elsewhere; to put the device
    /// to sleep, [`Command::Sleep`].
    PowerOff,
}

impl Command {
    /// [`Command::Bind`]: `input` in key map `layer` to `action` (`None`:
    /// back to what plugins and presets bind).
    pub fn bind(layer: impl Into<String>, input: impl Into<String>, action: Option<Id>) -> Self {
        Command::Bind {
            layer: layer.into(),
            input: input.into(),
            action,
        }
    }

    /// [`Command::Serve`]: server `server` on `port`, heard as `events`.
    pub fn serve(server: Id, port: u16, events: Id) -> Self {
        Command::Serve {
            server,
            port,
            events,
        }
    }

    /// [`Command::Connect`]: socket `socket` to `host` and `port`, heard as
    /// `events`.
    pub fn connect(socket: Id, host: impl Into<String>, port: u16, events: Id) -> Self {
        Command::Connect {
            socket,
            host: host.into(),
            port,
            events,
        }
    }

    /// [`Command::SocketSend`]: `data` on `socket`.
    pub fn socket_send(socket: Id, data: Vec<u8>) -> Self {
        Command::SocketSend { socket, data }
    }

    /// [`Command::SocketRead`]: what comes next on `socket`, as `read` says.
    pub fn socket_read(socket: Id, read: SocketRead) -> Self {
        Command::SocketRead { socket, read }
    }

    /// [`Command::Broadcast`]: `data` to `ports` on the local network,
    /// answers within `wait_ms` heard as `events` with `id`.
    pub fn broadcast(id: Id, ports: Vec<u16>, data: Vec<u8>, wait_ms: u32, events: Id) -> Self {
        let broadcast = Broadcast::new(id, ports, data, wait_ms, events);
        Command::Broadcast(alloc::boxed::Box::new(broadcast))
    }

    /// [`Command::Http`]: makes `request`.
    pub fn http(request: HttpRequest) -> Self {
        Command::Http(alloc::boxed::Box::new(request))
    }

    /// [`Command::Respond`]: answers a server's request with `response`.
    pub fn respond(response: ServerResponse) -> Self {
        Command::Respond(alloc::boxed::Box::new(response))
    }

    /// [`Command::Speak`]: speaks `speech`.
    pub fn speak(speech: Speech) -> Self {
        Command::Speak(alloc::boxed::Box::new(speech))
    }

    /// [`Command::Work`]: declares `work`.
    pub fn work(work: crate::work::Work) -> Self {
        Command::Work(alloc::boxed::Box::new(work))
    }

    /// [`Command::SetSecret`]: makes `change`.
    pub fn set_secret(change: crate::secrets::SecretChange) -> Self {
        Command::SetSecret(alloc::boxed::Box::new(change))
    }
}

/// The most text the clipboard holds, in bytes.
pub const MAX_CLIPBOARD: usize = 4096;

impl Command {
    /// Changes `setting` to `value` (see [`setting`](crate::setting)).
    pub fn set<T: crate::setting::SettingType>(
        setting: &crate::setting::Setting<T>,
        value: T::Input,
    ) -> Command {
        Command::SetSetting(setting.id().clone(), T::write(value))
    }

    /// [`Command::RunAction`]: runs `action` without params.
    pub fn run(action: impl Into<Id>) -> Command {
        Command::RunAction(action.into(), Params::new())
    }

    /// Opens a slot without parameters.
    pub fn open_slot(slot: Id) -> Command {
        Command::OpenSlot(slot, Params::new())
    }
}

#[cfg(test)]
mod size {
    /// Commands are queued by the hundred: the large ones are boxed, so
    /// that every command takes no more than a few words.
    #[test]
    fn commands_stay_small() {
        let size = core::mem::size_of::<super::Command>();
        assert!(size <= 88, "Command takes {size} bytes");
    }
}

#[cfg(test)]
mod printed {
    extern crate std;
    use alloc::format;

    /// A request printed for debugging, alone or in a command, shows no
    /// header value, body, query or param value: where secrets go.
    #[test]
    fn requests_print_no_secrets() {
        let request = super::HttpRequest::new(
            "POST",
            "https://sync.example/api?key=QUERYSECRET",
            crate::Id::new_static("demo.reply"),
        )
        .header("Authorization", "Basic HEADERSECRET")
        .body("{\"password\":\"BODYSECRET\"}")
        .with(crate::Params::new().with("token", "PARAMSECRET"));
        let printed = format!("{:?}", super::Command::http(request));
        for secret in ["QUERYSECRET", "HEADERSECRET", "BODYSECRET", "PARAMSECRET"] {
            assert!(!printed.contains(secret), "{printed}");
        }
        assert!(printed.contains("https://sync.example/api") && printed.contains("Authorization"));
        let change = crate::secrets::SecretChange::keep("demo.password", "Password", "KEPTSECRET");
        let printed = format!("{:?}", super::Command::set_secret(change));
        assert!(
            !printed.contains("KEPTSECRET") && printed.contains("demo.password"),
            "{printed}"
        );
    }
}
