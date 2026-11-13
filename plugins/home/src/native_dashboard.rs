//! The dashboard screen, viewing: the page of tools and widgets in its grid,
//! the footer's actions and dots, page turning by the page buttons, the focus
//! with the buttons, a tap that runs a tool or reaches a widget. What it is
//! made of without the host is `native_dash` (the parts' places), `native_focus`
//! (the buttons' focus), `native_pages` and `native_grid` (the pages) and
//! `native_source` (what is read of the device); this file reads, draws and
//! hands inputs on through the host.
//!
//! A widget of this plugin's own (the date, the clock, the card, the shelf, the
//! status line) is drawn by the plugin itself (`native_made`): the host never
//! calls it back from its own draw.
//!
//! Not yet (ignored acceptance cases, `PORTING-7.md`): the status bar above the
//! grid, and what the host lacks to run any of it (H1, H4).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    keys::InputMode,
    native::{
        Error, Host,
        callbacks::{DrawFrame, FrameRect, Input, InputGestureKind},
        canvas::PartRole,
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot::{self, Snapshot},
        ui::{Ui, WidgetInputKind},
    },
    widgets::{CellSpan, grid::Geometry},
};
use paperpaw_ui_kit::{
    Messages, Paint, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint, text,
};

use crate::native_add_screen as add_screen;
use crate::native_dash::{self as dash, Footer, Frame, Nav, Wanted};
use crate::native_focus::{Button, Focus, Owner, Press, Step, Target};
use crate::native_grid::{self as grid, What};
use crate::native_icons::{self as icons, Face, Fonts, NAME_LINES, Tool};
use crate::native_made::Made;
use crate::native_measure::{self as measure, PLACE};
use crate::native_pages;
use crate::native_shown::Shown;
use crate::native_source::HostSource;

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
    widget: Option<Made>,
    /// The cells a widget declared (its least size, square), kept when it is
    /// sized with a finger.
    span: Option<CellSpan>,
}

/// The page as laid out: what shows, where, and what the buttons reach.
#[derive(Debug)]
struct Laid {
    frame: Frame,
    cells: Vec<Cell>,
    targets: Vec<Target>,
    /// The footer's actions, in the order of `footer.actions`.
    actions: Vec<String>,
    pages: usize,
    dots: bool,
    tool_labels: bool,
    focus_at_once: bool,
    /// The arrow keys stay on the page.
    arrows_stay: bool,
    /// Where the pencil (*Done* while editing) is, and its word.
    pencil: Option<(Rect, String)>,
    /// The pencil's icon, where the host has one.
    pencil_icon: Option<paperpaw_sdk::native::fonts::IconGlyph>,
    /// Where each of the footer's actions is, and its icon.
    action_icons: Vec<(Rect, Option<paperpaw_sdk::native::fonts::IconGlyph>)>,
    /// Where *Add* is, while editing.
    plus: Option<Rect>,
    /// The items shown, each with its cells: what is kept when one is moved,
    /// sized or removed.
    items: Vec<grid::Item>,
    /// The setting that keeps the page (0 for `home.page-1`), and whether the
    /// screen is on its side.
    slot: usize,
    wide: bool,
    /// How wide and high the corner of an item is that a finger sizes it by.
    handle: u16,
    /// The status line above the grid (`home.status-bar`), made and placed.
    status: Option<(Made, Rect)>,
    /// How far the focus ring reaches outside a target: what a focus step
    /// draws again around it, kept here so that a step reads no style.
    ring_room: i32,
}

/// Which of the footer's buttons are there.
#[derive(Debug, Clone, Copy)]
struct Buttons {
    /// Editing: each item is one target, not what its widget answers.
    editing: bool,
    plus: bool,
    pencil: bool,
}

/// What a press on a target leaves to do besides what it did itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Nothing,
    /// Editing starts (`true`) or ends.
    Edit(bool),
    /// The *Add* screen opens for the page that shows.
    Add,
    /// The options of the item of this cell open.
    Options(usize),
}

