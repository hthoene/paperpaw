use super::*;

impl NativeFiles {
    pub(super) fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if self.covers_most == 0 {
            self.learn(host)?;
        }
        Ok(())
    }

    /// The device's class: a small one (the X4) holds fewer covers at once.
    fn learn(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.covers_most = MOST_SMALL;
        let device = match Device::open(host) {
            Ok(device) => device,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // No device service: the smaller number stays.
            Err(_) => return Ok(()),
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        if let Ok(row) = device.device(host, &mut input, &mut out)
            && matches!(row.class(), DeviceClass::Medium | DeviceClass::Large)
        {
            self.covers_most = MOST;
        }
        // A handle that cannot be released is no reason to lose the facts.
        device.release(host).ok();
        Ok(())
    }

    /// The order the user chose (`files.sort`).
    pub(super) fn order(host: &Host<'_>) -> Result<Order, Error> {
        let mut scratch = [0; 64];
        Ok(match settings::read_setting(host, SORT, &mut scratch)? {
            SettingValue::Text(key) => Order::from_key(key),
            _ => Order::default(),
        })
    }

    /// The library folder the user chose (`core.library-folder`), without
    /// slashes around it; the default when the setting says nothing.
    pub(super) fn folder(host: &Host<'_>) -> Result<String, Error> {
        let mut scratch = [0; 512];
        match settings::read_setting(host, FOLDER_SETTING, &mut scratch) {
            Ok(SettingValue::Text(text)) if !text.trim_matches('/').is_empty() => {
                copy(text.trim_matches('/'))
            }
            Ok(_) | Err(Error::Denied | Error::NotFound | Error::NoSpace) => copy(DEFAULT_FOLDER),
            Err(error) => Err(error),
        }
    }

    /// Opens a list of books of `scope`.
    pub(super) fn open_shelf(&mut self, host: &Host<'_>, scope: &Scope) -> Result<(), Error> {
        let (kind, value) = scope.params();
        self.open_list(host, kind, value)
    }

    /// Opens a list of books for the parameters `kind` and `value`.
    pub(super) fn open_list(
        &mut self,
        host: &Host<'_>,
        kind: &str,
        value: &str,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(KIND, kind)?, Param::new(VALUE, value)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen("files.shelf")?.handover(handover))
    }

    /// The parameters (`first`, `second`) a screen was opened with; empty
    /// for one that was not given.
    pub(super) fn handover_params(
        &self,
        host: &Host<'_>,
        handover: Option<HandoverId>,
        keys: [&str; 2],
    ) -> Result<[String; 2], Error> {
        let mut found = [String::new(), String::new()];
        let Some(handover) = handover else {
            return Ok(found);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        for param in ui.params(host, handover, &mut input, &mut out)? {
            let param = param?;
            if let Some(at) = keys.iter().position(|key| *key == param.key())
                && let Some(slot) = found.get_mut(at)
            {
                *slot = copy(param.value())?;
            }
        }
        Ok(found)
    }

    pub(super) fn opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let view = if token == SHELVES_TOKEN {
            View::Shelves(self.read_shelves(host)?)
        } else if token == NAMES_TOKEN {
            let [kind, _] = self.handover_params(host, handover, [KIND, VALUE])?;
            let shelf = Shelf::from_kind(&kind).ok_or(Error::Invalid)?;
            View::Names(self.read_names(
                host,
                shelf,
                Reading::from(CatalogCursor::start(), Vec::new()),
                0,
                0,
            )?)
        } else if token == CHOOSE_TOKEN {
            View::Chooser(self.open_chooser(host, handover)?)
        } else if token == INFO_TOKEN {
            let [path, _] = self.handover_params(host, handover, [PATH, VALUE])?;
            let library = self.library.as_ref().ok_or(Error::State)?;
            let info = native_info::read(host, library, &path)?;
            View::Info(Info {
                list: BookList::new(native_info::cells(&info)?, false),
                covers: Covers::new(1, MOST_SMALL)?,
                cover: info.cover,
            })
        } else {
            let [kind, value] = if token == BROWSER_TOKEN {
                [String::new(), String::new()]
            } else {
                self.handover_params(host, handover, [KIND, VALUE])?
            };
            if kind == LETTER {
                return self.opened_letter(host, instance, &value);
            }
            let selection = Selection {
                filter: Filter::All,
                order: Self::order(host)?,
                scope: Scope::from_params(&kind, &value)?,
                fixed: false,
            };
            View::Books(self.read_browser(
                host,
                selection,
                Reading::from(CatalogCursor::start(), Vec::new()),
                0,
                0,
            )?)
        };
        self.screens.open(instance, view)
    }

    /// Asks for the words of a search.
    pub(super) fn ask_search(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure(host)?;
        let mut say = Messages::new()?;
        let title = say.copy(host, "files.search")?;
        let prompt = say.copy(host, "files.search.prompt")?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui.as_ref().ok_or(Error::State)?.ask(
            host,
            Question::text(&title, &prompt)?,
            &mut input,
            &mut out,
        )?;
        self.asked = Some((request, Asked::Search));
        Ok(())
    }

    /// An answer to the question that was asked.
    pub(super) fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match asked {
            Asked::Search => self.searched(host, request),
            Asked::Sheet { path, actions } => {
                let chosen = usize::try_from(answer.choice())
                    .ok()
                    .and_then(|at| actions.get(at).copied());
                match chosen {
                    Some(action) => self.act(host, &path, action),
                    None => Ok(()),
                }
            }
            Asked::Delete { path } if answer.choice() == 1 => self.delete_book(host, &path),
            Asked::Delete { .. } => Ok(()),
            Asked::NewFolder => self.named_folder(host, request),
        }
    }

    /// The words typed: the books that have them, as a list of their own.
    fn searched(&mut self, host: &Host<'_>, request: RequestId) -> Result<(), Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let typed = match uitext::read_all(ui, host, request.text(), SEARCH_LIMIT) {
            Ok(typed) => typed,
            // More than the host searches for: nothing is searched, never a cut word.
            Err(Error::NoSpace) => return self.notify(host, "files.search.long"),
            Err(error) => return Err(error),
        };
        let words = typed.trim();
        if words.is_empty() {
            return Ok(());
        }
        self.open_shelf(host, &Scope::Search(copy(words)?))
    }

    /// A notice, best effort.
    pub(super) fn notify(&self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .notify(Notify::new(&text)?)
            .ok();
        Ok(())
    }

    /// Opens the book in the reader the user bound to `core.reader`: a
    /// hand-over with its path (`document.md` section 1).
    /// `false` when the book is no longer in the catalog.
    pub(super) fn open_book(&self, host: &Host<'_>, reference: BookRef<'_>) -> Result<bool, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let path = match library.book(host, reference, &mut input, &mut out) {
            Ok(book) => copy(book.path())?,
            Err(Error::State | Error::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        };
        self.open_path(host, &path)?;
        Ok(true)
    }

    /// Opens the book at `path` in the reader.
    pub(super) fn open_path(&self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(PATH, path)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
    }

    /// Opens the book information of the book at `path`.
    pub(super) fn open_info(&self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(PATH, path)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen("files.info")?.handover(handover))
    }

    /// Opens the menu `menu` (the library's) in the slot `core.menu`.
    pub(super) fn open_menu(&mut self, host: &Host<'_>, menu: &str) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new("menu", menu)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot("core.menu")?.handover(handover))
    }
}
