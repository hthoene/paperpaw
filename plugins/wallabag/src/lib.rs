//! Wallabag: articles saved to read later, fetched as books into the
//! `Wallabag` folder of the library, and archived in Wallabag once read.
//!
//! *Wallabag* in the tools sets up the account (the server, user name
//! and password, and an API client's ID and secret, made in Wallabag
//! under *API clients management*; typing them can be done on a phone
//! too, see the keyboard) and fetches: each unread article comes as an
//! EPUB Wallabag makes, with its pictures. Articles read to the end are
//! archived with the next fetch and their books removed, as are those
//! Wallabag says are archived or deleted elsewhere; a book with the
//! reader's highlights, notes or bookmarks stays in the library, let go
//! of by Wallabag. The password and the
//! client's secret are kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`).
//!
//! The list of articles is read as it streams from storage (see
//! `paperpaw_sdk::json::stream`) and each book is written straight to
//! storage, so this works on small devices too. Shipped switched off:
//! switch it on in the plugins.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod native_entry;
pub mod native_fetch;
#[cfg(test)]
mod native_fetch_tests;
#[doc(hidden)]
pub mod native_keep;
#[cfg(test)]
mod native_parity_tests;
/// The interface-7 wallabag plugin.
pub mod native_plugin;
pub mod native_wallabag;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{actions, events, groups, menus, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::json;
use paperpaw_sdk::json::stream::{self, Event as Json};
use paperpaw_sdk::replies::{HttpReply, Typed};
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::web::{clean_name, encode};
use paperpaw_sdk::{
    ActionSpec, Arg, Canvas, Command, Commands, Cx, Env, FixedAction, FixedScreen, Gesture,
    GestureKind, Handled, HttpRequest, Id, Manifest, MenuEntry, Params, Plugin, Rect, RefreshMode,
    RegisterError, Registrar, Screen, SettingSpec, Storage, StorageError,
};

pub const PLUGIN: Id = Id::new_static("wallabag");
/// Opens the set-up.
pub const OPEN: Id = Id::new_static("wallabag.open");
/// Fetches the articles.
pub const FETCH: Id = Id::new_static("wallabag.fetch");
/// Declared work: a fetch no reader waits for (a rule's).
const FETCH_DUE: paperpaw_sdk::event::Event = paperpaw_sdk::event::Event::new("wallabag.fetch-due");
/// The server's address, the user name and the API client's ID.
pub const SERVER: Setting<Text> = Setting::new("wallabag.server");
pub const USER: Setting<Text> = Setting::new("wallabag.user");
pub const CLIENT: Setting<Text> = Setting::new("wallabag.client-id");
/// The password and the API client's secret, among the passwords and
/// keys.
pub const PASSWORD: &str = "wallabag.password";
pub const SECRET: &str = "wallabag.client-secret";
const SCREEN: Id = Id::new_static("wallabag.setup");
/// Answers: the token, an archived article, the list, an article asked
/// about, a book.
const TOKEN: Event<HttpReply> = Event::new("wallabag.token");
const ARCHIVED: Event<HttpReply> = Event::new("wallabag.archived");
const LISTED: Event<HttpReply> = Event::new("wallabag.listed");
const CHECKED: Event<HttpReply> = Event::new("wallabag.checked");
const DOWNLOADED: Event<HttpReply> = Event::new("wallabag.downloaded");
/// Emitted with a message (`text`) when a fetch ends.
const DONE: Event<Finished> = Event::new("wallabag.done");

paperpaw_sdk::event_data! {
    /// How a fetch ended ([`DONE`]), in words.
    struct Finished {
        text: String,
    }
}
/// Typed on the keyboard; parameter `field`.
const ENTERED: Event<Typed> = Event::new("wallabag.entered");
const FIELD: &str = "field";
const ARTICLE: &str = "article";
const FOLDER: &str = "Wallabag";
/// Unread articles fetched at most.
const MAX_ARTICLES: usize = 30;
/// Reading progress, in percent, from which an article counts as read
/// where its pages are not counted yet.
const READ: f64 = 95.0;

fn data(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// The text of this plugin's file `name`, to change: empty when there is
/// none yet, an error when it cannot be read (then it must not be
/// written either, or what it holds would be lost).
fn load_for_update(storage: &dyn Storage, name: &str, cap: usize) -> Result<String, StorageError> {
    paperpaw_sdk::files::load_text_for_update(storage, &data(name), cap)
        .map(Option::unwrap_or_default)
}

/// The folder of the articles: `Wallabag` in the library.
fn folder(env: &Env<'_>) -> String {
    let library = paperpaw_sdk::library::folder(env);
    if library.is_empty() {
        String::from(FOLDER)
    } else {
        format!("{library}/{FOLDER}")
    }
}

/// The server as a base URL: `https://` unless another scheme is given,
/// no `/` at the end.
fn server(env: &Env<'_>) -> String {
    paperpaw_sdk::web::base_url(env.settings().text(SERVER.as_str()))
}

/// An article fetched: its id in Wallabag and its book.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Article {
    id: String,
    path: String,
}

/// The articles fetched, kept as `id<TAB>path` lines.
fn articles(storage: &dyn Storage) -> Result<Vec<Article>, StorageError> {
    let text = load_for_update(storage, "articles.txt", 64 * 1024)?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let (id, path) = line.split_once('\t')?;
            Some(Article {
                id: id.to_string(),
                path: path.to_string(),
            })
        })
        .collect())
}

