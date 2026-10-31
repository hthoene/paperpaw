//! A list of books with their covers and states: a series (by number, the
//! next book to read marked and focused), a folder, the favourites, the
//! books under a letter, or what a search found.
//!
//! The books are found in one pass over the catalog when the list opens
//! (how far each is read, and which are favourites, are in it); only
//! where their lines start is kept. A page's books are read when it is
//! shown.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

use super::index::{letter, sort_key, surname};
use super::rows::{self, Loaded, Sizes};

/// The parameter naming the series, folder, letter or search words.
const NAME: &str = "name";

/// What a list of books holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Series,
    Folder,
    Favourites,
    /// The authors (the list of them; not a list of books).
    Authors,
    /// The books of an author, of a tag.
    Author,
    Tag,
    Letter,
    Search,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Series => "series",
            Kind::Folder => "folder",
            Kind::Favourites => "favourites",
            Kind::Authors => "authors",
            Kind::Author => "author",
            Kind::Tag => "tag",
            Kind::Letter => "letter",
            Kind::Search => "search",
        }
    }

    fn of(mode: &str) -> Kind {
        [
            Kind::Series,
            Kind::Folder,
            Kind::Favourites,
            Kind::Author,
            Kind::Tag,
            Kind::Letter,
            Kind::Search,
        ]
        .into_iter()
        .find(|k| k.as_str() == mode)
        .unwrap_or(Kind::Favourites)
    }
}

/// The command that opens the list of `kind` named `name`.
pub(crate) fn open(kind: Kind, name: &str) -> Command {
    Command::OpenScreen(
        super::SHELF,
        Params::new()
            .with(params::MODE, kind.as_str())
            .with(NAME, name.to_string()),
    )
}

/// A book of the list.
#[derive(Debug, Clone, Copy)]
struct Entry {
    offset: u32,
    key: [u8; paperpaw_sdk::library::index::KEY],
    /// Its number in a series ([`catalog::series_order`]).
    number: u32,
    /// How far it is read.
    progress: u8,
}

pub(crate) struct ShelfScreen {
    style: UiStyle,
    /// Covers in grays ([`crate::COVERS`]).
    grays: bool,
    sizes: Sizes,
    header: ui::Header,
    kind: Kind,
    name: String,
    entries: Vec<Entry>,
    /// The series' next book to read.
    next: Option<usize>,
    /// The line below the header: how many books, how many finished.
    summary: Rect,
    /// The words of the header and of that line, written when the list
    /// is made, so drawing writes none.
    title_text: String,
    summary_text: String,
    list: ListView,
    loaded: Vec<Loaded>,
    loaded_page: Option<usize>,
    /// The version of the library's books the entries' keys are of
    /// ([`library::Library::version`]).
    version: Option<u64>,
    /// The catalog's stamp when its books were found ([`stamp`]).
    stamp: Option<u64>,
    fresh: bool,
}