mod arrange;

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
    /// What the reader used last before the show (`None`: not known).
    last_input: Option<InputMode>,
    /// Editing the home screen: the pencil is *Done*, *Add* shows; what is
    /// tapped offers the item's options.
    editing: bool,
    /// The options of an item, over the page.
    options: Option<arrange::Options>,
    /// An item the buttons are moving or sizing.
    moving: Option<arrange::Moving>,
    /// An item a finger has taken along.
    drag: Option<arrange::Drag>,
    /// The top left of the screen's region at the last draw: where the
    /// cells' places are counted from when a part of it is drawn again.
    origin: (i32, i32),
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

    /// Whether the home screen is being edited.
    #[must_use]
    pub const fn editing(&self) -> bool {
        self.editing
    }

    /// Opened again by the host while it is open: its view is kept (the page
    /// it is on, the widgets it made, the focus), editing starts where asked.
    /// A new dashboard would drop the widgets it made without letting them go.
    pub fn reopened(&mut self, edit: bool) {
        if edit && !self.editing {
            self.set_editing(true);
        }
    }

    /// Starts editing (opened to edit with `mode` = `edit`).
    pub fn start_editing(&mut self) {
        self.editing = true;
    }

    /// What shows is read and laid out again before the next draw or input.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Shown again (above another screen closed, a book left), after the
    /// reader used `last_input` last.
    pub fn shown(&mut self, last_input: Option<InputMode>) {
        self.stale = true;
        self.reshow = true;
        self.last_input = last_input;
        // A finger's drag does not outlive the screen's being covered.
        self.drag = None;
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
            if let Some((made, _)) = laid.status {
                made.release(host, ui);
            }
            release_cells(host, ui, laid.cells);
            return Err(error);
        }
        self.drop_widgets(host, ui);
        // The cells are new: what pointed at the old ones is no more.
        self.options = None;
        self.moving = None;
        self.focus.clamp(laid.targets.len());
        if self.reshow {
            // As on interface 6: at every show the focus is drawn only if the
            // reader chose to see it at once and used buttons last; a finger,
            // a pen or an input the host does not know of get none.
            self.focus
                .show(focus_at_show(laid.focus_at_once, self.last_input));
            self.reshow = false;
        }
        self.laid = Some(laid);
        self.drag = None;
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
            ..
        } = Shown::read(host, display, font)?;
        let ring_room = ring_room(&Style::read(host, display, font)?);
        let nav = self.nav.get_or_insert(Nav::new(pages.len()));
        nav.resize(pages.len());
        let (page, count) = (nav.page, nav.pages);
        let slot = pages.get(page).map_or(0, |p| p.slot);
        let items = match pages.get(page) {
            Some(page) => native_pages::expand(&src, &page.items)?,
            None => Vec::new(),
        };
        Self::measure_unplaced(host, ui, &mut src, &frame.geometry, &items)?;
        let (shown, _) = grid::fit(&src, &frame.geometry, &items)?;
        let editing = self.editing;
        let pencil_on = editing || src.pencil;
        let (word, done_word) = self.pencil_word(host, display, font, pencil_on)?;
        let footer = dash::footer(
            &metrics,
            frame.footer,
            &Wanted {
                editing,
                pencil: pencil_on,
                actions: src.footer.len(),
                done_word,
            },
        )?;
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
        let (targets, actions) = match made {
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
        let footer_icons = icons::footer(host, &src, &footer.actions);
        let pencil_icon = icons::pencil(host);
        let (action_icons, pencil_icon) = match (footer_icons, pencil_icon) {
            (Ok(actions), Ok(pencil)) => (actions, pencil),
            (Err(error), _) | (_, Err(error)) => {
                release_cells(host, ui, cells);
                return Err(error);
            }
        };
        let status = match Self::status_bar(host, ui, frame.status) {
            Ok(status) => status,
            Err(error) => {
                release_cells(host, ui, cells);
                return Err(error);
            }
        };
        Ok(Laid {
            frame,
            cells,
            targets,
            actions,
            pages: count,
            dots: src.dots,
            tool_labels: src.tool_labels,
            focus_at_once: src.focus_at_once,
            arrows_stay: src.arrows_stay,
            pencil: pencil_on.then_some((footer.pencil, word)),
            pencil_icon,
            action_icons,
            plus: editing.then_some(footer.plus),
            items,
            slot,
            wide,
            handle: (metrics.row / 2).max(16),
            status,
            ring_room,
        })
    }

    /// The status line above the grid: this plugin's own widget, placed where the
    /// frame keeps its strip. One that cannot be made leaves the strip empty.
    fn status_bar(
        host: &Host<'_>,
        ui: &Ui,
        strip: Option<Rect>,
    ) -> Result<Option<(Made, Rect)>, Error> {
        let Some(rect) = strip else {
            return Ok(None);
        };
        match Made::make(host, ui, native_pages::STATUS, PLACE) {
            Ok(made) => match made.place(host, ui, rect) {
                Ok(()) => Ok(Some((made, rect))),
                Err(error) => {
                    made.release(host, ui);
                    Err(error)
                }
            },
            Err(Error::NotFound | Error::Denied) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The widgets with no place yet (and no cells declared) are measured at the
    /// grid's width (made, asked and let go), so the grid gives each the rows it
    /// needs. A widget the
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
            // As interface 6: one that says how many cells it takes is not made.
            if src.declares(id) {
                continue;
            }
            match measure::measure_widget(host, ui, id, PLACE, geometry) {
                Ok(Some((rows, least))) => src.measured(id, rows, least)?,
                Ok(None) => {}
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// The cells of the items shown, what the buttons reach and the footer's
    /// actions. On an error the caller lets the cells made so far go.
    fn fill(
        host: &Host<'_>,
        ui: &Ui,
        src: &HostSource,
        frame: &Frame,
        shown: &[grid::Item],
        (footer, buttons): (&Footer, Buttons),
        cells: &mut Vec<Cell>,
    ) -> Result<(Vec<Target>, Vec<String>), Error> {
        for (index, item) in shown.iter().enumerate() {
            if let Some(cell) = Self::cell(host, ui, src, &frame.geometry, (index, item))? {
                push(cells, cell)?;
            }
        }
        let targets = Self::targets(host, ui, cells, footer, buttons)?;
        let mut actions = Vec::new();
        for (action, _, _) in src.footer.iter().take(footer.actions.len()) {
            push(&mut actions, text::copy(action)?)?;
        }
        Ok((targets, actions))
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
            What::Widget(id) => match Made::make(host, ui, id, PLACE) {
                Ok(made) => {
                    if let Err(error) = made.place(host, ui, rect) {
                        made.release(host, ui);
                        return Err(error);
                    }
                    match text::copy(Self::widget_title(src, id)) {
                        Ok(name) => (name, Some(made)),
                        Err(error) => {
                            made.release(host, ui);
                            return Err(error);
                        }
                    }
                }
                // A widget that is gone, or not for this screen: no cell.
                Err(Error::NotFound | Error::Denied) => return Ok(None),
                Err(error) => return Err(error),
            },
        };
        let what = match item.what.try_clone() {
            Ok(what) => what,
            Err(error) => {
                if let Some(made) = widget {
                    made.release(host, ui);
                }
                return Err(error);
            }
        };
        let measured = widget.as_ref().map(|made| made.measure(host, ui, geometry));
        let least = match measured {
            Some(Ok((_, least))) => least,
            Some(Err(Error::NoMemory)) => {
                if let Some(made) = widget {
                    made.release(host, ui);
                }
                return Err(Error::NoMemory);
            }
            // A widget the host cannot measure may not be made lower than one.
            Some(Err(_)) => 1,
            None => 1,
        };
        Ok(Some(Cell {
            what,
            item: index,
            least,
            rect,
            name,
            face,
            widget,
            span: match &item.what {
                What::Widget(id) => src.span(id),
                What::Tool(_) => None,
            },
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

    /// The places the buttons reach: tools, what the widgets answer, the
    /// footer's actions.
    fn targets(
        host: &Host<'_>,
        ui: &Ui,
        cells: &[Cell],
        footer: &Footer,
        buttons: Buttons,
    ) -> Result<Vec<Target>, Error> {
        let mut targets = Vec::new();
        for (index, cell) in cells.iter().enumerate() {
            match &cell.widget {
                // Editing: an item is one target, whatever it is.
                _ if buttons.editing => push(
                    &mut targets,
                    Target {
                        rect: cell.rect,
                        owner: Owner::Tool(index),
                    },
                )?,
                None => push(
                    &mut targets,
                    Target {
                        rect: cell.rect,
                        owner: Owner::Tool(index),
                    },
                )?,
                Some(made) => {
                    for (target, rect) in made.targets(host, ui, cell.rect)? {
                        push(
                            &mut targets,
                            Target {
                                rect,
                                owner: Owner::Widget {
                                    item: index,
                                    target,
                                },
                            },
                        )?;
                    }
                }
            }
        }
        footer_targets(&mut targets, footer, buttons)?;
        Ok(targets)
    }

    /// The word on the pencil (*Done* while editing, else *Edit*) and, while
    /// editing, how wide it is; nothing where the pencil is off.
    fn pencil_word(
        &self,
        host: &Host<'_>,
        display: &Snapshot,
        font: &Font<'_>,
        on: bool,
    ) -> Result<(String, u16), Error> {
        if !on {
            return Ok((String::new(), 0));
        }
        let key = if self.editing {
            "home.edit.done"
        } else {
            "home.edit.edit"
        };
        let word = Messages::new()?.copy(host, key)?;
        let wide = if self.editing {
            let style = Style::read(host, display, font)?;
            clamp_u32((font.measure_text(style.small, &word)? as u32).saturating_add(1))
        } else {
            0
        };
        Ok((word, wide))
    }

    /// Whether a page is held.
    #[must_use]
    pub const fn holds_page(&self) -> bool {
        self.laid.is_some()
    }

    /// Another screen is over this one: the page, and the widgets made for it,
    /// are let go and laid out again when it is shown (as it always is, shown
    /// again). What a finger or the buttons are doing with the page keeps it.
    pub fn cover(&mut self, host: &Host<'_>, ui: &Ui) {
        if self.options.is_some() || self.moving.is_some() || self.drag.is_some() {
            return;
        }
        self.drop_widgets(host, ui);
        self.stale = true;
    }

    /// The widgets of the page before are let go; a refusal is of no matter
    /// (the host drops them when the screen closes).
    fn drop_widgets(&mut self, host: &Host<'_>, ui: &Ui) {
        if let Some(laid) = self.laid.take() {
            if let Some((made, _)) = laid.status {
                made.release(host, ui);
            }
            release_cells(host, ui, laid.cells);
        }
    }

    /// Draws the page: tools and the footer first, then the widgets composed
    /// into their cells, then the focus over them.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        self.ensure(host, ui)?;
        self.ensure_dialog(host)?;
        self.origin = (frame.rect.x, frame.rect.y);
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

    /// The lines of the name of each tool shown (with names), with the top of its
    /// cell, in the order of the cells that are tools.
    fn tool_lines(
        &self,
        font: &Font<'_>,
        style: &Style,
        laid: &Laid,
    ) -> Result<Vec<(u16, Vec<String>)>, Error> {
        let mut lines = Vec::new();
        for cell in &laid.cells {
            if cell.face.is_none() {
                continue;
            }
            let own = if laid.tool_labels {
                icons::name_lines(font, style, cell.rect, &cell.name)?
            } else {
                Vec::new()
            };
            push(&mut lines, (cell.rect.y, own))?;
        }
        Ok(lines)
    }

    /// What each target is and which has the focus, for whoever asks what
    /// shows (`canvas.describe`: automation, reading aloud, the tests), as
    /// interface 6 told its cells.
    fn describe_targets(
        &self,
        below: &mut Vec<Paint>,
        describing: bool,
        laid: &Laid,
    ) -> Result<(), Error> {
        for (at, target) in laid.targets.iter().enumerate() {
            let focused = self.focus.shown() && self.focus.at() == at;
            paint::describe(
                below,
                describing,
                PartRole::Button,
                target.rect,
                focused,
                false,
            )?;
        }
        Ok(())
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
        self.describe_targets(&mut below, paint::asking(frame), laid)?;
        let icon_font = paperpaw_ui_kit::icons::font(host)?;
        let circle = paperpaw_ui_kit::icons::glyph(host, "circle")?;
        let fonts = Fonts {
            ui: font,
            icons: icon_font.as_ref(),
            circle: circle.as_ref(),
        };
        let wrapped = self.tool_lines(font, &style, laid)?;
        let mut tops: Vec<(u16, usize)> = Vec::new();
        tops.try_reserve_exact(wrapped.len())
            .map_err(|_| Error::NoMemory)?;
        for (top, lines) in &wrapped {
            tops.push((*top, lines.len()));
        }
        let row_lines = icons::row_lines(&tops)?;
        let mut tool = 0;
        for cell in &laid.cells {
            if self.editing {
                // The cells show while editing.
                paintpush(
                    &mut below,
                    Paint::Focus(
                        cell.rect,
                        style.corner,
                        Some(u32::from(style.line).max(1)),
                        style.ink,
                    ),
                )?;
                self.corner_mark(&mut below, &style, cell.rect, laid.handle)?;
            }
            if let Some(face) = cell.face {
                let shown = Tool {
                    lines: wrapped
                        .get(tool)
                        .map_or(&[][..], |(_, lines)| lines.as_slice()),
                    face,
                    row_lines: row_lines.get(tool).copied().unwrap_or(0),
                };
                icons::draw_tool(&mut below, &fonts, &style, cell.rect, &shown)?;
                tool += 1;
            }
        }
        if let Some((rect, word)) = &laid.pencil {
            match (self.editing, fonts.icons, laid.pencil_icon.as_ref()) {
                (false, Some(_), Some(glyph)) => paperpaw_ui_kit::icons::draw_centred(
                    &mut below,
                    fonts.icons.unwrap_or(font),
                    glyph,
                    style.title,
                    *rect,
                    style.note,
                )?,
                _ => name(&mut below, font, &style, *rect, word)?,
            }
        }
        if let Some(icon_font) = fonts.icons {
            for (rect, glyph) in &laid.action_icons {
                if let Some(glyph) = glyph {
                    paperpaw_ui_kit::icons::draw_centred(
                        &mut below,
                        icon_font,
                        glyph,
                        style.title,
                        *rect,
                        style.note,
                    )?;
                }
            }
        }
        if let Some(rect) = laid.plus {
            name(&mut below, font, &style, rect, "+")?;
        }
        if let Some(nav) = self.nav {
            for dot in dash::dots(laid.footer_area(), style.gap, laid.pages, nav.page) {
                if laid.dots {
                    self::dot(&mut below, &dot, &style)?;
                }
            }
        }
        self.drag_frame(&mut above, &style, laid)?;
        if self.focus.shown()
            && let Some(target) = laid.targets.get(self.focus.at())
        {
            ring(&mut above, target.rect, &style)?;
        }
        paint::replay_icons(frame, font, fonts.icons, below.iter())?;
        compose_each(
            laid.cells
                .iter()
                .filter_map(|cell| Some((cell.widget.as_ref()?, cell.rect)))
                .chain(laid.status.as_ref().map(|(made, rect)| (made, *rect))),
            |made, rect| made.draw(host, ui, frame, rect),
        )?;
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
        self.ensure(host, ui)?;
        if let Some(handled) = self.arrange_input(host, ui, input)? {
            return Ok(handled);
        }
        match *input {
            Input::Action { action, .. } => self.action(host, ui, action),
            Input::Gesture { position, kind, .. } => match touch_of(kind) {
                Touch::Tap => self.tapped(host, ui, position, WidgetInputKind::Tap),
                // A long press starts editing, as on interface 6; while editing
                // it is taken and runs nothing (no tool, no footer action, no
                // widget).
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
        let left = self.focus.shown().then(|| self.focus.at());
        match self.focus.press(press, &laid.targets, laid.arrows_stay) {
            Step::Ignored => Ok(Handled::No),
            Step::Held => Ok(Handled::Yes),
            Step::Shown(at) | Step::Moved(at) => {
                self.redraw_focus(host, laid, [left, Some(at)])?;
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

    /// Only the targets the focus left and came to are drawn again, each with
    /// the room of its ring, not the page (as interface 6 drew only those
    /// cells): a step takes a small part of a frame on the X4. A host that
    /// refuses a part draws the page.
    fn redraw_focus(
        &self,
        host: &Host<'_>,
        laid: &Laid,
        ats: [Option<usize>; 2],
    ) -> Result<(), Error> {
        // Nothing is allocated here: the focus has moved already, and a step
        // that could not be drawn would leave it moved unseen.
        let room = laid.ring_room;
        let mut scratch = [0; 64];
        let mut sender = CommandSender::new(host, &mut scratch);
        for target in ats
            .into_iter()
            .flatten()
            .filter_map(|at| laid.targets.get(at))
        {
            let area = focus_area(target.rect, room, self.origin);
            if sender.queue_redraw_area(RedrawMode::Partial, area).is_err() {
                return redraw(host);
            }
        }
        Ok(())
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
        let Some(laid) = self.laid.as_ref() else {
            return Ok(Handled::No);
        };
        // The status line's items that do something, by a tap only (they are out
        // of the buttons' focus order, as on interface 6).
        if !self.editing
            && let Some((made, rect)) = laid
                .status
                .as_ref()
                .filter(|(_, rect)| inside(*rect, point))
        {
            // A refusal of the line's targets leaves the tap to the grid.
            match made.targets(host, ui, *rect) {
                Ok(found) => {
                    for (target, at) in found {
                        if inside(at, point) {
                            made.input(host, ui, target, point, kind)?;
                            return Ok(Handled::Yes);
                        }
                    }
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        // The nearest target the point is in: widgets' own first (they answer
        // at their places), then the tools and the buttons.
        let hit = laid
            .targets
            .iter()
            .copied()
            .find(|target| inside(target.rect, point));
        if let Some(target) = hit {
            let next = self.run(host, ui, Some(target), Some((point, kind)))?;
            self.next(host, ui, next)?;
            return Ok(Handled::Yes);
        }
        // A tap inside a widget that has no target is the widget's as a whole.
        let widget = laid
            .cells
            .iter()
            .enumerate()
            .find(|(_, cell)| cell.widget.is_some() && inside(cell.rect, point))
            .map(|(item, _)| item);
        if let Some(item) = widget {
            let target = Target {
                rect: laid.cells[item].rect,
                owner: Owner::Widget { item, target: 0 },
            };
            let next = self.run(host, ui, Some(target), Some((point, kind)))?;
            self.next(host, ui, next)?;
            return Ok(Handled::Yes);
        }
        Ok(Handled::No)
    }

    /// Does what a target is for: runs a tool or a footer action, tells a
    /// widget (a press of the buttons is a tap at the middle of its target).
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
            Owner::Button(Button::Action(index)) => {
                let Some(action) = laid.actions.get(index) else {
                    return Ok(Next::Nothing);
                };
                let mut scratch = buffer(SERVICE_BUFFER)?;
                CommandSender::new(host, &mut scratch).run_action(action)?;
            }
            Owner::Widget { item, target: id } => {
                let Some(made) = laid.cells.get(item).and_then(|cell| cell.widget.as_ref()) else {
                    return Ok(Next::Nothing);
                };
                let (point, kind) = touched.unwrap_or((middle(target.rect), WidgetInputKind::Tap));
                made.input(host, ui, id, point, kind)?;
            }
            Owner::Button(_) => {}
        }
        Ok(Next::Nothing)
    }

    /// Does what a press left to do: edit or stop editing (laid out anew, the
    /// focus where it was), or open the *Add* screen for the page that shows.
    fn next(&mut self, host: &Host<'_>, ui: &Ui, next: Next) -> Result<(), Error> {
        match next {
            Next::Nothing => Ok(()),
            Next::Edit(on) => {
                self.set_editing(on);
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

    fn set_editing(&mut self, on: bool) {
        self.editing = on;
        self.stale = true;
    }
}

impl Laid {
    fn footer_area(&self) -> Rect {
        self.frame.footer
    }
}

/// The footer's buttons as targets, in the order the focus reaches them: *Add*
/// (while editing), the actions (not while editing), the pencil.
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
    for (index, rect) in footer.actions.iter().enumerate() {
        push(
            targets,
            Target {
                rect: *rect,
                owner: Owner::Button(Button::Action(index)),
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

/// What a press on `owner` is without asking the host: the pencil starts or
/// ends editing, *Add* opens its screen, and while editing everything else
/// runs nothing (arranging is not ported, `PORTING-7.md`). `None`: it runs.
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
    /// Runs the tool, the footer action or the widget under it.
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

/// Composes each widget into its cell. One the host cannot compose (or that
/// fails to draw) is left out: the rest of the page still shows (as on
/// interface 6). Returns how many were composed. Memory is the exception: a
/// widget that cannot draw for want of it fails the draw, so that the screen is
/// not left with a blank cell nobody sees the reason for.
///
/// # Errors
/// [`Error::NoMemory`] from a widget's draw.
fn compose_each<T>(
    widgets: impl Iterator<Item = (T, Rect)>,
    mut compose: impl FnMut(T, Rect) -> Result<(), Error>,
) -> Result<usize, Error> {
    let mut composed = 0;
    for (widget, rect) in widgets {
        match compose(widget, rect) {
            Ok(()) => composed += 1,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
    }
    Ok(composed)
}

/// Lets the widgets of these cells go.
fn release_cells(host: &Host<'_>, ui: &Ui, cells: Vec<Cell>) {
    for cell in cells {
        if let Some(made) = cell.widget {
            made.release(host, ui);
        }
    }
}

/// A word (the pencil's, *Add*'s plus) wrapped and centred in its cell.
fn name(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    cell: Rect,
    name: &str,
) -> Result<(), Error> {
    let room = f32::from(cell.width.saturating_sub(style.gap)).max(1.0);
    let lines = text::wrap_with(
        |l| font.measure_text(style.small, l),
        name,
        room,
        NAME_LINES,
    )?;
    let line = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
    let total = u32::from(line) * lines.len() as u32;
    let mut y = u32::from(cell.y) + u32::from(cell.height).saturating_sub(total) / 2;
    for text in &lines {
        let wide = font.measure_text(style.small, text)?;
        let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
        let row = Rect::new(cell.x, clamp_u32(y), cell.width, line);
        text::label(paint, font, text, style.small, x, row, style.ink, None)?;
        y = y.saturating_add(u32::from(line));
    }
    Ok(())
}

/// A dot of the page indicator.
fn dot(paint: &mut Vec<Paint>, dot: &dash::Dot, style: &Style) -> Result<(), Error> {
    let size = clamp_u32(dot.size);
    let rect = Rect::new(clamp_i32(dot.x), clamp_i32(dot.y), size, size);
    let radius = dot.size / 2;
    let op = if dot.current {
        Paint::Focus(rect, radius, None, style.ink)
    } else {
        Paint::Focus(rect, radius, Some(u32::from(style.line).max(1)), style.ink)
    };
    paintpush(paint, op)
}

/// The focus ring round a target.
pub(crate) fn ring(paint: &mut Vec<Paint>, target: Rect, style: &Style) -> Result<(), Error> {
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
            style.ink,
        ),
    )
}

/// Whether the focus is drawn at a show: the reader chose to see it at once and
/// used buttons last (a finger, a pen or an input the host does not know of
/// get none).
fn focus_at_show(at_once: bool, last_input: Option<InputMode>) -> bool {
    at_once && last_input == Some(InputMode::Buttons)
}

fn paintpush(paint: &mut Vec<Paint>, op: Paint) -> Result<(), Error> {
    paint::push(paint, op)
}

/// Where target `rect` and its ring (`room` round it) are, from the top left
/// of the screen's region at `origin`.
/// How far [`ring`] reaches outside its target: the air around it and its line.
fn ring_room(style: &Style) -> i32 {
    ring_room_of(style.gap, style.line)
}

fn ring_room_of(gap: u16, line: u16) -> i32 {
    i32::from(gap / 2).saturating_add(i32::from(line.max(2)))
}

fn focus_area(rect: Rect, room: i32, origin: (i32, i32)) -> FrameRect {
    let grown = |size: u16| {
        u32::from(size).saturating_add(u32::try_from(room).unwrap_or(0).saturating_mul(2))
    };
    FrameRect::new(
        i32::from(rect.x)
            .saturating_sub(room)
            .saturating_sub(origin.0),
        i32::from(rect.y)
            .saturating_sub(room)
            .saturating_sub(origin.1),
        grown(rect.width),
        grown(rect.height),
    )
}

pub(crate) fn redraw(host: &Host<'_>) -> Result<(), Error> {
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
    fn a_focus_step_draws_again_all_the_ring_covers() {
        // Air 12 / 2 = 6 around the target, then a line of at least 2.
        assert_eq!(ring_room_of(12, 1), 8);
        assert_eq!(ring_room_of(12, 3), 9);
        let target = Rect::new(100, 200, 50, 60);
        let area = focus_area(target, ring_room_of(12, 1), (0, 40));
        // The area reaches past the air and the line, measured from the region.
        let (x, y) = (100 - 6 - 2, 200 - 6 - 2 - 40);
        assert_eq!((area.x, area.y), (x, y));
        assert_eq!((area.width, area.height), (50 + 16, 60 + 16));
    }

    #[test]
    fn a_point_is_inside_a_rectangle_by_its_left_and_top_edges() {
        let r = Rect::new(10, 20, 30, 40);
        assert!(inside(r, Point::new(10, 20)));
        assert!(inside(r, Point::new(39, 59)));
        assert!(!inside(r, Point::new(40, 59)));
        assert!(!inside(r, Point::new(39, 60)));
        assert!(!inside(r, Point::new(9, 30)));
    }

    fn footer() -> Footer {
        Footer {
            pencil: Rect::new(300, 400, 60, 60),
            plus: Rect::new(10, 400, 120, 60),
            actions: [Rect::new(200, 400, 60, 60), Rect::new(240, 400, 60, 60)].into(),
        }
    }

    fn owners(footer: &Footer, buttons: Buttons) -> Vec<Owner> {
        let mut targets = Vec::new();
        footer_targets(&mut targets, footer, buttons).unwrap();
        targets.iter().map(|t| t.owner).collect()
    }

    #[test]
    fn the_footer_buttons_are_reached_add_first_and_the_pencil_last() {
        let f = footer();
        let both = Buttons {
            editing: true,
            plus: true,
            pencil: true,
        };
        assert_eq!(
            owners(&f, both),
            [
                Owner::Button(Button::Plus),
                Owner::Button(Button::Action(0)),
                Owner::Button(Button::Action(1)),
                Owner::Button(Button::Pencil)
            ]
        );
        let none = Buttons {
            editing: false,
            plus: false,
            pencil: false,
        };
        assert_eq!(owners(&f, none).len(), 2);
        let pencil = Buttons {
            editing: false,
            plus: false,
            pencil: true,
        };
        assert_eq!(
            owners(&f, pencil).last(),
            Some(&Owner::Button(Button::Pencil))
        );
    }

    #[test]
    fn the_pencil_toggles_editing_and_add_opens_its_screen() {
        let pencil = Owner::Button(Button::Pencil);
        assert_eq!(decide(pencil, false), Some(Next::Edit(true)));
        assert_eq!(decide(pencil, true), Some(Next::Edit(false)));
        assert_eq!(decide(Owner::Button(Button::Plus), true), Some(Next::Add));
    }

    #[test]
    fn while_editing_an_item_offers_its_options_and_the_rest_runs_nothing() {
        assert_eq!(decide(Owner::Tool(2), true), Some(Next::Options(2)));
        assert_eq!(
            decide(Owner::Widget { item: 1, target: 3 }, true),
            Some(Next::Options(1))
        );
        assert_eq!(
            decide(Owner::Button(Button::Action(0)), true),
            Some(Next::Nothing)
        );
        // Not editing: it runs (the host decides what).
        for owner in [
            Owner::Tool(0),
            Owner::Widget { item: 0, target: 1 },
            Owner::Button(Button::Action(0)),
        ] {
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
        dashboard.reopened(false);
        assert_eq!(dashboard.nav.map(|n| n.page), Some(2));
        assert!(dashboard.focus.shown());
        assert!(!dashboard.editing());
        // Not laid out again for nothing.
        assert!(!dashboard.stale);
        // Asked to edit: editing starts, the page stays.
        dashboard.reopened(true);
        assert!(dashboard.editing());
        assert!(dashboard.stale);
        assert_eq!(dashboard.nav.map(|n| n.page), Some(2));
    }

    #[test]
    fn the_focus_shows_at_once_only_after_buttons() {
        for (last, shown) in [
            (Some(InputMode::Buttons), true),
            (Some(InputMode::Touch), false),
            (Some(InputMode::Pen), false),
            (None, false),
        ] {
            assert_eq!(focus_at_show(true, last), shown, "{last:?}");
            // The setting off: never at once.
            assert!(!focus_at_show(false, last), "{last:?}");
        }
        let mut dashboard = Dashboard::new();
        dashboard.shown(Some(InputMode::Touch));
        assert!(dashboard.reshow && dashboard.stale);
        assert_eq!(dashboard.last_input, Some(InputMode::Touch));
    }

    #[test]
    fn a_long_press_is_taken_and_runs_nothing() {
        assert_eq!(touch_of(InputGestureKind::LongPress), Touch::Held);
        assert_eq!(touch_of(InputGestureKind::Tap), Touch::Tap);
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
        )
        .unwrap();
        // All three were tried, in order; the failing one alone is missing.
        assert_eq!(seen, [(1, 0), (2, 10), (3, 20)]);
        assert_eq!(composed, 2);
        // A host that refuses every one: none composed, no error.
        assert_eq!(
            compose_each([(1_u32, cell(0)), (2, cell(1))].into_iter(), |_, _| Err(
                Error::Denied
            )),
            Ok(0)
        );
        // Memory is not a refusal to leave out: the draw fails.
        assert_eq!(
            compose_each([(1_u32, cell(0)), (2, cell(1))].into_iter(), |_, _| Err(
                Error::NoMemory
            )),
            Err(Error::NoMemory)
        );
    }

    #[test]
    fn the_middle_of_a_target_is_where_a_button_press_taps() {
        assert_eq!(middle(Rect::new(10, 20, 30, 40)), Point::new(25, 40));
    }
}