fn save_articles(env: &Env<'_>, list: &[Article]) -> Result<(), StorageError> {
    let text: String = list
        .iter()
        .map(|a| format!("{}\t{}\n", a.id, a.path))
        .collect();
    env.storage().write(&data("articles.txt"), text.as_bytes())
}

/// Whose the articles fetched are: the server and the user name, one a
/// line, in `account.txt` beside them.
fn account(env: &Env<'_>) -> String {
    format!("{}\n{}\n", server(env), env.settings().read(&USER))
}

/// The articles fetched and those read to the end, from this account.
/// Those of another server or user (the settings changed) are let go of
/// first: their books stay in the library, never asked about at a server
/// that does not know them.
fn lists_of_account(env: &Env<'_>) -> Result<(Vec<Article>, Vec<String>), StorageError> {
    let (fetched, finished) = (articles(env.storage())?, finished(env.storage())?);
    let account = account(env);
    let kept = load_for_update(env.storage(), "account.txt", 4 * 1024)?;
    if kept == account {
        return Ok((fetched, finished));
    }
    // An older version kept no account: its lists are this one's.
    let (fetched, finished) = if kept.is_empty() {
        (fetched, finished)
    } else {
        save_articles(env, &[])?;
        env.storage().write(&data("finished.txt"), b"")?;
        (Vec::new(), Vec::new())
    };
    env.storage()
        .write(&data("account.txt"), account.as_bytes())?;
    Ok((fetched, finished))
}

