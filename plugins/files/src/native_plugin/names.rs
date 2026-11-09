use super::*;

impl NativeFiles {
    pub(super) fn read_shelves(&self, host: &Host<'_>) -> Result<Shelves, Error> {
        let mut say = Speaker {
            host,
            messages: Messages::new()?,
        };
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut rows = Vec::new();
        let mut items = Vec::new();
        // Always offered: looking for a next book costs host calls for each
        // series, so the list says when there is none (`files.list.none.next`).
        let next = say.say("files.list.show.next")?;
        push(&mut rows, ShelfRow::Next)?;
        push(&mut items, Item::noted(&next, "›", "")?)?;
        // The favourites, and each list of names, with how many they hold at
        // the row's end; a count the host will not give is left out.
        let favourites = say.say("files.favourites")?;
        let query = Query::new().show(Show::Favourites);
        let held = library
            .query(host, &query, &mut input, &mut out)
            .map(|page| page.end().total());
        let count = match counted(held)? {
            Some(total) => say.count("files.books", i64::from(total))?,
            None => copy("›")?,
        };
        push(&mut rows, ShelfRow::Favourites)?;
        push(&mut items, Item::noted(&favourites, &count, "")?)?;
        for shelf in Shelf::ALL {
            let title = say.say(shelf.title_key())?;
            let held = library
                .facets(host, &Query::new(), shelf.facet(), &mut input, &mut out)
                .map(|page| page.end().total());
            let count = match counted(held)? {
                Some(total) => say.count(shelf.count_key(), i64::from(total))?,
                None => copy("›")?,
            };
            push(&mut rows, ShelfRow::By(shelf))?;
            push(&mut items, Item::noted(&title, &count, "")?)?;
        }
        Ok(Shelves {
            rows,
            list: ItemList::new(items),
        })
    }

    /// The whole name at row `at` of the window that was read from
    /// `start`: the pages are read again, none kept.
    pub(super) fn whole_name(
        &self,
        host: &Host<'_>,
        shelf: Shelf,
        start: CatalogCursor,
        at: usize,
    ) -> Result<String, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let (mut cursor, mut before) = (start, 0usize);
        for _ in 0..=native_model::MOST_BOOKS {
            let query = Query::new().cursor(cursor);
            let page = library.facets(host, &query, shelf.facet(), &mut input, &mut out)?;
            if let Some(row) = page.rows().nth(at - before) {
                return copy(row?.text());
            }
            before += page.len();
            match page.end().next() {
                Some(next) if !page.is_empty() => cursor = next,
                _ => break,
            }
        }
        // The catalog changed under the list: it is read again.
        Err(Error::State)
    }

    /// Opens the list of the names of `shelf`.
    pub(super) fn open_names(&mut self, host: &Host<'_>, shelf: Shelf) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(KIND, shelf.kind())?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen("files.names")?.handover(handover))
    }

    /// The names of `shelf`, `at_least` of them at once and the focus on
    /// row `focus`.
    pub(super) fn read_names(
        &self,
        host: &Host<'_>,
        shelf: Shelf,
        reading: Reading,
        at_least: usize,
        focus: usize,
    ) -> Result<Names, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let Reading { start, mut starts } = reading;
        let (window, restarted) = {
            let mut source = NameReplies {
                host,
                library,
                shelf,
                input: &mut input,
                out: &mut out,
            };
            native_model::read_window(&mut source, start, CatalogCursor::start(), at_least.max(1))?
        };
        if restarted {
            starts.clear();
        }
        let mut names = Names {
            shelf,
            window,
            starts,
            rows: Vec::new(),
            list: ItemList::default(),
            empty: String::new(),
        };
        Self::layout_names(host, &mut names)?;
        names.list.set_focus(focus);
        Ok(names)
    }

    /// The rows and items of `names`.
    pub(super) fn layout_names(host: &Host<'_>, names: &mut Names) -> Result<(), Error> {
        let mut say = Speaker {
            host,
            messages: Messages::new()?,
        };
        let mut rows = Vec::new();
        let mut items = Vec::new();
        if !names.starts.is_empty() {
            let previous = say.say("files.list.previous")?;
            push(&mut rows, Row::Previous)?;
            push(&mut items, Item::noted(&previous, "‹", "")?)?;
        }
        for (at, name) in names.window.books.iter().enumerate() {
            let (title, note) = native_shelves::name_row(&mut say, names.shelf, name)?;
            push(&mut rows, Row::Book(at))?;
            // The count is at the row's end.
            push(&mut items, Item::noted(&title, &note, "")?)?;
        }
        if names.window.has_more() {
            let more = say.say("files.list.more")?;
            push(&mut rows, Row::More)?;
            push(&mut items, Item::noted(&more, "›", "")?)?;
        }
        names.empty = if names.window.books.is_empty() {
            say.say(names.shelf.none_key())?
        } else {
            String::new()
        };
        names.rows = rows;
        names.list = ItemList::new(items);
        Ok(())
    }

    /// "More" and "Previous" of a list of names (as of books).
    pub(super) fn slide_names(
        &mut self,
        host: &Host<'_>,
        token: u32,
        back: bool,
    ) -> Result<(), Error> {
        let Some(View::Names(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let shelf = view.shelf;
        let step = if back {
            match step_back(&view.starts)? {
                Some(step) => step,
                None => return Ok(()),
            }
        } else {
            step_on(&view.window, &view.starts, view.list.focus())?
        };
        let mut fresh = self.read_names(host, shelf, step.reading, step.at_least, step.focus)?;
        match step.then {
            Then::Keep => {}
            Then::FirstBook => {
                let at = fresh
                    .rows
                    .iter()
                    .position(|row| matches!(row, Row::Book(_)))
                    .unwrap_or(0);
                fresh.list.set_focus(at);
            }
            Then::LastRow => fresh.list.set_focus(fresh.rows.len().saturating_sub(1)),
        }
        if let Some(View::Names(old)) = self.screens.top(token) {
            *old = fresh;
        }
        redraw(host)
    }

    /// The row at `index` of the screen on `token` was chosen.
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
                    Row::Filter | Row::Note | Row::Field => Ok(()),
                }
            }
            Some(View::Books(_)) => self.chosen_book(host, token, index),
            Some(View::Chooser(_)) => self.chose_folder(host, token, index),
            Some(View::Info(_)) | None => Ok(()),
        }
    }
}

/// What the host said of a count: the number, none where it would not say
/// (the row then shows no count), or the lack of memory.
fn counted(held: Result<u32, Error>) -> Result<Option<u32>, Error> {
    match held {
        Ok(total) => Ok(Some(total)),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}
