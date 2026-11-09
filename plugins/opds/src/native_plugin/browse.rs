//! Browsing a catalog (the screen of `native_browse`): the feed's entries each
//! leading to a further feed or a book, *Search* and *More*, and the line
//! below that says how a load or a download goes. A book saved offers to be
//! read at once.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    commands::CommandSender,
    formatting::Arg,
    image::ImageReady,
    net::{HttpDone, HttpProgress},
    ui::{Instance, Open, Param, Question},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer, join, push},
    text::{concat, copy, decimal_u64},
};

use super::{Ask, NativeOpds, Pending, View};
use crate::native_browse::{Browser, Note, Row, Step};
use crate::native_catalogs as catalogs;
use crate::native_files::DataFiles;
use crate::native_web::{self as web, BOOK, COVER, DESCRIPTION, FEED, FILES};

/// The browse screen: the browsing, its rows and the line below.
#[derive(Debug)]
pub(super) struct BrowseView {
    browser: Browser,
    list: ItemList,
    rows: Vec<Row>,
    note: String,
}

impl BrowseView {
    pub(super) fn draw(&mut self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let feed = &self.browser.feed().title;
        let title = if feed.is_empty() {
            copy(&self.browser.catalog().name)?
        } else {
            copy(feed)?
        };
        let footer = if self.note.is_empty() {
            Footer::None
        } else {
            Footer::Note(&self.note)
        };
        self.list.draw_with(host, &mut frame, &title, true, footer)
    }

    /// The rows and the line below, as the browsing is now.
    fn rebuild(
        &mut self,
        host: &Host<'_>,
        extensions: &[String],
        touch: bool,
    ) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let rows = self.browser.rows()?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for row in &rows {
            let item = match row {
                Row::Search => Item::row(&say.copy(host, "opds.search")?, "›")?,
                Row::More => Item::row(&say.copy(host, "opds.more")?, "›")?,
                Row::Entry(_) => {
                    let (title, value, note) = self.browser.text_of(*row, extensions)?;
                    Item::noted(title.as_deref().unwrap_or_default(), &value, &note)?
                }
            };
            push(&mut items, item)?;
        }
        let mut note = note_text(host, &mut say, self.browser.note(), touch)?;
        // Without rows and with nothing to say, an empty list says so.
        if note.is_empty() && rows.is_empty() && !self.browser.loading() {
            note = say.copy(host, "opds.empty")?;
        }
        let focus = self.list.focus();
        self.list = ItemList::new(items);
        self.list.set_focus(focus);
        self.rows = rows;
        self.note = note;
        Ok(())
    }
}

