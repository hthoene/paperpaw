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
    fonts::{Font, FontRole},
    formatting::Arg,
    image::{ImageReady, ImageStatus, ImageStyle, PrepareImage, PrepareRoot},
    net::{HttpDone, HttpProgress},
    snapshot,
    ui::{Instance, Open, Param, Question},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Style,
    mem::{SERVICE_BUFFER, buffer, collect, join, push},
    paint::frame_rect,
    text::{concat, copy, decimal_u64},
};

use super::{Ask, Opds, Pending, View};
use crate::native_browse::{Browser, Note, Row, Step};
use crate::native_catalogs as catalogs;
use crate::native_covers::{self as covers, Covers, MAX_LIVE};
use crate::native_files::{DataFiles, Files};
use crate::native_web::{self as web, BOOK, COVER, DESCRIPTION, FEED, FILES};

/// The browse screen: the browsing, its rows and the line below.
#[derive(Debug)]
pub(super) struct BrowseView {
    browser: Browser,
    list: ItemList,
    rows: Vec<Row>,
    note: String,
    /// The covers of the feed shown, where the device has them.
    covers: Option<Covers>,
    /// A column for covers: some entry of the feed has a picture.
    column: bool,
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
        self.list
            .draw_with(host, &mut frame, &title, true, footer)?;
        self.draw_covers(host, &mut frame)
    }

    /// The entries whose rows were drawn in the last frame (the first few
    /// before any was drawn).
    ///
    /// # Errors
    /// `NoMemory`.
    fn visible(&self) -> Result<Vec<usize>, Error> {
        let first = usize::from(self.browser.feed().search.is_some());
        let count = self.browser.feed().entries.len();
        let drawn = collect(
            (0..count)
                .filter(|i| self.list.rect_of(i + first).is_some())
                .map(Ok),
        )?;
        if drawn.is_empty() {
            collect((0..count.min(MAX_LIVE)).map(Ok))
        } else {
            Ok(drawn)
        }
    }

    /// The covers over the column their rows keep for them. The kit's rows have
    /// no place for a picture, so a row's tap-zone column is the room kept, and
    /// the picture (or the plain ground, where there is none yet) is drawn
    /// over what the kit drew there.
    fn draw_covers(&self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let Some(covers) = self.covers.as_ref().filter(|_| self.column) else {
            return Ok(());
        };
        let first = usize::from(self.browser.feed().search.is_some());
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font);
        font.release()?;
        let style = style?;
        let canvas = frame.draw_canvas()?;
        for i in 0..self.browser.feed().entries.len() {
            let Some(row) = self.list.rect_of(i + first) else {
                continue;
            };
            let band = row.height.min(style.row);
            let cell = (band / 6).max(4);
            let side = cell * 3;
            let x = row.x.saturating_add(style.margin);
            let y = row.y.saturating_add(band.saturating_sub(side) / 2);
            let ground = if i + first == self.list.focus() && !style.ring {
                paperpaw_sdk::Color::BLACK
            } else {
                style.paper
            };
            let area = paperpaw_sdk::Rect::new(x, y, side, side);
            canvas.fill_rectangle(frame_rect(area), ground)?;
            if let Some((image, _, _)) = covers.image(i) {
                // A cover is taller than wide: as high as the row lets it be.
                let high = (side + side / 2).min(band.saturating_sub(2));
                let top = row.y.saturating_add(band.saturating_sub(high) / 2);
                let place = paperpaw_sdk::Rect::new(x, top, side, high);
                match canvas.draw_image(image, frame_rect(place), ImageStyle::new().small()) {
                    Ok(()) | Err(Error::NotFound) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
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
        self.column = self.covers.is_some()
            && self
                .browser
                .feed()
                .entries
                .iter()
                .any(|e| e.thumbnail.is_some());
        for row in &rows {
            let item = match row {
                Row::Search => Item::row(&say.copy(host, "opds.search")?, "›")?,
                Row::More => Item::row(&say.copy(host, "opds.more")?, "›")?,
                Row::Entry(_) => {
                    let (title, value, note) = self.browser.text_of(*row, extensions)?;
                    let mut item =
                        Item::noted(title.as_deref().unwrap_or_default(), &value, &note)?;
                    if self.column
                        && let Item::Row(row) = &mut item
                    {
                        // Keeps the room for a cover (see `draw_covers`).
                        row.zone = Some(0);
                    }
                    item
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

impl Opds {
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
        let catalog = match self.catalogs.get(index) {
            Some(found) => found.try_clone()?,
            None => catalogs::Catalog::default(),
        };
        let password = catalogs::password(&self.vault(host), &catalog)?;
        let mut browser = Browser::new(catalog, password, &self.folder, self.small)?;
        browser.start(host, &mut self.web)?;
        // Covers need the memory of a device that is not a small one.
        let with_covers = !self.small && Self::switch_setting(host, "opds.covers", true);
        let mut view = BrowseView {
            browser,
            list: ItemList::default(),
            rows: Vec::new(),
            note: String::new(),
            covers: if with_covers {
                Some(Covers::new(0, 0)?)
            } else {
                None
            },
            column: false,
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
        // A page turned or a row focused: the covers of the page shown.
        self.covers_step(host)?;
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
        // A new feed: its covers.
        self.covers_step(host)?;
        Self::redraw(host)
    }

    /// The end of a request that is not a browse screen's: a cover or the sync's.
    fn other_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        match done.token() {
            web::COVER => self.cover_done(host, done),
            _ => self.sync_done(host, done),
        }
    }

    /// What the covers of the screen on show do next: those of a feed left
    /// are let go of, and the next one the page shown wants is asked for, or
    /// decoded where it is kept.
    fn covers_step(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let files = DataFiles::new(host);
        let Some(View::Browse(view)) = self.screens.top(crate::declarations::BROWSE_SCREEN) else {
            return Ok(());
        };
        if view.covers.is_none() {
            return Ok(());
        }
        let visible = view.visible()?;
        let (generation, count) = (view.browser.generation(), view.browser.feed().entries.len());
        let Some(held) = view.covers.as_mut() else {
            return Ok(());
        };
        if held.generation() != generation {
            let ids = held.release_all()?;
            *held = Covers::new(generation, count)?;
            Self::release_images(host, &ids);
        }
        let entries = &view.browser.feed().entries;
        let wants = |i: usize| entries.get(i).is_some_and(|e| e.thumbnail.is_some());
        let Some(i) = held.pick(visible, wants) else {
            return Ok(());
        };
        let Some(url) = entries.get(i).and_then(|e| e.thumbnail.as_deref()) else {
            return Ok(());
        };
        let name = covers::cache_name(url)?;
        if covers::cached(&files, &name) {
            held.preparing(i);
            let cookie = held.cookie(i);
            return match Self::prepare_cover(host, &name, cookie) {
                Ok(()) => Ok(()),
                Err(Error::NoMemory) => Err(Error::NoMemory),
                Err(_) => {
                    held.failed(i);
                    Ok(())
                }
            };
        }
        match view.browser.cover_request(host, &mut self.web, url) {
            Ok(id) => held.fetching(i, id),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // No network or no room for a request: this cover is left out.
            Err(_) => held.failed(i),
        }
        Ok(())
    }

    /// Has the host decode the cover kept in `name`.
    fn prepare_cover(host: &Host<'_>, name: &str, cookie: u32) -> Result<(), Error> {
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .image_prepare(&PrepareImage::new(PrepareRoot::Data, name, cookie).side(covers::SIDE))
    }

    /// Gives pictures back to the host; one it no longer has is not an error.
    fn release_images(host: &Host<'_>, ids: &[paperpaw_sdk::native::image::ImageId]) {
        for id in ids {
            let mut scratch = [0; 64];
            match CommandSender::new(host, &mut scratch).image_release(*id) {
                Ok(()) | Err(_) => {}
            }
        }
    }

    /// A cover arrived: kept, then decoded.
    fn cover_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let files = DataFiles::new(host);
        let mut found = None;
        for view in self.screens.all() {
            if let View::Browse(view) = view
                && let Some(held) = view.covers.as_mut()
                && let Some(i) = held.fetched(done.id())
            {
                let url = view
                    .browser
                    .feed()
                    .entries
                    .get(i)
                    .and_then(|e| e.thumbnail.as_deref())
                    .map(copy)
                    .transpose()?;
                found = Some((i, url));
                break;
            }
        }
        let Some((i, url)) = found else {
            // Not a cover of a feed shown now.
            crate::native_files::remove(&files, web::COVER_FILE);
            return Ok(());
        };
        let kept = match (web::ended(done), url) {
            (web::Ended::Fine, Some(url)) => {
                let name = covers::cache_name(&url)?;
                match files.rename(web::COVER_FILE, &name) {
                    Ok(()) => {
                        covers::remember(&files, &name)?;
                        Some(name)
                    }
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => None,
                }
            }
            _ => None,
        };
        crate::native_files::remove(&files, web::COVER_FILE);
        if let Some(View::Browse(view)) = self.screens.top(crate::declarations::BROWSE_SCREEN)
            && let Some(held) = view.covers.as_mut()
        {
            match kept {
                Some(name) => {
                    held.preparing(i);
                    let cookie = held.cookie(i);
                    match Self::prepare_cover(host, &name, cookie) {
                        Ok(()) => {}
                        Err(Error::NoMemory) => return Err(Error::NoMemory),
                        Err(_) => held.failed(i),
                    }
                }
                None => held.failed(i),
            }
        }
        self.covers_step(host)
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

    /// A cover is decoded.
    pub(super) fn image_ready(&mut self, host: &Host<'_>, ready: &ImageReady) -> Result<(), Error> {
        let image = match (ready.status(), ready.image()) {
            (ImageStatus::Ready, Some(id)) => Some((id, ready.width(), ready.height())),
            _ => None,
        };
        let mut release = None;
        if let Some(View::Browse(view)) = self.screens.top(crate::declarations::BROWSE_SCREEN)
            && let Some(held) = view.covers.as_mut()
        {
            release = held.ready(ready.cookie(), image);
        } else if let Some((id, ..)) = image {
            // Nobody wants it any more.
            release = Some(id);
        }
        if let Some(id) = release {
            Self::release_images(host, &[id]);
        }
        self.covers_step(host)?;
        Self::redraw(host)
    }

    /// A browse screen closed: what it waits for is not waited for, and its
    /// pictures are given back.
    pub(super) fn close_browse(&mut self, host: &Host<'_>, instance: Instance) {
        let ids = match self.screens.get(instance) {
            Some(View::Browse(view)) => view.covers.as_mut().map(Covers::release_all),
            _ => return,
        };
        if let Some(Ok(ids)) = ids {
            Self::release_images(host, &ids);
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
}
