//! Plugins installed from the computer on interface 7 (`GET /api/plugins`,
//! `PUT /api/plugin/<id>/<path>`, `POST`, `GET` and `DELETE /api/plugin/<id>`),
//! host-free: the host's part is the [`Plugins`] trait.
//!
//! The files come one by one into the data folder (`staging/<id>/<path>`),
//! each checked against its SHA-256 on the way in; `POST` then asks the host to
//! install the folder (`command.install_plugin`). The host copies it, runs its own
//! checks, **asks the reader** (decision 132) and ends the request with
//! `install_done`, which is the answer the page's `GET` gives: `asking` until
//! then, then `installed` or `refused` with the reason. A refusal of the command
//! itself (`Denied`, `Unsupported`) is the answer too.
//!
//! The requests carry the page's key like every other (checked before, by
//! `native_web`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::install::{MAX_FILES, is_plugin_name, is_safe_path};
use paperpaw_sdk::native::{Error, device::DeviceOutcome};
use paperpaw_ui_kit::text::{concat, copy};

use crate::native_web::{Answer, Body, Request, param};

/// Where the files of plugins being sent are put, in the data folder.
pub const STAGING: &str = "staging";
/// The list of the installed plugins, in the data folder, for `send_file`
/// (an inline answer holds 2,048 bytes).
pub const LISTING_FILE: &str = "plugins.json";
/// Plugins whose files or state are kept at most.
const MOST_PLUGINS: usize = 4;
/// The first number of the install requests this plugin sends.
const FIRST_REQUEST: u32 = 0x1000;

/// What the plugin keeps of a plugin id, as the registry knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// No such plugin installed.
    Absent,
    /// Installed (an install is an update).
    Installed,
    /// Built into the firmware: never installed over.
    BuiltIn,
}

/// What became of one file put into the staging folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staged {
    Taken,
    /// Its SHA-256 differs from the one said.
    Damaged,
    /// The storage could not take it.
    Storage,
}

/// What the host does for the plugin requests.
pub trait Plugins {
    /// How the registry knows `id`.
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn known(&mut self, id: &str) -> Result<Known, Error>;
    /// Writes the list of the installed plugins to [`LISTING_FILE`].
    ///
    /// # Errors
    /// The host's refusal; `NoMemory`.
    fn write_listing(&mut self) -> Result<(), Error>;
    /// Takes the file `body` (in the data folder) as `path` of plugin `id`, if
    /// its SHA-256 is `sha256`; `body` is gone afterwards.
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
    /// Removes a file of the data folder, if it is there.
    fn discard(&mut self, path: &str);
    /// Removes the files staged for plugin `id`.
    fn clear(&mut self, id: &str);
    /// Asks the host to install the folder staged for `id` as request `request`.
    ///
    /// # Errors
    /// `Denied`, `Unsupported`, `Full` or `Busy` from the host; `NoMemory`.
    fn install(&mut self, request: u32, id: &str, update: bool) -> Result<(), Error>;
}

/// How one install stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The host asks the reader (request number).
    Asking(u32),
    /// Installed; `restart` where its code runs after a restart.
    Installed { restart: bool },
    /// Not installed, in a word or two.
    Refused(&'static str),
}

#[derive(Debug)]
struct Entry {
    id: String,
    /// The paths of the files staged.
    files: Vec<String>,
    state: Option<State>,
}

/// The installs asked for since the plugin started: what is staged, and how
/// each stands.
#[derive(Debug, Default)]
pub struct Installs {
    entries: Vec<Entry>,
    sent: u32,
}

/// What an end of an install says on the device: the plugin and whether it
/// was installed (and runs after a restart).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub id: String,
    pub result: Result<bool, &'static str>,
}

