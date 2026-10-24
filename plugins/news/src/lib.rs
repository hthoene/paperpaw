//! News: feeds (RSS or Atom) fetched as books to read, one a feed, in the
//! `News` folder of the library.
//!
//! *News* in the main menu lists the feeds: *Fetch the news* fetches all
//! of them, a feed is opened with a tap, and holding it offers to fetch
//! it alone or to remove it; *Add a feed* asks for its address (which
//! can come from a phone too, see the keyboard). Each feed becomes a page
//! with its newest articles as plain text (see `convert`), replaced by
//! the next fetch. Feeds are fetched one after the other and each is
//! read as it streams from storage, so this works on small devices too.

#![no_std]

extern crate alloc;

mod convert;
mod widget;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, groups, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{HttpReply, Typed};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::web::{clean_name, host};
use paperpaw_sdk::{
    ActionSpec, Arg, Canvas, Command, Commands, Cx, Env, Gesture, GestureKind, Handled,
    HttpRequest, Id, Manifest, MemoryClass, MenuEntry, Params, Plugin, Rect, RefreshMode,
    RegisterError, Registrar, Screen, ScreenSpec, SettingSpec, Storage, StorageError,
};

pub const PLUGIN: Id = Id::new_static("news");
/// Opens the list of feeds.
pub const OPEN: Id = Id::new_static("news.open");
/// Fetches all feeds.
pub const FETCH: Id = Id::new_static("news.fetch");
/// Articles kept of each feed.
pub const ARTICLES: Setting<i64> = Setting::new("news.articles");
const SCREEN: Id = Id::new_static("news.list");
/// Declared work: a fetch no reader waits for (a rule's).
const FETCH_DUE: Event = Event::new("news.fetch-due");
const CHOSEN: Event<choice::Picked> = Event::new("news.chosen");
/// A feed's answer.
const FETCHED: Event<HttpReply> = Event::new("news.fetched");
/// The address of a new feed, from the keyboard.
const ADDED: Event<Typed> = Event::new("news.added");
const OPTION_LABELS: &str = "news.option.";
const FOLDER: &str = "News";
/// Parameters of a fetch: the feed being fetched and where it was in the
/// list, whether the feeds after it follow, and how many were fetched
/// and failed so far.
const URL: &str = "url";
const INDEX: &str = "index";
const ALL: &str = "all";
const DONE: &str = "done";
const FAILED: &str = "failed";
/// Text kept of an article: little on small devices.
const MAX_TEXT_SMALL: usize = 8 * 1024;
const MAX_TEXT: usize = 64 * 1024;

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

