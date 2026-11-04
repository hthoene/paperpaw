//! The library as a calm list with covers and how far each book is
//! read. The library is one list of all books (those in folders
//! included), opening with the book read last as a larger card, so going
//! on reading is one confirm or one tap. Chips under the header filter
//! the books by how far they are read (All, Reading, Unread, Finished);
//! the last chip shows the shelves instead: favourites, authors, the
//! series and the folders (collections). A series lists its books with
//! their covers, numbers and states, the next one to read marked and
//! focused. "Find" in the header picks a book by the first letter of its
//! title or author, without a keyboard; on touch screens it also offers
//! search.
//!
//! On small screens (the X4) the filters sit behind one row above the
//! books, which shows what is shown and opens a choice of it (and of
//! "Find"); larger screens show them as chips ([`ui::Chips`]).
//!
//! Buttons work as in every list: the focus moves a book at a time, the
//! page buttons turn pages, confirm opens, holding confirm (the menu
//! button) offers what can be done with the focused book (its sheet:
//! open, finished, favourite, information, archive, the book menu's
//! actions, delete, asked about first). Above the first
//! book (and after the last) the focus reaches the filter's row.
//! Touch: tap the filter's row or a chip, a book, the search button (for
//! "Find"); hold a book for its actions; tap the title for the library's
//! menu.
//!
//! The books come from the library service ([`library::service`]), so a
//! plugin's library shows here as the kernel's does: what is known of
//! each book to filter and sort is its index ([`index`]), kept in a file
//! while a book covers the library; a page's books are read, with the
//! badges plugins put on them, and their words made ready, when it is
//! shown, never while drawing. The kernel makes the catalog anew; the
//! list follows it ([`events::CATALOG_CHANGED`]) and the changes of single
//! books ([`events::BOOK_CHANGED`]).

mod find;
mod grid;
mod index;
mod layout;
mod rows;
mod sheet;
pub(crate) mod shelf;
mod walking;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, settings, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::library;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui;
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, FixedAction, FixedScreen, Gesture, GestureKind, Handled, Id,
    Instant, Params, Rect, RefreshMode, RegisterError, Registrar, Screen,
};

use index::{Filter, NEVER, hash};
use layout::Layout;
use paperpaw_sdk::library::sorted::{self, Entry, Sorted};
use rows::{CoverPace, Loaded};

use crate::names::{self, Bound, Distinct, Firsts};

/// Finds a book by a letter ([`find`]).
pub(crate) const FIND: Id = Id::new_static("files.list.find");
/// A list of books: a series, a folder, the favourites, a letter, a
/// search ([`shelf`]).
const SHELF: Id = Id::new_static("files.list.shelf");
/// What can be done with a book ([`sheet`]).
const SHEET: Id = Id::new_static("files.list.sheet");
/// What the library shows, chosen from the filter's row.
const SHOWN: Event<paperpaw_sdk::ui::choice::Picked> = Event::new("files.list.shown");
/// The choices of what the library shows: the filters' names, then
/// "Find".
const SHOW_OPTIONS: [&str; 7] = [
    "all", "reading", "unread", "finished", "next", "shelves", "find",
];
/// The filters of [`SHOW_OPTIONS`], in their order.
const SHOW_FILTERS: [Filter; 6] = [
    Filter::All,
    Filter::Reading,
    Filter::Unread,
    Filter::Finished,
    Filter::Next,
    Filter::Shelves,
];
/// Where earlier versions kept the index while a book covered the
/// library: removed.
const KEPT: &str = ".paperpaw/plugins/files/index.bin";

static SCREENS: [FixedScreen; 3] = [
    FixedScreen::new(SHELF, "files.title", |env, params| {
        Box::new(shelf::ShelfScreen::new(env, params))
    }),
    FixedScreen::new(SHEET, "files.title", |env, params| {
        Box::new(sheet::BookSheet::new(env, params))
    }),
    FixedScreen::new(FIND, "files.list.find", |env, params| {
        Box::new(find::FindScreen::new(env, params))
    }),
];

/// Registers the list's screens and actions.
pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_screens(&SCREENS)?;
    r.add_actions(&ACTIONS)?;
    r.add_icons(&ICONS)?;
    Ok(())
}

static ACTIONS: [FixedAction; 2] = [
    FixedAction::new(FIND, "files.list.find")
        .handler(|_, _, out| out.push(Command::OpenScreen(FIND, Params::new()))),
    walking::ACTION,
];
static ICONS: [(Id, &str); 2] = [(FIND, "letter-case"), (LOOK_AGAIN, "refresh")];
pub(crate) use walking::{LOOK_AGAIN, card_gone, collecting, failed_words};

/// The library screen.
pub(crate) fn screen(env: &Env<'_>, _params: &Params) -> Box<dyn Screen> {
    Box::new(ListScreen::new(env))
}

/// The library's books sorted, as the kernel made them for a view
/// leaving `archive` out; none while it makes them anew.
fn open_sorted(env: &Env<'_>, archive: &str) -> Option<Sorted> {
    if env.storage().exists(KEPT) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "an index earlier versions kept, read by none: tried again next time"
        )]
        let _ = env.storage().remove(KEPT);
    }
    Sorted::open(env, archive)
}

/// The archive folder, as the setting names it: its books are on the
/// shelves, not in the list.
fn archive(env: &Env<'_>) -> String {
    env.settings()
        .read(&crate::ARCHIVE)
        .trim_matches('/')
        .to_string()
}

/// Opens the library's menu.
fn open_menu(cx: &mut Cx<'_, '_>) {
    cx.command(Command::OpenSlot(
        slots::MENU,
        Params::new().with(params::MENU, crate::MENU.as_str()),
    ));
}

/// Offers what can be done with the book at `path`.
fn offer(path: &str, cx: &mut Cx<'_, '_>) {
    cx.command(Command::OpenScreen(
        SHEET,
        Params::new().with(params::PATH, path.to_string()),
    ));
}

/// Opens the book at `path` in the reader.
fn read(path: &str, cx: &mut Cx<'_, '_>) {
    cx.command(paperpaw_sdk::readers::open(path));
}

