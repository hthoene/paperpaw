//! The dashboard screen: the page of tools and widgets in its grid on a plain
//! background, the pencil and the dots below, page turning by the page buttons,
//! the focus with the buttons, a tap that runs a tool or reaches a widget, and
//! arranging the page while editing. What it is made of without the host is
//! `native_dash` (the parts' places), `native_focus` (the buttons' focus),
//! `native_pages`, `native_grid` and `native_arrange` (the pages) and
//! `native_source` (what is read of the device); this file reads, draws and
//! hands inputs on through the host.
//!
//! The picture behind the page is the host's (`native_backdrop`,
//! `native_backdrop_host`): asked for when the screen shows, drawn first and not
//! while editing, a notice instead of the page while it is made.
//!
//! Tools show their icons above their names and the footer its icons
//! (`native_icons`, `native_dashboard/page.rs`). A widget that says it shows
//! something new (`event.widget_changed`) is drawn anew in its cell
//! ([`Dashboard::widget_changed`]). Not yet (named steps in `PORTING-7.md`): the
//! parts of the picture's recipe the host's drawing cannot do.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        image::ImageReady,
        snapshot::{self, Snapshot},
        ui::{Compose, Instance, MadeWidget, Ui, WidgetHandle, WidgetInputKind},
    },
    widgets::{ColourSpec, colour_spec, grid::Geometry},
};
use paperpaw_ui_kit::{
    Messages, Paint, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint, status_line, text,
};

use crate::native_add_screen as add_screen;
use crate::native_backdrop::{Backdrop, Landed};
use crate::native_backdrop_host as picture;
use crate::native_dash::{self as dash, Footer, Frame, Nav, Wanted};
use crate::native_focus::{Button, Focus, Owner, Press, Step, Target};
use crate::native_grid::{self as grid, What};
use crate::native_icons::{self as icons, Face};
use crate::native_measure::{self as measure, PLACE};
use crate::native_pages;
use crate::native_shown::Shown;
use crate::native_source::HostSource;
use crate::native_stage as stage;
use paperpaw_sdk::native::text::MAX_BREAKS;

/// One item of the page as it shows.
#[derive(Debug)]
struct Cell {
    what: What,
    /// The item it shows: its number among `Laid::items`.
    item: usize,
    /// The fewest rows a widget may be made (as it measured), one for a tool.
    least: u8,
    rect: Rect,
    /// A tool's name.
    name: String,
    /// A tool's icon and state (none for a widget).
    face: Option<Face>,
    /// A widget's instance, where the host made one.
    widget: Option<MadeWidget>,
    /// The widget shows nothing (it measured no height): no plate is drawn
    /// for it, as interface 6's widgets drew none.
    blank: bool,
}

/// The page as laid out: what shows, where, and what the buttons reach.
#[derive(Debug)]
struct Laid {
    frame: Frame,
    cells: Vec<Cell>,
    targets: Vec<Target>,
    pages: usize,
    dots: bool,
    focus_at_once: bool,
    /// The arrow keys stay on the page.
    arrows_stay: bool,
    /// What the items sit on; none: the background shows through.
    plate: Option<Color>,
    /// Where the pencil (*Done* while editing) is, and its word.
    pencil: Option<(Rect, String)>,
    /// Where *Add* is, while editing.
    plus: Option<Rect>,
    /// The icons of the pencil, *Done* and *Add*, where the host has them.
    icons: icons::Buttons,
    /// The items shown, each with its cells: what is kept when one is moved,
    /// sized or removed.
    items: Vec<grid::Item>,
    /// The setting that keeps the page (0 for `picture-home.page-1`), and
    /// whether the screen is on its side.
    slot: usize,
    wide: bool,
    /// How wide and high the corner of an item is that a finger sizes it by.
    handle: u16,
    /// The status bar, where its strip is kept.
    bar: Option<bar::Plan>,
}

/// What a widget's change draws anew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redraw {
    /// Its cell.
    Area(Rect),
    /// The whole screen (the focus moved with the targets).
    Screen,
}

impl Redraw {
    /// What a widget's change in `area` draws anew: its cell, or the whole
    /// screen where the focus shows and the targets moved (the ring moves).
    #[must_use]
    pub const fn after(area: Rect, moved: bool, focus_shown: bool) -> Self {
        if moved && focus_shown {
            Self::Screen
        } else {
            Self::Area(area)
        }
    }

    /// Asks the host to draw it anew, with a partial refresh (never the fast
    /// one, which breaks the picture's grays into dots).
    ///
    /// # Errors
    /// The host's refusal.
    pub fn queue(self, host: &Host<'_>) -> Result<(), Error> {
        match self {
            Self::Area(rect) => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch)
                    .queue_redraw_area(RedrawMode::Partial, paint::frame_rect(rect))
            }
            Self::Screen => redraw(host),
        }
    }
}

/// What a press on a target leaves to do besides what it did itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Nothing,
    /// Editing starts (`true`) or ends.
    Edit(bool),
    /// The options of the item of this cell open.
    Options(usize),
    /// The *Add* screen opens for the page that shows.
    Add,
}

mod arrange;
mod bar;
mod page;

/// The dashboard.
#[derive(Debug, Default)]
pub struct Dashboard {
    nav: Option<Nav>,
    focus: Focus,
    laid: Option<Laid>,
    /// What shows changed (a setting, the screen turned): laid out anew first.
    stale: bool,
    /// Shown (again): whether the focus shows is decided anew once laid out.
    reshow: bool,
    /// Editing the pages: the pencil is *Done*; what is tapped offers the
    /// item's options.
    editing: bool,
    /// The page (the setting that keeps it) to show first.
    first: Option<usize>,
    /// The options of an item, over the page.
    options: Option<arrange::Options>,
    /// An item the buttons are moving or sizing.
    moving: Option<arrange::Moving>,
    /// An item a finger has taken along.
    drag: Option<arrange::Drag>,
    /// When a bar that shows after a press goes again (the event clock, ms).
    revealed: Option<u64>,
    /// The picture behind the page.
    backdrop: Backdrop,
}

