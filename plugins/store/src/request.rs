//! Plugins another plugin asks for ([`Command::request_plugins`]), e.g.
//! the reader's packs of letters for a book in a script the fonts lack:
//! the reader is asked once, in a dialog that lists them with what they
//! take (*Download* or *Not now*), what is wanted is fetched in the
//! background (the queue, see [`crate::queue`]), and the plugin that
//! asked is told what came of it ([`events::PLUGINS_ANSWERED`]) once,
//! whatever it was: installed, declined, not offered, or not fetched.
//!
//! One request is open at a time. It is kept on the storage while the
//! dialog is shown and the plugins are fetched: `request.txt` (who asked,
//! then the ids) and, as each plugin is done, `request-result.txt`
//! (`+id` installed, `-id` not). A dialog closed without an answer (the
//! reader went back) is *Not now*: the one who asked is not told, as no
//! download starts.
//!
//! Who asks is what a plugin names itself: nothing here trusts it more than
//! that. Any plugin can also send the event itself, past the kernel's
//! checks, so the store re-applies them: at most
//! [`MAX_REQUESTED_PLUGINS`] ids, each once, none already on the device;
//! the dialog names the plugins by the store's own listing (an id the
//! listing lacks is not offered), never by what the request says; the
//! same request again while its dialog or download is going on is one
//! request (it is answered once), and one the reader just declined is
//! answered *Not now* at once for [`DECLINED_MS`], without a new dialog.

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::events::{self, PluginsAnswered, PluginsRequested};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::{
    Arg, Commands, Env, Id, Instant, MAX_REQUESTED_PLUGINS, RegisterError, Registrar, ask, files,
};

use crate::PLUGIN;
use crate::catalog::{Entry, Index};
use crate::queue;

/// The reader's answer to the dialog.
const ANSWERED: Event<Confirmed> = Event::new("store.request-answered");

/// How long a request the reader declined is not asked again (the one
/// who asked is told *Not now* at once meanwhile).
pub(crate) const DECLINED_MS: u64 = 60_000;

/// The last request shown: who asked, for what, when, and whether the
/// reader declined it.
type Last = Rc<RefCell<Option<(Id, Vec<String>, Instant, bool)>>>;

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    let last: Last = Rc::new(RefCell::new(None));
    let asked = last.clone();
    r.on(events::PLUGINS_REQUESTED, move |env, request, out| {
        requested(env, request, out, &asked);
    })?;
    r.on(ANSWERED, move |env, answered, out| {
        confirmed(env, answered, out, &last);
    })?;
    Ok(())
}

/// The ids asked for as the store takes them: each once, none on the
/// device already, at most [`MAX_REQUESTED_PLUGINS`] (what the kernel
/// checks too, which a plugin that sends the event itself skips).
fn wanted_of(env: &Env<'_>, request: &PluginsRequested) -> Vec<String> {
    let mut wanted: Vec<String> = Vec::new();
    for id in request.ids().take(MAX_REQUESTED_PLUGINS.saturating_mul(4)) {
        if wanted.len() >= MAX_REQUESTED_PLUGINS {
            break;
        }
        if env.plugins().get(id.as_str()).is_none() && !wanted.iter().any(|w| w == id.as_str()) {
            wanted.push(id.to_string());
        }
    }
    wanted
}

fn path() -> String {
    alloc::format!("{}/request.txt", Env::plugin_dir(PLUGIN.as_str()))
}

fn result_path() -> String {
    alloc::format!("{}/request-result.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The request open: who asked, and the ids wanted.
fn open(env: &Env<'_>) -> Option<(Id, Vec<String>)> {
    let text = paperpaw_sdk::read_file(env.storage(), &path(), 4 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())?;
    let mut lines = text.lines();
    let by = Id::parse(lines.next()?).ok()?;
    Some((by, lines.map(ToString::to_string).collect()))
}

/// Closes the request: what is kept of it goes.
fn close(env: &Env<'_>) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "files not there are nothing to remove"
    )]
    {
        let _ = env.storage().remove(&path());
        let _ = env.storage().remove(&result_path());
    }
}

