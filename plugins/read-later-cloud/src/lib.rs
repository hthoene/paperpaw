//! Readwise Reader: the articles saved to read later, fetched as books
//! into the `Reader` folder of the library, and archived in Reader once
//! read.
//!
//! *Readwise Reader* in the tools asks for the access token
//! (readwise.io/access_token, the same one *Readwise* uses; typing it can
//! be done on a phone too, see the keyboard) and fetches: each article in
//! the place chosen (*Later*, or the *Inbox*) comes as an HTML book of
//! its text (Reader gives no pictures in it). Articles read to the end are
//! archived in Reader with the next fetch and their books removed, as are
//! those Reader says are archived or deleted elsewhere; a book with the
//! reader's highlights, notes or bookmarks stays in the library, let go
//! of by Reader. The token is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`).
//!
//! Answers are saved to storage and read as they stream; the text of an
//! article is copied from the answer to its book in small pieces (see
//! [`html`]), so this works on small devices too. Reader allows twenty
//! requests a minute for lists: a fetch asks for at most nineteen and
//! stops where it is told to wait (the next fetch goes on). Shipped
//! switched off: switch it on in the plugins.

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

mod answers;
pub mod html;
pub mod native_answers;
#[cfg(test)]
mod native_answers_tests;
pub mod native_fetch;
#[cfg(test)]
mod native_fetch_tests;
pub mod native_lists;
#[cfg(test)]
mod native_parity_tests;
pub mod native_read_later;
pub mod native_secret;
mod screen;
#[cfg(test)]
mod test_volume;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, groups, menus, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::web::clean_name;
use paperpaw_sdk::{
    ActionSpec, Arg, Command, Commands, Env, FixedAction, FixedScreen, HttpRequest, Id, Manifest,
    MenuEntry, Params, Plugin, RegisterError, Registrar, SettingSpec, Storage, StorageError,
};

use answers::Answer;

pub use screen::{ENTERED, FIELD};

pub const PLUGIN: Id = Id::new_static("read-later-cloud");
/// Opens the set-up.
pub const OPEN: Id = Id::new_static("read-later-cloud.open");
/// Fetches the articles.
pub const FETCH: Id = Id::new_static("read-later-cloud.fetch");
/// Declared work: a fetch no reader waits for (a rule's).
const FETCH_DUE: Event = Event::new("read-later-cloud.fetch-due");
/// Where the articles are fetched from: [`FROM_LATER`] or [`FROM_INBOX`]
/// (each its name's key).
pub const FROM: Setting<Text> = Setting::new("read-later-cloud.from");
pub const FROM_LATER: &str = "read-later-cloud.from.later";
pub const FROM_INBOX: &str = "read-later-cloud.from.inbox";
/// The access token, among the passwords and keys.
pub const SECRET: &str = "read-later-cloud.token";
/// What tells the accounts apart, kept beside the token among the
/// passwords and keys (nothing derived from the token is left on the
/// card): a number made of the token the lists belong to.
pub const ACCOUNT: &str = "read-later-cloud.account";
const SCREEN: Id = Id::new_static("read-later-cloud.setup");
/// Answers: the list, an article asked about, a book, an archived one.
const LISTED: Event<HttpReply> = Event::new("read-later-cloud.listed");
const CHECKED: Event<HttpReply> = Event::new("read-later-cloud.checked");
const DOWNLOADED: Event<HttpReply> = Event::new("read-later-cloud.downloaded");
const ARCHIVED: Event<HttpReply> = Event::new("read-later-cloud.archived");
/// Emitted with a message (`text`) when a fetch ends.
const DONE: Event<Finished> = Event::new("read-later-cloud.done");

