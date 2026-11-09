//! Reading progress synced with KOReader's sync server ("kosync"), so a
//! book opens where one stopped on another device, PaperPaw or KOReader.
//!
//! - Books are identified as KOReader does (a partial MD5 of the file), so
//!   the same file matches on every device.
//! - Positions are KOReader's XPointers where the format has them (EPUB),
//!   besides the percentage.
//! - The progress is sent at most once a minute while reading (where
//!   PaperPaw runs the radio itself, only while it is on anyway: turning
//!   pages never keeps it on), and when a book is closed and the device
//!   sleeps or the app goes to the background; it is fetched when a book
//!   opens and when the app comes back. If another device is further, a dialog offers to
//!   go there (once for each position).
//! - Nothing is sent for a book while its progress is being fetched, nor
//!   while the dialog is open or was left without a choice (offered
//!   again when the server's position moves), nor before the server's
//!   progress was read (a failed fetch is tried again first): a further
//!   position on the server is never overwritten before the user chose
//!   to stay.
//! - *Sync progress* in the reading menu's *More* fetches the latest, then
//!   sends this device's position unless another device is further.
//! - The account (settings → *Progress sync*) keeps the user name in the
//!   settings and only the MD5 of the password, as KOReader does. Its
//!   screen also sets the server: KOReader's or any compatible one, also
//!   below a path (`https://example.org/kosync`) or over plain `http` in a
//!   home network. Another server signs out, however it was set: an
//!   account's key belongs to its server.
//!
//! Needs a device with network ([`Capabilities::network`](paperpaw_sdk::Capabilities)).

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod native_entry;
#[doc(hidden)]
pub mod native_model;
/// The interface-7 progress sync (slice 1; see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_sync;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_parity_tests;

mod account;
mod confirm;

use paperpaw_sdk::digest::md5;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::json;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::work::{Moments, Work};
use paperpaw_sdk::{
    ActionSpec, Arg, Command, Commands, Env, FixedScreen, HttpRequest, Id, Instant, Manifest,
    MenuEntry, Params, Plugin, RegisterError, Registrar, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("kosync");
/// The account's user name (empty when signed out); edited on the account
/// screen.
pub const USER: Setting<Text> = Setting::new("kosync.user");
/// The sync server.
pub const SERVER: Setting<Text> = Setting::new("kosync.server");
/// Send and fetch the progress by itself.
pub const AUTO: Setting<bool> = Setting::new("kosync.auto");
/// Sends the progress of the open book and fetches the latest now.
pub const SYNC_NOW: Id = Id::new_static("kosync.sync-now");

const GROUP: Id = Id::new_static("kosync.group");
const ACCOUNT_SCREEN: Id = Id::new_static("kosync.account");
const CONFIRM_SCREEN: Id = Id::new_static("kosync.confirm");
/// Emitted by the account screen when one signed in or out.
const ACCOUNT_CHANGED: Event = Event::new("kosync.account-changed");
/// Replies of the server.
const PULLED: Event<HttpReply> = Event::new("kosync.pulled");
const PUSHED: Event<HttpReply> = Event::new("kosync.pushed");
/// Declared work: the progress, sent while reading at the next moment
/// the radio is on anyway, at the latest after [`PUSH_WITHIN`] (ADR 0025).
const PUSH_DUE: Event = Event::new("kosync.push-due");
/// At the latest this long after a page turned, the progress goes out
/// (longer in Saving); closing the book or sleeping sends it before.
const PUSH_WITHIN: core::time::Duration = core::time::Duration::from_secs(30 * 60);
/// Books switched from while Offline, their progress sent once allowed.
const MOST_UNSENT: usize = 4;
/// Books whose offer was left without a choice, kept so that it does not
/// come again for the same position.
const MOST_DISMISSED: usize = 4;

/// Whether sending now keeps no radio on that would be off otherwise: the
/// system runs Wi-Fi, or PaperPaw's radio is in a network already.
fn radio_on_anyway(env: &Env<'_>) -> bool {
    env.wifi().is_none_or(|w| w.state.is_online())
}

/// KOReader's public server.
const DEFAULT_SERVER: &str = "https://sync.koreader.rocks";
/// The progress is sent at most this often while reading.
const PUSH_EVERY_MS: u64 = 60_000;
/// The reader is shown again often (after every menu); the progress is
/// fetched again for the same book only after this long.
const PULL_EVERY_MS: u64 = 60_000;
/// Another device counts as further from this much more progress (0..1).
const FURTHER: f64 = 0.002;
/// Parameters of a fetch: the local progress (0..1) and whether the user
/// asked for it.
const LOCAL: &str = "local";
const MANUAL: &str = "manual";

/// Where the reader is in which book.
#[derive(Debug, Clone, PartialEq)]
struct Position {
    path: String,
    at: Locator,
    /// Reading progress from 0 to 1, as the reader shows it.
    progress: f64,
}

/// Sending held back for a book while its progress on the server is being
/// fetched and, if another device is further there, until the user chose
/// whether to go there: sending first would overwrite the further position.
#[derive(Debug, Clone, PartialEq)]
struct Hold {
    path: String,
    /// Send this device's position when the hold ends with it kept.
    send: bool,
    /// Send it even if it was sent already (*Sync progress*).
    force: bool,
    /// The position to send then, kept as it was when it was held back,
    /// so it is sent also after another book was opened.
    position: Option<Position>,
}

impl Hold {
    /// A hold for the book at `path` with nothing to send yet.
    fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            send: false,
            force: false,
            position: None,
        }
    }
}