/// The ids of articles read to the end, to archive.
fn finished(storage: &dyn Storage) -> Result<Vec<String>, StorageError> {
    let text = load_for_update(storage, "finished.txt", 16 * 1024)?;
    Ok(text
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

fn save_finished(env: &Env<'_>, ids: &[String]) {
    let text: String = ids.iter().map(|id| format!("{id}\n")).collect();
    let _ = env.storage().write(&data("finished.txt"), text.as_bytes());
}

/// Notes an article read to the end, from the reader's page turns.
fn page_turned(env: &Env<'_>, reading: &events::Reading) {
    let path = reading.path.as_str();
    // On the last page, or near the end where pages are not counted yet.
    let read = match (reading.page, reading.pages) {
        (Some(page), Some(pages)) => page >= pages,
        _ => f64::from(reading.progress) >= READ,
    };
    if !read || !path.starts_with(&folder(env)) {
        return;
    }
    let Ok(articles) = articles(env.storage()) else {
        return;
    };
    let Some(article) = articles.into_iter().find(|a| a.path == path) else {
        return;
    };
    let Ok(mut ids) = finished(env.storage()) else {
        return;
    };
    if !ids.contains(&article.id) {
        ids.push(article.id);
        save_finished(env, &ids);
    }
}

/// Whether the reader made highlights, notes or bookmarks in the book at
/// `path`, or that cannot be told: then it is kept too.
fn annotated(storage: &dyn Storage, path: &str) -> bool {
    match storage.size(&paperpaw_sdk::annotations::file(path)) {
        Err(StorageError::NotFound) => false,
        Err(_) => true,
        Ok(_) => !paperpaw_sdk::annotations::marks(storage, path).is_empty(),
    }
}

/// Where the book of article `id`, titled `name`, goes in `dir`: `name`,
/// or `name (id)` where that is taken, by a file or by an article
/// fetched before (whose book the reader may have deleted); a free name
/// in any case, so no file is replaced. `None` where both are an
/// article's.
fn book_path(
    storage: &dyn Storage,
    dir: &str,
    name: &str,
    id: &str,
    list: &[Article],
) -> Option<String> {
    let listed = |path: &str| {
        list.iter()
            .any(|a| paperpaw_sdk::files::same_path(storage, &a.path, path))
    };
    let plain = format!("{dir}/{name}.epub");
    if !storage.exists(&plain) && !listed(&plain) {
        return Some(plain);
    }
    let path = paperpaw_sdk::files::free_path(storage, dir, &format!("{name} ({id}).epub"));
    (!listed(&path)).then_some(path)
}

/// What a fetch in progress knows.
#[derive(Debug, Default)]
struct Fetch {
    running: bool,
    token: String,
    /// Articles to archive, then to ask about (those fetched that are not
    /// in the list), then to download (id and title).
    archive: Vec<String>,
    checks: Vec<String>,
    downloads: Vec<(String, String)>,
    listed: bool,
    archived: usize,
    fetched: usize,
    failed: usize,
}

type Shared = Rc<RefCell<Fetch>>;

impl Fetch {
    fn request(
        &self,
        env: &Env<'_>,
        method: &str,
        path: &str,
        reply: &Event<HttpReply>,
    ) -> HttpRequest {
        let (agent, name) = paperpaw_sdk::web::user_agent_of(env);
        HttpRequest::new(method, &format!("{}{path}", server(env)), reply)
            .header(
                String::from("Authorization"),
                format!("Bearer {}", self.token),
            )
            .header(agent, name)
    }

    /// Starts a fetch: first a token for the account.
    fn start(&mut self, env: &Env<'_>, out: &mut Commands) {
        if self.running {
            return;
        }
        if !env.caps().network {
            return done(env, "wallabag.no-network", &[], out);
        }
        let (Some(password), Some(secret)) =
            (env.secrets().get(PASSWORD), env.secrets().get(SECRET))
        else {
            return done(env, "wallabag.setup-needed", &[], out);
        };
        let (user, client) = (env.settings().read(&USER), env.settings().read(&CLIENT));
        if server(env).is_empty() || user.is_empty() || client.is_empty() {
            return done(env, "wallabag.setup-needed", &[], out);
        }
        *self = Self {
            running: true,
            ..Self::default()
        };
        let body = format!(
            "grant_type=password&client_id={}&client_secret={}&username={}&password={}",
            encode(client),
            encode(secret),
            encode(user),
            encode(password)
        );
        let request = HttpRequest::new("POST", &format!("{}/oauth/v2/token", server(env)), &TOKEN)
            .header(
                String::from("Content-Type"),
                String::from("application/x-www-form-urlencoded"),
            )
            .body(body);
        out.push(Command::Notify(
            env.i18n().tr("wallabag.fetching").to_string(),
        ));
        out.push(Command::http(request));
    }

    fn ok(reply: &HttpReply) -> Option<u16> {
        reply.ok().then_some(reply.status)
    }

    fn token(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        let text = reply.text();
        let token = Self::ok(reply)
            .and_then(|_| json::text(&json::object(&text), "access_token").map(String::from));
        let Some(token) = token else {
            self.running = false;
            let key = match reply.status {
                0 => "wallabag.unreachable",
                _ => "wallabag.bad-login",
            };
            return done(env, key, &[], out);
        };
        self.token = token;
        let Ok((fetched, finished)) = lists_of_account(env) else {
            self.running = false;
            return done(env, "wallabag.unreadable", &[], out);
        };
        self.archive = finished
            .into_iter()
            .filter(|id| fetched.iter().any(|a| a.id == *id))
            .collect();
        self.next(env, out);
    }

    /// The next step: archive, list, ask about articles, download, or
    /// done.
    fn next(&mut self, env: &Env<'_>, out: &mut Commands) {
        if let Some(id) = self.archive.pop() {
            let request = self
                .request(env, "PATCH", &format!("/api/entries/{id}.json"), &ARCHIVED)
                .header(
                    String::from("Content-Type"),
                    String::from("application/x-www-form-urlencoded"),
                )
                .body(String::from("archive=1"))
                .with(Params::new().with(ARTICLE, id));
            out.push(Command::http(request));
            return;
        }
        if !self.listed {
            let path = format!(
                "/api/entries.json?archive=0&detail=metadata&sort=created&order=desc&perPage={MAX_ARTICLES}"
            );
            let mut request = self.request(env, "GET", &path, &LISTED);
            request = request.save_to(data("entries.json"));
            out.push(Command::http(request));
            return;
        }
        if let Some(id) = self.checks.pop() {
            let mut request =
                self.request(env, "GET", &format!("/api/entries/{id}.json"), &CHECKED);
            request = request.save_to(data("entry.json"));
            request = request.with(Params::new().with(ARTICLE, id));
            out.push(Command::http(request));
            return;
        }
        if let Some((id, title)) = self.downloads.pop() {
            let mut request = self.request(
                env,
                "GET",
                &format!("/api/entries/{id}/export.epub"),
                &DOWNLOADED,
            );
            request = request.save_to(data("article.tmp"));
            request = request.with(Params::new().with(ARTICLE, id).with(params::TITLE, title));
            out.push(Command::http(request));
            return;
        }
        self.running = false;
        let _ = env.storage().remove(&data("entries.json"));
        let _ = env.storage().remove(&data("entry.json"));
        let key = if self.failed > 0 {
            "wallabag.done-failed"
        } else {
            "wallabag.done"
        };
        let args = [
            ("fetched", Arg::Int(self.fetched as i64)),
            ("archived", Arg::Int(self.archived as i64)),
            ("failed", Arg::Int(self.failed as i64)),
        ];
        done(env, key, &args, out);
    }

    fn archived(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        let id = reply.context.get(ARTICLE).unwrap_or_default();
        if Self::ok(reply).is_some() {
            self.archived += 1;
            remove_article(env, id, out);
        } else {
            self.failed += 1;
        }
        self.next(env, out);
    }

    fn listed(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        self.listed = true;
        // Only a whole list counts: an answer that is not one (a page of
        // the server's own, an error, a list cut short) changes nothing.
        let unread = Self::ok(reply).and_then(|_| unread(env.storage()));
        let (Some(unread), Ok(fetched)) = (unread, articles(env.storage())) else {
            self.failed += 1;
            return self.next(env, out);
        };
        // Those not in the list may be archived or deleted elsewhere, or
        // just older than the newest: each is asked about before its book
        // goes.
        self.checks = fetched
            .iter()
            .filter(|a| !unread.iter().any(|(id, _)| *id == a.id))
            .map(|a| a.id.clone())
            .collect();
        // The list is newest first: popped from the end, the oldest come
        // first, so the newest are the most recent books.
        self.downloads = unread
            .into_iter()
            .filter(|(id, _)| !fetched.iter().any(|a| a.id == *id))
            .collect();
        self.next(env, out);
    }

    /// An article fetched but not among the newest unread: its book goes
    /// only when Wallabag says it is archived or no longer there.
    fn checked(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        let id = reply.context.get(ARTICLE).unwrap_or_default();
        let gone = match reply.status {
            404 => Some(true),
            _ if reply.ok() => is_archived(env.storage(), id),
            _ => None,
        };
        match gone {
            Some(true) => remove_article(env, id, out),
            Some(false) => {}
            None => self.failed += 1,
        }
        let _ = env.storage().remove(&data("entry.json"));
        self.next(env, out);
    }

    fn downloaded(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        let tmp = data("article.tmp");
        let id = reply.context.get(ARTICLE).unwrap_or_default();
        let title = reply.context.get(params::TITLE).unwrap_or_default();
        // The id goes into a file's name: a number only.
        let list = Self::ok(reply)
            .filter(|_| is_id(id))
            .and_then(|_| articles(env.storage()).ok());
        let saved = list.and_then(|mut list| {
            let dir = folder(env);
            let _ = env.storage().create_dir(&dir);
            let name =
                clean_name(title, native_wallabag::NAME_UNITS).unwrap_or_else(|| id.to_string());
            let path = book_path(env.storage(), &dir, &name, id, &list)?;
            env.storage().rename(&tmp, &path).ok()?;
            out.push(paperpaw_sdk::library::added(&path, PLUGIN.as_str()));
            list.push(Article {
                id: id.to_string(),
                path,
            });
            // Where it is not noted, the book stays as any other.
            save_articles(env, &list).ok()
        });
        if saved.is_some() {
            self.fetched += 1;
        } else {
            self.failed += 1;
            let _ = env.storage().remove(&tmp);
        }
        self.next(env, out);
    }
}

/// The unread articles in the list Wallabag sent: ids and titles. Ids
/// are numbers in Wallabag; others are left out, as they go into the
/// list of articles and into names of files. `None` when the answer is
/// not a whole list (its items never end).
fn unread(storage: &dyn Storage) -> Option<Vec<(String, String)>> {
    let mut items: Vec<(String, String)> = Vec::new();
    let mut whole = false;
    let list = ["_embedded", "items"];
    let item = ["_embedded", "items", "[]"];
    stream::read_file(storage, &data("entries.json"), &mut |event| match event {
        Json::Open(path) if path == item => items.push((String::new(), String::new())),
        Json::Close(path) if path == list => whole = true,
        Json::Value(path, value) if path.len() == 4 && path[..3] == item => {
            if let Some(last) = items.last_mut() {
                match path[3].as_str() {
                    "id" => last.0 = value.to_string(),
                    "title" => last.1 = value.to_string(),
                    _ => {}
                }
            }
        }
        _ => {}
    });
    items.retain(|(id, _)| is_id(id));
    items.truncate(MAX_ARTICLES);
    whole.then_some(items)
}

/// Whether article `id`, as Wallabag sent it (`entry.json`), is
/// archived; `None` when the answer is not that article.
fn is_archived(storage: &dyn Storage, id: &str) -> Option<bool> {
    let (mut same, mut archived) = (false, None);
    stream::read_file(storage, &data("entry.json"), &mut |event| {
        if let Json::Value([key], value) = event {
            match key.as_str() {
                "id" => same = value == id,
                "is_archived" => {
                    archived = match value {
                        "1" | "true" => Some(true),
                        "0" | "false" => Some(false),
                        _ => None,
                    }
                }
                _ => {}
            }
        }
    });
    archived.filter(|_| same)
}

/// Whether `id` is an id as Wallabag gives them: a number.
fn is_id(id: &str) -> bool {
    (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// Removes an article's book and what is noted about it (nothing from a
/// list that cannot be read); the book's removal is told. A book with the
/// reader's annotations stays, as does one another article in the list
/// is at too (an older version gave two articles one name): the article
/// is only let go of.
fn remove_article(env: &Env<'_>, id: &str, out: &mut Commands) {
    if let Ok(mut list) = articles(env.storage())
        && let Some(at) = list.iter().position(|a| a.id == id)
    {
        let article = list.remove(at);
        let shared = list.iter().any(|a| a.path == article.path);
        if !shared
            && !annotated(env.storage(), &article.path)
            && env.storage().remove(&article.path).is_ok()
        {
            out.push(events::BOOK_REMOVED.emit(events::BookRemoved::new(article.path)));
        }
        let _ = save_articles(env, &list);
    }
    if let Ok(mut ids) = finished(env.storage())
        && let Some(at) = ids.iter().position(|f| f == id)
    {
        ids.remove(at);
        save_finished(env, &ids);
    }
}

/// Ends a fetch with message `key`.
fn done(env: &Env<'_>, key: &str, args: &[(&str, Arg<'_>)], out: &mut Commands) {
    let text = env.i18n().format(key, args);
    out.push(Command::Notify(text.clone()));
    out.push(DONE.emit(Finished { text }));
}

/// The Wallabag plugin.
#[derive(Debug, Default)]
pub struct Wallabag;

/// The server and the account, set on the setup screen.
static ON_SETUP: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().editor(SCREEN);
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::text(SERVER.static_str(), "wallabag.server", "", groups::LIBRARY)
        .with_extra(&ON_SETUP)
        .kept_on_reset()
        .description("wallabag.server.help"),
    SettingSpec::text(USER.static_str(), "wallabag.user", "", groups::LIBRARY)
        .with_extra(&ON_SETUP)
        .kept_on_reset()
        .advanced()
        .description("wallabag.user.help"),
    SettingSpec::text(
        CLIENT.static_str(),
        "wallabag.client-id",
        "",
        groups::LIBRARY,
    )
    .with_extra(&ON_SETUP)
    .kept_on_reset()
    .advanced()
    .description("wallabag.client-id.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "wallabag.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(48)];
static ICONS: [(Id, &str); 2] = [(OPEN, "article"), (FETCH, "refresh")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(SCREEN, "wallabag.title", |env, _| {
    Box::new(SetupScreen::new(env))
})
.instances(paperpaw_sdk::Instances::SINGLE)];

impl Plugin for Wallabag {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Read-later articles (own server)",
            paperpaw_sdk::package_version!(),
        )
        .description("Fetches articles saved in wallabag, as books.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::default();
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        let s = state.clone();
        r.add_action(ActionSpec::new(FETCH, "wallabag.fetch").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                if env.reader() {
                    s.borrow_mut().start(env, out);
                } else {
                    let due = FETCH_DUE.id().clone();
                    out.push(Command::work(paperpaw_sdk::work::Work::when_cheap(
                        due,
                        core::time::Duration::from_secs(3600),
                    )));
                }
            },
        ))?;
        let s = state.clone();
        r.on(FETCH_DUE, move |env, (), out| {
            s.borrow_mut().start(env, out)
        })?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        type Step = fn(&mut Fetch, &Env<'_>, &HttpReply, &mut Commands);
        let steps: [(Event<HttpReply>, Step); 5] = [
            (TOKEN, Fetch::token),
            (ARCHIVED, Fetch::archived),
            (LISTED, Fetch::listed),
            (CHECKED, Fetch::checked),
            (DOWNLOADED, Fetch::downloaded),
        ];
        for (event, step) in steps {
            let s = state.clone();
            r.on(event, move |env, reply, out| {
                step(&mut s.borrow_mut(), env, reply, out);
            })?;
        }
        r.on(events::PAGE_TURNED, |env, reading, _| {
            page_turned(env, reading)
        })?;
        r.on(ENTERED, |_, typed, out| entered(typed, out))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// A row of the set-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Server,
    User,
    Password,
    Client,
    Secret,
    Fetch,
}

const ROWS: [Row; 6] = [
    Row::Server,
    Row::User,
    Row::Password,
    Row::Client,
    Row::Secret,
    Row::Fetch,
];

impl Row {
    fn title(self) -> &'static str {
        match self {
            Row::Server => "wallabag.server",
            Row::User => "wallabag.user",
            Row::Password => "wallabag.password",
            Row::Client => "wallabag.client-id",
            Row::Secret => "wallabag.client-secret",
            Row::Fetch => "wallabag.fetch",
        }
    }

    fn field(self) -> &'static str {
        match self {
            Row::Server => "server",
            Row::User => "user",
            Row::Password => "password",
            Row::Client => "client",
            Row::Secret => "secret",
            Row::Fetch => "",
        }
    }

    fn of(field: &str) -> Option<Self> {
        ROWS.into_iter()
            .find(|r| r.field() == field && !field.is_empty())
    }
}