/// A short mark of a feed's address, six hex digits (24 bits of its
/// FNV-1a hash): feeds of the same name keep books of their own.
fn mark(url: &str) -> String {
    let hash = url.bytes().fold(0x811c_9dc5_u32, |hash, b| {
        (hash ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    format!("{:06x}", hash & 0x00ff_ffff)
}

/// A book's name without the mark of its feed's address
/// (`Tagesschau (3fa9c2)` is "Tagesschau").
pub(crate) fn without_mark(name: &str) -> &str {
    match name.rsplit_once(" (") {
        Some((name, rest))
            if rest.len() == 7
                && rest.ends_with(')')
                && rest[..6].bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            name
        }
        _ => name,
    }
}

/// Where a feed's answer is kept until it is read: a file of its own, as
/// two fetches may be under way at once.
fn download(url: &str) -> String {
    data(&format!("feed-{}.tmp", mark(url)))
}

/// A feed: its address and its name (the feed's own title once fetched).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Feed {
    url: String,
    name: String,
}

impl Feed {
    fn shown(&self) -> String {
        if self.name.is_empty() {
            host(&self.url).trim_start_matches("www.").to_string()
        } else {
            self.name.clone()
        }
    }

    fn file_name(&self) -> String {
        clean_name(&self.shown(), 60).unwrap_or_else(|| String::from("News"))
    }

    /// The book it becomes in `folder`: its name and the mark of its
    /// address (`News/Tagesschau (3fa9c2).html`).
    fn book(&self, folder: &str) -> String {
        format!("{folder}/{} ({}).html", self.file_name(), mark(&self.url))
    }

    /// Where version 1 kept its book: by its name alone, which feeds of
    /// the same name shared.
    fn old_book(&self, folder: &str) -> String {
        format!("{folder}/{}.html", self.file_name())
    }

    /// Its book, if it has one: as kept now, or as version 1 kept it.
    fn found_book(&self, storage: &dyn Storage, folder: &str) -> Option<String> {
        [self.book(folder), self.old_book(folder)]
            .into_iter()
            .find(|book| storage.exists(book))
    }
}

/// Removes the book version 1 kept for `feed`, once no other feed of
/// `feeds` still reads it (a feed of the same name not fetched since).
/// Only a page News made ([`convert::HEAD`]): a file of the reader's own
/// at that name stays.
fn retire_old_book(storage: &dyn Storage, folder: &str, feeds: &[Feed], feed: &Feed) {
    let old = feed.old_book(folder);
    let needed = feeds.iter().any(|other| {
        other.url != feed.url
            && other.old_book(folder) == old
            && !storage.exists(&other.book(folder))
    });
    let mut start = [0u8; convert::HEAD.len()];
    let made_here = matches!(storage.read_at(&old, 0, &mut start), Ok(n) if n == start.len())
        && start == *convert::HEAD.as_bytes();
    if !needed && made_here {
        let _ = storage.remove(&old);
    }
}

/// A feed's title without its tagline ("Site - all the news" gives "Site").
fn short_title(title: &str) -> String {
    for separator in [" - ", " | ", " \u{2013} ", " \u{2014} ", ": "] {
        if let Some((name, _)) = title.split_once(separator)
            && name.trim().chars().count() >= 3
        {
            return name.trim().to_string();
        }
    }
    title.trim().to_string()
}

/// The folder of the news: `News` in the library.
fn folder(env: &Env<'_>) -> String {
    let library = paperpaw_sdk::library::folder(env);
    if library.is_empty() {
        String::from(FOLDER)
    } else {
        format!("{library}/{FOLDER}")
    }
}

/// The feeds; an error when their list cannot be read.
fn load(storage: &dyn Storage) -> Result<Vec<Feed>, StorageError> {
    let text = load_for_update(storage, "feeds.txt", 64 * 1024)?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let (url, name) = line.split_once('\t').unwrap_or((line, ""));
            (!url.trim().is_empty()).then(|| Feed {
                url: url.trim().to_string(),
                name: name.trim().to_string(),
            })
        })
        .collect())
}

fn save(storage: &dyn Storage, feeds: &[Feed]) {
    let text: String = feeds
        .iter()
        .map(|f| format!("{}\t{}\n", f.url, f.name.replace(['\t', '\n'], " ")))
        .collect();
    let _ = storage.write(&data("feeds.txt"), text.as_bytes());
}

/// Asks for `feed`, at `index` in the list, and with `all` for the feeds
/// after it too.
fn request(
    env: &Env<'_>,
    feed: &Feed,
    index: usize,
    all: bool,
    (done, failed): (usize, usize),
) -> Command {
    let (agent, name) = paperpaw_sdk::web::user_agent_of(env);
    let mut request = HttpRequest::new("GET", &feed.url, FETCHED)
        .header(agent, name)
        .save_to(download(&feed.url));
    let mut params = Params::new()
        .with(URL, feed.url.clone())
        .with(INDEX, format!("{index}"))
        .with(DONE, format!("{done}"))
        .with(FAILED, format!("{failed}"));
    if all {
        params = params.with(ALL, "1");
    }
    request = request.with(params);
    Command::http(request)
}

/// Starts fetching the feed at `from`, and with `all` those after it.
fn fetch(env: &Env<'_>, from: usize, all: bool, out: &mut Commands) {
    if !env.caps().network {
        out.push(Command::Notify(
            env.i18n().tr("news.no-network").to_string(),
        ));
        return;
    }
    let Ok(feeds) = load(env.storage()) else {
        out.push(Command::Notify(
            env.i18n().tr("news.unreadable").to_string(),
        ));
        return;
    };
    match feeds.get(from) {
        Some(feed) => {
            out.push(Command::Notify(env.i18n().tr("news.fetching").to_string()));
            out.push(request(env, feed, from, all, (0, 0)));
        }
        None => out.push(Command::Notify(env.i18n().tr("news.none").to_string())),
    }
}