#[derive(Debug, Default)]
struct State {
    loaded: bool,
    device_id: String,
    current: Option<Position>,
    /// The position last sent.
    sent: Option<Position>,
    /// The XPointer the reader said a place is (book, place, XPointer),
    /// as it does when a book is closed: no need to open the book.
    xpointer: Option<(String, Locator, String)>,
    last_push: Option<Instant>,
    /// The book last fetched for, and when.
    last_pull: Option<(String, Instant)>,
    /// The open book whose progress on the server was read since it was
    /// opened, and not failed to be read since: only then is its position
    /// sent without fetching first.
    read: Option<String>,
    hold: Option<Hold>,
    /// Holds of books left while their fetch was out: their position is
    /// sent when the answer says no other device is further.
    left: Vec<Hold>,
    /// A position the user chose not to go to: book and progress.
    declined: Option<(String, String)>,
    /// Offers left without a choice (not now): book and the progress
    /// offered. Offered again only when the server's progress differs;
    /// nothing is sent over it meanwhile.
    dismissed: Vec<(String, String)>,
    /// Book fingerprints already computed.
    digests: Vec<(String, String)>,
    /// Positions of books switched from while Offline, not sent yet.
    unsent: Vec<Position>,
    /// The server the account belongs to; another one signs out.
    server: Option<String>,
}

type Shared = Rc<RefCell<State>>;

fn dir() -> String {
    Env::plugin_dir(PLUGIN.as_str())
}

/// The account's key (the MD5 of its password), kept with the other
/// passwords and keys (see `paperpaw_sdk::secrets`).
pub(crate) const SECRET: &str = "kosync.key";

fn key<'a>(env: &Env<'a>) -> Option<&'a str> {
    env.secrets().get(SECRET).filter(|k| !k.is_empty())
}

fn read_value(storage: &dyn paperpaw_sdk::Storage, file: &str, key: &str) -> Option<String> {
    let data = paperpaw_sdk::read_file(storage, file, 4096).ok()?;
    let text = core::str::from_utf8(&data).ok()?;
    paperpaw_sdk::config::parse(text)
        .filter_map(Result::ok)
        .find(|e| e.key == key)
        .map(|e| e.value.as_str().into_owned())
}

/// Keeps the key of a signed-in account; `None` signs out. It cannot be
/// typed anew in the list of passwords and keys: it is no password.
pub(crate) fn save_key(key: Option<&str>) -> Command {
    Command::set_secret(match key {
        Some(key) => SecretChange::keep(SECRET, "kosync.secret", key).changeable(false),
        None => SecretChange::remove(SECRET),
    })
}

/// Signs out: the account's key and user name go.
pub(crate) fn sign_out() -> [Command; 3] {
    [
        save_key(None),
        Command::set(&USER, Cow::Borrowed("")),
        ACCOUNT_CHANGED.emit(()),
    ]
}