/// Keeps what was typed for a row of the set-up.
fn entered(typed: &Typed, out: &mut Commands) {
    let Some(row) = typed.context.get(FIELD).and_then(Row::of) else {
        return;
    };
    let text = typed.text.trim();
    let setting = |id: Setting<Text>| Command::set(&id, Cow::Owned(text.to_string()));
    let secret = |id: &str, label: &str| Command::set_secret(SecretChange::keep(id, label, text));
    // An empty answer keeps what was kept: a confirm by mistake on the
    // empty field (a password is never shown in it) loses nothing.
    if text.is_empty() && matches!(row, Row::Password | Row::Secret) {
        return;
    }
    out.push(match row {
        Row::Server => setting(SERVER),
        Row::User => setting(USER),
        Row::Client => setting(CLIENT),
        Row::Password => secret(PASSWORD, "wallabag.secret.password"),
        Row::Secret => secret(SECRET, "wallabag.secret.client"),
        Row::Fetch => return,
    });
}

/// The set-up: the account's details and *Fetch articles*, with a note
/// below on how to make an API client, or how the last fetch went.
struct SetupScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    note: Rect,
    message: String,
    /// Offline mode: the offer to go online, and whether *Fetch articles*
    /// waits for it.
    online: ui::offline::OfflineOffer,
    waiting: bool,
}