/// What the line below says of `note`.
fn note_text(
    host: &Host<'_>,
    say: &mut Messages,
    note: &Note,
    touch: bool,
) -> Result<String, Error> {
    fn at(value: &str) -> Arg<'_> {
        Arg::Text {
            name: "host",
            value,
        }
    }
    fn title(value: &str) -> Arg<'_> {
        Arg::Text {
            name: "title",
            value,
        }
    }
    match note {
        Note::Empty => Ok(String::new()),
        Note::Loading(host_name) => say.format(host, "opds.loading", &[at(host_name)]),
        Note::NoNetwork => say.copy(host, "opds.no-network"),
        Note::FailedHost(host_name) => say.format(host, "opds.failed-host", &[at(host_name)]),
        Note::Unauthorized => say.copy(
            host,
            if touch {
                "opds.unauthorized"
            } else {
                "opds.unauthorized.buttons"
            },
        ),
        Note::Error {
            host: host_name,
            status,
        } => say.format(
            host,
            "opds.error",
            &[
                at(host_name),
                Arg::Int {
                    name: "status",
                    value: i64::from(*status),
                },
            ],
        ),
        Note::NotACatalog => say.copy(host, "opds.not-a-catalog"),
        Note::NoMemory => say.copy(host, "opds.no-memory"),
        Note::NoSearch => say.copy(host, "opds.no-search"),
        Note::NoFormat => say.copy(host, "opds.no-format"),
        Note::NotABook => say.copy(host, "opds.not-a-book"),
        Note::CannotSave => say.copy(host, "opds.cannot-save"),
        Note::Stopped => say.copy(host, "opds.download-stopped"),
        Note::Downloading(name) => say.format(host, "opds.downloading", &[title(name)]),
        Note::Share {
            title: name,
            percent,
        } => say.format(
            host,
            "opds.downloading-share",
            &[
                title(name),
                Arg::Int {
                    name: "percent",
                    value: i64::from(*percent),
                },
            ],
        ),
        Note::Size { title: name, bytes } => {
            let mark = say.copy(host, "core.decimal")?;
            let size = size_text(*bytes, &mark)?;
            say.format(
                host,
                "opds.downloading-size",
                &[
                    title(name),
                    Arg::Text {
                        name: "size",
                        value: &size,
                    },
                ],
            )
        }
        Note::Saved { name, folder } => say.format(
            host,
            "opds.saved",
            &[
                Arg::Text {
                    name: "name",
                    value: name,
                },
                Arg::Text {
                    name: "folder",
                    value: folder,
                },
            ],
        ),
    }
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB`, in whole numbers
/// (no floats).
fn size_text(bytes: u64, mark: &str) -> Result<String, Error> {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    // Thousandths of the unit.
    let mut value = u128::from(bytes) * 1000;
    let mut unit = 0;
    while value >= 1_000_000 && unit + 1 < UNITS.len() {
        value /= 1024;
        unit += 1;
    }
    let mut digits = [0; 20];
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 || value >= 10_000 {
        let whole = u64::try_from((value + 500) / 1000).unwrap_or(u64::MAX);
        concat(&[decimal_u64(whole, &mut digits), " ", name])
    } else {
        let tenths = u64::try_from((value + 50) / 100).unwrap_or(u64::MAX);
        let mut low = [0; 20];
        concat(&[
            decimal_u64(tenths / 10, &mut digits),
            mark,
            decimal_u64(tenths % 10, &mut low),
            " ",
            name,
        ])
    }
}

impl NativeOpds {
    /// The browse screen opened for the catalog that was asked for.
    pub(super) fn opened_browse(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        let index = match self.pending.take() {
            Some(Pending::Browse(index)) => index,
            other => {
                self.pending = other;
                0
            }
        };
        self.reload(host)?;
        let catalog = self.catalogs.get(index).cloned().unwrap_or_default();
        let password = catalogs::password(&self.vault(host), &catalog)?;
        let mut browser = Browser::new(catalog, password, &self.folder, self.small)?;
        browser.start(host, &mut self.web)?;
        let mut view = BrowseView {
            browser,
            list: ItemList::default(),
            rows: Vec::new(),
            note: String::new(),
        };
        view.rebuild(host, &self.extensions, self.touch)?;
        self.screens.open(instance, View::Browse(view))
    }

    fn top_browse(&mut self) -> Option<&mut BrowseView> {
        match self.screens.top(crate::declarations::BROWSE_SCREEN) {
            Some(View::Browse(view)) => Some(view),
            _ => None,
        }
    }

    /// Makes the rows of the browse screen on show anew and asks for a redraw.
    fn redraw_browse(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (extensions, touch) = (core::mem::take(&mut self.extensions), self.touch);
        let rebuilt = match self.top_browse() {
            Some(view) => view.rebuild(host, &extensions, touch),
            None => Ok(()),
        };
        self.extensions = extensions;
        rebuilt?;
        Self::redraw(host)
    }

    pub(super) fn browse_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(View::Browse(view)) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // Back stops a download going on, else goes to the feed before; from
        // the first feed the host closes the screen.
        if let Input::Action {
            action: "core.back",
            ..
        } = *input
        {
            return self.browse_back(host);
        }
        let step = view.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step {
            self.browse_activate(host, index)?;
        }
        Ok(ItemList::handled(step))
    }

    fn browse_back(&mut self, host: &Host<'_>) -> Result<Handled, Error> {
        let files = DataFiles::new(host);
        let Some(View::Browse(view)) = self.screens.top(crate::declarations::BROWSE_SCREEN) else {
            return Ok(Handled::No);
        };
        if view.browser.downloading() {
            view.browser.stop_download(host, &mut self.web, &files);
            self.redraw_browse(host)?;
            return Ok(Handled::Yes);
        }
        let step = view.browser.back(host, &mut self.web)?;
        if step == Step::Leave {
            return Ok(Handled::No);
        }
        self.redraw_browse(host)?;
        Ok(Handled::Yes)
    }

    /// A row was chosen.
    fn browse_activate(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(View::Browse(view)) = self.screens.top(crate::declarations::BROWSE_SCREEN) else {
            return Ok(());
        };
        let Some(row) = view.rows.get(index).copied() else {
            return Ok(());
        };
        let step = view
            .browser
            .activate(host, &mut self.web, row, &self.extensions)?;
        self.browse_step(host, step)?;
        self.redraw_browse(host)
    }

    /// What the screen is to do after a step of the browsing.
    fn browse_step(&mut self, host: &Host<'_>, step: Step) -> Result<(), Error> {
        match step {
            Step::AskTerms => {
                let title = Messages::new()?.copy(host, "opds.search")?;
                self.ask_text(host, &title, None, None, false, Ask::Terms)
            }
            Step::Saved(path) => self.saved(host, path),
            _ => Ok(()),
        }
    }

    /// A book was saved: the choice to read it now.
    fn saved(&mut self, host: &Host<'_>, path: String) -> Result<(), Error> {
        let (text, title) = match self.top_browse() {
            Some(view) => (copy(&view.note)?, copy(&view.browser.catalog().name)?),
            None => return Ok(()),
        };
        // The note says where it is, once the rows are made anew.
        let mut say = Messages::new()?;
        let text = if text.is_empty() {
            say.copy(host, "opds.title")?
        } else {
            text
        };
        let labels = [
            say.copy(host, "opds.option.read")?,
            say.copy(host, "opds.option.ok")?,
        ];
        let choices = join(&labels, "\n")?;
        self.saved_path = Some(path);
        let question = Question::choice(&title, &text, &choices)?;
        self.ask(host, question, None, Ask::Read)
    }

    /// The book saved is opened in the reader the user bound to `core.reader`.
    pub(super) fn read_saved(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(path) = self.saved_path.take() else {
            return Ok(());
        };
        let params = [Param::new("path", &path)?];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let handover = ui.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
    }

    /// The words to search for were typed.
    pub(super) fn terms_typed(&mut self, host: &Host<'_>, text: &str) -> Result<(), Error> {
        if let Some(View::Browse(view)) = self.screens.top(crate::declarations::BROWSE_SCREEN) {
            view.browser.terms(host, &mut self.web, text)?;
        }
        self.redraw_browse(host)
    }

    /// The end of a request: the screen or the sync it belongs to takes it.
    pub(super) fn http_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let files = DataFiles::new(host);
        let mut handled = None;
        for (at, view) in self.screens.all().enumerate() {
            let View::Browse(view) = view else {
                continue;
            };
            let step =
                view.browser
                    .done(host, &mut self.web, &files, self.library.as_ref(), done)?;
            if let Some(step) = step {
                handled = Some((at, step));
                break;
            }
        }
        let Some((at, step)) = handled else {
            return self.other_done(host, done);
        };
        // The step of the screen that took it: its rows are made anew.
        let (extensions, touch) = (core::mem::take(&mut self.extensions), self.touch);
        let rebuilt = match self.screens.all().nth(at) {
            Some(View::Browse(view)) => view.rebuild(host, &extensions, touch),
            _ => Ok(()),
        };
        self.extensions = extensions;
        rebuilt?;
        self.browse_step(host, step)?;
        Self::redraw(host)
    }

    /// The end of a request that is not a browse screen's: a cover or the sync's.
    fn other_done(&mut self, _host: &Host<'_>, _done: &HttpDone) -> Result<(), Error> {
        Ok(())
    }

    /// How far a download came.
    pub(super) fn http_progress(
        &mut self,
        host: &Host<'_>,
        progress: &HttpProgress,
    ) -> Result<(), Error> {
        let mut redraw = false;
        for view in self.screens.all() {
            if let View::Browse(view) = view {
                redraw |= view.browser.progressed(progress)?;
            }
        }
        if redraw {
            self.redraw_browse(host)?;
        }
        Ok(())
    }

    pub(super) fn image_ready(
        &mut self,
        _host: &Host<'_>,
        _ready: &ImageReady,
    ) -> Result<(), Error> {
        Ok(())
    }

    /// A browse screen closed: what it waits for is not waited for.
    pub(super) fn close_browse(&mut self, host: &Host<'_>, instance: Instance) {
        if !matches!(self.screens.get(instance), Some(View::Browse(_))) {
            return;
        }
        let files = DataFiles::new(host);
        for kind in [FEED, DESCRIPTION, BOOK, COVER] {
            self.web.cancel(host, kind);
        }
        for name in FILES {
            crate::native_files::remove(&files, name);
        }
    }

    /// The plugin stops: every request is cancelled and its files go.
    pub(super) fn stop_all(&mut self, host: &Host<'_>) {
        let files = DataFiles::new(host);
        for kind in [
            FEED,
            DESCRIPTION,
            BOOK,
            COVER,
            web::SYNC_FEED,
            web::SYNC_BOOK,
        ] {
            self.web.cancel(host, kind);
        }
        for name in FILES {
            crate::native_files::remove(&files, name);
        }
    }

    pub(super) fn sync_action(&mut self, _host: &Host<'_>) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn work_due(&mut self, _host: &Host<'_>, _key: &str) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn sync_running(&self) -> bool {
        false
    }

    pub(super) fn sync_row(&mut self, _host: &Host<'_>) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn sync_one(&mut self, _host: &Host<'_>, _index: usize) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn sync_report_text(
        &self,
        _host: &Host<'_>,
        _say: &mut Messages,
    ) -> Result<Option<String>, Error> {
        Ok(None)
    }
}