impl ShelfScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let kind = Kind::of(params.get(params::MODE).unwrap_or_default());
        let name = params.get(NAME).unwrap_or_default().to_string();
        let style = UiStyle::new(env);
        let sizes = Sizes::new(env, &style);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let summary = Rect::new(
            0,
            header.area().height,
            display.width,
            sizes.small_line + style.gap(),
        );
        let top = summary.y + summary.height;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let stamp = stamp(env);
        let mut screen = Self {
            grays: crate::covers_in_grays(env),
            list: ListView::new(area, u32::from(sizes.row)).focus_on_keys(env),
            entries: find(env, kind, &name),
            next: None,
            summary,
            title_text: String::new(),
            summary_text: String::new(),
            loaded: Vec::new(),
            loaded_page: None,
            version: library::service(env).version(env),
            stamp,
            fresh: true,
            kind,
            name,
            style,
            sizes,
            header,
        };
        screen.mark_next();
        screen.list.set_focus(screen.next.unwrap_or(0));
        screen.load_page(env);
        screen.word(env);
        screen
    }

    /// Writes the title and the line below it: how many books, and for a
    /// series how many are finished.
    fn word(&mut self, env: &Env<'_>) {
        self.title_text = self.title(env);
        let len = self.entries.len();
        let books = env.i18n().plural("files.books", len as i64, &[]);
        let finished = self.entries.iter().filter(|e| e.progress == 100).count();
        self.summary_text = if self.kind == Kind::Series && finished > 0 {
            let done = env
                .i18n()
                .plural("files.list.finished.count", finished as i64, &[]);
            alloc::format!("{books} · {done}")
        } else {
            books
        };
    }

    /// The series' next book: the first not begun, after the finished
    /// and the one being read.
    fn mark_next(&mut self) {
        self.next = (self.kind == Kind::Series)
            .then(|| self.entries.iter().position(|e| e.progress == 0))
            .flatten();
    }

    fn load_page(&mut self, env: &Env<'_>) {
        let page = self.list.page();
        if self.loaded_page == Some(page) {
            return;
        }
        // The keys hold only for the books' version they are of: else
        // found anew first.
        let service = library::service(env);
        if service.version(env) != self.version {
            self.find_again(env);
            return;
        }
        self.loaded = Vec::new();
        let rows = self.list.rows();
        let series = self.kind == Kind::Series;
        let width = self.list.area().width;
        let mut loaded = Vec::new();
        let keys: Vec<u64> = self
            .entries
            .iter()
            .skip(page * rows)
            .take(rows)
            .map(|e| u64::from(e.offset))
            .collect();
        let records = service.records(env, &keys);
        for ((n, entry), record) in self
            .entries
            .iter_mut()
            .enumerate()
            .skip(page * rows)
            .take(rows)
            .zip(records)
        {
            let Some(record) = record else {
                continue;
            };
            let mut book = Loaded::load(env, record, self.sizes.cover, "");
            entry.progress = book.progress;
            // A series by numbers; elsewhere the author.
            let note = if series {
                book.series_note(env)
            } else {
                book.author.clone()
            };
            let next = self.next == Some(n);
            rows::prepare_row(env, &self.style, &self.sizes, width, &mut book, note, next);
            if series {
                book.tail = String::new();
            }
            loaded.push(book);
        }
        self.loaded = loaded;
        self.loaded_page = Some(page);
    }

    fn book(&self, index: usize) -> Option<&Loaded> {
        let rows = self.list.rows();
        (self.loaded_page == Some(index / rows))
            .then(|| self.loaded.get(index % rows))
            .flatten()
    }

    fn title(&self, env: &Env<'_>) -> String {
        match self.kind {
            Kind::Favourites => env.i18n().tr("files.favourites").to_string(),
            Kind::Search => env
                .i18n()
                .format("files.search.title", &[("text", Arg::Str(&self.name))]),
            Kind::Letter => env
                .i18n()
                .format("files.list.letter", &[("letter", Arg::Str(&self.name))]),
            Kind::Folder => paperpaw_sdk::files::name(&self.name).to_string(),
            Kind::Author if self.name.is_empty() => {
                env.i18n().tr("files.unknown-author").to_string()
            }
            _ => self.name.clone(),
        }
    }

    /// Finds its books anew (their states, where their lines are), the
    /// focus kept at its place.
    fn find_again(&mut self, env: &Env<'_>) {
        let focus = self.list.focus();
        self.version = library::service(env).version(env);
        self.stamp = stamp(env);
        self.entries = find(env, self.kind, &self.name);
        self.mark_next();
        self.list
            .set_focus(focus.min(self.entries.len().saturating_sub(1)));
        self.loaded_page = None;
        self.load_page(env);
        self.word(env);
    }

    fn open_book(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some(book) = self.book(index) {
            let path = book.path.clone();
            super::read(&path, cx);
        }
    }
}

/// The catalog's stamp, which changes with it and with what was noted
/// beside it (how far books are read, favourites): `None` while there is
/// none.
fn stamp(env: &Env<'_>) -> Option<u64> {
    match library::service(env).catalog_state(env) {
        catalog::State::Ready { stamp } => Some(stamp),
        _ => None,
    }
}

/// The books of the list `kind` named `name`, in its order: a series by
/// number, the others by title.
fn find(env: &Env<'_>, kind: Kind, name: &str) -> Vec<Entry> {
    let folder = alloc::format!("{name}/");
    let words: Vec<String> = name
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let wanted = name.bytes().next().unwrap_or(b'#');
    let mut entries = Vec::new();
    library::service(env).each(env, &mut |offset, line| {
        let found = match kind {
            Kind::Series => line.series == name,
            Kind::Folder => line.path.starts_with(&folder),
            Kind::Favourites => line.status.favourite,
            Kind::Author => line.author == name,
            Kind::Tag => line.tags().any(|t| t == name),
            Kind::Letter => {
                let title = sort_key(&line.title)[0];
                let title = if title.is_ascii_lowercase() {
                    title.to_ascii_uppercase()
                } else {
                    b'#'
                };
                // A book with no author is not under "#" for that.
                let by_author = letter(surname(&line.author));
                title == wanted || (by_author == wanted && by_author != b'#')
            }
            Kind::Search => {
                let text = alloc::format!(
                    "{} {} {} {}",
                    line.title,
                    line.author,
                    line.series,
                    paperpaw_sdk::files::name(&line.path)
                )
                .to_lowercase();
                !words.is_empty() && words.iter().all(|w| text.contains(w.as_str()))
            }
            Kind::Authors => false,
        };
        if found && let Ok(offset) = u32::try_from(offset) {
            entries.push(Entry {
                offset,
                key: sort_key(&line.title),
                number: catalog::series_order(&line.number),
                progress: line.status.progress.unwrap_or(0),
            });
        }
    });
    if kind == Kind::Series {
        entries.sort_unstable_by_key(|e| (e.number, e.key, e.offset));
    } else {
        entries.sort_unstable_by_key(|e| (e.key, e.offset));
    }
    entries
}

