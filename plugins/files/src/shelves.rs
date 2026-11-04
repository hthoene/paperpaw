//! The library's authors, series and tags.
//!
//! They are read from the library's catalog
//! ([`paperpaw_sdk::library::catalog`]), which the kernel makes anew in
//! the background (after PaperPaw starts, after books came, went or moved
//! or the library folder changed); a view shows the catalog there was
//! until the new one is complete, then reads it again
//! ([`events::CATALOG_CHANGED`]).
//!
//! A view lists the authors (or series, tags, folders) with how many
//! books each has; choosing one lists those books with their covers and
//! states ([`crate::list::shelf`]), a series by its numbers. Only a
//! window of the names is held in memory ([`crate::names`]), never the
//! whole catalog: moving past its ends takes the next (a pass over the
//! catalog).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode, Screen,
};

use crate::list::shelf::{self as books, Kind};
use crate::names::{Bound, Budget, Firsts};
use paperpaw_sdk::library::{
    self,
    catalog::{self, Record, State},
};

/// The view of authors (`mode` = `authors`), series (`series`) or tags
/// (`tags`).
pub(crate) const MODE_AUTHORS: &str = "authors";
pub(crate) const MODE_SERIES: &str = "series";
pub(crate) const MODE_TAGS: &str = "tags";
/// The folders in the folder of parameter `path` (`folders`).
pub(crate) const MODE_FOLDERS: &str = "folders";

/// What books are listed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shelf {
    Authors,
    Series,
    Tags,
    /// The folders in a folder (the path before them, with its `/`).
    Folders(String),
}

impl Shelf {
    fn of(params: &Params) -> Shelf {
        match params.get(params::MODE) {
            Some(MODE_SERIES) => Shelf::Series,
            Some(MODE_TAGS) => Shelf::Tags,
            Some(MODE_FOLDERS) => Shelf::Folders(prefix(params.get(params::PATH).unwrap_or(""))),
            _ => Shelf::Authors,
        }
    }

    /// The list of books one of its names opens.
    fn books(&self) -> Kind {
        match self {
            Shelf::Authors => Kind::Author,
            Shelf::Series => Kind::Series,
            Shelf::Tags => Kind::Tag,
            Shelf::Folders(_) => Kind::Folder,
        }
    }
}

/// Opens the folders in the folder `root`, all of them.
pub(crate) fn open_folders(root: &str) -> Command {
    Command::OpenScreen(
        crate::SHELF_SCREEN,
        Params::new()
            .with(params::MODE, MODE_FOLDERS)
            .with(params::PATH, root.to_string()),
    )
}

/// The path of folder `root` with its `/`, as the paths in it begin
/// (none for the whole card).
pub(crate) fn prefix(root: &str) -> String {
    let root = root.trim_matches('/');
    if root.is_empty() {
        String::new()
    } else {
        alloc::format!("{root}/")
    }
}

/// The folder of the book at `path` right in the folder of `prefix`
/// (see [`prefix`]), its whole path; none for a book right in it.
pub(crate) fn folder_in<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let inner = path.strip_prefix(prefix)?;
    let (folder, _) = inner.split_once('/')?;
    path.get(..path.len() - inner.len() + folder.len())
}

/// What a book is listed under in a view: its author, its series, each
/// of its tags, or its folder.
fn keys<'a>(record: &'a Record, shelf: &Shelf) -> Vec<&'a str> {
    match shelf {
        Shelf::Authors => alloc::vec![record.author.as_str()],
        Shelf::Series => alloc::vec![record.series.as_str()],
        Shelf::Tags => record.tags().collect(),
        Shelf::Folders(prefix) => folder_in(&record.path, prefix).into_iter().collect(),
    }
}

/// A window of the names of the authors, series, tags or folders
/// (`bound`, at most a `budget` of them), alphabetically, and how many books each
/// has; and whether there are names before its first and after its
/// last.
fn names(
    books: catalog::Books<'_>,
    shelf: &Shelf,
    budget: Budget,
    bound: Bound,
) -> (Vec<(String, usize)>, bool, bool) {
    let mut firsts = Firsts::new(budget, bound);
    books(&mut |_, line| {
        for key in keys(&line, shelf) {
            // Books without series are no series; those without author
            // are listed under "Unknown author".
            if *shelf == Shelf::Series && key.is_empty() {
                continue;
            }
            firsts.add(key, false);
        }
    });
    let (names, before, after) = firsts.into_names();
    let names = names.into_iter().map(|n| (n.name, n.count)).collect();
    (names, before, after)
}