/// A shelf of the shelves; its words are made when it is drawn (a
/// library of many series has many: each holds no more than its name).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShelfRow {
    /// "Next in your series" (a filter), else a shelf of `kind`.
    next: bool,
    /// All of the series or folders, where the shelves hold only the
    /// first of them ([`crate::names`]); the authors: more than
    /// `count`.
    more: bool,
    kind: shelf::Kind,
    /// The series' or folder's name (the folder's path for folders).
    name: String,
    count: usize,
    finished: usize,
}

impl ShelfRow {
    fn new(next: bool, kind: shelf::Kind, name: String, count: usize, finished: usize) -> Self {
        ShelfRow {
            next,
            more: false,
            kind,
            name,
            count,
            finished,
        }
    }

    /// A row of all the series or folders.
    fn all(kind: shelf::Kind) -> Self {
        ShelfRow {
            more: true,
            ..ShelfRow::new(false, kind, String::new(), 0, 0)
        }
    }

    /// Its icon, title and note.
    fn words(&self, env: &Env<'_>) -> (&'static str, String, String) {
        let books = env.i18n().plural("files.books", self.count as i64, &[]);
        match self.kind {
            _ if self.next => (
                "arrow-bar-to-right",
                env.i18n().tr("files.list.show.next").to_string(),
                books,
            ),
            shelf::Kind::Favourites => {
                ("star", env.i18n().tr("files.favourites").to_string(), books)
            }
            shelf::Kind::Authors => {
                let count = if self.more {
                    "files.list.authors.many"
                } else {
                    "files.list.authors.count"
                };
                (
                    "user",
                    env.i18n().tr("files.list.authors").to_string(),
                    env.i18n().plural(count, self.count as i64, &[]),
                )
            }
            shelf::Kind::Series if self.more => (
                "stack-2",
                env.i18n().tr("files.list.series.all").to_string(),
                env.i18n().tr("files.list.series").to_string(),
            ),
            shelf::Kind::Series => {
                let series = env.i18n().tr("files.list.series");
                let note = if self.finished > 0 {
                    let finished =
                        env.i18n()
                            .plural("files.list.finished.count", self.finished as i64, &[]);
                    format!("{series} · {books} · {finished}")
                } else {
                    format!("{series} · {books}")
                };
                ("stack-2", self.name.clone(), note)
            }
            _ if self.more => (
                "folder",
                env.i18n().tr("files.list.folders.all").to_string(),
                env.i18n().tr("files.folder").to_string(),
            ),
            _ => (
                "folder",
                paperpaw_sdk::files::name(&self.name).to_string(),
                format!("{} · {books}", env.i18n().tr("files.folder")),
            ),
        }
    }
}

/// The library; see the [module documentation](self).
struct ListScreen {
    layout: Layout,
    root: String,
    /// The archive folder, whose books are on the shelves only.
    archive: String,
    /// The books sorted ([`paperpaw_sdk::library::sorted`]), read a page
    /// at a time; none while the kernel makes them anew (the list says
    /// the library is being prepared).
    sorted: Option<Sorted>,
    /// The kernel was asked for them since they were last there.
    asked: bool,
    /// Since when the library waits for a kernel that does nothing
    /// (`watch`).
    idle_since: Option<Instant>,
    /// No catalog yet: the kernel is making the first.
    collecting: bool,
    /// The kernel could not make the catalog (`CATALOG_FAILED`): the
    /// folder could not be read; until a catalog is made.
    failed: bool,
    /// The memory card is lost until a restart ([`card_gone`]), where the
    /// catalog failed.
    card_gone: bool,
    filter: Filter,
    by_recent: bool,
    /// How many books (or stacks) the filter shows.
    count: usize,
    /// The next book of each series begun (the filter of them).
    next: Vec<Entry>,
    /// The shelves, while they are shown.
    shelves: Vec<ShelfRow>,
    /// The first book shown is the one read last, as a large card.
    card: bool,
    focus: usize,
    /// The focus shows: with buttons, and on touch screens once a button
    /// moved it.
    shown: bool,
    /// The focus is on the filter's row or the chips.
    filter_focused: bool,
    /// The books of the page shown, read when it was shown, and what the
    /// sorted books say of them (how many a stack holds).
    loaded: Vec<Loaded>,
    entries: Vec<Entry>,
    /// The page `loaded` holds.
    loaded_page: Option<usize>,
    /// The words of the filter's row, and of an empty list.
    filter_label: String,
    filter_end: String,
    empty_title: String,
    empty_text: String,
    /// The list is empty, there being no books: it offers to look for
    /// new ones ([`LOOK_AGAIN`]).
    look_again: bool,
    /// The search button on touch screens.
    find: Option<Rect>,
    /// Paths (hashed) of books moved or removed since the catalog was
    /// made: not shown until it is made anew.
    gone: Vec<u32>,
    /// While covered: the book the focus was on, to find again; the
    /// index is kept in a file.
    kept_focus: Option<u32>,
    covered: bool,
    /// When the covers not read yet are read.
    pace: CoverPace,
    /// Just opened: the first `on_show` need not load again.
    fresh: bool,
}

impl ListScreen {
    fn new(env: &Env<'_>) -> Self {
        let archive = archive(env);
        let mut screen = Self {
            layout: Layout::new(env),
            root: library::folder(env).to_string(),
            sorted: open_sorted(env, &archive),
            asked: false,
            idle_since: None,
            archive,
            collecting: collecting(env),
            failed: false,
            card_gone: false,
            filter: Filter::All,
            by_recent: env.settings().read(&crate::SORT) != crate::SORT_NAME,
            count: 0,
            next: Vec::new(),
            shelves: Vec::new(),
            card: false,
            focus: 0,
            shown: ui::focus_shown(env),
            filter_focused: false,
            loaded: Vec::new(),
            entries: Vec::new(),
            loaded_page: None,
            filter_label: String::new(),
            filter_end: String::new(),
            empty_title: String::new(),
            empty_text: String::new(),
            look_again: false,
            find: None,
            gone: Vec::new(),
            kept_focus: None,
            covered: false,
            pace: CoverPace::default(),
            fresh: true,
        };
        // A catalog without books may be out of date (books copied onto
        // the card with the library closed): the kernel walks it anew at
        // the first show after a start, and tells the view either way
        // (`CATALOG_CHANGED`); until then it says so, not that there are
        // no books.
        screen.collecting |=
            screen.sorted.as_ref().is_some_and(|s| !s.has_books()) && walking::readable(env);
        screen.unreadable(env);
        screen.refilter(env);
        screen
    }