/// What became of plugin `id`, which a request asked for: installed
/// (`done`) or not.
pub(crate) fn outcome(env: &Env<'_>, id: &str, done: bool) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "an outcome not kept makes the plugin count as not installed: the reader is told so, and asks again later"
    )]
    {
        let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
        let line = alloc::format!("{}{id}\n", if done { '+' } else { '-' });
        let _ = env.storage().append(&result_path(), line.as_bytes());
    }
}

/// Tells the one who asked, once.
fn answer(
    out: &mut Commands,
    by: Id,
    (installed, missing): (&[String], &[String]),
    declined: bool,
    error: String,
) {
    out.push(events::PLUGINS_ANSWERED.emit(PluginsAnswered::new(
        by,
        installed.join(" "),
        missing.join(" "),
        declined,
        error,
    )));
}

/// The names of the plugins `ids` (the library's, else their ids), where
/// the index has them, with their sizes.
pub(crate) fn entries(env: &Env<'_>, ids: &[String]) -> Vec<(String, Option<Entry>)> {
    let Some(index) = Index::kept(env.storage()) else {
        return ids.iter().map(|id| (id.clone(), None)).collect();
    };
    let mut found: Vec<Entry> = Vec::new();
    index.scan(env.storage(), &mut |_, entry: Entry| {
        if ids.contains(&entry.id) {
            found.push(entry);
        }
    });
    ids.iter()
        .filter_map(|id| {
            found
                .iter()
                .position(|entry| entry.id == *id)
                .map(|at| (id.clone(), Some(found.swap_remove(at))))
        })
        .collect()
}