pub(crate) struct ShelfScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    shelf: Shelf,
    /// The window of names shown ([`names`]), and whether there are
    /// names before its first, after its last.
    names: Vec<(String, usize)>,
    before: bool,
    after: bool,
    /// No catalog yet: the kernel is making the first.
    collecting: bool,
    /// The kernel could not make the catalog (`CATALOG_FAILED`).
    failed: bool,
    /// The memory card is lost until a restart (`list::card_gone`).
    card_gone: bool,
}

impl ShelfScreen {
    /// The style, the header and the list placed for `env`'s screen.
    fn frame(env: &Env<'_>) -> (UiStyle, ui::Header, ListView) {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let list = ListView::new(area, style.noted_row()).focus_on_keys(env);
        (style, header, list)
    }

    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let (style, header, list) = Self::frame(env);
        let shelf = Shelf::of(params);
        let mut screen = Self {
            list,
            names: Vec::new(),
            before: false,
            after: false,
            collecting: false,
            failed: false,
            card_gone: false,
            shelf,
            style,
            header,
        };
        screen.load(env, Bound::First);
        screen
    }

    /// The window of names from `bound` (the first where it has none).
    fn load(&mut self, env: &Env<'_>, bound: Bound) {
        let first = bound == Bound::First;
        // The window there was goes first: never two at once.
        self.names = Vec::new();
        let budget = Budget::of(env.device().memory);
        let each = &mut |f: &mut dyn FnMut(u64, Record)| library::service(env).each(env, f);
        let (names, before, after) = names(each, &self.shelf, budget, bound);
        self.names = names;
        (self.before, self.after) = (before, after);
        if self.names.is_empty() && !first {
            // Nothing there any more: from the first.
            self.load(env, Bound::First);
            return;
        }
        self.list.set_len(self.names.len());
    }

    /// Moves the focus `delta` rows on: past the window's ends into the
    /// next or the one before, and a row on from the last name to the
    /// first (and back), as lists go round.
    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.names.len();
        let to = self.list.focus() as isize + delta;
        let round = delta.abs() == 1;
        let next = if to >= len as isize && self.after {
            self.names.last().map(|(n, _)| Bound::After(n.clone()))
        } else if to >= len as isize && round && self.before {
            Some(Bound::First)
        } else if to < 0 && self.before {
            self.names.first().map(|(n, _)| Bound::Before(n.clone()))
        } else if to < 0 && round && self.after {
            Some(Bound::Last)
        } else {
            None
        };
        let Some(bound) = next else {
            let change = self.list.move_focus_in(cx.env(), delta, len);
            change.request(cx);
            return;
        };
        let backwards = bound.backwards();
        let env = *cx.env();
        self.load(&env, bound);
        let focus = if backwards {
            self.names.len().saturating_sub(1)
        } else {
            0
        };
        self.list.set_focus(focus);
        self.redraw(cx);
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Opens the books of name `index`.
    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some((name, _)) = self.names.get(index) {
            cx.command(books::open(self.shelf.books(), name));
        }
    }

    fn title(&self) -> &'static str {
        match self.shelf {
            Shelf::Authors => "files.by-author",
            Shelf::Series => "files.by-series",
            Shelf::Tags => "files.by-tag",
            Shelf::Folders(_) => "files.by-folder",
        }
    }
}