    fn len(&self) -> usize {
        if self.filter == Filter::Shelves {
            self.shelves.len()
        } else {
            self.count
        }
    }

    /// Whether there are books at all (in the archive too).
    fn has_books(&self) -> bool {
        self.sorted.as_ref().is_some_and(Sorted::has_books)
    }

    /// The sorted books are being made anew (there is a catalog).
    fn preparing(&self) -> bool {
        self.sorted.is_none() && !self.collecting && !self.failed
    }

    /// The hash of the book the focus is on, if its page is read.
    fn focused_book(&self) -> Option<u32> {
        let page = self.page_of(self.focus);
        if self.loaded_page != Some(page) {
            return None;
        }
        let entry = self.entries.get(self.focus - self.page_start(page))?;
        Some(entry.item.hash)
    }

    /// The entries of the filter from `from` on, `n` at most.
    fn entries_from(&self, env: &Env<'_>, from: usize, n: usize) -> Vec<Entry> {
        if self.filter == Filter::Next {
            return self.next.iter().skip(from).take(n).copied().collect();
        }
        match (
            &self.sorted,
            self.filter.view(self.by_recent, self.layout.grid.is_some()),
        ) {
            (Some(sorted), Some(view)) => sorted.page(env.storage(), view, from, n),
            _ => Vec::new(),
        }
    }

    /// The sorted books opened anew (after the catalog was made anew, or
    /// another archive); the kernel asked for them where they are not
    /// there yet ([`ListScreen::ask`]).
    fn reopen(&mut self, env: &Env<'_>) {
        self.sorted = None;
        self.sorted = open_sorted(env, &self.archive);
        if self.sorted.is_some() {
            self.asked = false;
        }
        if let Some(sorted) = &mut self.sorted {
            sorted.hide_all(env.storage(), &self.gone);
        }
        self.forget_gone();
        self.unreadable(env);
    }

    /// The sorted books let go of where their file went meanwhile (the
    /// kernel keeps those of two archive folders; another view's took its
    /// place): asked for anew ([`ListScreen::ask`]), being prepared.
    fn forget_gone(&mut self) {
        if self.sorted.as_ref().is_some_and(Sorted::is_gone) {
            self.sorted = None;
            self.asked = false;
        }
    }

    /// Asks the kernel for the sorted books where they are being made
    /// anew, once; and watches that the kernel answers ([`walking`]).
    fn ask(&mut self, cx: &mut Cx<'_, '_>) {
        if self.sorted.is_none() && !self.asked {
            self.asked = true;
            cx.command(sorted::request(&self.archive));
        }
        self.watch(cx);
    }

    /// Takes the shown books (or shelves) anew from the index, the focus
    /// on the book whose path hashes to `keep` where it is shown, else at
    /// the place it was (or the first, `keep_place` false).
    fn refilter_keeping(&mut self, env: &Env<'_>, keep: Option<u32>, keep_place: bool) {
        let place = self.focus;
        self.focus = 0;
        self.loaded = Vec::new();
        self.entries = Vec::new();
        self.loaded_page = None;
        self.next = Vec::new();
        self.card = false;
        let storage = env.storage();
        let view = self.filter.view(self.by_recent, self.layout.grid.is_some());
        let mut found = None;
        if self.filter == Filter::Shelves {
            self.shelves = self.make_shelves(env);
            self.count = self.shelves.len();
        } else {
            self.shelves = Vec::new();
            if let (Filter::Next, Some(sorted)) = (self.filter, &self.sorted) {
                self.next = sorted.next_in_series(storage, self.by_recent);
            }
            self.count = match (&self.sorted, view) {
                (Some(sorted), Some(view)) => sorted.len(view),
                _ => self.next.len(),
            };
            // The card: the book read last, while it is read (as covers,
            // it is the first cover).
            self.card = self.by_recent
                && self.layout.grid.is_none()
                && matches!(self.filter, Filter::All | Filter::Reading)
                && self
                    .entries_from(env, 0, 1)
                    .first()
                    .is_some_and(|e| e.item.recent != NEVER && e.item.progress != 100);
            found = keep.and_then(|hash| match (&self.sorted, view) {
                (Some(sorted), Some(view)) => sorted.find(storage, view, hash),
                _ => self.next.iter().position(|e| e.item.hash == hash),
            });
        }
        self.focus = match found {
            Some(at) => at,
            None if keep_place => place.min(self.len().saturating_sub(1)),
            None => 0,
        };
        self.load_page(env);
        self.word(env);
    }

    /// Takes the shown books anew, the focus on the first.
    fn refilter(&mut self, env: &Env<'_>) {
        self.refilter_keeping(env, None, false);
    }

    /// Takes the shown books anew, the focus where it was: on its book,
    /// else at its place.
    fn refilter_in_place(&mut self, env: &Env<'_>) {
        let keep = self.focused_book();
        self.refilter_keeping(env, keep, true);
    }

