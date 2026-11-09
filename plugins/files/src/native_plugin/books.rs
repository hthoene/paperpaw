use super::*;
use crate::{
    native_list::{Cell, Row as ListRow},
    native_model::line,
};

/// Whether the first book of a library's first window is shown as the card:
/// the library in the order of reading, its first book begun and not finished.
fn card_first(selection: &Selection, starts: &[CatalogCursor], first: Option<&BookLine>) -> bool {
    selection.scope == Scope::Library
        && selection.filter == Filter::All
        && selection.order == Order::Recent
        && starts.is_empty()
        && first.is_some_and(|book| !book.finished && book.progress.is_some_and(|p| p > 0))
}

/// The bar under a book: how far it is read, shown from 1 to 99 per cent.
const fn bar(book: &BookLine) -> Option<u8> {
    match book.progress {
        Some(percent) if !book.finished && percent > 0 && percent < 100 => Some(percent),
        _ => None,
    }
}

impl NativeFiles {
    /// Whether the books show as a grid of covers (`files.view`).
    pub(super) fn grid(host: &Host<'_>) -> Result<bool, Error> {
        let mut scratch = [0; 64];
        Ok(match settings::read_setting(host, VIEW, &mut scratch)? {
            SettingValue::Text(key) => key == VIEW_COVERS,
            _ => false,
        })
    }

    /// Whether the card answers: the probe of `files.md` section 3 (`stat` of
    /// the card's root); a read error means it is gone.
    pub(super) fn card_present(host: &Host<'_>) -> Result<bool, Error> {
        let files = match Files::open(host) {
            Ok(files) => files,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // No service to ask: nothing says the card is gone.
            Err(_) => return Ok(true),
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let found = files.stat(host, Root::Files, "", &mut input, &mut out);
        // A handle that cannot be released is no reason to lose the answer.
        files.release(host).ok();
        match found {
            Err(Error::Io) => Ok(false),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => Ok(true),
        }
    }

    /// The books of `selection`, `at_least` of them at once (a window read
    /// again keeps its size) and the focus on row `focus`.
    pub(super) fn read_browser(
        &self,
        host: &Host<'_>,
        selection: Selection,
        reading: Reading,
        at_least: usize,
        focus: usize,
    ) -> Result<Browser, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut say = Messages::new()?;
        let title = Self::title(host, &mut say, &selection.scope)?;
        let Reading { start, mut starts } = reading;
        let read = {
            let mut source = Replies {
                host,
                library,
                selection: &selection,
                input: &mut input,
                out: &mut out,
            };
            native_model::read_window(&mut source, start, CatalogCursor::start(), at_least.max(1))
        };
        let (window, restarted, unreadable) = match read {
            Ok((window, restarted)) => (window, restarted, false),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // The folder cannot be read: the list says so, it does not fail.
            Err(_) => (Window::new(CatalogCursor::start()), true, true),
        };
        if restarted {
            // The windows before were read in another generation.
            starts.clear();
        }
        let state = library
            .state(host, &mut input, &mut out)
            .map(|state| (state.phase(), state.books()));
        let card = if unreadable || state.is_err() {
            Self::card_present(host)?
        } else {
            true
        };
        let condition = match state {
            Ok(_) if unreadable && card => Condition::Failed,
            Ok(_) if unreadable => Condition::CardGone,
            state => Condition::of(state, card)?,
        };
        let archived = window.books.is_empty()
            && condition == Condition::Ready
            && selection.scope == Scope::Library
            && selection.filter == Filter::All
            && Self::archive_has_books(host, library, &mut input, &mut out);
        let covers = Covers::new(
            window.books.len(),
            if self.covers_most == 0 {
                MOST_SMALL
            } else {
                self.covers_most
            },
        )?;
        let mut browser = Browser {
            title,
            selection,
            window,
            starts,
            list: BookList::default(),
            covers,
            empty: String::new(),
            empty_text: String::new(),
        };
        self.layout(host, &mut browser, condition, archived)?;
        browser.list.set_focus(focus);
        Ok(browser)
    }

