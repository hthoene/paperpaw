//! Updates one after another: *Update all* keeps the plugins to update in
//! a queue on the storage. Each one's page at the library (with its files)
//! is fetched when its turn comes, and its download starts once the one
//! before is done, stopped, or set aside; screens may come and go
//! meanwhile. Without a connection, or after a restart, the queue waits
//! and goes on when the store is opened again (nothing is downloaded
//! unasked at the start).
//!
//! The queue is a line per plugin; the one whose turn it is stays first,
//! marked, until the next one's turn comes, so that one cut off by a
//! restart is begun anew.
//!
//! The first-run setup's offer (`setup`) puts its plugins in the queue
//! quietly, each marked: they are installed without the store being
//! opened, also after a restart (the reader asked for them), not said one
//! by one but once when the last of them is done ([`said`]). Plugins put
//! in by *Update all* are said as always, even behind quiet ones.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::params;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Arg, Command, Commands, Env, HttpRequest, Params, install};

use crate::catalog::{self, Device};
use crate::{CATALOG, INSTALLER, PLUGIN, failed, trust};

/// The answer with the page of the plugin whose turn it is.
pub(crate) const FETCHED: Event<HttpReply> = Event::new("store.queued");

/// The mark of the plugin whose turn it is.
const TURN: &str = "> ";
/// The mark of a plugin put in quietly (the setup's).
const QUIET: &str = "~";
/// The mark of a plugin another plugin asked for (see `request`): fetched
/// quietly too, and told to the one who asked, not said.
const REQUESTED: &str = "&";