    /// Writes the words of the filter's row and of an empty list, and
    /// where the search button is, so drawing writes none.
    fn word(&mut self, env: &Env<'_>) {
        let shown = SHOW_FILTERS
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut key = String::from("files.list.show.");
        key.push_str(SHOW_OPTIONS[shown]);
        self.filter_label = env.i18n().tr(&key).to_string();
        let total = self.sorted.as_ref().map_or(0, Sorted::total);
        let archived = self.sorted.as_ref().map_or(0, Sorted::archived);
        self.filter_end = if self.filter == Filter::Shelves || self.preparing() {
            String::new()
        } else {
            walking::count(env, self.len())
        };
        self.look_again = total == 0 && archived == 0;
        self.look_again &= !self.failed && !self.collecting && !self.preparing();
        (self.empty_title, self.empty_text) = if self.failed && total == 0 {
            let (title, text) = failed_words(self.card_gone);
            (
                env.i18n().tr(title).to_string(),
                env.i18n().tr(text).to_string(),
            )
        } else if self.collecting && total == 0 {
            (
                String::new(),
                env.i18n().tr("files.list.collecting").to_string(),
            )
        } else if self.preparing() {
            (
                String::new(),
                env.i18n().tr("files.list.preparing").to_string(),
            )
        } else if total == 0 && archived > 0 {
            (
                String::new(),
                env.i18n().tr("files.list.none.all").to_string(),
            )
        } else if total == 0 {
            (
                env.i18n().tr("files.empty.title").to_string(),
                env.i18n()
                    .format("files.empty", &[("dir", Arg::Str(&self.root))]),
            )
        } else {
            let key = match self.filter {
                Filter::Reading => "files.list.none.reading",
                Filter::Unread => "files.list.none.unread",
                Filter::Next => "files.list.none.next",
                _ => "files.list.none.finished",
            };
            (String::new(), env.i18n().tr(key).to_string())
        };
        self.find = self.find_button(env);
    }

    /// The shelves: the next books of the series begun, favourites and
    /// authors, then the series and the folders, each with how many books
    /// it has (and how many are finished), from one pass over the catalog.
    fn make_shelves(&self, env: &Env<'_>) -> Vec<ShelfRow> {
        let favourites = library::favourites(env.storage()).len();
        // Bounded however large the library: the authors counted (up to
        // a number), the first series and folders held.
        let memory = env.device().memory;
        let mut authors = Distinct::new(names::counted(memory));
        let budget = names::Budget::of(memory).shared(2);
        let mut series = Firsts::new(budget, Bound::First);
        let mut folders = Firsts::new(budget, Bound::First);
        let prefix = crate::shelves::prefix(&self.root);
        library::service(env).each(env, &mut |_, line| {
            let finished = line.reading() == library::catalog::Reading::Finished;
            authors.add(&line.author);
            if !line.series.is_empty() {
                series.add(&line.series, finished);
            }
            if let Some(folder) = crate::shelves::folder_in(&line.path, &prefix) {
                folders.add(folder, finished);
            }
        });
        let next = self
            .sorted
            .as_ref()
            .map_or(0, |s| s.next_in_series(env.storage(), false).len());
        let mut shelves = Vec::new();
        if next > 0 {
            shelves.push(ShelfRow::new(
                true,
                shelf::Kind::Series,
                String::new(),
                next,
                0,
            ));
        }
        shelves.push(ShelfRow::new(
            false,
            shelf::Kind::Favourites,
            String::new(),
            favourites,
            0,
        ));
        let (count, more) = authors.count();
        shelves.push(ShelfRow {
            more,
            ..ShelfRow::new(false, shelf::Kind::Authors, String::new(), count, 0)
        });
        for (kind, firsts) in [
            (shelf::Kind::Series, series),
            (shelf::Kind::Folder, folders),
        ] {
            let (named, _, cut) = firsts.into_names();
            shelves.extend(
                named
                    .into_iter()
                    .map(|n| ShelfRow::new(false, kind, n.name, n.count, n.finished)),
            );
            if cut {
                shelves.push(ShelfRow::all(kind));
            }
        }
        shelves
    }

    fn row_height(&self) -> u16 {
        if self.filter == Filter::Shelves {
            self.layout.style.noted_row() as u16
        } else {
            self.layout.sizes.row
        }
    }

    /// How high the books' rows of `page` are drawn: on small screens as
    /// many as fit, sharing what is left over, so a page ends at the
    /// foot of the screen.
    fn row_on(&self, page: usize) -> u16 {
        let row = self.row_height();
        if !self.layout.sizes.compact || self.filter == Filter::Shelves {
            return row;
        }
        let card = if self.card && page == 0 {
            self.layout.sizes.card
        } else {
            0
        };
        let room = self.layout.area.height.saturating_sub(card);
        let rows = (room / row.max(1)).max(1);
        room / rows
    }

    /// The covers' grid, while books show (not the shelves).
    fn covers(&self) -> Option<&grid::Grid> {
        self.layout
            .grid
            .as_ref()
            .filter(|_| self.filter != Filter::Shelves)
    }

    /// Entries on a page: rows, or covers.
    fn per_page(&self) -> usize {
        match self.covers() {
            Some(g) => g.per_page(),
            None => usize::from(self.layout.area.height / self.row_height().max(1)).max(1),
        }
    }

    /// Entries on the first page: the card and the rows below it.
    fn first_page(&self) -> usize {
        if !self.card {
            return self.per_page();
        }
        let below = self
            .layout
            .area
            .height
            .saturating_sub(self.layout.sizes.card);
        1 + usize::from(below / self.layout.sizes.row.max(1))
    }

    fn page_of(&self, index: usize) -> usize {
        let first = self.first_page();
        if index < first {
            0
        } else {
            1 + (index - first) / self.per_page()
        }
    }

    fn page_start(&self, page: usize) -> usize {
        match page {
            0 => 0,
            p => self.first_page() + (p - 1) * self.per_page(),
        }
    }

    fn pages(&self) -> usize {
        self.page_of(self.len().saturating_sub(1)) + 1
    }

    /// The entries of `page`.
    fn page_range(&self, page: usize) -> core::ops::Range<usize> {
        let start = self.page_start(page);
        start..self.page_start(page + 1).min(self.len())
    }

    /// Where entry `index` is drawn (on its page).
    fn place(&self, index: usize) -> Rect {
        let page = self.page_of(index);
        let slot = index - self.page_start(page);
        let a = self.layout.area;
        if let Some(g) = self.covers() {
            return g.place(a, a.y, slot);
        }
        let row = self.row_on(page);
        if self.card && page == 0 {
            if slot == 0 {
                return Rect::new(a.x, a.y, a.width, self.layout.sizes.card);
            }
            let y = a.y + self.layout.sizes.card + (slot as u16 - 1) * row;
            return Rect::new(a.x, y, a.width, row);
        }
        Rect::new(a.x, a.y + slot as u16 * row, a.width, row)
    }