paperpaw_sdk::event_data! {
    /// How a fetch ended ([`DONE`]), in words.
    struct Finished {
        text: String,
    }
}
const ARTICLE: &str = "article";
const FOLDER: &str = "Reader";
const SERVER: &str = "https://readwise.io";
/// Articles asked for in the list. Reader answers twenty list requests a
/// minute (an article's text is one): the list, up to [`MAX_CHECKS`]
/// articles asked about and up to [`MAX_DOWNLOADS`] books come to nineteen.
const MAX_LISTED: usize = 20;
const MAX_CHECKS: usize = 8;
const MAX_DOWNLOADS: usize = 10;
/// Reading progress, in percent, from which an article counts as read
/// where its pages are not counted yet.
const READ: f64 = 95.0;
/// The most a saved answer may hold, in bytes: a list or a question about
/// an article is small; an article's text is cut at [`html::MAX_TEXT`],
/// and escaping it makes it a little longer. The kernel saves up to 64 MiB
/// to a file; what is more than this is removed at once.
const MAX_LIST: u64 = 512 * 1024;
const MAX_ARTICLE_ANSWER: u64 = 4 * 1024 * 1024;

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

/// The folder of the articles: `Reader` in the library.
fn folder(env: &Env<'_>) -> String {
    let library = paperpaw_sdk::library::folder(env);
    if library.is_empty() {
        String::from(FOLDER)
    } else {
        format!("{library}/{FOLDER}")
    }
}