impl Screen for ShelfScreen {
    /// Turned or resized: laid out anew, the focus on its name.
    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let (style, header, fresh) = Self::frame(&env);
        // The focus, whether it shows and the page stay with the list.
        self.list.set_area(fresh.area());
        self.list.set_row(fresh.row());
        (self.style, self.header) = (style, header);
        cx.request_redraw();
        Handled::Yes
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.names.len();
        let title = env.i18n().tr(self.title());
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        if len == 0 {
            let area = self.list.area();
            // An empty view says what would be in it.
            let (icon, text) = match self.shelf {
                _ if self.failed => {
                    let (title, text) = crate::list::failed_words(self.card_gone);
                    let (title, text) = (env.i18n().tr(title), env.i18n().tr(text));
                    ui.empty(area, "books", title, text);
                    return;
                }
                _ if self.collecting => {
                    let message = env.i18n().tr("files.list.collecting");
                    ui.message(area, message);
                    return;
                }
                Shelf::Series => ("books", "files.no-series"),
                Shelf::Tags => ("tag", "files.no-tags"),
                Shelf::Authors | Shelf::Folders(_) => ("books", "files.no-books"),
            };
            let text = ui::tr_input(env, text);
            ui.empty(area, icon, "", text);
            return;
        }
        let (names, folders) = (&self.names, matches!(self.shelf, Shelf::Folders(_)));
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let (name, count) = &names[index];
            let name = if name.is_empty() {
                env.i18n().tr("files.unknown-author")
            } else if folders {
                paperpaw_sdk::files::name(name)
            } else {
                name.as_str()
            };
            let note = env.i18n().plural("files.books", *count as i64, &[]);
            ui::Row::value(name, "›")
                .note(&note)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let state = library::service(&env).catalog_state(&env);
        self.collecting = crate::list::collecting(&env);
        if state == State::Missing {
            // No catalog yet: the kernel makes it, then it shows.
            cx.command(library::request_catalog());
        }
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        // The folder could not be read: said until a catalog is made.
        if *event == events::CATALOG_FAILED {
            self.failed = true;
            self.card_gone = crate::list::card_gone(cx.env());
            self.collecting = false;
            self.redraw(cx);
            return;
        }
        // The new catalog, or a book gone (noted beside it): the names
        // again, keeping the place.
        if [
            events::CATALOG_CHANGED.id(),
            events::BOOK_REMOVED.id(),
            events::BOOK_MOVED.id(),
        ]
        .contains(&event)
        {
            let env = *cx.env();
            self.collecting = false;
            self.failed = false;
            self.card_gone = false;
            let bound = match self.names.first() {
                Some((first, _)) if self.before => Bound::From(first.clone()),
                _ => Bound::First,
            };
            self.load(&env, bound);
            self.redraw(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.move_focus(delta, cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = self.names.len();
        let paging = self.list.paging(len);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, len) {
            Some(index) => {
                self.list.set_focus(index);
                self.activate(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::library;

    #[test]
    fn a_folder_is_found_only_in_its_folder() {
        assert_eq!(folder_in("books/A/b.epub", "books/"), Some("books/A"));
        assert_eq!(folder_in("books/b.epub", "books/"), None);
        assert_eq!(folder_in("other/A/b.epub", "books/"), None);
        assert_eq!(folder_in("A/b.epub", ""), Some("A"));
    }

    #[test]
    fn catalog_lines_and_views() {
        let storage = MemoryStorage::new();
        let books = [
            (
                "b/2.epub",
                "Ann",
                "Second",
                "Tales",
                "2",
                &["Fantasy", "Kids"][..],
            ),
            ("b/1.epub", "Ann", "First", "Tales", "1", &["Fantasy"]),
            ("b/x.epub", "bob", "Alone", "", "", &[]),
            ("b/y.txt", "", "Nobody's", "", "", &[]),
        ];
        let records: Vec<Record> = books
            .iter()
            .map(|(path, author, title, series, number, tags)| {
                let some = |s: &str| (!s.is_empty()).then(|| String::from(s));
                let book = library::Book::default()
                    .author(some(author))
                    .title(some(title))
                    .series(some(series))
                    .series_index(some(number))
                    .tags(tags.iter().map(|t| String::from(*t)).collect());
                Record::of(path, &book)
            })
            .collect();
        catalog::write(&storage, &records).unwrap();
        let ten = Budget {
            names: 10,
            bytes: usize::MAX,
        };
        let authors = names(
            &mut |f| catalog::each_at(&storage, f),
            &Shelf::Authors,
            ten,
            Bound::First,
        )
        .0;
        assert_eq!(
            authors,
            [
                (String::new(), 1),
                (String::from("Ann"), 2),
                (String::from("bob"), 1)
            ]
        );
        assert_eq!(
            names(
                &mut |f| catalog::each_at(&storage, f),
                &Shelf::Series,
                ten,
                Bound::First
            )
            .0,
            [(String::from("Tales"), 2)]
        );
        assert_eq!(
            names(
                &mut |f| catalog::each_at(&storage, f),
                &Shelf::Tags,
                ten,
                Bound::First
            )
            .0,
            [(String::from("Fantasy"), 2), (String::from("Kids"), 1)]
        );
        // Searching: every word, anywhere, in any case.
        let found: Vec<String> = catalog::search(&storage, "ann TALES")
            .into_iter()
            .map(|l| l.title)
            .collect();
        assert_eq!(found, ["First", "Second"]);
        assert_eq!(catalog::search(&storage, "kids")[0].title, "Second");
        assert!(catalog::search(&storage, "nothing").is_empty());
    }
}
