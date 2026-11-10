//! Plugins installed from the computer (e.g. one's own build, from the
//! workshop): its files one by one, then the install in one step, with
//! the SDK's local install ([`Staging`]). The requests carry the key as
//! every other; a plugin with code is installed only once the reader
//! agreed on the device.
//!
//! - `GET /api/plugins`: the plugins installed, as JSON: `{"plugins":
//!   [{"id", "version", "on", "origin": "store"|"local"|"built-in"}]}`.
//! - `PUT /api/plugin/<id>/<path>?sha256=<hex>`: one file of plugin
//!   `<id>`, refused (*422*) where its SHA-256 differs.
//! - `POST /api/plugin/<id>`: installs what was sent: *200* `{"state":
//!   "installed"}`, or *202* `{"state": "asking"}` while the device asks
//!   about its code; *422* `{"state": "refused", "why": …}`.
//! - `GET /api/plugin/<id>`: how the last install of `<id>` stands, as the
//!   same JSON (`"state": "none"` before any).
//! - `DELETE /api/plugin/<id>`: the files sent are dropped.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::install::local::{Commit, Refusal, Staging};
use paperpaw_sdk::install::{self, Origin};
use paperpaw_sdk::served::{json_string, param};
use paperpaw_sdk::{Commands, Env, PluginKind, ServerResponse};

/// How an install stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// The device asks the reader about its code, this code (by its
    /// SHA-256 as installers name it).
    Asking([u8; 32]),
    Installed {
        code: bool,
    },
    Refused(Refusal),
}

/// The installs asked for while the screen is open.
#[derive(Debug, Default)]
pub(crate) struct Installs {
    states: Vec<(String, State)>,
}

/// What a request asks of the screen besides its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ask {
    /// Whether to install the code of plugin `id`.
    Code(String),
    /// Plugin `id` was installed.
    Installed(String),
}

/// A request about plugins, if `path` is one.
pub(crate) fn is_plugin_request(path: &str) -> bool {
    path == "/api/plugins" || path.starts_with("/api/plugin/")
}

impl Installs {
    fn set(&mut self, plugin: &str, state: State) {
        self.states.retain(|(id, _)| id != plugin);
        self.states.push((String::from(plugin), state));
    }

    fn state(&self, plugin: &str) -> Option<State> {
        self.states
            .iter()
            .find(|(id, _)| id == plugin)
            .map(|(_, state)| *state)
    }

    /// Answers a request about plugins (its key checked), and what the
    /// screen is to do besides.
    pub(crate) fn answer(
        &mut self,
        env: &Env<'_>,
        number: u32,
        method: &str,
        target: &str,
        body_file: Option<&str>,
        out: &mut Commands,
    ) -> (ServerResponse, Option<Ask>) {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let json = |status, body: String| {
            ServerResponse::new(number, status, "application/json", body.into_bytes())
        };
        if method == "GET" && path == "/api/plugins" {
            return (json(200, listing(env)), None);
        }
        let rest = path.strip_prefix("/api/plugin/").unwrap_or_default();
        let (id, file) = rest.split_once('/').unwrap_or((rest, ""));
        let Some(staging) = Staging::new(id) else {
            return (json(400, String::from("{\"why\":\"name\"}")), None);
        };
        // The cable is sent this plugin now: not both at once.
        if matches!(method, "PUT" | "POST") && staged_over_cable(env, id) {
            if let Some(body) = body_file {
                let _ = env.storage().remove(body);
            }
            return (
                json(409, String::from("{\"why\":\"being sent over the cable\"}")),
                None,
            );
        }
        match (method, file) {
            ("PUT", file) if !file.is_empty() => {
                let Some(sha256) = param(query, "sha256").and_then(|hex| digest(&hex)) else {
                    if let Some(body) = body_file {
                        let _ = env.storage().remove(body);
                    }
                    return (json(400, String::from("{\"why\":\"sha256\"}")), None);
                };
                let put = match body_file {
                    Some(body) => staging.put(env.storage(), file, body, &sha256),
                    None => Err(Refusal::Damaged),
                };
                match put {
                    Ok(()) => (json(200, String::from("{\"ok\":true}")), None),
                    Err(refusal) => (json(422, refused(refusal)), None),
                }
            }
            ("POST", "") => {
                let committed = staging.commit(env, Commit::default(), out);
                let (state, ask) = match committed {
                    Ok(done) => (
                        State::Installed { code: done.code },
                        Some(Ask::Installed(String::from(id))),
                    ),
                    // Asked about this code: other code put in its place
                    // meanwhile is refused.
                    Err(Refusal::NeedsApproval) => match staging.code(env.storage()) {
                        Some(sha256) => (State::Asking(sha256), Some(Ask::Code(String::from(id)))),
                        None => (State::Refused(Refusal::BadCode), None),
                    },
                    Err(refusal) => (State::Refused(refusal), None),
                };
                self.set(id, state);
                (json(status(state), state_json(Some(state))), ask)
            }
            ("GET", "") => {
                let state = self.state(id);
                (json(200, state_json(state)), None)
            }
            ("DELETE", "") => {
                staging.abort(env.storage());
                self.states.retain(|(kept, _)| kept != id);
                (json(200, String::from("{\"ok\":true}")), None)
            }
            _ => (json(404, String::from("{\"why\":\"not found\"}")), None),
        }
    }