    /// Reads the books of the page the focus is on, if they are not read,
    /// and makes their words ready.
    fn load_page(&mut self, env: &Env<'_>) {
        if self.filter == Filter::Shelves {
            self.loaded = Vec::new();
            return;
        }
        let page = self.page_of(self.focus);
        if self.loaded_page == Some(page) {
            return;
        }
        // The page before goes first: never two pages of covers at once.
        self.loaded = Vec::new();
        self.entries = Vec::new();
        let width = self.layout.area.width;
        let mut loaded = Vec::new();
        // The page's books in one go, by their keys, which hold only for
        // the books' version they were sorted of: else opened anew first.
        let service = library::service(env);
        if self.sorted.as_ref().is_some_and(|s| !s.is_current(env)) {
            self.rebuild(env, false);
            return;
        }
        let range = self.page_range(page);
        let entries = self.entries_from(env, range.start, range.len());
        let keys: Vec<u64> = entries.iter().map(|e| u64::from(e.item.offset)).collect();
        let records = service.records(env, &keys);
        let (style, sizes) = (&self.layout.style, &self.layout.sizes);
        for (index, record) in self.page_range(page).zip(records) {
            let Some(record) = record else {
                continue;
            };
            let card = self.card && index == 0;
            // Each size of cover kept apart: the card's, the grid's, the
            // rows'.
            let (cover, variant) = match self.covers() {
                _ if card => (sizes.card_cover, "card"),
                Some(g) => (g.cover, "grid"),
                None => (sizes.cover, ""),
            };
            let mut book = Loaded::load(env, record, cover, variant);
            match self.covers() {
                _ if card => rows::prepare_card(env, style, sizes, width, &mut book),
                Some(g) => {
                    let count = entries.get(index - range.start).map_or(1, |e| e.stack);
                    grid::prepare_cell(env, style, sizes, g, &mut book, count);
                }
                None => {
                    let note = book.author.clone();
                    rows::prepare_row(env, style, sizes, width, &mut book, note, false);
                }
            }
            loaded.push(book);
        }
        self.loaded = loaded;
        self.entries = entries;
        self.loaded_page = Some(page);
    }