impl Installs {
    /// Whether a request is one of the installs.
    #[must_use]
    pub fn is_install_request(path: &str) -> bool {
        path == "/api/plugins" || path.starts_with("/api/plugin/")
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    /// The entry of `id`, made where there is room: one with nothing staged
    /// and no state to tell is let go for it.
    fn entry(&mut self, id: &str) -> Result<Option<&mut Entry>, Error> {
        if let Some(at) = self.position(id) {
            return Ok(self.entries.get_mut(at));
        }
        if self.entries.len() >= MOST_PLUGINS {
            let idle = self
                .entries
                .iter()
                .position(|entry| !matches!(entry.state, Some(State::Asking(_))));
            match idle {
                Some(at) => {
                    self.entries.remove(at);
                }
                None => return Ok(None),
            }
        }
        self.entries.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.entries.push(Entry {
            id: copy(id)?,
            files: Vec::new(),
            state: None,
        });
        Ok(self.entries.last_mut())
    }

    fn state(&self, id: &str) -> Option<State> {
        self.position(id)
            .and_then(|at| self.entries.get(at))
            .and_then(|entry| entry.state)
    }

    /// Whether the host is asking the reader about plugin `id` now.
    #[must_use]
    pub fn is_asking(&self, id: &str) -> bool {
        matches!(self.state(id), Some(State::Asking(_)))
    }

    /// Whether files of plugin `id` are staged, or the host asks about it: the
    /// cable's offers leave such a plugin alone.
    #[must_use]
    pub fn holds(&self, id: &str) -> bool {
        self.position(id)
            .and_then(|at| self.entries.get(at))
            .is_some_and(|entry| {
                !entry.files.is_empty() || matches!(entry.state, Some(State::Asking(_)))
            })
    }

    fn asking_any(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.state, Some(State::Asking(_))))
    }

    fn set(&mut self, id: &str, state: State) {
        if let Some(entry) = self.position(id).and_then(|at| self.entries.get_mut(at)) {
            entry.state = Some(state);
        }
    }

    fn forget(&mut self, id: &str) {
        self.entries.retain(|entry| entry.id != id);
    }

    /// The host ended install request `request`: the state it leads to, the
    /// staged files removed. `None` where it is not one of this plugin's
    /// asking (a stale or foreign end).
    pub fn ended(
        &mut self,
        plugins: &mut dyn Plugins,
        request: u32,
        result: Result<bool, &'static str>,
    ) -> Option<Ended> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.state == Some(State::Asking(request)))?;
        let id = copy(&entry.id).ok()?;
        entry.state = Some(match result {
            Ok(restart) => State::Installed { restart },
            Err(why) => State::Refused(why),
        });
        entry.files.clear();
        plugins.clear(&id);
        Some(Ended { id, result })
    }

    /// Forgets the plugins that are not being asked about (their staged files
    /// are removed by the caller: the screen opened or closed).
    pub fn forget_idle(&mut self) {
        self.entries
            .retain(|entry| matches!(entry.state, Some(State::Asking(_))));
    }

    /// Answers a plugin request (its key checked), and removes the body it
    /// carried whatever came of it.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals that are not the plugin's to answer.
    pub fn answer(
        &mut self,
        plugins: &mut dyn Plugins,
        request: &Request<'_>,
        path: &str,
        query: &str,
    ) -> Result<Answer, Error> {
        let answer = self.route(plugins, request, path, query);
        if let Some(body) = request.body {
            plugins.discard(body);
        }
        answer
    }

    fn route(
        &mut self,
        plugins: &mut dyn Plugins,
        request: &Request<'_>,
        path: &str,
        query: &str,
    ) -> Result<Answer, Error> {
        if request.method == "GET" && path == "/api/plugins" {
            plugins.write_listing()?;
            return Ok(Answer {
                status: 200,
                kind: "application/json",
                body: Body::Data(LISTING_FILE),
            });
        }
        let rest = path.strip_prefix("/api/plugin/").unwrap_or_default();
        let (id, file) = rest.split_once('/').unwrap_or((rest, ""));
        if !is_plugin_name(id) {
            return Ok(json(400, "{\"why\":\"name\"}"));
        }
        match (request.method, file) {
            ("PUT", file) if !file.is_empty() => self.put(plugins, request, id, file, query),
            ("POST", "") => self.install(plugins, id),
            ("GET", "") => state_answer(self.state(id)),
            ("DELETE", "") => {
                if matches!(self.state(id), Some(State::Asking(_))) {
                    return Ok(json(409, "{\"why\":\"asking\"}"));
                }
                plugins.clear(id);
                self.forget(id);
                Ok(json(200, "{\"ok\":true}"))
            }
            _ => Ok(json(404, "{\"why\":\"not found\"}")),
        }
    }

    /// One file of plugin `id`: refused (*422*) where its SHA-256 differs.
    fn put(
        &mut self,
        plugins: &mut dyn Plugins,
        request: &Request<'_>,
        id: &str,
        file: &str,
        query: &str,
    ) -> Result<Answer, Error> {
        let sha256 = param(query, "sha256")?
            .and_then(|hex| paperpaw_sdk::digest::hex::decode_array::<32>(&hex));
        let Some(sha256) = sha256 else {
            return Ok(json(400, "{\"why\":\"sha256\"}"));
        };
        if matches!(self.state(id), Some(State::Asking(_))) {
            return Ok(json(409, "{\"why\":\"being installed\"}"));
        }
        if !is_safe_path(file) {
            return refused("name");
        }
        // A request with no body is an empty file: not what its SHA-256 says.
        let Some(body) = request.body else {
            return refused("damaged");
        };
        let Some(entry) = self.entry(id)? else {
            return Ok(json(409, "{\"why\":\"too many plugins\"}"));
        };
        let new = !entry.files.iter().any(|staged| staged == file);
        if new && entry.files.len() >= MAX_FILES {
            return refused("too many files");
        }
        if new {
            entry.files.try_reserve(1).map_err(|_| Error::NoMemory)?;
        }
        match plugins.stage(id, file, body, &sha256)? {
            Staged::Taken => {
                if new {
                    let kept = copy(file)?;
                    if let Some(entry) = self.entry(id)? {
                        entry.files.push(kept);
                    }
                }
                Ok(json(200, "{\"ok\":true}"))
            }
            Staged::Damaged => refused("damaged"),
            Staged::Storage => refused("storage"),
        }
    }

    /// Installs the plugin from the files sent: the host is asked, and asks
    /// the reader.
    fn install(&mut self, plugins: &mut dyn Plugins, id: &str) -> Result<Answer, Error> {
        if matches!(self.state(id), Some(State::Asking(_))) {
            return state_answer(self.state(id));
        }
        if self.asking_any() {
            return Ok(json(409, "{\"why\":\"another plugin is being installed\"}"));
        }
        let staged = self
            .position(id)
            .and_then(|at| self.entries.get(at))
            .is_some_and(|entry| !entry.files.is_empty());
        let known = plugins.known(id)?;
        let refusal = match (staged, known) {
            (false, _) => Some("not the plugin"),
            (_, Known::BuiltIn) => Some("built in"),
            _ => None,
        };
        if let Some(why) = refusal {
            return self.refuse(plugins, id, why);
        }
        self.sent = self.sent.wrapping_add(1);
        let request = FIRST_REQUEST.wrapping_add(self.sent & 0xff_ffff);
        match plugins.install(request, id, known == Known::Installed) {
            Ok(()) => {
                self.set(id, State::Asking(request));
                state_answer(self.state(id))
            }
            Err(Error::Denied) => self.refuse(plugins, id, "not allowed"),
            Err(Error::Unsupported) => self.refuse(plugins, id, "not supported"),
            // Four requests wait already: the files stay for another try.
            Err(Error::Full | Error::Busy) => Ok(json(409, "{\"why\":\"busy\"}")),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => self.refuse(plugins, id, "refused"),
        }
    }

    /// Not installed, and why: what was sent goes.
    fn refuse(
        &mut self,
        plugins: &mut dyn Plugins,
        id: &str,
        why: &'static str,
    ) -> Result<Answer, Error> {
        plugins.clear(id);
        if let Some(entry) = self.entry(id)? {
            entry.files.clear();
            entry.state = Some(State::Refused(why));
        }
        state_answer(Some(State::Refused(why)))
    }
}

