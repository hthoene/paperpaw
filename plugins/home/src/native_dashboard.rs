//! The dashboard screen, viewing: the page of tools and widgets in its grid,
//! the footer's actions and dots, page turning by the page buttons, the focus
//! with the buttons, a tap that runs a tool or reaches a widget. What it is
//! made of without the host is `native_dash` (the parts' places), `native_focus`
//! (the buttons' focus), `native_pages` and `native_grid` (the pages) and
//! `native_source` (what is read of the device); this file reads, draws and
//! hands inputs on through the host.
//!
//! Not yet (ignored acceptance cases, `PORTING-7.md`): editing, the status line
//! above the grid, the icons of tools and of the footer, the widgets of the
//! home screen's own, and the widgets' updates.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot::{self, Snapshot},
        ui::{Compose, Ui, WidgetHandle, WidgetInputKind},
    },
    widgets::grid::Geometry,
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
use crate::native_pages;
use crate::native_shown::Shown;
use crate::native_source::HostSource;

/// Where the widgets are made (the place the plugin declares).
const PLACE: &str = "home.pages";
/// Lines a tool's name takes at most.
const NAME_LINES: usize = 3;

/// One item of the page as it shows.
#[derive(Debug)]
struct Cell {
    what: What,
    rect: Rect,
    /// A tool's name.
    name: String,
    /// A widget's instance, where the host made one.
    widget: Option<WidgetHandle>,
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
    /// Where *Add* is, while editing.
    plus: Option<Rect>,
}