/// Where Reader keeps what is asked for: its `location`.
fn location(env: &Env<'_>) -> &'static str {
    if env.settings().text(FROM.as_str()) == FROM_INBOX {
        "new"
    } else {
        "later"
    }
}

/// An article fetched: its id in Reader and its book.
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

/// Whose the articles fetched are: a number made of the token (the
/// token itself is kept with the secrets).
fn account(token: &str) -> String {
    format!("{:016x}", paperpaw_sdk::digest::fnv1a64(token))
}

/// The articles fetched and those read to the end, for this token. Those
/// of another token (another account) are let go of first: their books
/// stay in the library, never asked about at a Reader that does not know
/// them. The token an account is told by is noted among the secrets, as
/// this says (`out`).
fn lists_of_account(
    env: &Env<'_>,
    token: &str,
    out: &mut Commands,
) -> Result<(Vec<Article>, Vec<String>), StorageError> {
    let (fetched, finished) = (articles(env.storage())?, finished(env.storage())?);
    let account = account(token);
    if env.secrets().get(ACCOUNT) == Some(account.as_str()) {
        return Ok((fetched, finished));
    }
    if !fetched.is_empty() || !finished.is_empty() {
        save_articles(env, &[])?;
        env.storage().write(&data("finished.txt"), b"")?;
    }
    out.push(Command::set_secret(
        SecretChange::keep(ACCOUNT, "read-later-cloud.secret.account", &account).changeable(false),
    ));
    Ok((Vec::new(), Vec::new()))
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
    let plain = format!("{dir}/{name}.html");
    if !storage.exists(&plain) && !listed(&plain) {
        return Some(plain);
    }
    let path = paperpaw_sdk::files::free_path(storage, dir, &format!("{name} ({id}).html"));
    (!listed(&path)).then_some(path)
}

/// `text` as HTML text.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// The start of an article's book: its title and author. Its text goes
/// after, then [`TAIL`].
fn head(title: &str, author: &str) -> String {
    let (title, author) = (escaped(title), escaped(author));
    let by = if author.is_empty() {
        String::new()
    } else {
        format!("<p>{author}</p>")
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body><h1>{title}</h1>{by}"
    )
}

const TAIL: &str = "</body></html>";

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
        HttpRequest::new(method, &format!("{SERVER}{path}"), reply)
            .header(
                String::from("Authorization"),
                format!("Token {}", self.token),
            )
            .header(agent, name)
    }

    /// Starts a fetch.
    fn start(&mut self, env: &Env<'_>, out: &mut Commands) {
        if self.running {
            return;
        }
        if !env.caps().network {
            return done(env, "read-later-cloud.no-network", &[], out);
        }
        let Some(token) = env.secrets().get(SECRET).filter(|t| !t.is_empty()) else {
            return done(env, "read-later-cloud.setup-needed", &[], out);
        };
        let Ok((fetched, finished)) = lists_of_account(env, token, out) else {
            return done(env, "read-later-cloud.unreadable", &[], out);
        };
        *self = Self {
            running: true,
            token: token.to_string(),
            archive: finished
                .into_iter()
                .filter(|id| fetched.iter().any(|a| a.id == *id))
                .collect(),
            ..Self::default()
        };
        out.push(Command::Notify(
            env.i18n().tr("read-later-cloud.fetching").to_string(),
        ));
        self.next(env, out);
    }

    /// Ends the fetch where Reader cannot be reached, does not know the
    /// token or asks to wait: asking on would not help.
    fn stopped(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) -> bool {
        let key = match reply.status {
            0 => "read-later-cloud.unreachable",
            401 | 403 => "read-later-cloud.bad-token",
            429 => "read-later-cloud.wait",
            _ => return false,
        };
        self.finish(env);
        done(env, key, &[], out);
        true
    }

    /// Ends the fetch where a saved answer is larger than `most` bytes:
    /// the file is removed, and nothing more is asked of a Reader that
    /// sends so much.
    fn too_big(
        &mut self,
        env: &Env<'_>,
        reply: &HttpReply,
        file: &str,
        most: u64,
        out: &mut Commands,
    ) -> bool {
        let over = reply.ok()
            && env
                .storage()
                .size(&data(file))
                .is_ok_and(|size| size > most);
        if !over {
            return false;
        }
        self.finish(env);
        done(env, "read-later-cloud.too-big", &[], out);
        true
    }

    fn finish(&mut self, env: &Env<'_>) {
        self.running = false;
        for name in ["list.json", "doc.json", "article.tmp"] {
            let _ = env.storage().remove(&data(name));
        }
    }

    /// The next step: archive, list, ask about articles, download, or
    /// done.
    fn next(&mut self, env: &Env<'_>, out: &mut Commands) {
        if let Some(id) = self.archive.pop() {
            let request = self
                .request(env, "PATCH", &format!("/api/v3/update/{id}/"), &ARCHIVED)
                .header(
                    String::from("Content-Type"),
                    String::from("application/json"),
                )
                .body(String::from(r#"{"location":"archive"}"#))
                .with(Params::new().with(ARTICLE, id));
            out.push(Command::http(request));
            return;
        }
        if !self.listed {
            let path = format!(
                "/api/v3/list/?location={}&category=article&limit={MAX_LISTED}",
                location(env)
            );
            let request = self
                .request(env, "GET", &path, &LISTED)
                .save_to(data("list.json"))
                .max_file(MAX_LIST);
            out.push(Command::http(request));
            return;
        }
        if let Some(id) = self.checks.pop() {
            let request = self
                .request(env, "GET", &format!("/api/v3/list/?id={id}"), &CHECKED)
                .save_to(data("doc.json"))
                .max_file(MAX_ARTICLE_ANSWER)
                .with(Params::new().with(ARTICLE, id));
            out.push(Command::http(request));
            return;
        }
        if let Some((id, title)) = self.downloads.pop() {
            let request = self
                .request(
                    env,
                    "GET",
                    &format!("/api/v3/list/?id={id}&withHtmlContent=true"),
                    &DOWNLOADED,
                )
                .save_to(data("doc.json"))
                .max_file(MAX_ARTICLE_ANSWER)
                .with(Params::new().with(ARTICLE, id).with(params::TITLE, title));
            out.push(Command::http(request));
            return;
        }
        self.finish(env);
        let key = if self.failed > 0 {
            "read-later-cloud.done-failed"
        } else {
            "read-later-cloud.done"
        };
        let args = [
            ("fetched", Arg::Int(self.fetched as i64)),
            ("archived", Arg::Int(self.archived as i64)),
            ("failed", Arg::Int(self.failed as i64)),
        ];
        done(env, key, &args, out);
    }

    fn archived(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if self.stopped(env, reply, out) {
            return;
        }
        let id = reply.context.get(ARTICLE).unwrap_or_default();
        // Not there any more (404): as good as archived.
        if reply.ok() || reply.status == 404 {
            self.archived += 1;
            remove_article(env, id, out);
        } else {
            self.failed += 1;
        }
        self.next(env, out);
    }

    fn listed(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if self.stopped(env, reply, out) {
            return;
        }
        if self.too_big(env, reply, "list.json", MAX_LIST, out) {
            return;
        }
        self.listed = true;
        // Only a whole list counts: an answer that is not one (a page of
        // the server's own, an error, a list cut short) changes nothing.
        let unread = reply
            .ok()
            .then(|| answers::listed(env.storage(), &data("list.json"), MAX_LISTED))
            .flatten();
        let (Some(unread), Ok(fetched)) = (unread, articles(env.storage())) else {
            self.failed += 1;
            return self.next(env, out);
        };
        // Those not in the list may be archived or deleted elsewhere, or
        // just older than the newest: each is asked about before its book
        // goes. The oldest fetched are asked about first, so that none
        // waits for ever.
        self.checks = fetched
            .iter()
            .filter(|a| !unread.iter().any(|(id, _)| *id == a.id))
            .map(|a| a.id.clone())
            .take(MAX_CHECKS)
            .collect();
        self.checks.reverse();
        // The list is newest first: popped from the end, the oldest of
        // the newest ten come first.
        self.downloads = unread
            .into_iter()
            .filter(|(id, _)| !fetched.iter().any(|a| a.id == *id))
            .take(MAX_DOWNLOADS)
            .collect();
        self.next(env, out);
    }

    /// An article fetched but not among the newest in its place: its book
    /// goes only when Reader says it is archived or no longer there.
    fn checked(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if self.stopped(env, reply, out) {
            return;
        }
        if self.too_big(env, reply, "doc.json", MAX_LIST, out) {
            return;
        }
        let id = reply.context.get(ARTICLE).unwrap_or_default();
        let answer = reply
            .ok()
            .then(|| answers::asked(env.storage(), &data("doc.json"), id))
            .flatten();
        match answer {
            Some(Answer::Gone) => remove_article(env, id, out),
            Some(Answer::Known { location, .. }) if location == "archive" => {
                remove_article(env, id, out)
            }
            Some(Answer::Known { .. }) => {}
            None => self.failed += 1,
        }
        let _ = env.storage().remove(&data("doc.json"));
        self.next(env, out);
    }

    fn downloaded(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if self.stopped(env, reply, out) {
            return;
        }
        if self.too_big(env, reply, "doc.json", MAX_ARTICLE_ANSWER, out) {
            return;
        }
        let id = reply.context.get(ARTICLE).unwrap_or_default();
        let title = reply.context.get(params::TITLE).unwrap_or_default();
        let saved = reply.ok().then(|| make_book(env, id, title, out)).flatten();
        if saved.is_some() {
            self.fetched += 1;
        } else {
            self.failed += 1;
        }
        let _ = env.storage().remove(&data("article.tmp"));
        let _ = env.storage().remove(&data("doc.json"));
        self.next(env, out);
    }
}

/// Makes the book of article `id` from the answer in `doc.json`, and
/// notes it. `None` where that fails (nothing is left of it).
fn make_book(env: &Env<'_>, id: &str, title: &str, out: &mut Commands) -> Option<()> {
    // The id goes into a file's name and a list: letters and digits only.
    if !answers::is_id(id) {
        return None;
    }
    let storage = env.storage();
    let doc = data("doc.json");
    let tmp = data("article.tmp");
    let Some(Answer::Known { author, .. }) = answers::asked(storage, &doc, id) else {
        return None;
    };
    storage.write(&tmp, head(title, &author).as_bytes()).ok()?;
    if html::copy_article(storage, &doc, &tmp).ok()? != html::Copied::Whole {
        return None;
    }
    storage.append(&tmp, TAIL.as_bytes()).ok()?;
    let mut list = articles(storage).ok()?;
    let dir = folder(env);
    let _ = storage.create_dir(&dir);
    let name = clean_name(title, 80).unwrap_or_else(|| id.to_string());
    let path = book_path(storage, &dir, &name, id, &list)?;
    storage.rename(&tmp, &path).ok()?;
    out.push(paperpaw_sdk::library::added(&path, PLUGIN.as_str()));
    list.push(Article {
        id: id.to_string(),
        path,
    });
    // Where it is not noted, the book stays as any other.
    let _ = save_articles(env, &list);
    Some(())
}

/// Removes an article's book and what is noted about it (nothing from a
/// list that cannot be read); the book's removal is told. A book with the
/// reader's highlights, notes or bookmarks stays, let go of.
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

/// Tells how a fetch ended, and the screen of the set-up too.
fn done(env: &Env<'_>, key: &str, args: &[(&str, Arg<'_>)], out: &mut Commands) {
    let text = env.i18n().format(key, args);
    out.push(Command::Notify(text.clone()));
    out.push(DONE.emit(Finished { text }));
}