/// A feed answered: made into its book, then the next one. Feeds are
/// found by their address, as the list may change during a fetch.
fn fetched(env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
    let number = |key| reply.context.get(key).and_then(|n| n.parse::<usize>().ok());
    let (Some(url), Some(index)) = (reply.context.get(URL), number(INDEX)) else {
        return;
    };
    let mut done = number(DONE).unwrap_or(0);
    let mut failed = number(FAILED).unwrap_or(0);
    let Ok(mut feeds) = load(env.storage()) else {
        let _ = env.storage().remove(&download(url));
        out.push(Command::Notify(
            env.i18n().tr("news.unreadable").to_string(),
        ));
        return;
    };
    // A feed removed meanwhile is left out.
    let at = feeds.iter().position(|f| f.url == url);
    if let Some(at) = at {
        match make_book(env, reply, &feeds, at) {
            Ok(feed) => {
                done += 1;
                if feeds[at] != feed {
                    feeds[at] = feed;
                    save(env.storage(), &feeds);
                }
            }
            Err(Made::Failed) => failed += 1,
            // The rest would fail as well: stopped, with the reason.
            Err(Made::Memory) => {
                let _ = env.storage().remove(&download(url));
                out.push(Command::Notify(env.i18n().tr("news.no-memory").to_string()));
                return;
            }
        }
    }
    let _ = env.storage().remove(&download(url));
    // The next one: after this feed, or in its place if it was removed.
    let next = at.map_or(index, |at| at + 1);
    if reply.context.get(ALL).is_some()
        && let Some(feed) = feeds.get(next)
    {
        out.push(request(env, feed, next, true, (done, failed)));
        return;
    }
    let message = if failed == 0 {
        env.i18n().plural("news.fetched", done as i64, &[])
    } else {
        env.i18n().plural(
            "news.fetched-failed",
            failed as i64,
            &[("done", Arg::Int(done as i64))],
        )
    };
    out.push(Command::Notify(message));
}

/// Why a feed was not made into its book.
enum Made {
    Failed,
    /// The heap had no room to read it.
    Memory,
}

/// Makes the answer for `feeds[at]` its book: the feed, with the name it
/// may have brought; an error when that failed or the answer held no
/// article, as a Wi-Fi's sign-in page does: then the book there is stays.
fn make_book(env: &Env<'_>, reply: &HttpReply, feeds: &[Feed], at: usize) -> Result<Feed, Made> {
    if !reply.ok() {
        return Err(Made::Failed);
    }
    let max_text = if env.device().memory == MemoryClass::Small {
        MAX_TEXT_SMALL
    } else {
        MAX_TEXT
    };
    let articles = env.settings().read(&ARTICLES).clamp(1, 200) as usize;
    let folder = folder(env);
    let _ = env.storage().create_dir(&folder);
    let mut feed = feeds[at].clone();
    let page = data("page.tmp");
    let converted = convert::convert(
        env.storage(),
        &download(&feed.url),
        &page,
        articles,
        max_text,
        &|year, month, day| date_text(env, year, month, day),
    );
    let converted = match converted {
        Ok(converted) if converted.articles > 0 => converted,
        Err(convert::Failure::Memory) => {
            let _ = env.storage().remove(&page);
            return Err(Made::Memory);
        }
        _ => {
            let _ = env.storage().remove(&page);
            return Err(Made::Failed);
        }
    };
    if feed.name.is_empty() && !converted.title.is_empty() {
        feed.name = short_title(&converted.title);
    }
    let book = feed.book(&folder);
    if env.storage().rename(&page, &book).is_err() {
        let _ = env.storage().remove(&page);
        return Err(Made::Failed);
    }
    keep_headlines(env, &book, &converted.headlines);
    retire_old_book(env.storage(), &folder, feeds, &feed);
    Ok(feed)
}

/// A day as the reader's language writes it short, with the year:
/// `Sat 24 Oct 2026`.
fn date_text(env: &Env<'_>, year: i32, month: u8, day: u8) -> String {
    let i18n = env.i18n();
    let weekday = format!(
        "core.weekday.short.{}",
        paperpaw_sdk::time::weekday(year, month, day)
    );
    let month = format!("core.month.short.{month}");
    let short = i18n.format(
        "core.date.short",
        &[
            ("weekday", Arg::Str(i18n.tr(&weekday))),
            ("day", Arg::Int(i64::from(day))),
            ("month", Arg::Str(i18n.tr(&month))),
        ],
    );
    format!("{short} {year}")
}

/// Where the newest headlines are kept: `title<TAB>book` lines, the
/// feed fetched last first.
fn headlines_file() -> String {
    data("headlines.txt")
}