/// Which of the footer's buttons are there.
#[derive(Debug, Clone, Copy)]
struct Buttons {
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
}

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
    /// Editing the home screen: the pencil is *Done*, *Add* shows; what is
    /// tapped runs nothing.
    editing: bool,
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

    /// Starts editing (opened to edit with `mode` = `edit`).
    pub fn start_editing(&mut self) {
        self.editing = true;
    }

    /// What shows is read and laid out again before the next draw or input.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Shown again (above another screen closed, a book left).
    pub fn shown(&mut self) {
        self.stale = true;
        self.reshow = true;
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
            src,
            metrics,
            frame,
            pages,
            ..
        } = Shown::read(host, display, font)?;
        let nav = self.nav.get_or_insert(Nav::new(pages.len()));
        nav.resize(pages.len());
        let (page, count) = (nav.page, nav.pages);
        let items = match pages.get(page) {
            Some(page) => native_pages::expand(&src, &page.items)?,
            None => Vec::new(),
        };
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
            plus: editing.then_some(footer.plus),
        })
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
        for item in shown {
            if let Some(cell) = Self::cell(host, ui, src, &frame.geometry, item)? {
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
        item: &grid::Item,
    ) -> Result<Option<Cell>, Error> {
        let Some(at) = item.at else {
            return Ok(None);
        };
        let rect = geometry.rect(at);
        let (name, widget) = match &item.what {
            What::Tool(action) => (text::copy(src.tool_name(action))?, None),
            What::Widget(id) => {
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                match ui.widget_make(host, id, PLACE, &mut input, &mut out) {
                    Ok(handle) => {
                        let placed = ui.widget_place(
                            host,
                            handle,
                            paint::frame_rect(rect),
                            &mut input,
                            &mut out,
                        );
                        if let Err(error) = placed {
                            drop_widget(host, ui, handle);
                            return Err(error);
                        }
                        (String::new(), Some(handle))
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
                if let Some(widget) = widget {
                    drop_widget(host, ui, widget);
                }
                return Err(error);
            }
        };
        Ok(Some(Cell {
            what,
            rect,
            name,
            widget,
        }))
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
            match cell.widget {
                None => push(
                    &mut targets,
                    Target {
                        rect: cell.rect,
                        owner: Owner::Tool(index),
                    },
                )?,
                Some(widget) if !widget.is_own() => {
                    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                    let found = ui.widget_targets(
                        host,
                        widget,
                        paint::frame_rect(cell.rect),
                        &mut input,
                        &mut out,
                    )?;
                    for target in found {
                        let target = target?;
                        let at = target.rect();
                        push(
                            &mut targets,
                            Target {
                                rect: Rect::new(
                                    clamp(at.x),
                                    clamp(at.y),
                                    clamp(at.width),
                                    clamp(at.height),
                                ),
                                owner: Owner::Widget {
                                    item: index,
                                    target: target.target(),
                                },
                            },
                        )?;
                    }
                }
                // Drawn by this plugin itself: the widgets of the home screen
                // are not ported yet.
                Some(_) => {}
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

    /// The widgets of the page before are let go; a refusal is of no matter
    /// (the host drops them when the screen closes).
    fn drop_widgets(&mut self, host: &Host<'_>, ui: &Ui) {
        if let Some(laid) = self.laid.take() {
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
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = self.paint(host, frame, &display, &font, laid);
        let released = font.release();
        drawn?;
        released
    }

    fn paint(
        &self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        display: &Snapshot,
        font: &Font<'_>,
        laid: &Laid,
    ) -> Result<(), Error> {
        let style = Style::read(host, display, font)?;
        let mut below = Vec::new();
        let mut above = Vec::new();
        for cell in &laid.cells {
            if matches!(cell.what, What::Tool(_)) && laid.tool_labels {
                name(&mut below, font, &style, cell.rect, &cell.name)?;
            }
        }
        if let Some((rect, word)) = &laid.pencil {
            name(&mut below, font, &style, *rect, word)?;
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
        if self.focus.shown()
            && let Some(target) = laid.targets.get(self.focus.at())
        {
            ring(&mut above, target.rect, &style)?;
        }
        paint::replay(frame, font, below.iter())?;
        compose_each(
            laid.cells
                .iter()
                .filter_map(|cell| Some((cell.widget.filter(|w| !w.is_own())?, cell.rect))),
            |widget, rect| Ui::compose(host, Compose::Widget(widget), paint::frame_rect(rect)),
        );
        paint::replay(frame, font, above.iter())
    }

    /// Takes an input: the page buttons turn the page, the others move the
    /// focus or run what it stands on, a tap runs a tool or reaches a widget.
    ///
    /// # Errors
    /// Allocation and host errors.
    pub fn input(&mut self, host: &Host<'_>, ui: &Ui, input: &Input<'_>) -> Result<Handled, Error> {
        self.ensure(host, ui)?;
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
                let Some(widget) = laid.cells.get(item).and_then(|cell| cell.widget) else {
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
    /// focus where it was), or open the *Add* screen for the page that shows.
    fn next(&mut self, host: &Host<'_>, ui: &Ui, next: Next) -> Result<(), Error> {
        match next {
            Next::Nothing => Ok(()),
            Next::Edit(on) => {
                self.set_editing(on);
                self.ensure(host, ui)?;
                redraw(host)
            }
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
        if let Some(widget) = cell.widget {
            drop_widget(host, ui, widget);
        }
    }
}

/// A tool's name under where its icon goes (the icons wait for the host's
/// admission, `PORTING-7.md`): wrapped and centred in its cell.
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
            plus: false,
            pencil: false,
        };
        assert_eq!(owners(&f, none).len(), 2);
        let pencil = Buttons {
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
    fn while_editing_what_is_tapped_runs_nothing() {
        for owner in [
            Owner::Tool(0),
            Owner::Widget { item: 0, target: 1 },
            Owner::Button(Button::Action(0)),
        ] {
            assert_eq!(decide(owner, true), Some(Next::Nothing));
            // Not editing: it runs (the host decides what).
            assert_eq!(decide(owner, false), None);
        }
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
    fn the_middle_of_a_target_is_where_a_button_press_taps() {
        assert_eq!(middle(Rect::new(10, 20, 30, 40)), Point::new(25, 40));
    }
}