    /// The reader answered whether to install the code of `plugin`.
    pub(crate) fn approved(
        &mut self,
        env: &Env<'_>,
        plugin: &str,
        yes: bool,
        out: &mut Commands,
    ) -> bool {
        let Some(staging) = Staging::new(plugin) else {
            return false;
        };
        let Some(State::Asking(sha256)) = self.state(plugin) else {
            return false;
        };
        if !yes {
            staging.abort(env.storage());
            self.set(plugin, State::Refused(Refusal::NeedsApproval));
            return false;
        }
        let state = match staging.commit(env, Commit::approved(sha256), out) {
            Ok(done) => State::Installed { code: done.code },
            Err(refusal) => State::Refused(refusal),
        };
        self.set(plugin, state);
        matches!(state, State::Installed { .. })
    }
}

/// Whether the Wi-Fi transfer, open, has files of plugin `id` staged.
pub(crate) fn staged_over_wifi(env: &Env<'_>, id: &str) -> bool {
    let open = env
        .stack()
        .iter()
        .any(|entry| entry.screen == crate::SCREEN);
    open && Staging::new(id).is_some_and(|staging| !staging.files(env.storage()).is_empty())
}

/// Whether the cable transfer, open, has an offer of plugin `id`.
fn staged_over_cable(env: &Env<'_>, id: &str) -> bool {
    let open = env
        .stack()
        .iter()
        .any(|entry| entry.screen == crate::CABLE_SCREEN);
    open && crate::cable::offered_plugin(env).as_deref() == Some(id)
}

fn status(state: State) -> u16 {
    match state {
        State::Asking(_) => 202,
        State::Installed { .. } => 200,
        State::Refused(_) => 422,
    }
}

fn state_json(state: Option<State>) -> String {
    match state {
        None => String::from("{\"state\":\"none\"}"),
        Some(State::Asking(_)) => String::from("{\"state\":\"asking\"}"),
        Some(State::Installed { code }) => {
            format!("{{\"state\":\"installed\",\"restart\":{code}}}")
        }
        Some(State::Refused(refusal)) => refused(refusal),
    }
}

fn refused(refusal: Refusal) -> String {
    format!(
        "{{\"state\":\"refused\",\"why\":{}}}",
        json_string(why(refusal))
    )
}

/// Why a plugin was not installed, in a word or two.
pub(crate) fn why(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Name => "name",
        Refusal::Damaged => "damaged",
        Refusal::Storage => "storage",
        Refusal::NotThePlugin => "not the plugin",
        Refusal::BuiltIn => "built in",
        Refusal::BadCode => "bad code",
        Refusal::NoCode => "no code plugins here",
        Refusal::OtherBuild => "built for another PaperPaw",
        Refusal::NeedsApproval => "not approved",
        _ => "refused",
    }
}

/// 64 hexadecimal digits as 32 bytes.
fn digest(hex: &str) -> Option<[u8; 32]> {
    paperpaw_sdk::digest::hex::decode_array(hex)
}

/// The plugins installed, as JSON.
pub(crate) fn listing(env: &Env<'_>) -> String {
    let mut json = String::from("{\"plugins\":[");
    // Read once for all of them, where one is not built in.
    let origins = core::cell::OnceCell::new();
    for (i, info) in env.plugins().iter().enumerate() {
        let version = info.update.unwrap_or(info.manifest.version);
        let id = info.manifest.id.as_str();
        let origin = match info.kind {
            PluginKind::BuiltIn => "built-in",
            _ => match origins
                .get_or_init(|| install::Origins::read(env.storage()))
                .of(id, version)
            {
                Origin::Store => "store",
                _ => "local",
            },
        };
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            "{{\"id\":{},\"version\":{},\"on\":{},\"origin\":{}}}",
            json_string(id),
            json_string(&format!("{version}")),
            info.enabled,
            json_string(origin)
        ));
    }
    json.push_str("]}");
    json
}