fn path() -> String {
    format!("{}/queue.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The queue's lines, the one whose turn it is first and marked.
fn lines(env: &Env<'_>) -> Vec<String> {
    paperpaw_sdk::read_file(env.storage(), &path(), 16 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.lines().map(ToString::to_string).collect())
        .unwrap_or_default()
}

fn keep(env: &Env<'_>, lines: &[String]) {
    if lines.is_empty() {
        let _ = env.storage().remove(&path());
        return;
    }
    let mut text = lines.join("\n");
    text.push('\n');
    let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = env.storage().write(&path(), text.as_bytes());
}

/// A line's plugin, and whether it was put in quietly.
fn entry(line: &str) -> (&str, bool) {
    let line = line.strip_prefix(TURN).unwrap_or(line);
    match line
        .strip_prefix(QUIET)
        .or_else(|| line.strip_prefix(REQUESTED))
    {
        Some(id) => (id, true),
        None => (line, false),
    }
}

/// Whether the plugin whose turn it is was asked for by a plugin
/// ([`add_requested`]).
pub(crate) fn requested(env: &Env<'_>) -> bool {
    lines(env).first().is_some_and(|line| {
        line.strip_prefix(TURN)
            .is_some_and(|rest| rest.starts_with(REQUESTED))
    })
}

/// Whether plugins asked for by a plugin are in the queue still.
pub(crate) fn requested_waiting(env: &Env<'_>) -> bool {
    lines(env).iter().any(|line| {
        line.strip_prefix(TURN)
            .unwrap_or(line)
            .starts_with(REQUESTED)
    })
}

/// The plugins in the queue, in order (the one whose turn it is first).
pub(crate) fn waiting(env: &Env<'_>) -> Vec<String> {
    lines(env)
        .iter()
        .map(|line| entry(line).0.to_string())
        .collect()
}

/// Whether the plugin whose turn it is was put in quietly: what becomes
/// of it is kept ([`note`]), not said.
pub(crate) fn quiet(env: &Env<'_>) -> bool {
    lines(env)
        .first()
        .is_some_and(|line| line.starts_with(TURN) && entry(line).1)
}

/// Whether plugins put in quietly wait: they go on at the start.
pub(crate) fn has_quiet(env: &Env<'_>) -> bool {
    lines(env).iter().any(|line| entry(line).1)
}

/// Where what became of the quiet plugins is kept: `+<name>` installed,
/// `-<name>` not.
fn quiet_path() -> String {
    format!("{}/queue-quiet.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// Keeps that the quiet plugin `name` was installed (`done`) or not.
pub(crate) fn note(env: &Env<'_>, name: &str, done: bool) {
    let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let line = format!(
        "{}{}\n",
        if done { '+' } else { '-' },
        name.replace('\n', " ")
    );
    let _ = env.storage().append(&quiet_path(), line.as_bytes());
}

/// Plugin `id`'s name where its page did not come: the setup's own for
/// the plugins it offers, else its id.
fn name_of(env: &Env<'_>, id: &str) -> String {
    env.i18n()
        .get(&format!("store.offer.{id}"))
        .map_or_else(|| id.to_string(), ToString::to_string)
}

/// Says, once no quiet plugin waits any more, what they became.
fn said(env: &Env<'_>, out: &mut Commands) {
    if has_quiet(env) {
        return;
    }
    let Some(text) = paperpaw_sdk::read_file(env.storage(), &quiet_path(), 16 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return;
    };
    let _ = env.storage().remove(&quiet_path());
    let names = |mark: char| {
        text.lines()
            .filter_map(|line| line.strip_prefix(mark))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (done, mut not) = (names('+'), names('-'));
    // The faces of Literata have a hint of their own: what helps is the
    // network, not the store.
    let styles = name_of(env, crate::styles::PACK);
    if let Some(rest) = crate::styles::without(&not, &styles) {
        not = rest;
        if crate::styles::hint_due(env) {
            out.push(Command::Notify(
                env.i18n()
                    .format("store.styles.later", &[("name", Arg::Str(&styles))]),
            ));
        }
    }
    if !done.is_empty() {
        out.push(Command::Notify(
            env.i18n()
                .format("store.offer.done", &[("names", Arg::Str(&done))]),
        ));
    }
    if !not.is_empty() {
        out.push(Command::Notify(
            env.i18n()
                .format("store.offer.not-done", &[("names", Arg::Str(&not))]),
        ));
    }
}

/// Puts `ids` in the queue (behind those waiting) and starts the first
/// if nothing is being downloaded.
pub(crate) fn add(env: &Env<'_>, ids: &[String], out: &mut Commands) {
    put(env, ids, "", out);
}

/// Puts `ids` in the queue as plugins another plugin asked for (see
/// `request`).
pub(crate) fn add_requested(env: &Env<'_>, ids: &[String], out: &mut Commands) {
    put(env, ids, REQUESTED, out);
}

/// Puts `ids` in the queue quietly (the first-run setup's, see the
/// [module](self)).
pub(crate) fn add_quietly(env: &Env<'_>, ids: &[String], out: &mut Commands) {
    put(env, ids, QUIET, out);
}

fn put(env: &Env<'_>, ids: &[String], mark: &str, out: &mut Commands) {
    let mut all = lines(env);
    let known = waiting(env);
    for id in ids {
        if !known.contains(id) {
            all.push(format!("{mark}{id}"));
        }
    }
    keep(env, &all);
    next(env, out);
}

/// A download cut off by a restart: its turn again, first, when the
/// queue goes on.
pub(crate) fn interrupted(env: &Env<'_>) {
    let mut all = lines(env);
    if let Some(first) = all.first_mut()
        && let Some(id) = first.strip_prefix(TURN)
    {
        *first = id.to_string();
        keep(env, &all);
    }
}

/// The address of plugin `id`'s page at the library.
fn page_url(env: &Env<'_>, device: &Device<'_>, id: &str) -> String {
    let list = env.settings().read(&CATALOG);
    let community = crate::community(env);
    catalog::plugin_url(
        list.trim(),
        device.name,
        env.i18n().language(),
        id,
        community,
        device.fingerprint,
    )
}

/// Gives the next plugin its turn (asks for its page), if nothing is
/// being downloaded: the one before is done, stopped or set aside.
pub(crate) fn next(env: &Env<'_>, out: &mut Commands) {
    if INSTALLER.progress(env.storage()).is_some() {
        return;
    }
    // No connection: it waits for the next time the store is opened.
    let Some(device) = Device::of(env).filter(|_| env.caps().network) else {
        return;
    };
    let mut all = lines(env);
    if all.first().is_some_and(|first| first.starts_with(TURN)) {
        all.remove(0);
    }
    all.retain(|line| install::is_plugin_name(entry(line).0));
    let Some(first) = all.first().cloned() else {
        keep(env, &[]);
        crate::request::finished(env, out);
        return said(env, out);
    };
    let id = entry(&first).0.to_string();
    all[0] = format!("{TURN}{first}");
    keep(env, &all);
    said(env, out);
    let url = page_url(env, &device, &id);
    let mut request = HttpRequest::new("GET", &url, FETCHED);
    request = request
        .save_to(catalog::plugin_part(&id))
        .max_file(catalog::MAX_PAGE_FILE);
    request = request.with(Params::new().with(params::ID, id));
    out.push(Command::http(request));
}

/// The page of the plugin whose turn it is came: its download starts, or
/// the next one's turn comes.
pub(crate) fn fetched(env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
    let Some(id) = reply.context.get(params::ID).map(ToString::to_string) else {
        return;
    };
    crate::styles::answered(env, &id, reply.status);
    let fetched = catalog::plugin_part(&id);
    let plugin = Device::of(env)
        .filter(|_| (200..300).contains(&reply.status))
        .and_then(|device| {
            let address = reply
                .final_url
                .clone()
                .unwrap_or_else(|| page_url(env, &device, &id));
            catalog::plugin(env.storage(), &fetched, &address, &device)
        })
        // Not installed yet (the setup's), or newer than the one installed.
        .filter(|plugin| {
            plugin.entry.id == id
                // A built-in plugin is switched on, never downloaded.
                && !plugin.entry.switched_off(env)
                // Made for a newer PaperPaw: nothing to install.
                && !plugin.entry.unbuilt()
                && (plugin.entry.installed(env).is_none() || plugin.entry.newer(env))
        });
    let _ = env.storage().remove(&fetched);
    match plugin {
        Some(plugin) => {
            trust::expect(env.storage(), &plugin.entry.id, plugin.named().as_ref());
            if !INSTALLER.start(env, &plugin.package, out) {
                if requested(env) {
                    crate::request::outcome(env, &id, false);
                } else if quiet(env) {
                    note(env, &plugin.entry.name, false);
                } else {
                    out.push(failed(env, &plugin.entry.name));
                }
                next(env, out);
            }
        }
        None => {
            if requested(env) {
                // Already there, or not fetched.
                crate::request::outcome(env, &id, env.plugins().get(&id).is_some());
            } else if quiet(env) {
                note(env, &name_of(env, &id), false);
            }
            next(env, out);
        }
    }
}
