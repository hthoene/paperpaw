use super::*;

impl NativeFiles {
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
        let (window, restarted) = {
            let mut source = Replies {
                host,
                library,
                selection: &selection,
                input: &mut input,
                out: &mut out,
            };
            native_model::read_window(&mut source, start, CatalogCursor::start(), at_least.max(1))?
        };
        if restarted {
            // The windows before were read in another generation.
            starts.clear();
        }
        // The books are still being found: the rows so far say so.
        let walking = matches!(
            library.state(host, &mut input, &mut out)?.phase(),
            CatalogPhase::Walking | CatalogPhase::Empty
        ) && selection.scope == Scope::Library;
        let mut browser = Browser {
            title,
            selection,
            window,
            starts,
            rows: Vec::new(),
            list: ItemList::default(),
            empty: String::new(),
            empty_text: String::new(),
        };
        self.layout(host, &mut browser, walking)?;
        browser.list.set_focus(focus);
        Ok(browser)
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

    /// The rows and items of `browser`'s books.
    pub(super) fn layout(
        &self,
        host: &Host<'_>,
        browser: &mut Browser,
        walking: bool,
    ) -> Result<(), Error> {
        let mut say = Speaker {
            host,
            messages: Messages::new()?,
        };
        let mut rows = Vec::new();
        let mut items = Vec::new();
        if browser.selection.scope == Scope::Library {
            let show = say.say("files.list.show")?;
            let name = say.say(browser.selection.filter.key())?;
            let count = if browser.selection.filter == Filter::Shelves {
                String::new()
            } else {
                say.messages.format(
                    host,
                    "files.books",
                    &[Arg::Count(i64::from(browser.window.total))],
                )?
            };
            push(&mut rows, Row::Filter)?;
            push(&mut items, Item::noted(&show, &name, &count)?)?;
        }
        if walking {
            let note = say.say("files.list.collecting")?;
            push(&mut rows, Row::Note)?;
            push(&mut items, Item::heading(&note)?)?;
        }
        if !browser.starts.is_empty() {
            let previous = say.say("files.list.previous")?;
            push(&mut rows, Row::Previous)?;
            push(&mut items, Item::noted(&previous, "‹", "")?)?;
        }
        for (at, book) in browser.window.books.iter().enumerate() {
            let line = native_model::line(&mut say, book)?;
            push(&mut rows, Row::Book(at))?;
            push(
                &mut items,
                Item::noted(&line.title, &line.value, &line.note)?,
            )?;
        }
        if browser.window.has_more() {
            let more = say.say("files.list.more")?;
            push(&mut rows, Row::More)?;
            push(&mut items, Item::noted(&more, "›", "")?)?;
        }
        browser.empty = String::new();
        browser.empty_text = String::new();
        if browser.window.books.is_empty() && !walking {
            let library = browser.selection.scope == Scope::Library;
            match browser.selection.filter.none_key() {
                Some(key) if library => browser.empty = say.say(key)?,
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
        }
        browser.rows = rows;
        browser.list = ItemList::new(items);
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
            Then::LastRow => fresh.list.set_focus(fresh.rows.len().saturating_sub(1)),
        }
        if let Some(View::Books(old)) = self.screens.top(token) {
            *old = fresh;
        }
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

    pub(super) fn chosen(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        match self.screens.top(token) {
            Some(View::Shelves(view)) => match view.rows.get(index).copied() {
                Some(ShelfRow::Next) => self.open_shelf(host, &Scope::Next),
                Some(ShelfRow::Favourites) => self.open_shelf(host, &Scope::Favourites),
                Some(ShelfRow::By(shelf)) => self.open_names(host, shelf),
                None => Ok(()),
            },
            Some(View::Names(view)) => {
                let Some(row) = view.rows.get(index).copied() else {
                    return Ok(());
                };
                match row {
                    Row::More => self.slide_names(host, token, false),
                    Row::Previous => self.slide_names(host, token, true),
                    Row::Book(at) => {
                        let Some(name) = view.window.books.get(at) else {
                            return Ok(());
                        };
                        let (shelf, start) = (view.shelf, view.window.start);
                        let scope = if name.cut {
                            // Held cut: the host has the whole name.
                            let whole = self.whole_name(host, shelf, start, at)?;
                            shelf.scope(&whole)?
                        } else {
                            shelf.scope(&name.text)?
                        };
                        self.open_shelf(host, &scope)
                    }
                    Row::Filter | Row::Note => Ok(()),
                }
            }
            Some(View::Info(_)) => Ok(()),
            Some(View::Books(view)) => {
                let Some(row) = view.rows.get(index).copied() else {
                    return Ok(());
                };
                match row {
                    Row::Note => Ok(()),
                    Row::Filter => {
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
                        if let Some(View::Books(old)) = self.screens.top(token) {
                            *old = fresh;
                        }
                        if next == Filter::Shelves {
                            let mut scratch = [0; 128];
                            CommandSender::new(host, &mut scratch)
                                .open(Open::screen("files.shelves")?)?;
                        }
                        redraw(host)
                    }
                    Row::More => self.slide_on(host, token),
                    Row::Previous => self.slide_back(host, token),
                    Row::Book(at) => {
                        let Some(book) = view.window.books.get(at) else {
                            return Ok(());
                        };
                        let reference = BookRef::catalog(book.key, view.window.generation)?;
                        if self.open_book(host, reference)? {
                            return Ok(());
                        }
                        // The book is gone (deleted on a computer, say): the
                        // list is read again and the user is told.
                        self.reread(host)?;
                        let mut scratch = [0; 256];
                        let mut say = Messages::new()?;
                        let text = say.copy(host, "files.list.gone")?;
                        // A notice is best effort: the list is already right.
                        CommandSender::new(host, &mut scratch)
                            .notify(Notify::new(&text)?)
                            .ok();
                        Ok(())
                    }
                }
            }
            None => Ok(()),
        }
    }
}
impl Browser {
    /// Focus on the first book (after the filter row and the way back).
    pub(super) fn focus_first_book(&mut self) {
        let at = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Book(_)))
            .unwrap_or(0);
        self.list.set_focus(at);
    }
}