impl Dashboard {
    /// A dashboard to be laid out when it first draws.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stale: true,
            reshow: true,
            ..Self::default()
        }
    }

    /// Whether the pages are being edited.
    #[must_use]
    pub const fn editing(&self) -> bool {
        self.editing
    }

    /// Opened to edit (`mode` = `edit`, or the setting `slot` keeps a page: its
    /// own editor): editing starts, on that page where there is one.
    pub fn start_editing(&mut self, slot: Option<usize>) {
        self.editing = true;
        self.first = slot;
    }

    /// Opened again by the host while it is open: its view is kept (the page it
    /// is on, the widgets it made, the focus), editing starts where asked. A new
    /// dashboard would drop the widgets it made without letting them go.
    pub fn reopened(&mut self, edit: bool, slot: Option<usize>) {
        if edit && !self.editing {
            self.editing = true;
            self.stale = true;
        }
        if slot.is_some() {
            self.first = slot;
            self.stale = true;
        }
    }

    /// What shows is read and laid out again before the next draw or input.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Shown again (above another screen closed, a book left).
    pub fn shown(&mut self) {
        self.stale = true;
        self.reshow = true;
        // A finger's drag does not outlive the screen's being covered.
        self.drag = None;
    }

    /// Wants the picture the settings ask for: the host is asked to make it unless
    /// it is held, asked for or given up already, and a picture that is no more
    /// wanted is given back. `cookie` counts the plugin's requests.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals reading the settings. A
    /// request the host refuses leaves the page plain, it is no error.
    pub fn want_picture(&mut self, host: &Host<'_>, cookie: &mut u32) -> Result<(), Error> {
        let recipe = picture::recipe(host)?;
        let change = self.backdrop.want(recipe, cookie)?;
        if let Some(image) = change.release {
            picture::release(host, image);
        }
        if let Some((number, side)) = change.ask {
            let tone = self.backdrop.tone();
            if picture::ask(host, self.backdrop.path(), number, (side, tone.as_ref())).is_err() {
                self.backdrop.refused();
                return Ok(());
            }
            // The notice shows while the host makes the picture.
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Fast)?;
        }
        Ok(())
    }

    /// Whether this screen waits for the answer to request `cookie`.
    #[must_use]
    pub fn waits_for(&self, cookie: u32) -> bool {
        self.backdrop.waits_for(cookie)
    }

    /// The host's answer to the request for the picture: the page is drawn anew,
    /// with the whole screen refreshed (the picture's grays), as interface 6 did
    /// once the picture was made. Where none was made the page stays plain and
    /// the reader is told why.
    ///
    /// # Errors
    /// Host errors of the redraw.
    pub fn image_ready(&mut self, host: &Host<'_>, ready: &ImageReady) -> Result<(), Error> {
        let landed = self.backdrop.ready(
            ready.cookie(),
            ready.status(),
            ready.image(),
            (ready.width(), ready.height()),
            ready.luma(),
        );
        match landed {
            Landed::Ignore(held) => {
                if let Some(image) = held {
                    picture::release(host, image);
                }
                return Ok(());
            }
            Landed::Failed(reason) => {
                picture::tell(host, reason).ok();
            }
            Landed::Ready => {}
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Full)
    }

    /// A widget says it shows something new (`event.widget_changed`): where it is
    /// one of the page's, what its cell answers to the buttons is read anew (not
    /// while editing: there each item is one target), as interface 6 did, and the
    /// area to draw anew is returned (its cell, or the whole screen where the
    /// focus shows and the targets moved). The host composes the new content at
    /// the draw. `None`: not this page's widget (or the page is to be laid out
    /// anyway, or the picture is being made), nothing to draw.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; another refusal of the targets keeps the old ones.
    #[inline(never)]
    pub fn widget_changed(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        instance: Instance,
    ) -> Result<Option<Redraw>, Error> {
        let Some(area) = self.cell_showing(instance.get()) else {
            return Ok(None);
        };
        let Some(laid) = self.laid.as_ref() else {
            return Ok(None);
        };
        if self.editing {
            return Ok(Some(Redraw::Area(area)));
        }
        let footer = Footer {
            pencil: laid.pencil.as_ref().map_or_else(Rect::default, |p| p.0),
            plus: laid.plus.unwrap_or_default(),
        };
        let buttons = Buttons {
            editing: false,
            plus: laid.plus.is_some(),
            pencil: laid.pencil.is_some(),
        };
        let targets = match Self::targets(host, ui, &laid.cells, &footer, buttons) {
            Ok(targets) => targets,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Some(Redraw::Area(area))),
        };
        let moved = targets != laid.targets;
        if let Some(laid) = self.laid.as_mut() {
            laid.targets = targets;
            self.focus.clamp(laid.targets.len());
            Self::remeasure_blank(host, ui, laid, instance.get())?;
        }
        Ok(Some(Redraw::after(area, moved, self.focus.shown())))
    }

    /// The cell of the page's widget `instance` (the host's number), where the
    /// page as laid out shows it: none while the page is to be laid out anew
    /// anyway or the picture is made (the notice shows instead of the page).
    fn cell_showing(&self, instance: u32) -> Option<Rect> {
        if self.stale || self.backdrop.making() {
            return None;
        }
        self.laid
            .as_ref()?
            .cells
            .iter()
            .find_map(|cell| match cell.widget {
                Some(MadeWidget::Widget(w)) if w.instance().get() == instance => Some(cell.rect),
                _ => None,
            })
    }

    /// Whether the widget `instance` shows nothing now (the shelf with its
    /// first book, or none left): its plate comes or goes with it. A refusal
    /// keeps what was measured.
    fn remeasure_blank(
        host: &Host<'_>,
        ui: &Ui,
        laid: &mut Laid,
        instance: u32,
    ) -> Result<(), Error> {
        let geometry = laid.frame.geometry;
        let Some(cell) = laid.cells.iter_mut().find(|cell| {
            matches!(cell.widget, Some(MadeWidget::Widget(w)) if w.instance().get() == instance)
        }) else {
            return Ok(());
        };
        let Some(made) = cell.widget else {
            return Ok(());
        };
        match measure::measure_shown(host, ui, made, &geometry) {
            Ok((_, _, blank)) => cell.blank = blank,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
        Ok(())
    }

    /// The screen is closed: the picture goes back to the host.
    pub fn close(&mut self, host: &Host<'_>) {
        if let Some(image) = self.backdrop.clear().release {
            picture::release(host, image);
        }
    }

    /// Lays the page out when it is stale: the widgets of the page before are
    /// let go and those of this one made, with the places of the grid.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals; the old layout stays where
    /// the new one could not be made.
    pub fn ensure(&mut self, host: &Host<'_>, ui: &Ui) -> Result<(), Error> {
        if !self.stale && self.laid.is_some() {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let made = self.lay_out(host, ui, &display, &font);
        let released = font.release();
        let laid = made?;
        if let Err(error) = released {
            release_cells(host, ui, laid.cells);
            return Err(error);
        }
        self.drop_widgets(host, ui);
        // The cells are new: what pointed at the old ones is no more.
        self.options = None;
        self.moving = None;
        self.drag = None;
        // The pages are arranged upright: on its side they are derived.
        self.editing &= !laid.wide;
        self.focus.clamp(laid.targets.len());
        if self.reshow {
            // As on interface 6: at every show the focus is drawn only if the
            // reader chose to see it at once.
            self.focus.show(laid.focus_at_once);
            self.reshow = false;
        }
        self.laid = Some(laid);
        self.stale = false;
        Ok(())
    }

    fn lay_out(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        display: &Snapshot,
        font: &Font<'_>,
    ) -> Result<Laid, Error> {
        let Shown {
            mut src,
            metrics,
            frame,
            pages,
            wide,
        } = Shown::read(host, display, font)?;
        let nav = self.nav.get_or_insert(Nav::new(pages.len()));
        nav.resize(pages.len());
        if let Some(at) = self
            .first
            .take()
            .and_then(|slot| pages.iter().position(|p| p.slot == slot))
        {
            nav.page = at;
        }
        let (page, count) = (nav.page, nav.pages);
        let slot = pages.get(page).map_or(0, |p| p.slot);
        let items = match pages.get(page) {
            Some(page) => {
                let items = native_pages::expand(&src, &page.items)?;
                native_pages::placed_anew(&items, wide)?
            }
            None => Vec::new(),
        };
        Self::measure_unplaced(host, ui, &mut src, &frame.geometry, &items)?;
        let (shown, _) = grid::fit(&src, &frame.geometry, &items)?;
        // Editing is for a screen upright (`ensure` drops it where it is not).
        let editing = self.editing && !wide;
        let pencil_on = editing || (src.pencil && !wide);
        let (word, done_word) = Self::pencil_word(host, display, font, pencil_on, editing)?;
        let footer = dash::footer(&metrics, frame.footer, &Wanted { editing, done_word });
        let button_icons = icons::Buttons::read(host, pencil_on, editing)?;
        let mut cells = Vec::new();
        let buttons = Buttons {
            editing,
            plus: editing,
            pencil: pencil_on,
        };
        let made = Self::fill(
            host,
            ui,
            &src,
            &frame,
            &shown,
            (&footer, buttons),
            &mut cells,
        );
        let targets = match made {
            Ok(made) => made,
            Err(error) => {
                // The widgets already made are let go, none is leaked.
                release_cells(host, ui, cells);
                return Err(error);
            }
        };
        let items = match grid::copy_items(&shown) {
            Ok(items) => items,
            Err(error) => {
                release_cells(host, ui, cells);
                return Err(error);
            }
        };
        let bar = match bar::Plan::new(frame.strip, &src, (metrics.margin, metrics.style_gap)) {
            Ok(bar) => bar,
            Err(error) => {
                release_cells(host, ui, cells);
                return Err(error);
            }
        };
        Ok(Laid {
            frame,
            cells,
            targets,
            pages: count,
            dots: src.dots,
            focus_at_once: src.focus_at_once,
            arrows_stay: src.arrows_stay,
            plate: plate_of(&src),
            pencil: pencil_on.then_some((footer.pencil, word)),
            plus: editing.then_some(footer.plus),
            icons: button_icons,
            items,
            slot,
            wide,
            handle: (metrics.row / 2).max(16),
            bar,
        })
    }

    /// The widgets with no place yet are measured at the grid's width (made,
    /// asked and let go), so the grid gives each the rows it needs. A widget the
    /// host cannot measure keeps the default of one row.
    fn measure_unplaced(
        host: &Host<'_>,
        ui: &Ui,
        src: &mut HostSource,
        geometry: &Geometry,
        items: &[grid::Item],
    ) -> Result<(), Error> {
        for item in items {
            let (None, What::Widget(id)) = (item.at, &item.what) else {
                continue;
            };
            match measure::measure_widget(host, ui, id, PLACE, geometry) {
                Ok(Some((rows, least))) => src.measured(id, rows, least)?,
                Ok(None) => {}
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// The cells of the items shown and what the buttons reach. On an error the
    /// caller lets the cells made so far go.
    fn fill(
        host: &Host<'_>,
        ui: &Ui,
        src: &HostSource,
        frame: &Frame,
        shown: &[grid::Item],
        (footer, buttons): (&Footer, Buttons),
        cells: &mut Vec<Cell>,
    ) -> Result<Vec<Target>, Error> {
        for (index, item) in shown.iter().enumerate() {
            if let Some(cell) = Self::cell(host, ui, src, &frame.geometry, (index, item))? {
                push(cells, cell)?;
            }
        }
        Self::targets(host, ui, cells, footer, buttons)
    }

    /// The cell of an item: a tool's name, or a widget made and placed.
    fn cell(
        host: &Host<'_>,
        ui: &Ui,
        src: &HostSource,
        geometry: &Geometry,
        (index, item): (usize, &grid::Item),
    ) -> Result<Option<Cell>, Error> {
        let Some(at) = item.at else {
            return Ok(None);
        };
        let rect = geometry.rect(at);
        let mut face = None;
        let (name, widget) = match &item.what {
            What::Tool(action) => {
                face = Some(icons::face(host, src, action)?);
                (text::copy(src.tool_name(action))?, None)
            }
            What::Widget(id) => {
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                match ui.widget_make(host, id, PLACE, &mut input, &mut out) {
                    Ok(made) => {
                        if let MadeWidget::Widget(handle) = made {
                            let placed = ui.widget_place(
                                host,
                                handle,
                                paint::frame_rect(rect),
                                &mut input,
                                &mut out,
                            );
                            match placed {
                                // The X4 until its nested callbacks are measured
                                // (#250): the host lays no other plugin's widget
                                // out, so its cell stays empty, the page shows.
                                Ok(()) | Err(Error::Unsupported) => {}
                                Err(error) => {
                                    drop_widget(host, ui, handle);
                                    return Err(error);
                                }
                            }
                        }
                        (text::copy(Self::widget_title(src, id))?, Some(made))
                    }
                    // A widget that is gone, or not for this screen: no cell.
                    Err(Error::NotFound | Error::Denied) => return Ok(None),
                    Err(error) => return Err(error),
                }
            }
        };
        let what = match item.what.try_clone() {
            Ok(what) => what,
            Err(error) => {
                if let Some(MadeWidget::Widget(widget)) = widget {
                    drop_widget(host, ui, widget);
                }
                return Err(error);
            }
        };
        let (least, blank) = match widget {
            Some(made) => match measure::measure_shown(host, ui, made, geometry) {
                Ok((_, least, blank)) => (least, blank),
                Err(Error::NoMemory) => {
                    if let MadeWidget::Widget(handle) = made {
                        drop_widget(host, ui, handle);
                    }
                    return Err(Error::NoMemory);
                }
                // A widget the host cannot measure may not be made lower than one.
                Err(_) => (1, false),
            },
            None => (1, false),
        };
        Ok(Some(Cell {
            what,
            item: index,
            least,
            rect,
            name,
            face,
            widget,
            blank,
        }))
    }

    /// A widget's name as the registry lists it (its id where it has none).
    fn widget_title<'a>(src: &'a HostSource, id: &'a str) -> &'a str {
        let base = native_pages::base(id);
        src.widget_rows()
            .iter()
            .find(|row| row.id == base)
            .map_or(id, |row| row.title.as_str())
    }

    /// The places the buttons reach: tools, what the widgets answer, the pencil.
    fn targets(
        host: &Host<'_>,
        ui: &Ui,
        cells: &[Cell],
        footer: &Footer,
        buttons: Buttons,
    ) -> Result<Vec<Target>, Error> {
        let mut targets = Vec::new();
        for (index, cell) in cells.iter().enumerate() {
            match cell.widget {
                // Editing: an item is one target, whatever it is.
                _ if buttons.editing => push(&mut targets, tool_target(cell.rect, index))?,
                None => push(&mut targets, tool_target(cell.rect, index))?,
                Some(MadeWidget::Widget(widget)) => {
                    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                    let found = match ui.widget_targets(
                        host,
                        widget,
                        paint::frame_rect(cell.rect),
                        &mut input,
                        &mut out,
                    ) {
                        Ok(found) => found,
                        // A widget the host lays out for no one (the X4's
                        // closed nested callbacks): nothing to reach in it.
                        Err(Error::Unsupported) => continue,
                        Err(error) => return Err(error),
                    };
                    for target in found {
                        let target = target?;
                        let at = target.rect();
                        let rect =
                            Rect::new(clamp(at.x), clamp(at.y), clamp(at.width), clamp(at.height));
                        let owner = Owner::Widget {
                            item: index,
                            target: target.target(),
                        };
                        push(&mut targets, Target { rect, owner })?;
                    }
                }
                // Drawn by this plugin itself: none is.
                Some(_) => {}
            }
        }
        footer_targets(&mut targets, footer, buttons)?;
        Ok(targets)
    }

    /// The word on the pencil (*Done* while editing, else *Edit*) and, while
    /// editing, how wide it is; nothing where the pencil is off.
    fn pencil_word(
        host: &Host<'_>,
        display: &Snapshot,
        font: &Font<'_>,
        on: bool,
        editing: bool,
    ) -> Result<(String, u16), Error> {
        if !on {
            return Ok((String::new(), 0));
        }
        let key = if editing {
            "picture-home.edit.done"
        } else {
            "picture-home.edit.edit"
        };
        let word = Messages::new()?.copy(host, key)?;
        let wide = if editing {
            let style = Style::read(host, display, font)?;
            clamp_u32((font.measure_text(style.small, &word)? as u32).saturating_add(1))
        } else {
            0
        };
        Ok((word, wide))
    }

    /// The widgets of the page before are let go; a refusal is of no matter (the
    /// host drops them when the screen closes).
    fn drop_widgets(&mut self, host: &Host<'_>, ui: &Ui) {
        if let Some(laid) = self.laid.take() {
            release_cells(host, ui, laid.cells);
        }
    }

    /// Draws the page: the plates and the tools' names first, then the widgets
    /// composed into their cells, then the focus over them.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        if self.backdrop.making() {
            // Nothing else while the picture is made, as on interface 6.
            return Self::draw_notice(host, frame);
        }
        self.ensure(host, ui)?;
        self.ensure_dialog(host)?;
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = self.paint(host, ui, frame, &display, &font, laid);
        let released = font.release();
        drawn?;
        released
    }

    /// The notice that the picture is being made.
    fn draw_notice(host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = Style::read(host, &display, &font).and_then(|style| {
            let screen = (display.display_width(), display.display_height());
            picture::notice(host, frame, &font, &style, screen)
        });
        let released = font.release();
        drawn?;
        released
    }

    fn paint(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
        display: &Snapshot,
        font: &Font<'_>,
        laid: &Laid,
    ) -> Result<(), Error> {
        let style = Style::read(host, display, font)?;
        let mut below = Vec::new();
        let mut above = Vec::new();
        // The bar's items its owners draw are composed once the paint is replayed.
        let shown = laid.bar.as_ref().is_some_and(|plan| self.bar_shown(plan));
        let mut buffers = [Vec::new(), Vec::new()];
        if shown {
            buffers = [buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?];
        }
        let mut placed = Vec::new();
        let face = status_line::Face {
            host,
            font,
            small: style.small,
        };
        self.bar_paint(
            &face,
            ui,
            (laid, style.corner),
            (&mut below, &mut placed),
            &mut buffers,
        )?;
        // The picture, not while the page is arranged.
        let drawing = if self.editing {
            None
        } else {
            self.backdrop.drawing()
        };
        let icon_font = paperpaw_ui_kit::icons::font(host)?;
        let circle = paperpaw_ui_kit::icons::glyph(host, icons::CIRCLE)?;
        let fonts = icons::Fonts {
            ui: font,
            icons: icon_font.as_ref(),
            circle: circle.as_ref(),
        };
        // Names hyphenated in the interface language, as Beta 3's grid did.
        let hyphens = icons::Hyphens::of(host);
        let mut breaks = |word: &str, out: &mut [u8; MAX_BREAKS]| hyphens.breaks(host, word, out);
        self.paint_page(
            &mut below,
            &fonts,
            &style,
            (laid, drawing.is_some()),
            &mut breaks,
        )?;
        self.drag_frame(&mut above, &style, laid)?;
        if self.focus.shown()
            && let Some(target) = laid.targets.get(self.focus.at())
        {
            ring(&mut above, target.rect, &style)?;
        }
        // The picture first.
        if let Some(drawing) = drawing {
            picture::draw(frame, font, &style, &drawing)?;
        }
        paint::replay_icons(frame, font, fonts.icons, below.iter())?;
        for item in &placed {
            // An owner that cannot draw now (busy, gone) leaves its place empty.
            Ui::compose(host, item.target, item.rect).ok();
        }
        compose_each(
            laid.cells.iter().filter_map(|cell| match cell.widget {
                Some(MadeWidget::Widget(widget)) => Some((widget, cell.rect)),
                _ => None,
            }),
            |widget, rect| Ui::compose(host, Compose::Widget(widget), paint::frame_rect(rect)),
        );
        paint::replay(frame, font, above.iter())?;
        let mut dialog = Vec::new();
        self.dialog_paint(font, &mut dialog)?;
        paint::replay(frame, font, dialog.iter())
    }

    /// Takes an input: the page buttons turn the page, the others move the
    /// focus or run what it stands on, a tap runs a tool or reaches a widget.
    ///
    /// # Errors
    /// Allocation and host errors.
    pub fn input(&mut self, host: &Host<'_>, ui: &Ui, input: &Input<'_>) -> Result<Handled, Error> {
        if self.backdrop.making() {
            // The page is not on the screen: the host's own keys (back) apply.
            return Ok(Handled::No);
        }
        self.ensure(host, ui)?;
        if let Some(handled) = self.bar_press(host, input)? {
            return Ok(handled);
        }
        if let Some(handled) = self.arrange_input(host, ui, input)? {
            return Ok(handled);
        }
        match *input {
            Input::Action { action, .. } => self.action(host, ui, action),
            Input::Gesture { position, kind, .. } => match touch_of(kind) {
                Touch::Tap => self.tapped(host, ui, position, WidgetInputKind::Tap),
                // A long press starts editing, as on interface 6; while editing
                // it is taken and runs nothing.
                Touch::Held => {
                    if !self.editing {
                        self.next(host, ui, Next::Edit(true))?;
                    }
                    Ok(Handled::Yes)
                }
                Touch::Pass => Ok(Handled::No),
            },
            _ => Ok(Handled::No),
        }
    }

    fn action(&mut self, host: &Host<'_>, ui: &Ui, action: &str) -> Result<Handled, Error> {
        if action == "core.next-page" || action == "core.prev-page" {
            return self.turn(host, ui, action == "core.next-page");
        }
        if action == "core.back" && self.editing {
            // Back ends editing, as on interface 6.
            self.next(host, ui, Next::Edit(false))?;
            return Ok(Handled::Yes);
        }
        let Some(press) = Press::of(action) else {
            return Ok(Handled::No);
        };
        let Some(laid) = self.laid.as_ref() else {
            return Ok(Handled::No);
        };
        match self.focus.press(press, &laid.targets, laid.arrows_stay) {
            Step::Ignored => Ok(Handled::No),
            Step::Held => Ok(Handled::Yes),
            Step::Shown(_) | Step::Moved(_) => {
                redraw(host)?;
                Ok(Handled::Yes)
            }
            Step::Run(at) => {
                let target = laid.targets.get(at).copied();
                let next = self.run(host, ui, target, None)?;
                self.next(host, ui, next)?;
                Ok(Handled::Yes)
            }
        }
    }

    fn turn(&mut self, host: &Host<'_>, ui: &Ui, forward: bool) -> Result<Handled, Error> {
        let Some(nav) = self.nav.as_mut() else {
            return Ok(Handled::No);
        };
        if !nav.turn(forward) {
            // The first or last page: taken, nothing turns (as on interface 6).
            return Ok(Handled::Yes);
        }
        self.focus.reset();
        self.stale = true;
        self.ensure(host, ui)?;
        redraw(host)?;
        Ok(Handled::Yes)
    }

    fn tapped(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        point: Point,
        kind: WidgetInputKind,
    ) -> Result<Handled, Error> {
        if !self.editing && self.bar_tap(host, ui, point)? {
            return Ok(Handled::Yes);
        }
        let Some(laid) = self.laid.as_ref() else {
            return Ok(Handled::No);
        };
        // The nearest target the point is in: widgets' own first (they answer
        // at their places), then the tools and the buttons.
        let hit = laid
            .targets
            .iter()
            .copied()
            .find(|target| inside(target.rect, point));
        let target = hit.or_else(|| {
            // A tap inside a widget that has no target is the widget's as a whole.
            let (item, cell) = laid
                .cells
                .iter()
                .enumerate()
                .find(|(_, cell)| cell.widget.is_some() && inside(cell.rect, point))?;
            Some(Target {
                rect: cell.rect,
                owner: Owner::Widget { item, target: 0 },
            })
        });
        let Some(target) = target else {
            return Ok(Handled::No);
        };
        let next = self.run(host, ui, Some(target), Some((point, kind)))?;
        self.next(host, ui, next)?;
        Ok(Handled::Yes)
    }

    /// Does what a target is for: runs a tool, tells a widget (a press of the
    /// buttons is a tap at the middle of its target).
    fn run(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        target: Option<Target>,
        touched: Option<(Point, WidgetInputKind)>,
    ) -> Result<Next, Error> {
        let (Some(target), Some(laid)) = (target, self.laid.as_ref()) else {
            return Ok(Next::Nothing);
        };
        if let Some(next) = decide(target.owner, self.editing) {
            return Ok(next);
        }
        match target.owner {
            Owner::Tool(index) => {
                let Some(Cell {
                    what: What::Tool(action),
                    ..
                }) = laid.cells.get(index)
                else {
                    return Ok(Next::Nothing);
                };
                let mut scratch = buffer(SERVICE_BUFFER)?;
                CommandSender::new(host, &mut scratch).run_action(action)?;
            }
            Owner::Widget { item, target: id } => {
                let Some(MadeWidget::Widget(widget)) =
                    laid.cells.get(item).and_then(|cell| cell.widget)
                else {
                    return Ok(Next::Nothing);
                };
                let (point, kind) = touched.unwrap_or((middle(target.rect), WidgetInputKind::Tap));
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                ui.widget_input(host, widget, id, point, kind, &mut input, &mut out)?;
            }
            Owner::Button(_) => {}
        }
        Ok(Next::Nothing)
    }

    /// Does what a press left to do: edit or stop editing (laid out anew, the
    /// focus where it was), or open an item's options.
    fn next(&mut self, host: &Host<'_>, ui: &Ui, next: Next) -> Result<(), Error> {
        match next {
            Next::Nothing => Ok(()),
            Next::Edit(true) if self.laid.as_ref().is_some_and(|laid| laid.wide) => {
                // Pages are arranged upright: on its side they are derived.
                stage::say(host, "picture-home.edit-upright")
            }
            Next::Edit(on) => {
                self.editing = on;
                self.stale = true;
                self.ensure(host, ui)?;
                redraw(host)
            }
            Next::Options(cell) => self.open_options(host, cell),
            Next::Add => {
                let page = self.nav.map_or(0, |nav| nav.page);
                add_screen::open(host, ui, page, None)
            }
        }
    }
}

/// Which of the footer's buttons are there.
#[derive(Debug, Clone, Copy)]
struct Buttons {
    /// Editing: each item is one target, not what its widget answers.
    editing: bool,
    plus: bool,
    pencil: bool,
}

/// The footer's buttons as targets, in the order the focus reaches them: *Add*
/// (while editing), then the pencil.
fn footer_targets(
    targets: &mut Vec<Target>,
    footer: &Footer,
    buttons: Buttons,
) -> Result<(), Error> {
    if buttons.plus {
        push(
            targets,
            Target {
                rect: footer.plus,
                owner: Owner::Button(Button::Plus),
            },
        )?;
    }
    if buttons.pencil {
        push(
            targets,
            Target {
                rect: footer.pencil,
                owner: Owner::Button(Button::Pencil),
            },
        )?;
    }
    Ok(())
}

fn tool_target(rect: Rect, index: usize) -> Target {
    Target {
        rect,
        owner: Owner::Tool(index),
    }
}

/// What the items sit on: white as the built-in home screen has it, a gray or
/// colour, or nothing (`picture-home.plate`, as interface 6 read it).
fn plate_of(src: &HostSource) -> Option<Color> {
    match colour_spec(&src.plate) {
        ColourSpec::Is(colour) => Some(colour),
        ColourSpec::None => None,
        _ => Some(Color::WHITE),
    }
}

/// What a press on `owner` is without asking the host: the pencil starts or ends
/// editing, *Add* opens its screen, and while editing an item offers its options
/// and nothing else runs. `None`: it runs.
const fn decide(owner: Owner, editing: bool) -> Option<Next> {
    match owner {
        Owner::Button(Button::Pencil) => Some(Next::Edit(!editing)),
        Owner::Button(Button::Plus) => Some(Next::Add),
        // An item tapped or chosen while editing offers its options.
        Owner::Tool(cell) if editing => Some(Next::Options(cell)),
        Owner::Widget { item, .. } if editing => Some(Next::Options(item)),
        _ if editing => Some(Next::Nothing),
        _ => None,
    }
}

/// What a touch gesture is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    /// Runs the tool or the widget under it.
    Tap,
    /// Taken; nothing runs.
    Held,
    /// Not for the dashboard.
    Pass,
}

fn touch_of(kind: InputGestureKind) -> Touch {
    match kind {
        InputGestureKind::Tap => Touch::Tap,
        InputGestureKind::LongPress => Touch::Held,
        _ => Touch::Pass,
    }
}

/// Composes each widget into its cell. One the host cannot compose is left
/// out: the rest of the page still shows (as on interface 6). Returns how many
/// were composed.
fn compose_each<T>(
    widgets: impl Iterator<Item = (T, Rect)>,
    mut compose: impl FnMut(T, Rect) -> Result<(), Error>,
) -> usize {
    let mut composed = 0;
    for (widget, rect) in widgets {
        if compose(widget, rect).is_ok() {
            composed += 1;
        }
    }
    composed
}

/// Lets a widget go; a refusal is of no matter (the host drops them when the
/// screen closes).
fn drop_widget(host: &Host<'_>, ui: &Ui, widget: WidgetHandle) {
    if let (Ok(mut input), Ok(mut out)) = (buffer(SERVICE_BUFFER), buffer(64)) {
        let _ = ui.widget_drop(host, widget, &mut input, &mut out);
    }
}

/// Lets the widgets of these cells go.
fn release_cells(host: &Host<'_>, ui: &Ui, cells: Vec<Cell>) {
    for cell in cells {
        if let Some(MadeWidget::Widget(widget)) = cell.widget {
            drop_widget(host, ui, widget);
        }
    }
}

/// The focus ring round a target.
fn ring(paint: &mut Vec<Paint>, target: Rect, style: &Style) -> Result<(), Error> {
    let air = style.gap / 2;
    let rect = Rect::new(
        target.x.saturating_sub(air),
        target.y.saturating_sub(air),
        target.width.saturating_add(2 * air),
        target.height.saturating_add(2 * air),
    );
    paintpush(
        paint,
        Paint::Focus(
            rect,
            style.corner,
            Some(u32::from(style.line).max(2)),
            Color::BLACK,
        ),
    )
}

fn paintpush(paint: &mut Vec<Paint>, op: Paint) -> Result<(), Error> {
    paint::push(paint, op)
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

fn inside(rect: Rect, point: Point) -> bool {
    point.x >= i32::from(rect.x)
        && point.y >= i32::from(rect.y)
        && point.x < i32::from(rect.x) + i32::from(rect.width)
        && point.y < i32::from(rect.y) + i32::from(rect.height)
}

fn middle(rect: Rect) -> Point {
    Point::new(
        i32::from(rect.x) + i32::from(rect.width) / 2,
        i32::from(rect.y) + i32::from(rect.height) / 2,
    )
}

fn clamp(value: impl TryInto<u16>) -> u16 {
    value.try_into().unwrap_or(u16::MAX)
}

fn clamp_u32(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn clamp_i32(value: i32) -> u16 {
    u16::try_from(value.max(0)).unwrap_or(u16::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_inside_a_rectangle_by_its_left_and_top_edges() {
        let r = Rect::new(10, 20, 30, 40);
        assert!(inside(r, Point::new(10, 20)));
        assert!(inside(r, Point::new(39, 59)));
        assert!(!inside(r, Point::new(40, 59)));
        assert!(!inside(r, Point::new(39, 60)));
        assert!(!inside(r, Point::new(9, 30)));
    }

    #[test]
    fn the_pencil_toggles_editing() {
        let pencil = Owner::Button(Button::Pencil);
        assert_eq!(decide(pencil, false), Some(Next::Edit(true)));
        assert_eq!(decide(pencil, true), Some(Next::Edit(false)));
    }

    #[test]
    fn add_opens_its_screen_and_is_reached_before_the_pencil() {
        assert_eq!(decide(Owner::Button(Button::Plus), true), Some(Next::Add));
        let footer = Footer {
            pencil: Rect::new(300, 400, 120, 60),
            plus: Rect::new(10, 400, 120, 60),
        };
        let owners = |buttons: Buttons| {
            let mut targets = Vec::new();
            footer_targets(&mut targets, &footer, buttons).unwrap();
            targets.iter().map(|t| t.owner).collect::<Vec<_>>()
        };
        assert_eq!(
            owners(Buttons {
                editing: true,
                plus: true,
                pencil: true
            }),
            [Owner::Button(Button::Plus), Owner::Button(Button::Pencil)]
        );
        // Not editing: the pencil alone, or nothing where it is off.
        assert_eq!(
            owners(Buttons {
                editing: false,
                plus: false,
                pencil: true
            }),
            [Owner::Button(Button::Pencil)]
        );
        assert!(
            owners(Buttons {
                editing: false,
                plus: false,
                pencil: false
            })
            .is_empty()
        );
        crate::fail_alloc::sweep(|| {
            let mut targets = Vec::new();
            footer_targets(
                &mut targets,
                &footer,
                Buttons {
                    editing: true,
                    plus: true,
                    pencil: true,
                },
            )
        });
    }

    #[test]
    fn while_editing_an_item_offers_its_options_and_the_rest_runs_nothing() {
        assert_eq!(decide(Owner::Tool(2), true), Some(Next::Options(2)));
        assert_eq!(
            decide(Owner::Widget { item: 1, target: 3 }, true),
            Some(Next::Options(1))
        );
        // Not editing: it runs (the host decides what).
        for owner in [Owner::Tool(0), Owner::Widget { item: 0, target: 1 }] {
            assert_eq!(decide(owner, false), None);
        }
    }

    #[test]
    fn a_dashboard_opened_again_keeps_its_page_and_focus() {
        let mut nav = Nav::new(3);
        assert!(nav.turn(true));
        assert!(nav.turn(true));
        let mut dashboard = Dashboard {
            nav: Some(nav),
            ..Dashboard::new()
        };
        dashboard.focus.show(true);
        dashboard.stale = false;
        dashboard.reopened(false, None);
        assert_eq!(dashboard.nav.map(|n| n.page), Some(2));
        assert!(dashboard.focus.shown());
        assert!(!dashboard.editing());
        // Not laid out again for nothing.
        assert!(!dashboard.stale);
        // Asked to edit: editing starts, the page stays.
        dashboard.reopened(true, None);
        assert!(dashboard.editing());
        assert!(dashboard.stale);
        assert_eq!(dashboard.nav.map(|n| n.page), Some(2));
    }

    #[test]
    fn a_page_setting_opens_its_page_for_editing() {
        let mut dashboard = Dashboard::new();
        dashboard.start_editing(Some(2));
        assert!(dashboard.editing());
        assert_eq!(dashboard.first, Some(2));
        let mut open = Dashboard::new();
        open.stale = false;
        open.reopened(false, Some(1));
        assert_eq!(open.first, Some(1));
        assert!(open.stale);
    }

    #[test]
    fn a_long_press_is_taken_and_runs_nothing() {
        assert_eq!(touch_of(InputGestureKind::LongPress), Touch::Held);
        assert_eq!(touch_of(InputGestureKind::Tap), Touch::Tap);
    }

    #[test]
    fn the_plate_is_white_a_colour_or_none_as_the_setting_says() {
        let src = |plate: &str| {
            let mut src = HostSource::default();
            src.plate = plate.into();
            src
        };
        assert_eq!(plate_of(&src("")), Some(Color::WHITE));
        for (value, colour) in [
            (paperpaw_sdk::widgets::PAPER_NORMAL, Some(Color::WHITE)),
            (paperpaw_sdk::widgets::PAPER_NONE, None),
        ] {
            assert_eq!(plate_of(&src(value)), colour, "{value}");
        }
        // The gray the setting names is the one the SDK's grid paints.
        let gray = plate_of(&src(paperpaw_sdk::widgets::PAPER_LIGHT_GRAY));
        assert!(gray.is_some() && gray != Some(Color::WHITE));
        assert_eq!(
            plate_of(&src("#808080")),
            Some(Color::rgb(0x80, 0x80, 0x80))
        );
    }

    #[test]
    fn a_widget_that_fails_to_compose_leaves_the_rest_of_the_page_drawn() {
        let cell = |n: u16| Rect::new(n * 10, 0, 10, 10);
        let mut seen = Vec::new();
        let composed = compose_each(
            [(1_u32, cell(0)), (2, cell(1)), (3, cell(2))].into_iter(),
            |id, rect| {
                seen.push((id, rect.x));
                if id == 2 { Err(Error::Denied) } else { Ok(()) }
            },
        );
        // All three were tried, in order; the failing one alone is missing.
        assert_eq!(seen, [(1, 0), (2, 10), (3, 20)]);
        assert_eq!(composed, 2);
        // A host that refuses every one: none composed, no error.
        assert_eq!(
            compose_each([(1_u32, cell(0)), (2, cell(1))].into_iter(), |_, _| Err(
                Error::NoMemory
            )),
            0
        );
    }

    #[test]
    fn a_widgets_change_draws_its_cell_or_the_screen_where_the_focus_moved() {
        let cell = Rect::new(10, 20, 200, 100);
        assert_eq!(Redraw::after(cell, false, false), Redraw::Area(cell));
        assert_eq!(Redraw::after(cell, false, true), Redraw::Area(cell));
        assert_eq!(Redraw::after(cell, true, false), Redraw::Area(cell));
        assert_eq!(Redraw::after(cell, true, true), Redraw::Screen);
    }

    #[test]
    fn a_widgets_change_is_not_looked_for_on_a_page_not_laid_out_or_behind_the_notice() {
        // Not laid out yet (stale): it is drawn as it is when laid out.
        let fresh = Dashboard::new();
        assert_eq!(fresh.cell_showing(7), None);
        let mut laid_none = Dashboard::new();
        laid_none.stale = false;
        assert_eq!(laid_none.cell_showing(7), None);
        // The picture is being made: the notice shows, not the page.
        let mut making = Dashboard::new();
        making.stale = false;
        let asked = crate::native_picture::Asked {
            path: "a.png",
            bytes: 0,
            modified: 0,
            fit: "picture-home.fit.fill",
            anchor: "picture-home.anchor.center",
            zoom: 100,
            lighten: 0,
            brightness: "",
            contrast: "",
            render: "",
        };
        let recipe = crate::native_picture::Recipe::new(&asked, 480, 800).unwrap();
        let mut cookie = 0;
        making.backdrop.want(recipe, &mut cookie).unwrap();
        assert!(making.backdrop.making());
        assert_eq!(making.cell_showing(7), None);
    }

    #[test]
    fn the_middle_of_a_target_is_where_a_button_press_taps() {
        assert_eq!(middle(Rect::new(10, 20, 30, 40)), Point::new(25, 40));
    }
}