/// Moves a key kept in the plugin's folder by earlier versions to the
/// other passwords and keys.
fn move_key(env: &Env<'_>, out: &mut Commands) {
    let file = format!("{}/account.ini", dir());
    let Some(old) = read_value(env.storage(), &file, "key") else {
        return;
    };
    if !old.is_empty() && key(env).is_none() {
        // The file goes at a start that finds the key kept: if the
        // secrets cannot be saved, it is moved again from here.
        out.push(save_key(Some(&old)));
        return;
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "the file of an earlier version: where it stays, the key is moved again at the next start"
    )]
    let _ = env.storage().remove(&file);
}

impl State {
    fn load(&mut self, env: &Env<'_>) {
        self.server.get_or_insert_with(|| server(env));
        if self.loaded {
            return;
        }
        self.loaded = true;
        if !self.device_id.is_empty() {
            return;
        }
        let device = format!("{}/device.ini", dir());
        self.device_id = match read_value(env.storage(), &device, "id") {
            Some(id) => id,
            None => {
                // Different on each installation: the time and where memory
                // happened to be.
                let probe = Box::new(0u8);
                let seed = format!("{}{}{:p}", env.now().millis(), env.device().id, &*probe);
                let id = md5::of(&seed).to_uppercase();
                let text = format!("[device]\nid = {id}\n");
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "not kept, another id is made at the next start, and the sync server sees another device"
                )]
                let _ = env.storage().write(&device, text.as_bytes());
                id
            }
        };
    }

    /// Whether to sync by itself now.
    fn active(&self, env: &Env<'_>) -> bool {
        env.caps().network && env.settings().read(&AUTO) && self.signed_in(env)
    }

    /// Whether to sync by itself now, and the power mode lets the network
    /// be used (not Offline): otherwise what is not sent waits, silently.
    fn may_send(&self, env: &Env<'_>) -> bool {
        self.active(env) && env.power().online()
    }

    /// Sends what waited while Offline: the books switched from, the open
    /// one's progress, and fetches its latest.
    fn allowed(&mut self, env: &Env<'_>, out: &mut Commands) {
        if !self.may_send(env) {
            return;
        }
        // Fetched first as well: each is sent unless another device is
        // further there (see `pulled`). The open book's own fetch below
        // covers it.
        let open = self.current.as_ref().map(|c| c.path.clone());
        for position in core::mem::take(&mut self.unsent) {
            if open.as_ref() != Some(&position.path) {
                self.pull_left(env, out, position);
            }
        }
        // The latest first: what this device sends waits for it (`Hold`).
        if let Some(path) = self.current.as_ref().map(|c| c.path.clone()) {
            self.pull(env, out, &path, false);
        }
        self.push(env, out, false);
    }

    fn signed_in(&self, env: &Env<'_>) -> bool {
        !env.settings().read(&USER).is_empty() && key(env).is_some()
    }

    fn digest(&mut self, env: &Env<'_>, path: &str) -> Option<String> {
        if let Some((_, digest)) = self.digests.iter().find(|(p, _)| p == path) {
            return Some(digest.clone());
        }
        let digest = md5::partial(env.storage(), path)?;
        self.digests.push((path.to_string(), digest.clone()));
        Some(digest)
    }

    /// A request to the server with the account's credentials.
    fn request(
        &self,
        env: &Env<'_>,
        method: &str,
        path: &str,
        body: Option<String>,
        reply: &Event<HttpReply>,
        params: Params,
    ) -> Option<Command> {
        let user = env.settings().read(&USER);
        let auth = Some((user, key(env)?));
        Some(Command::http(server_request(
            env, method, path, body, reply, params, auth,
        )))
    }

    /// Sends the current position, unless it was sent already or is held
    /// back (see [`Hold`]). The server's progress not read yet for the
    /// book (its fetch failed, say), it is fetched first.
    fn push(&mut self, env: &Env<'_>, out: &mut Commands, force: bool) {
        let Some(position) = self.current.clone() else {
            return;
        };
        if self.read.as_ref() != Some(&position.path) {
            if !self.pulled_lately(env, &position.path) {
                self.pull(env, out, &position.path, false);
            }
            if self.hold.as_ref().is_none_or(|h| h.path != position.path) {
                // Not fetched now (tried a moment ago): held all the same.
                let held = self.hold_for(&position.path);
                self.hold = Some(held.unwrap_or_else(|| Hold::new(&position.path)));
            }
        }
        if let Some(hold) = self.hold.as_mut().filter(|h| h.path == position.path) {
            // Sent when the hold ends, if this position is kept.
            hold.send = true;
            hold.force |= force;
            hold.position = Some(position);
            return;
        }
        self.send(env, out, position, force);
    }

    /// Sends `position`, unless it was sent already (or `force`).
    fn send(&mut self, env: &Env<'_>, out: &mut Commands, position: Position, force: bool) {
        if !force && self.sent.as_ref() == Some(&position) {
            return;
        }
        self.transmit(env, out, position, true);
    }

    /// Puts `position` to the server; `tracked` when it is to be known as
    /// the position sent last.
    fn transmit(&mut self, env: &Env<'_>, out: &mut Commands, position: Position, tracked: bool) {
        // Not worth opening the book for.
        if !self.signed_in(env) {
            return;
        }
        let Some(digest) = self.digest(env, &position.path) else {
            return;
        };
        let said = self
            .xpointer
            .as_ref()
            .filter(|(path, at, _)| *path == position.path && *at == position.at)
            .map(|(.., xpointer)| xpointer.clone());
        let (xpointer, progress) = match said {
            Some(xpointer) => (xpointer, position.progress),
            None => {
                // Opened as the reader opens it, so its places fit (and it
                // is not converted a second time).
                let options = paperpaw_sdk::reading::open_options(env);
                let document = env
                    .formats()
                    .open_with(env.storage(), &position.path, &options)
                    .ok();
                let xpointer = document
                    .as_ref()
                    .and_then(|d| d.xpointer(env.storage(), position.at))
                    .unwrap_or_default();
                let progress =
                    document.map_or(position.progress, |d| f64::from(d.progress(position.at)));
                (xpointer, progress)
            }
        };
        let body = format!(
            "{{\"document\":{},\"progress\":{},\"percentage\":{:.4},\"device\":{},\"device_id\":{}}}",
            json::string(&digest),
            json::string(&xpointer),
            progress,
            json::string(&env.device_name()),
            json::string(&self.device_id),
        );
        let params = Params::new().with(params::PATH, position.path.clone());
        let target = "/syncs/progress";
        if let Some(command) = self.request(env, "PUT", target, Some(body), &PUSHED, params) {
            out.push(command);
            if tracked {
                self.sent = Some(position);
                self.last_push = Some(env.now());
            }
        }
    }

    /// Asks for the latest position of the book at `path`, holding back
    /// its sending until the answer; *Sync progress* (`manual`) sends after
    /// it unless another device is further.
    fn pull(&mut self, env: &Env<'_>, out: &mut Commands, path: &str, manual: bool) {
        let local = self
            .current
            .as_ref()
            .filter(|c| c.path == path)
            .map_or(0.0, |c| c.progress);
        if let Some(command) = self.fetch(env, path, local, manual) {
            out.push(command);
            self.last_pull = Some((path.to_string(), env.now()));
            let held = self.hold_for(path);
            self.hold = Some(Hold {
                path: path.to_string(),
                send: manual || held.as_ref().is_some_and(|h| h.send),
                force: manual || held.as_ref().is_some_and(|h| h.force),
                position: held.and_then(|h| h.position),
            });
        }
    }

    /// Asks for the latest position of a book switched from: `position`
    /// is sent with the answer unless another device is further there.
    fn pull_left(&mut self, env: &Env<'_>, out: &mut Commands, position: Position) {
        if let Some(command) = self.fetch(env, &position.path, position.progress, false) {
            out.push(command);
            self.left.retain(|h| h.path != position.path);
            let hold = Hold::new(&position.path);
            self.left.push(Hold {
                send: true,
                position: Some(position),
                ..hold
            });
        }
    }

    /// The request for the latest position of the book at `path`, with
    /// the progress here (`local`, 0..1).
    fn fetch(&mut self, env: &Env<'_>, path: &str, local: f64, manual: bool) -> Option<Command> {
        let digest = self.digest(env, path)?;
        let params = Params::new()
            .with(params::PATH, path.to_string())
            .with(LOCAL, format!("{local}"))
            .with(MANUAL, if manual { "1" } else { "0" });
        let target = format!("/syncs/progress/{digest}");
        self.request(env, "GET", &target, None, &PULLED, params)
    }

    /// Takes the hold of the book at `path`, also one kept among the books
    /// left; another book's hold is kept there, its answer still to come.
    fn hold_for(&mut self, path: &str) -> Option<Hold> {
        let mut held = self.hold.take();
        if let Some(other) = held.take_if(|h| h.path != path)
            && other.send
        {
            self.left.retain(|h| h.path != other.path);
            self.left.push(other);
        }
        held.or_else(|| {
            let at = self.left.iter().position(|h| h.path == path)?;
            Some(self.left.swap_remove(at))
        })
    }

    /// Whether the book at `path` was fetched for less than a while ago.
    fn pulled_lately(&self, env: &Env<'_>, path: &str) -> bool {
        self.last_pull
            .as_ref()
            .is_some_and(|(p, at)| p == path && env.now().since(*at) < PULL_EVERY_MS)
    }

    /// The server's progress of the open book at `path` could not be read:
    /// its sending stays held back until a fetch succeeds.
    fn unread(&mut self, path: &str, hold: Option<Hold>) {
        if self.read.as_deref() == Some(path) {
            self.read = None;
        }
        let held = hold.or_else(|| self.hold_for(path));
        self.hold = Some(held.unwrap_or_else(|| Hold::new(path)));
    }

    /// Handles the server's answer to a fetch.
    fn pulled(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        let manual = reply.context.get(MANUAL) == Some("1");
        let path = reply.context.get(params::PATH).unwrap_or_default();
        let hold = self.hold.take_if(|h| h.path == path);
        // The book was left meanwhile: its position held back goes, unless
        // another device is further (no offer then, the book is closed).
        if self.current.as_ref().is_none_or(|c| c.path != path) {
            if let Some(at) = self.left.iter().position(|h| h.path == path) {
                let left = self.left.swap_remove(at);
                let text = reply.text();
                let body = json::object(&text);
                let from_here = json::text(&body, "device_id") == Some(self.device_id.as_str());
                if let Some(position) = left.position
                    && reply.status == 200
                    && remote_progress(&text, &body).is_some_and(|remote| {
                        (from_here && position.progress > FURTHER)
                            || remote <= position.progress + FURTHER
                    })
                {
                    self.send(env, out, position, left.force);
                }
            }
            return;
        }
        let status = reply.status.to_string();
        let status = status.as_str();
        if reply.status != 200 {
            if manual {
                out.push(message(env, failure(status), status));
            }
            self.unread(path, hold);
            return;
        }
        let text = reply.text();
        let body = json::object(&text);
        let Some(remote) = remote_progress(&text, &body) else {
            // Not knowing the server's progress, nothing may overwrite it.
            if manual {
                out.push(message(env, "kosync.unreadable", status));
            }
            self.unread(path, hold);
            return;
        };
        self.read = Some(path.to_string());
        let local: f64 = reply
            .context
            .get(LOCAL)
            .and_then(|l| l.parse().ok())
            .unwrap_or(0.0);
        // Sent from here, unless the book starts here anew: copied again,
        // say, its position lost.
        let from_here =
            json::text(&body, "device_id") == Some(self.device_id.as_str()) && local > FURTHER;
        let offered = format!("{remote}");
        let declined = self
            .declined
            .as_ref()
            .is_some_and(|(p, r)| p == path && *r == offered);
        let dismissed = self
            .dismissed
            .iter()
            .any(|(p, r)| p == path && *r == offered);
        let further = !from_here && remote > local + FURTHER;
        if further && dismissed && !manual {
            // Not now, the user said: held back still, without asking.
            self.hold = Some(hold.unwrap_or_else(|| Hold::new(path)));
            return;
        }
        if further && (manual || !declined) {
            // Further on another device: offer to go there, and send
            // nothing until the user chose.
            let device = json::text(&body, "device").unwrap_or("?");
            let xpointer = json::text(&body, "progress").unwrap_or_default();
            out.push(Command::OpenScreen(
                CONFIRM_SCREEN,
                Params::new()
                    .with(params::PATH, path.to_string())
                    .with(confirm::XPOINTER, xpointer.to_string())
                    .with(confirm::PERCENTAGE, format!("{remote}"))
                    .with(confirm::DEVICE, device.to_string()),
            ));
            self.hold = Some(hold.unwrap_or_else(|| Hold::new(path)));
            return;
        }
        if manual {
            out.push(message(env, "kosync.up-to-date", status));
        }
        if let Some(hold) = hold.filter(|h| h.send) {
            self.push(env, out, hold.force);
        }
    }

    /// The user closed the offer to go where another device is.
    fn answered(&mut self, env: &Env<'_>, answer: &confirm::Answered, out: &mut Commands) {
        let hold = self.hold.take_if(|h| h.path == answer.path);
        self.dismissed.retain(|(p, _)| *p != answer.path);
        // Gone there: the reader tells the new position as it shows it.
        if answer.went {
            return;
        }
        if !answer.stay {
            // Left without a choice: not now. Held back still, and offered
            // again when the server's progress is another.
            self.dismissed
                .push((answer.path.clone(), answer.percentage.clone()));
            let extra = self.dismissed.len().saturating_sub(MOST_DISMISSED);
            self.dismissed.drain(..extra);
            if self.hold.is_none() {
                self.hold = Some(hold.unwrap_or_else(|| Hold::new(&answer.path)));
            } else {
                // Another book's hold meanwhile: fetched first again.
                self.read = None;
            }
            return;
        }
        self.declined = Some((answer.path.clone(), answer.percentage.clone()));
        // This device's position is the one to keep.
        if self.current.as_ref().is_some_and(|c| c.path == answer.path) {
            self.push(env, out, true);
        }
    }

    /// The server setting changed: the account's key must not go to
    /// another server.
    fn server_changed(&mut self, env: &Env<'_>, out: &mut Commands) {
        let now = server(env);
        let before = self.server.replace(now.clone());
        if before.as_ref() != Some(&now) && self.signed_in(env) {
            out.extend(sign_out());
        }
    }
}