    /// The book of entry `index`, if its page is read.
    fn book(&self, index: usize) -> Option<&Loaded> {
        let page = self.page_of(index);
        if self.loaded_page != Some(page) {
            return None;
        }
        self.loaded.get(index - self.page_start(page))
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Cx<'_, '_>) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        let env = *cx.env();
        self.refilter(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Moves the focus by `delta` entries. Past the first (or the last)
    /// it reaches the filter's row (or the chips), as in other lists the
    /// header's note; of the page, where the arrow keys stay on the page
    /// (`ARROWS_STAY_ON_PAGE`), and from there the page's other end.
    fn step(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.len();
        let before = self.focus;
        let was_filter = self.shown && self.filter_focused;
        if !self.has_books() {
            // Nothing to move to: no books, no filter shown.
            return;
        }
        let on = cx.env().settings().read(&settings::ARROWS_STAY_ON_PAGE);
        let (first, end) = if on {
            let page = self.page_of(self.focus.min(len.saturating_sub(1)));
            (self.page_start(page), self.page_start(page + 1).min(len))
        } else {
            (0, len)
        };
        if self.filter_focused {
            self.filter_focused = false;
            self.focus = grid::entered(first..end, self.focus, delta, on);
        } else if !self.shown {
            // The first press shows where the focus is.
            self.shown = true;
        } else {
            let next = self.focus as isize + delta;
            if next < first as isize || next >= end as isize {
                self.filter_focused = true;
            } else {
                self.focus = next as usize;
            }
        }
        self.moved(before, was_filter, cx);
    }

    /// Draws what a moved focus changed: the entries it left and reached
    /// (the filter's row among them where it was or is focused) on one
    /// page, in a fast refresh as menus move theirs, else the new page.
    /// Set exactly: the parts lie apart, and joined they may cover most
    /// of the screen and its covers, where the kernel would refresh it
    /// all (only the full refreshes the ghosting asks for are still
    /// made).
    fn moved(&mut self, before: usize, was_filter: bool, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let is_filter = self.shown && self.filter_focused;
        if self.page_of(before) == self.page_of(self.focus) {
            if was_filter || is_filter {
                cx.request_redraw_area(self.filter_area());
            }
            if !was_filter && before < self.len() {
                cx.request_redraw_area(self.place(before));
            }
            if !is_filter && self.focus < self.len() {
                cx.request_redraw_area(self.place(self.focus));
            }
            cx.set_refresh(RefreshMode::Fast);
        } else {
            self.load_page(&env);
            self.find = self.find_button(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    /// Turns to the next (`1`) or the previous (`-1`) page.
    fn turn(&mut self, pages: isize, cx: &mut Cx<'_, '_>) {
        let page = self.page_of(self.focus) as isize + pages;
        if page < 0 || page >= self.pages() as isize {
            return;
        }
        self.focus = self.page_start(page as usize);
        self.filter_focused = false;
        let env = *cx.env();
        self.load_page(&env);
        self.find = self.find_button(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Opens entry `index`: a book in the reader, a shelf's books.
    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if self.filter == Filter::Shelves {
            let Some(row) = self.shelves.get(index) else {
                return;
            };
            if row.next {
                self.set_filter(Filter::Next, cx);
            } else if row.kind == shelf::Kind::Authors {
                cx.command(Command::run(crate::BY_AUTHOR));
            } else if row.more && row.kind == shelf::Kind::Series {
                cx.command(Command::run(crate::BY_SERIES));
            } else if row.more {
                cx.command(crate::shelves::open_folders(&self.root));
            } else {
                cx.command(shelf::open(row.kind, &row.name));
            }
            return;
        }
        let Some(book) = self.book(index) else {
            return;
        };
        let start = self.page_start(self.page_of(index));
        if self.entries.get(index - start).is_some_and(|e| e.stack > 1) {
            // A series as a stack: its books.
            cx.command(shelf::open(shelf::Kind::Series, &book.series));
            return;
        }
        let path = book.path.clone();
        let env = *cx.env();
        if env.storage().exists(&path) {
            read(&path, cx);
            return;
        }
        // Gone without a word (deleted on a computer, say): left out, and
        // the catalog made anew.
        self.gone.push(hash(&path));
        if let Some(sorted) = &mut self.sorted {
            sorted.hide(env.storage(), hash(&path));
        }
        self.forget_gone();
        self.ask(cx);
        cx.command(library::request_catalog_anew());
        self.refilter_in_place(&env);
        cx.command(Command::Notify(
            env.i18n().tr("files.list.gone").to_string(),
        ));
        cx.request_redraw();
    }

    /// Offers what can be done with book `index`; `false` for a shelf.
    fn offer(&mut self, index: usize, cx: &mut Cx<'_, '_>) -> bool {
        match self.book(index) {
            Some(book) if self.filter != Filter::Shelves => {
                let path = book.path.clone();
                offer(&path, cx);
                true
            }
            _ => false,
        }
    }

    /// On touch screens, where the search button is: at the end of the
    /// filter's row, or in the header left of the page arrows (as
    /// [`ui::Header`] places them).
    fn find_button(&self, env: &Env<'_>) -> Option<Rect> {
        if !env.device().input.touch || !self.has_books() {
            return None;
        }
        let style = &self.layout.style;
        if let Some(row) = self.layout.filter_row {
            // At the end of the filter's row, its icon on the margin.
            let square = row.height;
            let size = style.title() * ui::ICON_SCALE;
            let icon = ui::icon_width(env, "search", size) as u16;
            let right = (row.x + row.width).saturating_sub(style.margin());
            let x = (right + (square - icon) / 2).saturating_sub(square);
            // At the start of it where the interface runs right to left.
            return Some(rows::Flip::new(style, row).rect(Rect::new(x, row.y, square, square)));
        }
        let a = self.layout.header.area();
        let square = a.height;
        let pages = self.pages();
        let right = if self.len() > 0 && pages > 1 {
            let font = style.font(env)?;
            let label = format!("{} / {pages}", self.page_of(self.focus) + 1);
            let label = font.measure(style.small(), &label) as u16;
            (a.x + a.width).saturating_sub(2 * (square * 4 / 5) + label)
        } else {
            (a.x + a.width).saturating_sub(style.margin())
        };
        let button = Rect::new(right.saturating_sub(square), a.y, square, square);
        Some(rows::Flip::new(style, a).rect(button))
    }

    /// Where the filter is: its row, or the chips' band.
    fn filter_area(&self) -> Rect {
        match self.layout.filter_row {
            Some(row) => row,
            None => {
                let top = self.layout.header.area().y + self.layout.header.area().height;
                let a = self.layout.area;
                Rect::new(0, top, a.width, a.y.saturating_sub(top))
            }
        }
    }

    /// Offers what to show: the filters and "Find".
    fn choose_shown(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let title = env.i18n().tr("files.list.show");
        cx.command(
            paperpaw_sdk::ask::choice(title, &SHOWN)
                .options("files.list.show.", &SHOW_OPTIONS)
                .command(),
        );
    }

    /// The catalog was made anew: the index from it, the place kept.
    /// `told`: the kernel said it is done, made or not (a full or
    /// unreadable card), so books are no longer being collected.
    fn rebuild(&mut self, env: &Env<'_>, told: bool) {
        self.collecting = !told && collecting(env);
        let keep = self.focused_book();
        self.gone.clear();
        self.reopen(env);
        self.refilter_keeping(env, keep, true);
    }

    /// The button that looks for new books on an empty library, and the
    /// room above it.
    fn look_button(&self) -> Option<(Rect, Rect)> {
        let style = &self.layout.style;
        (self.look_again && self.len() == 0)
            .then(|| walking::button(self.layout.area, style.row(), style.margin()))
    }

    fn draw_shelf(&self, ui: &mut ui::Ui<'_>, row: &ShelfRow, rect: Rect, focused: bool) {
        let style = &self.layout.style;
        // A row, whose words are those drawn in it.
        ui.canvas().describe(Role::Row, "", rect).focused(focused);
        let ink = style.focus_ink(focused);
        let room = ui::icon_room(ui.env(), ui.style(), true);
        let band = ui::title_area(style, rect);
        let (icon, title, note) = row.words(ui.env());
        ui.row_icon(band, room, Some(icon), ink);
        let text = Rect::new(
            rect.x + room,
            rect.y,
            rect.width.saturating_sub(room),
            rect.height,
        );
        ui::Row::value(&title, "›")
            .note(&note)
            .focused(focused)
            .draw(ui, text);
    }
}

impl Screen for ListScreen {
    /// Its covers in black and white unless set otherwise
    /// ([`crate::COVERS`]): the focus moves over them all the time, and
    /// after a frame in grays the next change refreshes the whole screen
    /// (see [`Screen::pictures_in_grays`]).
    fn pictures_in_grays(&self) -> bool {
        self.layout.grays
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if self.covered {
            // Below an overlay left open before it showed again: nothing
            // to draw until it shows ([`Screen::on_show`]).
            return;
        }
        let layout = &self.layout;
        let style = layout.style;
        let len = self.len();
        let paging = (len > 0).then(|| ui::Paging::new(self.page_of(self.focus), self.pages()));
        let title = env.i18n().tr("files.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        layout.header.draw(&mut ui, title, None, paging);
        let has_books = self.has_books();
        let focused_filter = self.shown && self.filter_focused;
        if let Some(row) = layout.filter_row {
            if has_books {
                let (label, end) = (&self.filter_label, &self.filter_end);
                rows::draw_filter_row(&mut ui, row, focused_filter, label, end, self.find);
            }
        } else {
            if let Some(button) = self.find {
                let size = style.title() * ui::ICON_SCALE;
                let w = ui::icon_width(env, "search", size);
                let x = f32::from(button.x) + (f32::from(button.width) - w) / 2.0;
                let ink = style.header_colors().1;
                ui.icon("search", size, x, button, ink);
            }
            // No chips before there are books to sort. Next in your series
            // is on the shelves: their chip is the chosen one then.
            if has_books {
                let shown = if self.filter == Filter::Next {
                    Filter::Shelves
                } else {
                    self.filter
                };
                let chosen = Filter::ALL.iter().position(|f| *f == shown);
                layout.chips.draw(
                    &mut ui,
                    chosen.unwrap_or(usize::MAX),
                    chosen.filter(|_| focused_filter),
                );
            }
        }
        if len == 0 {
            let area = layout.area;
            if self.collecting && !self.failed && !has_books {
                ui.message(area, &self.empty_text);
            } else if let Some((button, above)) = self.look_button() {
                ui.empty(above, "books", &self.empty_title, &self.empty_text);
                let label = env.i18n().tr("files.look-again");
                ui.button(button, label, false, self.shown);
            } else {
                let (title, text) = (&self.empty_title, &self.empty_text);
                ui.empty(area, "books", title, text);
            }
            return;
        }
        let sizes = &layout.sizes;
        let page = self.page_of(self.focus);
        for index in self.page_range(page) {
            let rect = self.place(index);
            if ui.canvas().clip().intersection(&rect).is_none() {
                continue;
            }
            let focused = self.shown && !self.filter_focused && index == self.focus;
            if self.card && index == 0 {
                if let Some(book) = self.book(0) {
                    rows::draw_card(&mut ui, sizes, rect, focused, book);
                }
                continue;
            }
            if let Some(g) = self.covers() {
                if let Some(book) = self.book(index) {
                    let start = self.page_start(self.page_of(index));
                    let count = self.entries.get(index - start).map_or(1, |e| e.stack);
                    grid::draw_cell(&mut ui, sizes, g, rect, focused, book, count);
                }
                continue;
            }
            if focused {
                ui.focus(rect);
            } else {
                ui.separator(rect, 0);
            }
            if self.filter == Filter::Shelves {
                self.draw_shelf(&mut ui, &self.shelves[index], rect, focused);
            } else if let Some(book) = self.book(index) {
                rows::draw_book_row(&mut ui, sizes, rect, focused, book, false);
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.pace.heard(cx.env().now());
        self.shown(cx);
        self.wake_for_covers(cx);
    }

    /// Turned or resized: laid out anew for the size, the focus on its
    /// book, the filter and the order as they are.
    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        self.layout = Layout::new(&env);
        if !self.covered {
            self.refilter_in_place(&env);
            cx.wake_at(env.now());
        }
        cx.request_redraw();
        Handled::Yes
    }

    fn on_hide(&mut self) {
        // Covered by a book (or another full screen): the covers and what
        // was read of the sorted books go, memory for the book.
        if !self.covered {
            self.kept_focus = self.focused_book();
        }
        self.loaded = Vec::new();
        self.loaded_page = None;
        if self.covered {
            return;
        }
        self.covered = true;
        self.entries = Vec::new();
        self.next = Vec::new();
        self.shelves = Vec::new();
        self.sorted = None;
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        self.pace.heard(cx.env().now());
        let handled = self.act(action, cx);
        self.wake_for_covers(cx);
        handled
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        self.pace.heard(cx.env().now());
        let handled = self.gesture(gesture, cx);
        self.wake_for_covers(cx);
        handled
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        self.heard(event, params, cx);
        self.wake_for_covers(cx);
    }

    /// The covers of the page shown not read yet, one a tick (each opens
    /// its book), the first shown first, none while input comes; drawn as
    /// each comes.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.looked(cx);
        if !self.covered && self.pace.tick(cx, &mut self.loaded) {
            cx.request_redraw();
        }
    }
}

/// What the screen does, its covers looked for after each
/// ([`ListScreen::wake_for_covers`]).
impl ListScreen {
    fn shown(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if core::mem::take(&mut self.fresh) {
            // The kernel makes the catalog where it is not there, or was
            // not made since PaperPaw started; it shows once it is.
            cx.command(library::request_catalog());
        } else if core::mem::take(&mut self.covered) {
            // Back from a book or a screen over the whole library: the
            // sorted books read through again, with what changed
            // meanwhile (the books read last, the status of those read).
            self.archive = archive(&env);
            self.reopen(&env);
            self.collecting = collecting(&env);
            let keep = self.kept_focus.take();
            self.refilter_keeping(&env, keep, true);
            cx.request_redraw();
        }
        self.ask(cx);
    }

    fn act(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if *a == LOOK_AGAIN {
            return self.looking(cx);
        }
        let touch = cx.env().device().input.touch;
        let side = *a == actions::LEFT || *a == actions::RIGHT;
        // Over a grid of covers from the right, the left button goes on.
        let (back, on) = if self.covers().is_some() && self.layout.style.is_rtl() {
            (actions::RIGHT, actions::LEFT)
        } else {
            (actions::LEFT, actions::RIGHT)
        };
        if side && self.filter_focused && self.layout.filter_row.is_none() {
            // On the chips, left and right go from chip to chip.
            let steps = if *a == actions::LEFT { -1 } else { 1 };
            self.set_filter(self.filter.step(steps), cx);
        } else if *a == actions::NEXT_PAGE || *a == actions::PREV_PAGE {
            self.turn(if *a == actions::NEXT_PAGE { 1 } else { -1 }, cx);
        } else if *a == actions::UP || *a == actions::DOWN {
            // A row: of covers in the grid.
            let row = self.covers().map_or(1, |g| isize::from(g.columns as i16));
            self.step(if *a == actions::UP { -row } else { row }, cx);
        } else if [actions::PREVIOUS, back].contains(a) {
            self.step(-1, cx);
        } else if [actions::NEXT, on].contains(a) {
            self.step(1, cx);
        } else if *a == actions::CONFIRM && self.look_button().is_some() {
            cx.command(Command::run(LOOK_AGAIN));
        } else if *a == actions::CONFIRM && self.filter_focused {
            self.choose_shown(cx);
        } else if *a == actions::CONFIRM {
            self.activate(self.focus, cx);
        } else if *a == actions::MENU {
            if touch || self.filter_focused || !self.offer(self.focus, cx) {
                open_menu(cx);
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let point = gesture.start;
        let hit = self
            .page_range(self.page_of(self.focus))
            .find(|&i| self.place(i).contains(point));
        if gesture.kind == GestureKind::LongPress {
            if let Some(index) = hit {
                self.focus = index;
                return if self.offer(index, cx) {
                    Handled::Yes
                } else {
                    Handled::No
                };
            }
            return Handled::No;
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = Some(ui::Paging::new(self.page_of(self.focus), self.pages()));
        let style = self.layout.style;
        if let Some(action) = self.layout.header.tap(cx.env(), &style, point, paging) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.look_button().is_some_and(|(b, _)| b.contains(point)) {
            cx.command(Command::run(LOOK_AGAIN));
            return Handled::Yes;
        }
        if self.layout.header.area().contains(point) {
            // The search button, else the title: the library's menu.
            if self.find.is_some_and(|b| b.contains(point)) {
                cx.command(Command::OpenScreen(FIND, Params::new()));
            } else {
                open_menu(cx);
            }
            return Handled::Yes;
        }
        if let Some(row) = self.layout.filter_row
            && row.contains(point)
        {
            if self.find.is_some_and(|b| b.contains(point)) {
                cx.command(Command::OpenScreen(FIND, Params::new()));
            } else {
                self.choose_shown(cx);
            }
            return Handled::Yes;
        }
        if let Some(n) = self.layout.chips.hit(point) {
            self.set_filter(Filter::ALL[n], cx);
            return Handled::Yes;
        }
        match hit {
            Some(index) => {
                self.focus = index;
                self.activate(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn heard(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = SHOWN.read(event, params) {
            match SHOW_OPTIONS.iter().position(|o| *o == picked.option) {
                Some(n) if n < SHOW_FILTERS.len() => {
                    self.filter_focused = false;
                    self.set_filter(SHOW_FILTERS[n], cx);
                    cx.request_redraw();
                }
                Some(_) => cx.command(Command::OpenScreen(FIND, Params::new())),
                None => {}
            }
            return;
        }
        let env = *cx.env();
        if *event == events::CATALOG_FAILED {
            // The folder could not be read: said instead of "collecting"
            // (the old catalog, if any, stays shown) until one is made.
            self.failed = true;
            self.card_gone = card_gone(&env);
            self.collecting = false;
            self.refilter(&env);
            cx.request_redraw();
            return;
        }
        if *event == events::CATALOG_CHANGED {
            // Made anew (books came, went or moved): the index from it,
            // or, while covered, when it shows (the one kept is of the old
            // catalog).
            self.failed = false;
            self.card_gone = false;
            if self.covered {
                self.gone.clear();
                self.collecting = false;
            } else if !self.collecting && self.sorted.as_ref().is_some_and(|s| s.is_current(&env)) {
                // The catalog as it was (a walk found nothing changed, or
                // a first walk's end added none to the books it showed):
                // the sorted books hold, nothing is read anew; the count
                // is of the books found so far no more.
                let said = core::mem::take(&mut self.filter_end);
                self.word(&env);
                if self.filter_end != said {
                    cx.request_redraw();
                }
            } else {
                self.rebuild(&env, true);
                self.ask(cx);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            return;
        }
        if events::BOOK_CHANGED.read(event, params).is_some() {
            // Finished, a favourite, read further (on its sheet, say): how
            // the books are known anew from the status logged, its page
            // read again. While covered, done when it shows.
            if !self.covered {
                match &mut self.sorted {
                    Some(sorted) if sorted.is_current(&env) => sorted.refresh(env.storage()),
                    _ => self.reopen(&env),
                }
                self.forget_gone();
                self.loaded_page = None;
                self.refilter_in_place(&env);
                self.ask(cx);
                cx.request_redraw();
            }
            return;
        }
        let gone = if let Some(moved) = events::BOOK_MOVED.read(event, params) {
            Some(moved.from)
        } else {
            events::BOOK_REMOVED
                .read(event, params)
                .map(|removed| removed.path)
        };
        if let Some(path) = gone {
            // Moved (to the archive, say) or removed: not shown where it
            // was until the kernel made the catalog anew.
            self.gone.push(hash(&path));
            if let Some(sorted) = &mut self.sorted {
                sorted.hide(env.storage(), hash(&path));
            }
            self.forget_gone();
            self.ask(cx);
            if !self.covered {
                self.refilter_in_place(&env);
                cx.request_redraw();
            }
        } else if *event == events::SETTING_CHANGED {
            let root = library::folder(&env);
            if root != self.root {
                // Another library folder: the kernel makes its catalog;
                // while covered, the index is made when it shows.
                if self.covered {
                    self.root = root.to_string();
                    self.gone.clear();
                    self.kept_focus = None;
                } else {
                    *self = Self::new(&env);
                    self.fresh = false;
                }
            } else if !self.covered {
                // The view, the order, the size of the interface: laid out
                // anew, the focus on its book; another archive folder: the
                // index anew.
                self.by_recent = env.settings().read(&crate::SORT) != crate::SORT_NAME;
                self.layout = Layout::new(&env);
                if archive(&env) != self.archive {
                    self.archive = archive(&env);
                    self.reopen(&env);
                    self.ask(cx);
                }
                self.refilter_in_place(&env);
            } else {
                // Laid out anew when it shows.
                self.by_recent = env.settings().read(&crate::SORT) != crate::SORT_NAME;
                self.layout = Layout::new(&env);
            }
            cx.request_redraw();
        }
    }

    /// Wakes for the covers of the page shown not read yet.
    fn wake_for_covers(&self, cx: &mut Cx<'_, '_>) {
        if !self.covered {
            self.pace.wake(cx, &self.loaded);
        }
    }
}