    /// Whether the library has books at all, the archive's included (every
    /// book of an empty list may be in it). A refused ask says no.
    fn archive_has_books(
        host: &Host<'_>,
        library: &Library,
        input: &mut [u8],
        out: &mut [u8],
    ) -> bool {
        library
            .query(host, &Query::new(), input, out)
            .is_ok_and(|page| page.end().total() > 0)
    }

    pub(super) fn title(
        host: &Host<'_>,
        say: &mut Messages,
        scope: &Scope,
    ) -> Result<String, Error> {
        match scope {
            Scope::Library | Scope::Folder(_) => say.copy(host, "files.title"),
            Scope::Favourites => say.copy(host, "files.favourites"),
            Scope::Next => say.copy(host, "files.list.show.next"),
            Scope::Author(name) | Scope::Series(name) | Scope::Tag(name) => copy(name),
            Scope::Search(text) => say.format(
                host,
                "files.search.title",
                &[Arg::Text {
                    name: "text",
                    value: text,
                }],
            ),
        }
    }

    /// The cells of `browser`'s books, and what an empty list says.
    pub(super) fn layout(
        &self,
        host: &Host<'_>,
        browser: &mut Browser,
        condition: Condition,
        archived: bool,
    ) -> Result<(), Error> {
        let mut say = Speaker {
            host,
            messages: Messages::new()?,
        };
        let library = browser.selection.scope == Scope::Library;
        let mut cells = Vec::new();
        if library {
            let show = say.say("files.list.show")?;
            let name = say.say(browser.selection.filter.key())?;
            let count = if browser.selection.filter == Filter::Shelves || condition.counting() {
                String::new()
            } else {
                say.count("files.books", i64::from(browser.window.total))?
            };
            let value = if count.is_empty() {
                name
            } else {
                paperpaw_ui_kit::text::concat(&[&name, " · ", &count])?
            };
            push(&mut cells, Cell::plain(ListRow::Filter, &show, &value)?)?;
        }
        if let (Condition::Scanning { books }, true) = (condition, library) {
            // The count leads: the books found so far.
            let note = say.count("files.list.so-far", i64::from(books))?;
            push(&mut cells, Cell::plain(ListRow::Note, &note, "")?)?;
        }
        if !browser.starts.is_empty() {
            let previous = say.say("files.list.previous")?;
            push(&mut cells, Cell::plain(ListRow::Previous, &previous, "‹")?)?;
        }
        let card = card_first(
            &browser.selection,
            &browser.starts,
            browser.window.books.first(),
        );
        for (at, book) in browser.window.books.iter().enumerate() {
            let line = line(&mut say, book)?;
            let first = card && at == 0;
            let value = if first {
                say.say("files.list.continue-reading")?
            } else {
                line.value
            };
            push(
                &mut cells,
                Cell {
                    row: ListRow::Book(at),
                    title: line.title,
                    value,
                    note: line.note,
                    progress: bar(book),
                    card: first,
                    chips: native_model::chips(book)?,
                },
            )?;
        }
        if browser.window.has_more() {
            let more = say.say("files.list.more")?;
            push(&mut cells, Cell::plain(ListRow::More, &more, "›")?)?;
        }
        self.say_empty(host, &mut say, browser, condition, archived)?;
        browser.list = BookList::new(cells, Self::grid(host)?);
        Ok(())
    }