/// The progress (0..1) in a sync server's answer `text`, read as `body`:
/// 0 for a book it has none of (an object without one, e.g. `{}`); `None`
/// if the answer is no object or its progress no number from 0 to 1.
fn remote_progress(text: &str, body: &[(String, json::Value)]) -> Option<f64> {
    if body.is_empty() {
        let inner = text
            .trim()
            .strip_prefix('{')
            .and_then(|t| t.strip_suffix('}'));
        return inner.is_some_and(|i| i.trim().is_empty()).then_some(0.0);
    }
    if !body.iter().any(|(key, _)| key == "percentage") {
        return Some(0.0);
    }
    let progress = json::number(body, "percentage")
        .or_else(|| json::text(body, "percentage").and_then(|p| p.trim().parse().ok()))?;
    (0.0..=1.0).contains(&progress).then_some(progress)
}

/// The server's base URL.
pub(crate) fn server(env: &Env<'_>) -> String {
    normalize_server(env.settings().read(&SERVER))
}

/// A server address as entered, as a base URL: `https://` unless another
/// scheme is given, no `/` at the end, KOReader's server if empty.
pub(crate) fn normalize_server(text: &str) -> String {
    let server = paperpaw_sdk::web::base_url(text);
    if server.is_empty() {
        DEFAULT_SERVER.to_string()
    } else {
        server
    }
}