impl Screen for ShelfScreen {
    /// Its covers as the library's ([`crate::COVERS`]).
    fn pictures_in_grays(&self) -> bool {
        self.grays
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.entries.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, &self.title_text, None, self.list.paging(len));
        let x = style.margin() as f32;
        let faint = style.faint(ui::Ink::DETAIL);
        ui::Label::new(&self.summary_text)
            .px(style.small())
            .color(faint)
            .draw_from(&mut ui, x, self.summary);
        if len == 0 {
            let (icon, title, text) = match self.kind {
                Kind::Favourites => (
                    "star",
                    env.i18n().tr("files.no-favourites.title"),
                    ui::tr_input(env, "files.no-favourites"),
                ),
                _ => ("books", "", env.i18n().tr("files.search.none")),
            };
            ui.empty(self.list.area(), icon, title, text);
            return;
        }
        // Each book a row that says itself (its title first), so drawn
        // here rather than by the list view, which describes its rows.
        let rows = self.list.rows();
        let first = self.list.page() * rows;
        for (slot, index) in (first..len).take(rows).enumerate() {
            let rect = self.list.row_rect(slot);
            if ui.canvas().clip().intersection(&rect).is_none() {
                continue;
            }
            let focused = self.list.active() && self.list.shown() && index == self.list.focus();
            if focused {
                ui.focus(rect);
            } else {
                ui.separator(rect, 0);
            }
            if self.loaded_page != Some(index / rows) {
                continue;
            }
            if let Some(book) = self.loaded.get(index % rows) {
                let next = self.next == Some(index);
                rows::draw_book_row(&mut ui, &self.sizes, rect, focused, book, next);
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.grays = crate::covers_in_grays(cx.env());
        if core::mem::take(&mut self.fresh) {
            return;
        }
        // Back from a book or its actions: states and the next may have
        // changed. Where nothing was noted since, the books found hold
        // and only the page's are read again (they went while hidden).
        let env = *cx.env();
        let same = library::service(&env).version(&env) == self.version;
        if same && self.stamp.is_some() && stamp(&env) == self.stamp {
            self.load_page(&env);
        } else {
            self.find_again(&env);
        }
        cx.request_redraw();
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        // A new catalog (its books where their lines are now), or a book
        // gone (noted beside it).
        if [
            events::CATALOG_CHANGED.id(),
            events::BOOK_REMOVED.id(),
            events::BOOK_MOVED.id(),
        ]
        .contains(&event)
        {
            let env = *cx.env();
            self.find_again(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn on_hide(&mut self) {
        self.loaded = Vec::new();
        self.loaded_page = None;
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let len = self.entries.len();
        if let Some(change) = self.list.navigate(a, len) {
            if change == ListChange::Page {
                let env = *cx.env();
                self.load_page(&env);
            }
            change.request(cx);
        } else if *a == actions::CONFIRM {
            self.open_book(self.list.focus(), cx);
        } else if *a == actions::MENU {
            if let Some(book) = self.book(self.list.focus()) {
                let path = book.path.clone();
                super::offer(&path, cx);
            }
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.entries.len();
        let hit = self.list.hit(gesture.start, len);
        match gesture.kind {
            GestureKind::LongPress => {
                let Some(book) = hit.and_then(|i| self.book(i)) else {
                    return Handled::No;
                };
                let path = book.path.clone();
                super::offer(&path, cx);
                Handled::Yes
            }
            GestureKind::Tap => {
                let paging = self.list.paging(len);
                if let Some(action) = self
                    .header
                    .tap(cx.env(), &self.style, gesture.start, paging)
                {
                    cx.command(Command::run(action));
                    return Handled::Yes;
                }
                match hit {
                    Some(index) => {
                        self.list.set_focus(index);
                        self.open_book(index, cx);
                        Handled::Yes
                    }
                    None => Handled::No,
                }
            }
            _ => Handled::No,
        }
    }
}