impl SetupScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let height = style.row() * ROWS.len() as u16;
        let note_top = top + height + style.gap();
        Self {
            list: ListView::new(
                Rect::new(0, top, display.width, height),
                u32::from(style.row()),
            )
            .focus_on_keys(env),
            note: Rect::new(
                0,
                note_top,
                display.width,
                display.height.saturating_sub(note_top),
            ),
            message: env.i18n().tr("wallabag.help").to_string(),
            online: ui::offline::OfflineOffer::new(),
            waiting: false,
            style,
            header,
        }
    }

    /// Where the offer goes: from the rows' top to the bottom.
    fn offer_area(&self, env: &Env<'_>) -> Rect {
        let top = self.list.area().y;
        let display = env.device().display;
        Rect::new(0, top, display.width, display.height.saturating_sub(top))
    }

    /// Whether the offer to go online shows in place of the rows.
    fn offering(&self, env: &Env<'_>) -> bool {
        self.waiting && self.online.shows(env)
    }

    /// What the reader chose on the offer: the fetch starts, or is told
    /// to need the network.
    fn offered(&mut self, chosen: Option<ui::offline::Offer>, cx: &mut Cx<'_, '_>) {
        let Some(chosen) = chosen else {
            return;
        };
        self.waiting = false;
        match chosen {
            ui::offline::Offer::Online => cx.command(Command::run(FETCH)),
            _ => self.message = cx.env().i18n().tr("wallabag.no-network").to_string(),
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn activate(&mut self, row: Row, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if row == Row::Fetch {
            // Offline mode: asked first, once, where a fetch could start.
            let set_up =
                env.secrets().get(PASSWORD).is_some() && env.secrets().get(SECRET).is_some();
            if env.caps().network && set_up && self.online.shows(&env) {
                self.waiting = true;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            } else {
                cx.command(Command::run(FETCH));
            }
            return;
        }
        let (text, password) = match row {
            Row::Server => (env.settings().read(&SERVER), false),
            Row::User => (env.settings().read(&USER), false),
            Row::Client => (env.settings().read(&CLIENT), false),
            _ => ("", true),
        };
        let question = paperpaw_sdk::ask::text(env.i18n().tr(row.title()), &ENTERED)
            .initial(text)
            .with(FIELD, row.field());
        let question = match row {
            _ if password => question.password(),
            Row::Server => question.url(),
            _ => question,
        };
        cx.command(question.command());
    }
}

impl Screen for SetupScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let title = env.i18n().tr("wallabag.title");
        self.header.draw(&mut ui, title, None, None);
        if self.offering(env) {
            return self.online.draw(&mut ui, self.offer_area(env));
        }
        self.list
            .draw(&mut ui, ROWS.len(), |ui, index, rect, focused| {
                let row = ROWS[index];
                let kept = |id: &str| {
                    if env.secrets().get(id).is_some() {
                        "••••••"
                    } else {
                        ""
                    }
                };
                let value = match row {
                    Row::Server => {
                        let url = server(env);
                        url.strip_prefix("https://").unwrap_or(&url).to_string()
                    }
                    Row::User => env.settings().read(&USER).to_string(),
                    Row::Client => env.settings().read(&CLIENT).to_string(),
                    Row::Password => kept(PASSWORD).to_string(),
                    Row::Secret => kept(SECRET).to_string(),
                    Row::Fetch => String::from("›"),
                };
                let title = env.i18n().tr(row.title());
                ui::Row::value(title, &value)
                    .focused(focused)
                    .draw(ui, rect);
            });
        ui.message(self.note, &self.message);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if *a != actions::BACK && self.offering(cx.env()) {
            let chosen = self.online.on_action(a, cx).done();
            self.offered(chosen, cx);
            return Handled::Yes;
        }
        let delta = if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            -1
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            1
        } else if *a == actions::CONFIRM {
            self.activate(ROWS[self.list.focus().min(ROWS.len() - 1)], cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list
            .move_focus_in(cx.env(), delta, ROWS.len())
            .request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.offering(cx.env()) {
            let chosen = self.online.on_gesture(&gesture, cx).done();
            self.offered(chosen, cx);
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, ROWS.len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        cx.request_redraw_area(self.list.area());
        self.activate(ROWS[index], cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, p: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(finished) = DONE.read(event, p) {
            self.message = finished.text;
        } else if let Some(typed) = ENTERED.read(event, p) {
            // On to the next row.
            if let Some(row) = typed.context.get(FIELD).and_then(Row::of) {
                let at = ROWS.iter().position(|r| *r == row).unwrap_or(0);
                self.list.set_focus((at + 1).min(ROWS.len() - 1));
            }
        } else {
            return;
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn reads_the_unread_articles() {
        let list = r#"{"page": 1, "_embedded": {"items": [
            {"id": 7, "title": "Slow \"news\"", "tags": [{"id": 1, "title": "tag"}]},
            {"title": "Second", "id": 9, "_links": {"self": {"href": "/api/entries/9"}}}
        ]}, "_links": {"self": {"href": "x"}}}"#;
        let storage = paperpaw_sdk::MemoryStorage::new().with(&data("entries.json"), list);
        assert_eq!(
            unread(&storage).unwrap(),
            [
                (String::from("7"), String::from("Slow \"news\"")),
                (String::from("9"), String::from("Second")),
            ]
        );
        let empty = r#"{"page": 1, "_embedded": {"items": []}}"#;
        let storage = paperpaw_sdk::MemoryStorage::new().with(&data("entries.json"), empty);
        assert_eq!(unread(&storage), Some(Vec::new()));
    }

    /// Answers that are not a whole list are no list at all, not an empty
    /// one (which would have every book removed).
    #[test]
    fn only_a_whole_list_counts() {
        for answer in [
            "<!DOCTYPE html><html><body>Down for maintenance</body></html>",
            r#"{"error": "server_error", "error_description": "Try again later"}"#,
            r#"{"page": 1, "_embedded": {"items": [{"id": 9, "title": "Newer"}, {"id": 7"#,
            "",
        ] {
            let storage = paperpaw_sdk::MemoryStorage::new().with(&data("entries.json"), answer);
            assert_eq!(unread(&storage), None, "{answer}");
        }
        assert_eq!(unread(&paperpaw_sdk::MemoryStorage::new()), None);
    }

    #[test]
    fn archived_articles_by_their_answer() {
        let answer = |body: &str| {
            let storage = paperpaw_sdk::MemoryStorage::new().with(&data("entry.json"), body);
            is_archived(&storage, "7")
        };
        let content = "x".repeat(20_000);
        assert_eq!(
            answer(&format!(
                r#"{{"is_archived": 1, "content": "{content}", "id": 7}}"#
            )),
            Some(true)
        );
        assert_eq!(answer(r#"{"id": 7, "is_archived": true}"#), Some(true));
        assert_eq!(answer(r#"{"id": 7, "is_archived": 0}"#), Some(false));
        // Another article, a nested field, or no answer of that kind.
        assert_eq!(answer(r#"{"id": 8, "is_archived": 1}"#), None);
        assert_eq!(answer(r#"{"id": 7, "x": {"is_archived": 1}}"#), None);
        assert_eq!(answer("<html>Login</html>"), None);
    }

    /// Every setting is explained, in each language.
    #[test]
    fn settings_have_help() {
        for ini in [
            include_str!("../i18n/en.ini"),
            include_str!("../i18n/de.ini"),
        ] {
            let catalog = Catalog::parse(ini).unwrap();
            for spec in &SETTINGS {
                let help = spec.description.as_deref();
                assert!(
                    help.is_some_and(|key| catalog.get(key).is_some()),
                    "{}",
                    spec.id.as_str()
                );
            }
        }
    }

    /// Lists that cannot be read are not taken for empty ones, which the
    /// next change would write over.
    #[test]
    fn unreadable_lists_are_not_empty() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with(
                &data("articles.txt"),
                b"7\tWallabag/A.epub\n\xff\n".as_slice(),
            )
            .with(&data("finished.txt"), "1\n".repeat(10_000));
        assert!(articles(&storage).is_err());
        assert!(finished(&storage).is_err());
        let none = paperpaw_sdk::MemoryStorage::new();
        assert_eq!(articles(&none), Ok(Vec::new()));
        assert_eq!(finished(&none), Ok(Vec::new()));
    }

    /// A hostile server's ids do not plant lines in the list of articles
    /// (which would remove any file at the next sync).
    #[test]
    fn ids_are_numbers() {
        let list = r#"{"_embedded": {"items": [
            {"id": "x\t.paperpaw/secrets.txt\ny", "title": "Planted"},
            {"id": "../../x", "title": "Out"},
            {"id": "", "title": "None"},
            {"id": 12, "title": "Fine"}
        ]}}"#;
        let storage = paperpaw_sdk::MemoryStorage::new().with(&data("entries.json"), list);
        assert_eq!(
            unread(&storage).unwrap(),
            [(String::from("12"), String::from("Fine"))]
        );
    }

    #[test]
    fn rows_by_field() {
        for row in ROWS {
            if row != Row::Fetch {
                assert_eq!(Row::of(row.field()), Some(row));
            }
        }
        assert_eq!(Row::of(""), None);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn lists_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with(
                &super::data("articles.txt"),
                include_str!("../testdata/articles.txt.v1"),
            )
            .with(
                &super::data("finished.txt"),
                include_str!("../testdata/finished.txt.v1"),
            );
        let articles = super::articles(&storage).unwrap();
        assert_eq!(articles.len(), 2);
        assert_eq!(articles[0].id, "123");
        assert_eq!(articles[1].path, "Wallabag/456 E-ink at night.epub");
        assert_eq!(super::finished(&storage).unwrap(), ["123"]);
    }
}

#[cfg(test)]
mod list_tests;