/// A base URL as shown: without `https://` (but with `http://`, which is
/// worth knowing).
pub(crate) fn server_label(url: &str) -> &str {
    url.strip_prefix("https://").unwrap_or(url)
}

/// A request to the sync server, authenticated as user and key (the MD5 of
/// the password) if given.
pub(crate) fn server_request(
    env: &Env<'_>,
    method: &str,
    path: &str,
    body: Option<String>,
    reply: &Event<HttpReply>,
    params: Params,
    auth: Option<(&str, &str)>,
) -> HttpRequest {
    let mut headers = Vec::from([
        (
            "Accept".to_string(),
            "application/vnd.koreader.v1+json".to_string(),
        ),
        ("Content-Type".to_string(), "application/json".to_string()),
    ]);
    if let Some((user, key)) = auth {
        headers.push(("x-auth-user".to_string(), user.to_string()));
        headers.push(("x-auth-key".to_string(), key.to_string()));
    }
    let mut request =
        HttpRequest::new(method, &format!("{}{path}", server(env)), reply).with(params);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    match body {
        Some(body) => request.body(body),
        None => request,
    }
}

/// The position in the parameters of a reading event.
fn position(reading: &events::Reading) -> Position {
    Position {
        path: reading.path.clone(),
        at: reading.at(),
        progress: f64::from(reading.progress) / 100.0,
    }
}