    /// What an empty list says: the condition's words, or what the filter or
    /// scope has none of.
    fn say_empty(
        &self,
        host: &Host<'_>,
        say: &mut Speaker<'_, '_>,
        browser: &mut Browser,
        condition: Condition,
        archived: bool,
    ) -> Result<(), Error> {
        browser.empty = String::new();
        browser.empty_text = String::new();
        if !browser.window.books.is_empty() || condition.counting() {
            return Ok(());
        }
        if let Some((title, text)) = condition.words() {
            browser.empty = say.say(title)?;
            if !text.is_empty() {
                browser.empty_text = say.say(text)?;
            }
            return Ok(());
        }
        let library = browser.selection.scope == Scope::Library;
        match browser.selection.filter.none_key() {
            Some(key) if library => browser.empty = say.say(key)?,
            _ if library && archived => browser.empty = say.say("files.list.none.all")?,
            _ if library => {
                browser.empty = say.say("files.empty.title")?;
                let folder = Self::folder(host)?;
                browser.empty_text = say.messages.format(
                    host,
                    "files.empty",
                    &[Arg::Text {
                        name: "dir",
                        value: &folder,
                    }],
                )?;
            }
            _ if browser.selection.scope == Scope::Next => {
                browser.empty = say.say("files.list.none.next")?;
            }
            _ if browser.selection.scope == Scope::Favourites => {
                browser.empty = say.say("files.no-favourites.title")?;
                browser.empty_text = say.say("files.no-favourites")?;
            }
            _ => browser.empty = say.say("files.search.none")?,
        }
        Ok(())
    }

    /// Reads the books of `token`'s screen as `plan` says, and shows them.
    pub(super) fn replace_books(
        &mut self,
        host: &Host<'_>,
        token: u32,
        plan: impl FnOnce(&Browser) -> Result<Option<Plan>, Error>,
    ) -> Result<(), Error> {
        let Some(View::Books(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(plan) = plan(view)? else {
            return Ok(());
        };
        let mut fresh = self.read_browser(
            host,
            plan.selection,
            plan.reading,
            plan.at_least,
            plan.focus,
        )?;
        match plan.then {
            Then::Keep => {}
            Then::FirstBook => fresh.focus_first_book(),
            Then::LastRow => fresh
                .list
                .set_focus(fresh.list.cells().len().saturating_sub(1)),
        }
        Self::put_browser(host, self.screens.top(token), fresh);
        redraw(host)
    }

    /// "More books": a window with room grows, a full one slides on to the
    /// books after it (the starts of the ones before are kept for "Previous").
    pub(super) fn slide_on(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        self.replace_books(host, token, |view| {
            let step = step_on(&view.window, &view.starts, view.list.focus())?;
            Ok(Some(Plan::of(view.selection.clone(), step)))
        })
    }

    /// "Previous books": the window before, read in full, on the row that
    /// goes on again.
    pub(super) fn slide_back(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        self.replace_books(host, token, |view| {
            Ok(step_back(&view.starts)?.map(|step| Plan::of(view.selection.clone(), step)))
        })
    }

    /// The cell at `index` was chosen on a list of books.
    pub(super) fn chosen_book(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
    ) -> Result<(), Error> {
        let Some(View::Books(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(row) = view.list.cells().get(index).map(|cell| cell.row) else {
            return Ok(());
        };
        match row {
            ListRow::Note => Ok(()),
            ListRow::Filter => {
                let next = view.selection.filter.next();
                let mut selection = view.selection.clone();
                selection.filter = if next == Filter::Shelves {
                    Filter::All
                } else {
                    next
                };
                let fresh = self.read_browser(
                    host,
                    selection,
                    Reading::from(CatalogCursor::start(), Vec::new()),
                    0,
                    index,
                )?;
                Self::put_browser(host, self.screens.top(token), fresh);
                if next == Filter::Shelves {
                    let mut scratch = [0; 128];
                    CommandSender::new(host, &mut scratch).open(Open::screen("files.shelves")?)?;
                }
                redraw(host)
            }
            ListRow::More => self.slide_on(host, token),
            ListRow::Previous => self.slide_back(host, token),
            ListRow::Book(at) => {
                let Some(book) = view.window.books.get(at) else {
                    return Ok(());
                };
                let reference = BookRef::catalog(book.key, view.window.generation)?;
                if self.open_book(host, reference)? {
                    return Ok(());
                }
                // The book is gone (deleted on a computer, say): the list is
                // read again and the user is told.
                self.reread(host)?;
                self.notify(host, "files.list.gone")
            }
        }
    }
}

impl Browser {
    /// Focus on the first book (after the filter row and the way back).
    pub(super) fn focus_first_book(&mut self) {
        let at = self.list.first_book().unwrap_or(0);
        self.list.set_focus(at);
    }
}
