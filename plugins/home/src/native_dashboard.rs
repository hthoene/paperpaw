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
    Paint, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint, text,
};

use crate::native_dash::{self as dash, Footer, Frame, Metrics, Nav, Wanted};
use crate::native_focus::{Button, Focus, Owner, Press, Step, Target};
use crate::native_grid::{self as grid, What};
use crate::native_pages::{self, Source};
use crate::native_source::HostSource;

/// Where the widgets are made (the place the plugin declares).
const PLACE: &str = "home.pages";
/// Lines a tool's name takes at most.
const NAME_LINES: usize = 3;
/// The setting of the gap between widgets.
const GAP: &str = "core.widgets.gap";

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
}

/// The dashboard.
#[derive(Debug, Default)]
pub struct Dashboard {
    nav: Option<Nav>,
    focus: Focus,
    laid: Option<Laid>,
    /// What shows changed (a setting, the screen turned): laid out anew first.
    stale: bool,
}

impl Dashboard {
    /// A dashboard to be laid out when it first draws.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stale: true,
            ..Self::default()
        }
    }

    /// What shows is read and laid out again before the next draw or input.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Shown again (above another screen closed, a book left).
    pub fn shown(&mut self) {
        self.stale = true;
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
        released?;
        self.drop_widgets(host, ui);
        self.focus.clamp(laid.targets.len());
        if laid.focus_at_once && self.laid.is_none() {
            self.focus.show(true);
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
        let style = Style::read(host, display, font)?;
        let src = HostSource::read(host, display.display_dpi())?;
        let metrics = metrics(host, display, &style, font)?;
        let wide = dash::landscape(&metrics);
        let columns = native_pages::columns_upright(&src);
        let frame = dash::frame(&metrics, wide, src.status_bar(), columns, src.rows);
        let pages = if wide {
            native_pages::pages_wide(&src, &frame.geometry)?
        } else {
            native_pages::pages(&src)?
        };
        let nav = self.nav.get_or_insert(Nav::new(pages.len()));
        nav.resize(pages.len());
        let (page, count) = (nav.page, nav.pages);
        let items = match pages.get(page) {
            Some(page) => native_pages::expand(&src, &page.items)?,
            None => Vec::new(),
        };
        let (shown, _) = grid::fit(&src, &frame.geometry, &items)?;
        let footer = dash::footer(
            &metrics,
            frame.footer,
            &Wanted {
                editing: false,
                // No pencil until editing is ported.
                pencil: false,
                actions: src.footer.len(),
                done_word: 0,
            },
        )?;
        let mut cells = Vec::new();
        for item in &shown {
            if let Some(cell) = Self::cell(host, ui, &src, &frame.geometry, item)? {
                push(&mut cells, cell)?;
            }
        }
        let targets = Self::targets(host, ui, &cells, &footer)?;
        let mut actions = Vec::new();
        for (action, _, _) in src.footer.iter().take(footer.actions.len()) {
            push(&mut actions, text::copy(action)?)?;
        }
        Ok(Laid {
            frame,
            cells,
            targets,
            actions,
            pages: count,
            dots: src.dots,
            tool_labels: src.tool_labels,
            focus_at_once: src.focus_at_once,
        })
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
                        ui.widget_place(
                            host,
                            handle,
                            paint::frame_rect(rect),
                            &mut input,
                            &mut out,
                        )?;
                        (String::new(), Some(handle))
                    }
                    // A widget that is gone, or not for this screen: no cell.
                    Err(Error::NotFound | Error::Denied) => return Ok(None),
                    Err(error) => return Err(error),
                }
            }
        };
        Ok(Some(Cell {
            what: item.what.try_clone()?,
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
        for (index, rect) in footer.actions.iter().enumerate() {
            push(
                &mut targets,
                Target {
                    rect: *rect,
                    owner: Owner::Button(Button::Action(index)),
                },
            )?;
        }
        Ok(targets)
    }

    /// The widgets of the page before are let go; a refusal is of no matter
    /// (the host drops them when the screen closes).
    fn drop_widgets(&mut self, host: &Host<'_>, ui: &Ui) {
        let Some(laid) = self.laid.take() else {
            return;
        };
        for cell in laid.cells {
            if let Some(widget) = cell.widget
                && let (Ok(mut input), Ok(mut out)) = (buffer(SERVICE_BUFFER), buffer(64))
            {
                let _ = ui.widget_drop(host, widget, &mut input, &mut out);
            }
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
        for cell in &laid.cells {
            if let Some(widget) = cell.widget.filter(|w| !w.is_own()) {
                Ui::compose(host, Compose::Widget(widget), paint::frame_rect(cell.rect))?;
            }
        }
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
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tapped(host, ui, position, WidgetInputKind::Tap),
            Input::Gesture {
                position,
                kind: InputGestureKind::LongPress,
                ..
            } => self.tapped(host, ui, position, WidgetInputKind::LongPress),
            _ => Ok(Handled::No),
        }
    }

    fn action(&mut self, host: &Host<'_>, ui: &Ui, action: &str) -> Result<Handled, Error> {
        if action == "core.next-page" || action == "core.prev-page" {
            return self.turn(host, ui, action == "core.next-page");
        }
        let Some(press) = Press::of(action) else {
            return Ok(Handled::No);
        };
        let Some(laid) = self.laid.as_ref() else {
            return Ok(Handled::No);
        };
        match self.focus.press(press, &laid.targets, false) {
            Step::Ignored => Ok(Handled::No),
            Step::Held => Ok(Handled::Yes),
            Step::Shown(_) | Step::Moved(_) => {
                redraw(host)?;
                Ok(Handled::Yes)
            }
            Step::Run(at) => {
                let target = laid.targets.get(at).copied();
                self.run(host, ui, target, None)?;
                Ok(Handled::Yes)
            }
        }
    }

    fn turn(&mut self, host: &Host<'_>, ui: &Ui, forward: bool) -> Result<Handled, Error> {
        let Some(nav) = self.nav.as_mut() else {
            return Ok(Handled::No);
        };
        if !nav.turn(forward) {
            return Ok(Handled::No);
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
            self.run(host, ui, Some(target), Some((point, kind)))?;
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
            self.run(host, ui, Some(target), Some((point, kind)))?;
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
    ) -> Result<(), Error> {
        let (Some(target), Some(laid)) = (target, self.laid.as_ref()) else {
            return Ok(());
        };
        match target.owner {
            Owner::Tool(index) => {
                let Some(Cell {
                    what: What::Tool(action),
                    ..
                }) = laid.cells.get(index)
                else {
                    return Ok(());
                };
                let mut scratch = buffer(SERVICE_BUFFER)?;
                CommandSender::new(host, &mut scratch).run_action(action)
            }
            Owner::Button(Button::Action(index)) => {
                let Some(action) = laid.actions.get(index) else {
                    return Ok(());
                };
                let mut scratch = buffer(SERVICE_BUFFER)?;
                CommandSender::new(host, &mut scratch).run_action(action)
            }
            Owner::Button(_) => Ok(()),
            Owner::Widget { item, target: id } => {
                let Some(widget) = laid.cells.get(item).and_then(|cell| cell.widget) else {
                    return Ok(());
                };
                let (point, kind) = touched.unwrap_or((middle(target.rect), WidgetInputKind::Tap));
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                ui.widget_input(host, widget, id, point, kind, &mut input, &mut out)
            }
        }
    }
}

impl Laid {
    fn footer_area(&self) -> Rect {
        self.frame.footer
    }
}

/// The metrics of the style on this display.
fn metrics(
    host: &Host<'_>,
    display: &Snapshot,
    style: &Style,
    font: &Font<'_>,
) -> Result<Metrics, Error> {
    let gap = match paperpaw_ui_kit::style::setting_text(host, GAP)?.as_str() {
        "core.gap.small" => style.gap / 2,
        "core.gap.large" => style.gap.saturating_mul(2),
        _ => style.gap,
    };
    let line = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
    Ok(Metrics {
        width: display.display_width(),
        height: display.display_height(),
        margin: style.margin,
        row: style.row,
        gap,
        style_gap: style.gap,
        status: line.saturating_add(style.gap / 2),
    })
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
    fn the_middle_of_a_target_is_where_a_button_press_taps() {
        assert_eq!(middle(Rect::new(10, 20, 30, 40)), Point::new(25, 40));
    }
}