/// The translation key telling why a request with HTTP status `status`
/// failed (0: no answer).
pub(crate) fn failure(status: &str) -> &'static str {
    match status {
        "0" => "kosync.unreachable",
        "401" | "403" => "kosync.wrong",
        _ => "kosync.server-error",
    }
}

/// A message about a request in the confirmation overlay.
fn message(env: &Env<'_>, key: &str, status: &str) -> Command {
    let text = env.i18n().format(key, &[("status", Arg::Str(status))]);
    Command::OpenScreen(CONFIRM_SCREEN, Params::new().with(params::TEXT, text))
}

/// The kosync plugin.
#[derive(Debug, Default)]
pub struct Kosync;

/// The account, signed in on a screen of its own.
static ACCOUNT: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor(ACCOUNT_SCREEN);

/// The account, whether books sync by themselves, and the server.
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::text(USER.static_str(), "kosync.setting.user", "", GROUP)
        .with_extra(&ACCOUNT)
        .kept_on_reset()
        .description("kosync.setting.user.help"),
    SettingSpec::switch(AUTO.static_str(), "kosync.setting.auto", true, GROUP)
        .description("kosync.setting.auto.help"),
    // Set on the account screen too, which tells that it signs out.
    SettingSpec::text(
        SERVER.static_str(),
        "kosync.setting.server",
        DEFAULT_SERVER,
        GROUP,
    )
    .with_extra(&ACCOUNT)
    .kept_on_reset()
    .advanced()
    .description("kosync.setting.server.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ENTRIES: [MenuEntry; 1] =
    [MenuEntry::new(paperpaw_sdk::builtin::menus::READER_MORE, SYNC_NOW).order(25)];
static ICONS: [(Id, &str); 2] = [(GROUP, "devices"), (SYNC_NOW, "refresh")];

static SCREENS: [FixedScreen; 2] = [
    FixedScreen::new(ACCOUNT_SCREEN, "kosync.group", |env, _| {
        Box::new(account::AccountScreen::new(env))
    }),
    FixedScreen::new(CONFIRM_SCREEN, "kosync.group", |env, params| {
        Box::new(confirm::ConfirmScreen::new(env, params))
    }),
];

impl Plugin for Kosync {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Progress sync", paperpaw_sdk::package_version!())
            .description(
                "Syncs the reading position with KOReader's sync server, also with KOReader.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::new(RefCell::new(State::default()));
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "kosync.group").order(28);
            &SPEC
        })?;
        r.add_icons(&ICONS)?;
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;

        let s = state.clone();
        r.on(events::STARTED, move |env, _, out| {
            move_key(env, out);
            s.borrow_mut().server = Some(server(env));
        })?;
        let s = state.clone();
        r.on(events::SETTING_CHANGED, move |env, changed, out| {
            if changed.id == *SERVER.id() {
                s.borrow_mut().server_changed(env, out);
            }
        })?;
        let s = state.clone();
        r.on(events::BOOK_OPENED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            state.load(env);
            let position = position(reading);
            let active = state.active(env);
            // Another book: the last one's position first.
            let other = state
                .current
                .as_ref()
                .is_some_and(|c| c.path != position.path);
            if active && other {
                if state.may_send(env) {
                    state.push(env, out, false);
                } else if let Some(last) = state.current.clone() {
                    // Offline: sent once allowed, silently.
                    state.unsent.retain(|u| u.path != last.path);
                    state.unsent.push(last);
                    let extra = state.unsent.len().saturating_sub(MOST_UNSENT);
                    state.unsent.drain(..extra);
                }
            }
            let path = position.path.clone();
            if other {
                state.read = None;
            }
            state.current = Some(position);
            if !state.pulled_lately(env, &path) && state.may_send(env) {
                state.pull(env, out, &path, false);
            }
        })?;
        let s = state.clone();
        r.on(events::PAGE_TURNED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            state.load(env);
            state.current = Some(position(reading));
            let due = state
                .last_push
                .is_none_or(|at| env.now().since(at) >= PUSH_EVERY_MS);
            if due && state.may_send(env) && radio_on_anyway(env) {
                state.push(env, out, false);
            } else if state.active(env) && env.work().get(PUSH_DUE.id()).is_none() {
                // When the radio is on anyway, at the latest in a while.
                out.push(Command::work(
                    Work::network(PUSH_DUE.id().clone())
                        .within(PUSH_WITHIN)
                        .at_moments(Moments::ONLINE),
                ));
            }
        })?;
        let s = state.clone();
        r.on(events::BOOK_CLOSED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            state.load(env);
            // Where it was left, as an XPointer too where the reader says it.
            state.current = Some(position(reading));
            if let Some(xpointer) = reading.xpointer.clone() {
                state.xpointer = Some((reading.path.clone(), reading.at(), xpointer));
            }
            if state.may_send(env) {
                state.push(env, out, false);
            }
        })?;
        let s = state.clone();
        r.on(PUSH_DUE, move |env, (), out| {
            let mut state = s.borrow_mut();
            state.load(env);
            if state.may_send(env) {
                state.push(env, out, false);
            }
        })?;
        let s = state.clone();
        r.on(events::WORK_ALLOWED, move |env, (), out| {
            let mut state = s.borrow_mut();
            state.load(env);
            state.allowed(env, out);
        })?;
        let s = state.clone();
        r.on(events::SUSPEND, move |env, _, out| {
            let mut state = s.borrow_mut();
            state.load(env);
            if state.may_send(env) {
                state.push(env, out, false);
            }
        })?;
        let s = state.clone();
        r.on(events::RESUME, move |env, _, out| {
            // Maybe read on another device meanwhile.
            let mut state = s.borrow_mut();
            state.load(env);
            let path = state.current.as_ref().map(|c| c.path.clone());
            if let Some(path) = path.filter(|p| !state.pulled_lately(env, p) && state.may_send(env))
            {
                state.pull(env, out, &path, false);
            }
        })?;
        let s = state.clone();
        r.on(confirm::ANSWERED, move |env, answer, out| {
            s.borrow_mut().answered(env, answer, out);
        })?;
        let s = state.clone();
        r.on(ACCOUNT_CHANGED, move |_, (), _| {
            s.borrow_mut().loaded = false
        })?;
        let s = state.clone();
        r.on(PUSHED, move |_, reply, _| {
            // Not sent after all: again with the next page.
            if reply.status != 200 {
                let mut state = s.borrow_mut();
                state.sent = None;
                state.last_push = None;
            }
        })?;
        let s = state.clone();
        r.on(PULLED, move |env, reply, out| {
            s.borrow_mut().pulled(env, reply, out);
        })?;

        // "Sync progress" in the reading menu's "More".
        let s = state;
        r.add_action(ActionSpec::new(SYNC_NOW, "kosync.sync-now").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                let mut state = s.borrow_mut();
                state.load(env);
                if !env.caps().network || !state.signed_in(env) {
                    out.push(Command::OpenScreen(ACCOUNT_SCREEN, Params::new()));
                    return;
                }
                let Some(path) = state.current.as_ref().map(|c| c.path.clone()) else {
                    return;
                };
                // The server's first: sent after the answer, unless
                // another device is further and the user goes there.
                state.pull(env, out, &path, true);
            },
        ))?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