/// The Readwise Reader plugin.
#[derive(Debug, Default)]
pub struct ReadLaterCloud;

static SETTINGS: [SettingSpec; 1] = [SettingSpec::choice(
    FROM.static_str(),
    "read-later-cloud.from",
    &[FROM_LATER, FROM_INBOX],
    FROM_LATER,
    groups::LIBRARY,
)
.kept_on_reset()
.description("read-later-cloud.from.help")];

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "read-later-cloud.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(49)];
static ICONS: [(Id, &str); 2] = [(OPEN, "article"), (FETCH, "refresh")];

static SCREENS: [FixedScreen; 1] =
    [
        FixedScreen::new(SCREEN, "read-later-cloud.title", |env, _| {
            Box::new(screen::SetupScreen::new(env))
        })
        .instances(paperpaw_sdk::Instances::SINGLE),
    ];

impl Plugin for ReadLaterCloud {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Read-later articles (cloud)",
            paperpaw_sdk::package_version!(),
        )
        .description("Fetches articles saved in Readwise Reader.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::default();
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        let s = state.clone();
        r.add_action(ActionSpec::new(FETCH, "read-later-cloud.fetch").handler(
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
        let steps: [(Event<HttpReply>, Step); 4] = [
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
        r.on(ENTERED, |_, typed, out| screen::entered(typed, out))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    use super::*;

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

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
                for choice in [FROM_LATER, FROM_INBOX] {
                    assert!(catalog.get(choice).is_some(), "{choice}");
                }
            }
        }
    }

    #[test]
    fn titles_and_authors_are_text() {
        assert_eq!(
            head("A <b> & C", ""),
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>A &lt;b&gt; &amp; C</title></head><body><h1>A &lt;b&gt; &amp; C</h1>"
        );
        assert!(head("T", "J <x>").ends_with("<p>J &lt;x&gt;</p>"));
    }

    #[test]
    fn unreadable_lists_are_not_empty() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with(
                &data("articles.txt"),
                b"a1\tReader/A.html\n\xff\n".as_slice(),
            )
            .with(&data("finished.txt"), "1\n".repeat(10_000));
        assert!(articles(&storage).is_err());
        assert!(finished(&storage).is_err());
        let none = paperpaw_sdk::MemoryStorage::new();
        assert_eq!(articles(&none), Ok(Vec::new()));
        assert_eq!(finished(&none), Ok(Vec::new()));
    }
}