/// Keeps `titles` as the headlines of `book`, before the others'.
fn keep_headlines(env: &Env<'_>, book: &str, titles: &[String]) {
    let mut lines: Vec<String> = titles
        .iter()
        .map(|t| format!("{}\t{book}", t.replace(['\t', '\n'], " ")))
        .collect();
    if let Ok(old) = paperpaw_sdk::read_file(env.storage(), &headlines_file(), 32 * 1024) {
        for line in String::from_utf8_lossy(&old).lines() {
            if !line.ends_with(&format!("\t{book}")) && lines.len() < 40 {
                lines.push(line.to_string());
            }
        }
    }
    let _ = env
        .storage()
        .write(&headlines_file(), lines.join("\n").as_bytes());
}

/// The newest headlines and their books, at most `max`.
fn headlines(env: &Env<'_>, max: usize) -> Vec<(String, String)> {
    let Ok(data) = paperpaw_sdk::read_file(env.storage(), &headlines_file(), 32 * 1024) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&data)
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, book)| env.storage().exists(book))
        .take(max)
        .map(|(t, b)| (t.to_string(), b.to_string()))
        .collect()
}

/// The news plugin.
#[derive(Debug, Default)]
pub struct News;

/// How many articles a feed keeps, in steps of five.
static STEP_5: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().step(5);
static SETTINGS: [SettingSpec; 1] = [SettingSpec::number(
    ARTICLES.static_str(),
    "news.setting.articles",
    5,
    200,
    30,
    groups::LIBRARY,
)
.with_extra(&STEP_5)
.advanced()
.description("news.setting.articles.help")];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for News {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "News", paperpaw_sdk::package_version!())
            .description("Fetches news feeds (RSS, Atom) as books to read.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "news.title",
            Box::new(|env, _| Box::new(FeedsScreen::new(env))),
        ))?;
        r.add_action(ActionSpec::new(OPEN, "news.title").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
            },
        ))?;
        r.add_action(ActionSpec::new(FETCH, "news.fetch").handler(
            |env: &Env<'_>, _, out: &mut Commands| {
                if env.reader() {
                    fetch(env, 0, true, out);
                } else {
                    let due = FETCH_DUE.id().clone();
                    out.push(Command::work(paperpaw_sdk::work::Work::when_cheap(
                        due,
                        core::time::Duration::from_secs(3600),
                    )));
                }
            },
        ))?;
        r.on(FETCH_DUE, |env, (), out| fetch(env, 0, true, out))?;
        r.on(FETCHED, fetched)?;
        r.add_menu_entry(MenuEntry::new(menus::TOOLS, OPEN).order(44))?;
        r.add_icon(OPEN, Cow::Borrowed("rss"))?;
        r.add_icon(FETCH, Cow::Borrowed("refresh"))?;
        widget::register(r)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The feeds: *Fetch the news* first, the feeds, then *Add a feed*;
/// before there is a feed only *Add a feed*, and what feeds are.
struct FeedsScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    feeds: Vec<Feed>,
    /// Whether the list of feeds could be read: if not, it is not
    /// changed either.
    readable: bool,
}

impl FeedsScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let feeds = load(env.storage());
        // Rows as high as the help notes need (the largest text).
        let fetch = env
            .i18n()
            .format("news.fetch.help", &[("folder", Arg::Str(FOLDER))]);
        let notes = [fetch.as_str(), env.i18n().tr("news.add.help")];
        let row = style.noted_row_for(env, display.width, notes);
        Self {
            list: ListView::new(area, row).focus_on_keys(env),
            style,
            header,
            readable: feeds.is_ok(),
            feeds: feeds.unwrap_or_default(),
        }
    }

    fn len(&self) -> usize {
        if self.feeds.is_empty() {
            1
        } else {
            self.feeds.len() + 2
        }
    }

    /// Whether `row` is *Add a feed*, the last.
    fn is_add(&self, row: usize) -> bool {
        row + 1 == self.len()
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.is_add(row) {
            cx.command(
                ask::text(env.i18n().tr("news.address"), &ADDED)
                    .initial("https://")
                    .command(),
            );
            return;
        }
        if row == 0 {
            fetch_into(&env, 0, true, cx);
            return;
        }
        let index = row - 1;
        let Some(feed) = self.feeds.get(index) else {
            return;
        };
        if let Some(book) = feed.found_book(env.storage(), &folder(&env)) {
            cx.command(paperpaw_sdk::readers::open(book));
        } else {
            fetch_into(&env, index, false, cx);
        }
    }

    fn offer(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let Some(feed) = row.checked_sub(1).and_then(|i| self.feeds.get(i)) else {
            return;
        };
        cx.command(
            paperpaw_sdk::ask::choice(feed.shown(), &CHOSEN)
                .about(format!("{}", row - 1))
                .options(OPTION_LABELS, &["open", "fetch", "remove"])
                .command(),
        );
    }
}