/// The plugins' names, joined, for a notice.
pub(crate) fn names(list: &[(String, Option<Entry>)]) -> String {
    list.iter()
        .map(|(id, entry)| entry.as_ref().map_or(id.as_str(), |e| e.name.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Plugins asked for: the dialog, or at once the answer where nothing can
/// be fetched.
fn requested(env: &Env<'_>, request: &PluginsRequested, out: &mut Commands, last: &Last) {
    let by = request.by.clone();
    let wanted = wanted_of(env, request);
    // Nothing to fetch (all are on the device): nothing is missing.
    if wanted.is_empty() {
        return answer(out, by, (&[], &[]), false, String::new());
    }
    // What was just shown for the same is the same request: declined a
    // moment ago, it is not asked again (and *Not now* for it at once);
    // its dialog still open or its download going on, it is the one.
    let same = last.borrow().as_ref().and_then(|(who, ids, at, declined)| {
        (*who == by && *ids == wanted && env.now().since(*at) < DECLINED_MS).then_some(*declined)
    });
    match same {
        Some(true) => return answer(out, by, (&[], &wanted), true, String::new()),
        Some(false) if open(env).is_some() => return,
        _ => {}
    }
    // One request at a time: another says *Not now* for itself. A dialog closed without an answer
    // left its request: it is taken as closed where nothing is fetched.
    if open(env).is_some() {
        if queue::requested_waiting(env) {
            return answer(out, by, (&[], &wanted), true, String::new());
        }
        close(env);
    }
    let list = entries(env, &wanted);
    // The library has not what is asked for (it does not offer it for this
    // device, or yet): not an error to tell the reader of.
    if list.is_empty() {
        return answer(out, by, (&[], &wanted), false, String::new());
    }
    let i18n = env.i18n();
    let ids: Vec<String> = list.iter().map(|(id, _)| id.clone()).collect();
    if !env.caps().network {
        let error = i18n.format(
            "store.request.offline",
            &[("names", Arg::Str(&names(&list)))],
        );
        return answer(out, by, (&[], &ids), false, error);
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a request not kept is not answered when its dialog is: the one who asked asks again"
    )]
    {
        let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
        let text = alloc::format!("{by}\n{}\n", ids.join("\n"));
        if env.storage().write(&path(), text.as_bytes()).is_err() {
            return answer(out, by, (&[], &ids), true, String::new());
        }
    }
    let lines: Vec<String> = list
        .iter()
        .map(|(id, entry)| match entry {
            Some(entry) => i18n.format(
                "store.request.item",
                &[
                    ("name", Arg::Str(&entry.name)),
                    ("size", Arg::Str(&files::shown_size(env, entry.size))),
                ],
            ),
            None => id.clone(),
        })
        .collect();
    *last.borrow_mut() = Some((by.clone(), ids.clone(), env.now(), false));
    let key = alloc::format!("store.request.by.{by}");
    let question = i18n
        .get(&key)
        .unwrap_or_else(|| i18n.tr("store.request.by.other"))
        .to_string();
    out.push(
        ask::confirm(question, &ANSWERED)
            .text(lines.join("\n"))
            .yes(i18n.tr("store.request.download"))
            .no(i18n.tr("store.request.not-now"))
            .focus_yes()
            .command(),
    );
}

/// The reader's word on the dialog: the plugins are fetched, or the one
/// who asked is told *Not now*.
fn confirmed(env: &Env<'_>, answered: &Confirmed, out: &mut Commands, last: &Last) {
    let Some((by, ids)) = open(env) else {
        return;
    };
    if !answered.yes {
        *last.borrow_mut() = Some((by.clone(), ids.clone(), env.now(), true));
        close(env);
        return answer(out, by, (&[], &ids), true, String::new());
    }
    // The connection went meanwhile.
    if !env.caps().network {
        close(env);
        let list: Vec<(String, Option<Entry>)> = ids.iter().map(|id| (id.clone(), None)).collect();
        let error = env.i18n().format(
            "store.request.offline",
            &[("names", Arg::Str(&names(&list)))],
        );
        return answer(out, by, (&[], &ids), false, error);
    }
    queue::add_requested(env, &ids, out);
    // What the reader sees of it: the names, how far, how it ends.
    out.push(crate::fetch::open(&by, &ids));
}

/// Fetches `ids` again for `by` after a download that did not end well
/// (the screen's *Try again*): the reader said *Download* before, so no
/// dialog; the one who asked is told once more what came of it.
pub(crate) fn again(env: &Env<'_>, by: &Id, ids: &[String], out: &mut Commands) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a request not kept is not answered when its download ends: the screen shows what was fetched, and tries again"
    )]
    {
        let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
        let text = alloc::format!("{by}\n{}\n", ids.join("\n"));
        let _ = env.storage().remove(&result_path());
        let _ = env.storage().write(&path(), text.as_bytes());
    }
    queue::add_requested(env, ids, out);
}

/// The queue is empty: every plugin asked for is installed or not; the
/// one who asked is told, and the request is closed.
pub(crate) fn finished(env: &Env<'_>, out: &mut Commands) {
    let Some((by, ids)) = open(env) else {
        return;
    };
    let results = paperpaw_sdk::read_file(env.storage(), &result_path(), 4 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let installed: Vec<String> = ids
        .iter()
        .filter(|id| {
            results
                .lines()
                .any(|l| l.strip_prefix('+') == Some(id.as_str()))
        })
        .cloned()
        .collect();
    let missing: Vec<String> = ids
        .iter()
        .filter(|id| !installed.contains(id))
        .cloned()
        .collect();
    let error = match missing.is_empty() {
        true => String::new(),
        false => {
            let list: Vec<(String, Option<Entry>)> =
                missing.iter().map(|id| (id.clone(), None)).collect();
            let named = entries(env, &missing);
            let list = if named.is_empty() { list } else { named };
            env.i18n().format(
                "store.request.failed",
                &[("names", Arg::Str(&names(&list)))],
            )
        }
    };
    close(env);
    answer(out, by, (&installed, &missing), false, error);
}