fn json(status: u16, body: &'static str) -> Answer {
    Answer {
        status,
        kind: "application/json",
        body: Body::Text(body),
    }
}

/// A refusal of what was put: *422* with the reason.
fn refused(why: &'static str) -> Result<Answer, Error> {
    state_answer(Some(State::Refused(why)))
}

/// The state as the page and the tools read it: `{"state": …}`, *202* while
/// the reader is asked, *422* when refused.
fn state_answer(state: Option<State>) -> Result<Answer, Error> {
    let (status, body) = match state {
        None => (200, copy("{\"state\":\"none\"}")?),
        Some(State::Asking(_)) => (202, copy("{\"state\":\"asking\"}")?),
        Some(State::Installed { restart }) => (
            200,
            concat(&[
                "{\"state\":\"installed\",\"restart\":",
                if restart { "true" } else { "false" },
                "}",
            ])?,
        ),
        Some(State::Refused(why)) => (
            422,
            concat(&["{\"state\":\"refused\",\"why\":\"", why, "\"}"])?,
        ),
    };
    Ok(Answer {
        status,
        kind: "application/json",
        body: Body::Owned(body),
    })
}

/// Why an install did not end well, in a word or two, from the end the host
/// sent (`event.install_done`: its status and its reason, `docs/abi/device.md`
/// 5.5).
#[must_use]
pub fn why(status: Result<(), Error>, outcome: DeviceOutcome) -> &'static str {
    match outcome {
        DeviceOutcome::Ok => match status {
            Ok(()) => "refused",
            Err(Error::Denied) => "not allowed",
            Err(Error::NoSpace | Error::Full) => "no room",
            Err(_) => "refused",
        },
        DeviceOutcome::Declined | DeviceOutcome::Dismissed => "not approved",
        DeviceOutcome::Damaged | DeviceOutcome::ContentChanged => "damaged",
        DeviceOutcome::OtherAuthor
        | DeviceOutcome::KeyChanged
        | DeviceOutcome::Revoked
        | DeviceOutcome::Unsigned
        | DeviceOutcome::UnknownKey => "not trusted",
        DeviceOutcome::NewerInterface
        | DeviceOutcome::OlderInterface
        | DeviceOutcome::OtherChip => "built for another PaperPaw",
        DeviceOutcome::NoRoom | DeviceOutcome::CardFull => "no room",
        DeviceOutcome::CardError => "storage",
        DeviceOutcome::NotNewer => "not newer",
        DeviceOutcome::IdTaken => "built in",
        DeviceOutcome::Unsupported => "not supported",
        DeviceOutcome::Cancelled => "cancelled",
        DeviceOutcome::RestartPending => "restart pending",
        _ => "refused",
    }
}

#[cfg(test)]
pub(crate) mod tests;