/// The commands of a fetch, given to `cx`.
fn fetch_into(env: &Env<'_>, from: usize, all: bool, cx: &mut Cx<'_, '_>) {
    let mut out = paperpaw_sdk::Commands::new();
    fetch(env, from, all, &mut out);
    for command in out {
        cx.command(command);
    }
}

impl Screen for FeedsScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("news.title"),
            None,
            self.list.paging(len),
        );
        let feeds = &self.feeds;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let (title, value, note) = if row + 1 == len {
                let note = env.i18n().tr("news.add.help").to_string();
                (env.i18n().tr("news.add").to_string(), "›", note)
            } else if row == 0 {
                let note = env
                    .i18n()
                    .format("news.fetch.help", &[("folder", Arg::Str(FOLDER))]);
                (env.i18n().tr("news.fetch").to_string(), "", note)
            } else {
                let feed = &feeds[row - 1];
                (feed.shown(), "", feed.url.clone())
            };
            ui::Row::value(&title, value)
                .note(&note)
                .focused(focused)
                .draw(ui, rect);
        });
        if feeds.is_empty() {
            let area = self.list.area();
            let top = area.y + self.list.row() as u16;
            let below = Rect::new(
                area.x,
                top,
                area.width,
                area.bottom().saturating_sub(u32::from(top)) as u16,
            );
            ui.empty(
                below,
                "rss",
                env.i18n().tr("news.empty.title"),
                env.i18n().tr("news.empty"),
            );
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        let rows = self.list.rows() as isize;
        let a = action;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.open(self.list.focus(), cx);
            return Handled::Yes;
        } else if *a == actions::MENU {
            self.offer(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        let change = self.list.move_focus(delta, len);
        change.request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.len();
        if gesture.kind == GestureKind::Tap
            && let Some(action) =
                self.header
                    .tap(&env, &self.style, gesture.start, self.list.paging(len))
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(row) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(row);
        match gesture.kind {
            GestureKind::Tap => self.open(row, cx),
            GestureKind::LongPress => self.offer(row, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(typed) = ADDED.read(event, params) {
            let url = typed.text.trim().to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() < 10 {
                cx.command(Command::Notify(
                    env.i18n().tr("news.bad-address").to_string(),
                ));
                return;
            }
            if !self.readable {
                cx.command(Command::Notify(
                    env.i18n().tr("news.unreadable").to_string(),
                ));
                return;
            }
            let index = match self.feeds.iter().position(|f| f.url == url) {
                Some(index) => index,
                None => {
                    self.feeds.push(Feed {
                        url,
                        name: String::new(),
                    });
                    save(env.storage(), &self.feeds);
                    self.feeds.len() - 1
                }
            };
            fetch_into(&env, index, false, cx);
            self.redraw(cx);
        } else if let Some(picked) = CHOSEN.read(event, params) {
            let Ok(index) = picked.path.parse::<usize>() else {
                return;
            };
            match picked.option.as_str() {
                "open" => self.open(index + 1, cx),
                "fetch" => fetch_into(&env, index, false, cx),
                "remove" if !self.readable => {
                    cx.command(Command::Notify(
                        env.i18n().tr("news.unreadable").to_string(),
                    ));
                }
                "remove" if index < self.feeds.len() => {
                    // Its book stays, a book like any other now: the
                    // reader may have notes in it, and deletes it as any.
                    self.feeds.remove(index);
                    save(env.storage(), &self.feeds);
                    self.list.set_len(self.len());
                    self.redraw(cx);
                }
                _ => {}
            }
        } else if *event == FETCHED {
            // Names may have come with the feeds.
            if let Ok(feeds) = load(env.storage()) {
                self.feeds = feeds;
                self.readable = true;
            }
            self.redraw(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn short_titles() {
        assert_eq!(
            short_title("tagesschau.de - die erste Adresse für Nachrichten"),
            "tagesschau.de"
        );
        assert_eq!(short_title("Blog | Thoughts"), "Blog");
        assert_eq!(short_title("A - b"), "A - b");
        assert_eq!(short_title("Plain"), "Plain");
    }

    #[test]
    fn feeds_are_kept() {
        let storage = MemoryStorage::new();
        let feeds = [
            Feed {
                url: String::from("https://example.org/feed.xml"),
                name: String::from("Example\tnews"),
            },
            Feed {
                url: String::from("https://www.other.org/rss"),
                name: String::new(),
            },
        ];
        save(&storage, &feeds);
        let loaded = load(&storage).unwrap();
        assert_eq!(loaded[0].name, "Example news");
        assert_eq!(loaded[1].shown(), "other.org");
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn feeds_of_version_1_are_read() {
        let storage = MemoryStorage::new().with(
            &super::data("feeds.txt"),
            include_str!("../testdata/feeds.txt.v1"),
        );
        let feeds = super::load(&storage).unwrap();
        assert_eq!(feeds.len(), 2);
        assert_eq!(feeds[0].url, "https://www.tagesschau.de/xml/rss2/");
        assert_eq!(feeds[0].name, "Tagesschau");
        assert_eq!(feeds[1].name, "");
    }

    fn feed(url: &str, name: &str) -> Feed {
        Feed {
            url: String::from(url),
            name: String::from(name),
        }
    }

    /// Feeds of the same name keep books of their own, and their pages
    /// show the name alone.
    #[test]
    fn feeds_of_one_name_have_books_of_their_own() {
        let a = feed("https://a.example/rss", "Daily");
        let b = feed("https://b.example/rss", "Daily");
        assert_ne!(a.book("News"), b.book("News"));
        assert_eq!(a.book("News"), a.book("News"));
        let book = a.book("News");
        assert!(book.starts_with("News/Daily ("), "{book}");
        let stem = book.trim_end_matches(".html").rsplit('/').next().unwrap();
        assert_eq!(without_mark(stem), "Daily");
        assert_eq!(without_mark("Daily"), "Daily");
        assert_eq!(without_mark("Daily (2026)"), "Daily (2026)");
        // Answers under way at once are kept apart.
        assert_ne!(download(&a.url), download(&b.url));
    }

    /// A book kept as version 1 named it is still found, and goes once
    /// no feed reads it any more.
    #[test]
    fn books_of_version_1_are_kept_until_replaced() {
        let a = feed("https://a.example/rss", "Daily");
        let b = feed("https://b.example/rss", "Daily");
        let c = feed("https://c.example/rss", "Weekly");
        let old = format!("{}Daily</title>", convert::HEAD);
        let storage = MemoryStorage::new()
            .with("News/Daily.html", old.as_str())
            .with("News/Weekly.html", old.as_str());
        assert_eq!(
            a.found_book(&storage, "News").as_deref(),
            Some("News/Daily.html")
        );
        let feeds = [a.clone(), b.clone(), c.clone()];
        // A fetched: B still reads the old book.
        storage.write(&a.book("News"), b"a").unwrap();
        retire_old_book(&storage, "News", &feeds, &a);
        assert!(storage.exists("News/Daily.html"));
        assert_eq!(
            b.found_book(&storage, "News").as_deref(),
            Some("News/Daily.html")
        );
        // B fetched too: the old book goes.
        storage.write(&b.book("News"), b"b").unwrap();
        retire_old_book(&storage, "News", &feeds, &b);
        assert!(!storage.exists("News/Daily.html"));
        assert_eq!(a.found_book(&storage, "News"), Some(a.book("News")));
        // C removed: its old book goes, the others' stay.
        retire_old_book(&storage, "News", &feeds[..2], &c);
        assert!(!storage.exists("News/Weekly.html"));
        assert!(storage.exists(&a.book("News")) && storage.exists(&b.book("News")));
        assert_eq!(c.found_book(&storage, "News"), None);
    }

    /// A list of feeds that cannot be read is not taken for an empty one,
    /// which adding a feed would write over.
    #[test]
    fn an_unreadable_list_is_not_empty() {
        let broken = MemoryStorage::new().with(
            &super::data("feeds.txt"),
            b"https://a.example/rss\tA\n\xff\n".as_slice(),
        );
        assert!(load(&broken).is_err());
        assert_eq!(load(&MemoryStorage::new()), Ok(Vec::new()));
    }
}

#[cfg(test)]
mod feeds_tests;
