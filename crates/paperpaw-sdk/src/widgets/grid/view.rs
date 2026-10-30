//! [`GridView`]: a grid of widgets and tools on a screen, edited in place.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use super::{Cells, Geometry, Item, What, dragged, draw_cell_focus, draw_tool_lines};
use crate::builtin::{actions, params, slots};
use crate::event::Event;
use crate::ui::choice::Picked;
use crate::ui::{self, UiStyle};
use crate::widgets::{SizeClass, Widget, WidgetContext, WidgetInput};
use crate::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Params, Point,
    PointerEvent, PointerPhase, Rect, RefreshMode, semantics::Role,
};

/// Translation keys of an item's options: this, then the option.
const OPTIONS: &str = "core.grid.";

/// What a [`GridView`] did with an input, for its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridResponse {
    /// Not the grid's: the screen may use it.
    Ignored,
    /// Done; nothing for the screen to do.
    Handled,
    /// The items were moved, sized or removed: keep them (every item
    /// with its place, those without room as they were).
    Changed(Vec<Item>),
    /// Editing began (`true`) or ended: lay the screen out again.
    Editing(bool),
    /// One of the screen's own buttons was chosen (see
    /// [`GridView::layout`]).
    Button(u16),
    /// One of the screen's own options for an item was chosen (see
    /// [`GridView::set_options`]): the item, and the option's translation
    /// key.
    Option { item: Item, option: String },
}

/// Whether an item may go to the cells it is dropped on, among the items
/// as they are (see [`GridView::set_drop_check`]); `false` keeps it where
/// it was.
pub type DropCheck = fn(&Env<'_>, &[Item], &Item, Cells) -> bool;

impl GridResponse {
    /// Whether the input was used.
    pub fn handled(&self) -> Handled {
        if *self == GridResponse::Ignored {
            Handled::No
        } else {
            Handled::Yes
        }
    }
}

/// An item on the grid: where it is, and its widget, which keeps what it
/// shows.
struct Shown {
    item: Item,
    cells: Cells,
    area: Rect,
    widget: Option<Box<dyn Widget>>,
}

impl fmt::Debug for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shown")
            .field("item", &self.item)
            .field("cells", &self.cells)
            .field("area", &self.area)
            .finish_non_exhaustive()
    }
}

/// What editing is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Choosing an item (tap, or the buttons' focus).
    Choose,
    /// The buttons move the item with the focus a cell at a time.
    Move,
    /// The buttons make it wider, narrower, taller or lower.
    Resize,
}

/// An item dragged with a finger: from where, and where it would go.
#[derive(Debug, Clone, Copy)]
struct Drag {
    index: usize,
    /// Its size is changed (dragged by the corner), not its place.
    resize: bool,
    start: Point,
    to: Cells,
    moved: bool,
}

/// Editing in place: the cells show, items are moved and sized by
/// dragging, or with the buttons through their options.
#[derive(Debug)]
struct Edit {
    mode: Mode,
    drag: Option<Drag>,
}

/// What a target does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// What a tap runs, and a long press where it says (boxed: rare).
    Run(Command, Option<Box<Command>>),
    /// An item, when editing: its index.
    Item(usize),
    /// One of the screen's own buttons.
    Button(u16),
}

/// A grid of widgets and tools on a screen of a plugin's own, edited in
/// place; see the [module documentation](super).
///
/// Its targets are the items' (the places in widgets that do something,
/// the tools; while editing, the items themselves), then the screen's
/// own buttons: taps and the buttons' focus reach them in that order.
/// An item's options are asked with [`ask::choice`](crate::ask::choice);
/// they come back as the plugin's event, which the screen hands to
/// [`on_event`](Self::on_event).
#[derive(Debug)]
pub struct GridView {
    style: UiStyle,
    geometry: Geometry,
    items: Vec<Shown>,
    /// Items without room, kept as they are.
    unplaced: Vec<Item>,
    buttons: Vec<(Rect, u16)>,
    /// Where taps and the buttons' focus go, in order.
    targets: Vec<(Rect, Target)>,
    /// Where taps alone go
    /// ([`WidgetTarget::touch`](crate::widgets::WidgetTarget::touch)), and
    /// what a long press there runs, where it says.
    touch: Vec<(Rect, Command, Option<Box<Command>>)>,
    /// The item each target is of (none for the screen's own buttons).
    owners: Vec<Option<usize>>,
    /// The screen showing it, told to its widgets
    /// ([`WidgetContext::host`](crate::widgets::WidgetContext::host)).
    host: Option<Id>,
    /// The screen's own options for items, as translation keys.
    extra: Vec<alloc::borrow::Cow<'static, str>>,
    /// Whether an item may go where it is dropped.
    drop_check: Option<DropCheck>,
    /// What each target is called, as told to screen readers while it
    /// has the focus: its item's name; empty for the screen's own
    /// buttons, which tell theirs.
    names: Vec<String>,
    /// The target with the focus, shown once buttons are used.
    focus: usize,
    focus_shown: bool,
    edit: Option<Edit>,
    labels: bool,
    chosen: Event<Picked>,
}

impl GridView {
    /// An empty grid, not edited; `chosen` is the event the options of an
    /// item come back with (asked with [`ask::choice`](crate::ask::choice)).
    /// Lay it out with [`layout`](Self::layout).
    pub fn asking(env: &Env<'_>, chosen: Event<Picked>) -> Self {
        Self {
            style: UiStyle::new(env),
            geometry: Geometry::default(),
            items: Vec::new(),
            unplaced: Vec::new(),
            buttons: Vec::new(),
            targets: Vec::new(),
            touch: Vec::new(),
            owners: Vec::new(),
            host: None,
            extra: Vec::new(),
            drop_check: None,
            names: Vec::new(),
            focus: 0,
            focus_shown: crate::ui::focus_shown(env),
            edit: None,
            labels: true,
            chosen,
        }
    }

    /// Whether tools show their names below their icons (they do at first).
    pub fn set_tool_labels(&mut self, on: bool) {
        self.labels = on;
    }

    /// Places `items` in `geometry` (those without room are kept, not
    /// shown), with the screen's own `buttons` after them (their area and
    /// a number the screen knows them by, see [`GridResponse::Button`]).
    /// The widgets are made anew, what they show read.
    pub fn layout(
        &mut self,
        env: &Env<'_>,
        geometry: Geometry,
        items: &[Item],
        buttons: &[(Rect, u16)],
    ) {
        self.style = UiStyle::new(env);
        let placed = super::places(env, &geometry, items);
        // By index: of equal items, each without room is kept.
        self.unplaced = items
            .iter()
            .enumerate()
            .filter(|(index, _)| !placed.iter().any(|(p, _)| p == index))
            .map(|(_, item)| item.clone())
            .collect();
        self.items = placed
            .into_iter()
            .map(|(index, cells)| {
                let item = items[index].clone();
                let area = geometry.rect(cells);
                let host = self.host.as_ref().map(Id::as_str);
                let widget = match &item.what {
                    What::Widget(id) => env.widgets().make(id, env).map(|mut widget| {
                        widget.place(env, area);
                        let context = WidgetContext::new(SizeClass::of(env, area)).host(host);
                        widget.context(env, &context);
                        widget
                    }),
                    What::Tool(_) => None,
                };
                Shown {
                    widget,
                    area,
                    item,
                    cells,
                }
            })
            .collect();
        self.geometry = geometry;
        self.buttons = buttons.to_vec();
        self.target(env);
    }

    /// Lets go of the widgets and what they keep (covers made small, …)
    /// while a screen above needs the memory
    /// ([`Screen::on_squeeze`](crate::Screen::on_squeeze)): the screen
    /// lays the grid out again before it draws it next.
    pub fn let_go(&mut self) {
        for shown in &mut self.items {
            shown.widget = None;
        }
    }

    /// The targets of the items and the buttons as they are now.
    fn target(&mut self, env: &Env<'_>) {
        self.targets.clear();
        self.touch.clear();
        self.owners.clear();
        self.names.clear();
        let editing = self.edit.is_some();
        for (index, shown) in self.items.iter().enumerate() {
            let before = self.targets.len();
            if editing {
                self.targets.push((shown.area, Target::Item(index)));
            } else {
                match (&shown.item.what, &shown.widget) {
                    (What::Widget(_), Some(widget)) => {
                        for target in widget.targets(env, shown.area) {
                            if target.touch {
                                let hold = target.hold.map(Box::new);
                                self.touch.push((target.area, target.command, hold));
                            } else {
                                self.targets.push((
                                    target.area,
                                    Target::Run(target.command, target.hold.map(Box::new)),
                                ));
                            }
                        }
                    }
                    (What::Widget(_), None) => {}
                    (What::Tool(action), _) => self
                        .targets
                        .push((shown.area, Target::Run(Command::run(action.clone()), None))),
                }
            }
            let name = shown.item.what.title(env);
            let added = self.targets.len() - before;
            self.names.extend(core::iter::repeat_n(name, added));
            self.owners.extend(core::iter::repeat_n(Some(index), added));
        }
        for (area, button) in &self.buttons {
            self.targets.push((*area, Target::Button(*button)));
            self.names.push(String::new());
            self.owners.push(None);
        }
        self.focus = self.focus.min(self.targets.len().saturating_sub(1));
    }

    /// The grid's cells and where they are.
    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// Lets go of the widgets and what they keep (e.g. covers made
    /// small), while the screen is hidden
    /// ([`Screen::on_hide`](crate::Screen::on_hide)): nothing of them is
    /// drawn until [`layout`](Self::layout) makes them anew.
    pub fn release(&mut self) {
        for shown in &mut self.items {
            shown.widget = None;
        }
    }

    /// Whether the grid is edited.
    pub fn editing(&self) -> bool {
        self.edit.is_some()
    }

    /// Begins or ends editing (the focus to the first target); lay the
    /// screen out again after, its buttons may differ.
    pub fn set_editing(&mut self, on: bool) {
        self.edit = on.then_some(Edit {
            mode: Mode::Choose,
            drag: None,
        });
        self.focus = 0;
    }

    /// Whether the focus shows before the buttons are used (at first only
    /// on devices without touch); the first press shows it otherwise.
    pub fn set_focus_shown(&mut self, shown: bool) {
        self.focus_shown = shown;
    }

    /// Puts the focus on the first target (e.g. when another page shows).
    pub fn reset_focus(&mut self) {
        self.focus = 0;
    }

    /// The items with their places as they are now, those without room
    /// after them.
    pub fn items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self
            .items
            .iter()
            .map(|s| Item::placed(s.item.what.clone(), s.cells))
            .collect();
        items.extend(self.unplaced.iter().cloned());
        items
    }

    /// Has the widgets read what they show anew (see
    /// [`Widget::update`]; call it about each minute and after events
    /// that may change them) and draws those whose content changed again;
    /// `true` if any did.
    pub fn update_widgets(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) -> bool {
        self.update(env, cx, |_| true)
    }

    /// The same for the widgets that change by themselves alone
    /// ([`Widget::live`]): each minute, or when the device's status
    /// changed.
    pub fn update_live(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) -> bool {
        let mut changed = false;
        for shown in &mut self.items {
            let Some(widget) = &mut shown.widget else {
                continue;
            };
            if widget.live() && widget.update(env) {
                cx.request_redraw_area(shown.area);
                changed = true;
            }
        }
        if changed && self.edit.is_none() {
            self.target(env);
        }
        changed
    }

    /// The same for the items showing widget `id` alone, e.g. once
    /// something they show was looked up.
    pub fn update_widget(&mut self, id: &str, env: &Env<'_>, cx: &mut Cx<'_, '_>) -> bool {
        self.update(env, cx, |shown| {
            crate::widgets::base(shown) == id || shown == id
        })
    }

    fn update(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>, which: impl Fn(&str) -> bool) -> bool {
        let mut changed = false;
        for shown in &mut self.items {
            let (What::Widget(id), Some(widget)) = (&shown.item.what, &mut shown.widget) else {
                continue;
            };
            if which(id) && widget.update(env) {
                cx.request_redraw_area(shown.area);
                changed = true;
            }
        }
        // Their targets may have changed with them.
        if changed && self.edit.is_none() {
            self.target(env);
        }
        changed
    }

    /// Draws the items (while editing: the cells, the items' frames and
    /// the corners that size them, where a dragged item would go) and the
    /// focus once buttons are used. The screen draws its buttons itself.
    pub fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let editing = self.edit.is_some();
        if editing {
            self.draw_cells(canvas);
        }
        let line = self.style.line.max(1);
        let clip = canvas.clip();
        // The target with the focus where it is a tool's (told with the
        // tool below, as every tool is).
        let focused_tool = self
            .targets
            .get(self.focus)
            .filter(|(_, target)| self.focus_shown && matches!(target, Target::Run(..)))
            .map(|(area, _)| *area)
            .filter(|area| {
                self.items
                    .iter()
                    .any(|s| s.area == *area && matches!(s.item.what, What::Tool(_)))
            });
        let mut tool_ink = None; // The focused tool's icon and name.
        for shown in &self.items {
            // Those outside the part drawn are left (e.g. under a menu).
            if shown.area.intersection(&clip).is_none() {
                continue;
            }
            canvas.fill(shown.area, Color::WHITE);
            match (&shown.item.what, &shown.widget) {
                (What::Widget(_), Some(widget)) => widget.draw(canvas, env, shown.area),
                (What::Widget(_), None) => {}
                (What::Tool(action), _) => {
                    let ink = draw_tool_lines(canvas, env, shown.area, action, self.labels, 0);
                    tool_ink = (focused_tool == Some(shown.area))
                        .then_some(ink)
                        .or(tool_ink);
                    // A button named after its action, also where its
                    // name is not shown (told after its name, which stays
                    // a text of its own).
                    let name = shown.item.what.title(env);
                    let focused = !editing && focused_tool == Some(shown.area);
                    let told = canvas.describe(Role::Button, &name, shown.area);
                    told.focused(focused);
                }
            }
            if editing {
                // In the gap round it, not over what it shows (and apart
                // from its neighbour's).
                let pad = (self.style.gap / 2).saturating_sub(line) as u16;
                let around = Rect::new(
                    shown.area.x.saturating_sub(pad),
                    shown.area.y.saturating_sub(pad),
                    shown.area.width + 2 * pad,
                    shown.area.height + 2 * pad,
                );
                let corner = self.style.corner;
                canvas.stroke_round(around, line, corner + u32::from(pad), Color::BLACK);
                canvas.fill_round(handle(shown.area, &self.style), corner, Color::BLACK);
            }
        }
        if let Some(Edit {
            drag: Some(drag), ..
        }) = &self.edit
            && drag.moved
        {
            let width = if self.free(drag.index, drag.to) && self.allowed(env, drag.index, drag.to)
            {
                self.style.line * 4
            } else {
                self.style.line
            };
            let to = self.geometry.rect(drag.to);
            canvas.stroke_round(to, width, self.style.corner, Color::BLACK);
        }
        // The place with the focus told by its item's name, as a
        // button: what a screen reader says, and tools follow, as the
        // buttons move the focus.
        if self.focus_shown
            && focused_tool.is_none()
            && let Some((area, _)) = self.targets.get(self.focus)
        {
            let name = self.names.get(self.focus).map_or("", String::as_str);
            canvas.describe(Role::Button, name, *area).focused(true);
        }
        // The widget's own focus where it draws one.
        let own = self.focus_shown
            && self
                .owners
                .get(self.focus)
                .copied()
                .flatten()
                .and_then(|item| self.items.get(item))
                .and_then(|shown| shown.widget.as_ref())
                .zip(self.targets.get(self.focus))
                .is_some_and(|(widget, (area, _))| widget.draw_focus(canvas, env, *area));
        if self.focus_shown
            && !own
            && let Some((area, _)) = self.targets.get(self.focus)
        {
            draw_cell_focus(canvas, &self.style, *area, tool_ink);
        }
    }

    /// The grid's cells, lightly.
    fn draw_cells(&self, canvas: &mut Canvas<'_>) {
        let g = self.geometry;
        for row in 0..g.rows {
            for col in 0..g.columns {
                let cell = g.rect(Cells::new(col, row, 1, 1));
                canvas.stroke_round(cell, 1, self.style.corner, Color::gray(200));
            }
        }
    }

    /// While editing, *Back* says it ends editing.
    pub fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if *action == actions::BACK && self.edit.is_some() {
            return Hint::Text(env.i18n().tr("core.grid.done").to_string());
        }
        Hint::Default
    }

    /// What holding Select runs on the focused target, where it has a
    /// long press of its own and the focus shows.
    fn focused_hold(&self) -> Option<Command> {
        if !self.focus_shown || self.edit.is_some() {
            return None;
        }
        match self.targets.get(self.focus) {
            Some((_, Target::Run(_, Some(hold)))) => Some((**hold).clone()),
            _ => None,
        }
    }

    /// The area around a target that its focus frame takes.
    fn around(&self, area: Rect) -> Rect {
        area.grow((self.style.line * 4) as u16)
    }

    /// Moves the focus to `to`, redrawing both targets; their widgets are
    /// told ([`WidgetInput::Focus`]).
    fn focus_on(&mut self, to: usize, cx: &mut Cx<'_, '_>) {
        if let Some((area, _)) = self.targets.get(self.focus) {
            cx.request_redraw_area(self.around(*area));
        }
        let from = self.focus;
        self.focus = to;
        if let Some((area, _)) = self.targets.get(to) {
            cx.request_redraw_area(self.around(*area));
        }
        let owner = |at: usize| self.owners.get(at).copied().flatten();
        let (left, came) = (owner(from), owner(to));
        if from != to {
            if let Some(item) = left {
                self.tell(item, WidgetInput::Focus(None), cx);
            }
            if let Some(item) = came {
                let area = self.targets.get(to).map(|(area, _)| *area);
                self.tell(item, WidgetInput::Focus(area), cx);
            }
        }
        // Exactly: two targets far apart make a large rectangle, and a
        // widget with a picture in it, a large one, which the kernel would
        // show in grays or in full (it takes the rectangle and the picture
        // for a page); the picture stays as it is.
        cx.set_refresh(RefreshMode::Fast);
    }

    /// `cells` for item `index` as its widget's hint keeps them
    /// ([`CellSpan::keep`](crate::widgets::CellSpan::keep)), the side
    /// that changed `wide` or high.
    fn kept(&self, index: usize, cells: Cells, wide: bool, env: &Env<'_>) -> Cells {
        let hint = self
            .items
            .get(index)
            .and_then(|shown| match &shown.item.what {
                What::Widget(id) => env.widgets().cells(id),
                What::Tool(_) => None,
            });
        match hint {
            Some(hint) => hint.keep(cells, wide),
            None => cells,
        }
    }

    /// Tells item `index`'s widget of `input`: `true` if it took it, then
    /// drawn again, with what it asks.
    fn tell(&mut self, index: usize, input: WidgetInput, cx: &mut Cx<'_, '_>) -> bool {
        let env = *cx.env();
        let Some(shown) = self.items.get_mut(index) else {
            return false;
        };
        let Some(widget) = &mut shown.widget else {
            return false;
        };
        let mut out = crate::Commands::new();
        let taken = widget.on_input(&env, input, shown.area, &mut out);
        if taken {
            cx.request_redraw_area(shown.area);
            // As a focus step is (see `focus_on`).
            cx.set_refresh(RefreshMode::Fast);
        }
        for command in out {
            cx.command(command);
        }
        taken
    }

    /// The targets anew after item `index`'s widget took a swipe (its
    /// page turned). Where the focus was on it, it goes to its first
    /// target, so that what Select runs is what shows (the widget told);
    /// elsewhere it stays on the target it was on.
    fn retarget(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let owner = |grid: &Self, at: usize| grid.owners.get(at).copied().flatten();
        let start = |grid: &Self, item: usize| grid.owners.iter().position(|o| *o == Some(item));
        let was = owner(self, self.focus);
        let had = was == Some(index);
        let before = (
            self.focus,
            self.targets.get(self.focus).map(|(area, _)| *area),
        );
        // Where it was among its item's targets, or a button's place from
        // the end: the widget's own count of targets may change.
        let place = match was {
            Some(item) => Ok((item, self.focus - start(self, item).unwrap_or(self.focus))),
            None => Err(self.targets.len() - self.focus),
        };
        self.target(&env);
        let len = self.targets.len();
        if len == 0 {
            return;
        }
        let to = match place {
            _ if had => start(self, index).unwrap_or(self.focus),
            Ok((item, at)) => {
                let first = start(self, item).unwrap_or(self.focus);
                let count = self.owners.iter().filter(|o| **o == Some(item)).count();
                first + at.min(count.saturating_sub(1))
            }
            Err(from_end) => len.saturating_sub(from_end),
        };
        self.focus = to.min(len - 1);
        if let Some(shown) = self.items.get(index) {
            cx.request_redraw_area(self.around(shown.area));
        }
        let area = self.targets.get(self.focus).map(|(area, _)| *area);
        if had && self.focus_shown && before != (self.focus, area) {
            match owner(self, self.focus) {
                Some(item) if item == index => {
                    self.tell(index, WidgetInput::Focus(area), cx);
                }
                came => {
                    // None of its own left: the focus went on.
                    self.tell(index, WidgetInput::Focus(None), cx);
                    if let Some(item) = came {
                        self.tell(item, WidgetInput::Focus(area), cx);
                    }
                    if let Some(area) = area {
                        cx.request_redraw_area(self.around(area));
                    }
                }
            }
        }
    }

    /// Says which screen shows the grid, for its widgets
    /// ([`WidgetContext::host`](crate::widgets::WidgetContext::host)).
    pub fn set_host(&mut self, host: Id) {
        self.host = Some(host);
    }

    /// The screen's own options for items while editing, after the grid's
    /// (move, size, settings, remove), as translation keys of their names
    /// (no commas): chosen, the grid answers
    /// [`GridResponse::Option`].
    pub fn set_options(&mut self, options: &[&'static str]) {
        self.extra = options
            .iter()
            .map(|o| alloc::borrow::Cow::Borrowed(*o))
            .collect();
    }

    /// Asks `check` before an item goes where it is dragged or moved by
    /// the buttons: `false` keeps it where it was, as where another item
    /// is in the way.
    pub fn set_drop_check(&mut self, check: DropCheck) {
        self.drop_check = Some(check);
    }

    /// Whether every target belongs to item `index` (no other, no
    /// button of the screen).
    fn alone(&self, index: usize) -> bool {
        self.owners.iter().all(|owner| *owner == Some(index))
    }

    /// The target nearest below (or above) the focused one.
    fn vertical(&self, down: bool) -> Option<usize> {
        let (from, _) = self.targets.get(self.focus)?;
        let cx = i32::from(from.x) + i32::from(from.width) / 2;
        self.targets
            .iter()
            .enumerate()
            .filter(|(_, (r, _))| {
                if down {
                    r.y >= from.y + from.height
                } else {
                    r.y + r.height <= from.y
                }
            })
            .min_by_key(|(_, (r, _))| {
                let dy = (i32::from(r.y) - i32::from(from.y)).abs();
                let dx = (i32::from(r.x) + i32::from(r.width) / 2 - cx).abs();
                dy * 4 + dx
            })
            .map(|(i, _)| i)
    }

    fn run(&mut self, index: usize, cx: &mut Cx<'_, '_>) -> GridResponse {
        let Some((_, target)) = self.targets.get(index).cloned() else {
            return GridResponse::Handled;
        };
        match target {
            Target::Run(command, _) => {
                // A tool shown faint by its plugin: nothing to do now.
                let faint = matches!(&command, Command::RunAction(action, _)
                    if !cx.env().menus().state(cx.env(), action.as_str()).enabled);
                if !faint {
                    cx.command(command);
                }
            }
            Target::Item(item) => self.options(item, cx),
            Target::Button(button) => return GridResponse::Button(button),
        }
        GridResponse::Handled
    }

    /// A placement of widget `id` (`id@n`) no item here and no values set
    /// so far use; `id` itself where the first [`PLACEMENTS`] all are.
    fn place(&self, env: &Env<'_>, id: &str) -> alloc::string::String {
        crate::widgets::free_placement(env, id, |placed| {
            self.items
                .iter()
                .any(|i| matches!(&i.item.what, What::Widget(w) if w == placed))
        })
    }

    /// Offers what can be done with item `index`.
    fn options(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(shown) = self.items.get(index) else {
            return;
        };
        let mut options: Vec<String> =
            alloc::vec![format!("{OPTIONS}move"), format!("{OPTIONS}resize")];
        if let What::Widget(id) = &shown.item.what
            && crate::widgets::has_settings(&env, id)
        {
            options.push(format!("{OPTIONS}settings"));
        }
        options.push(format!("{OPTIONS}remove"));
        options.extend(self.extra.iter().map(|o| String::from(o.as_ref())));
        let options: Vec<&str> = options.iter().map(String::as_str).collect();
        cx.command(
            crate::ask::choice(shown.item.what.title(&env), &self.chosen)
                .about(format!("{index}"))
                .options("", &options)
                .command(),
        );
    }

    /// Whether item `index` may take the cells `to`.
    fn free(&self, index: usize, to: Cells) -> bool {
        to.fits(self.geometry.columns, self.geometry.rows)
            && !self
                .items
                .iter()
                .enumerate()
                .any(|(i, s)| i != index && s.cells.overlaps(&to))
    }

    /// Whether the screen lets item `index` go to `to` (see
    /// [`GridView::set_drop_check`]).
    fn allowed(&self, env: &Env<'_>, index: usize, to: Cells) -> bool {
        let Some(check) = self.drop_check else {
            return true;
        };
        let Some(shown) = self.items.get(index) else {
            return false;
        };
        check(env, &self.items(), &shown.item, to)
    }

    /// Puts item `index` at `to` if it may go there; `true` if it did.
    fn set_cells(&mut self, index: usize, to: Cells, cx: &mut Cx<'_, '_>) -> bool {
        if !self.free(index, to) || !self.allowed(cx.env(), index, to) {
            return false;
        }
        let Some(shown) = self.items.get(index) else {
            return false;
        };
        cx.request_redraw_area(self.around(shown.area));
        let area = self.geometry.rect(to);
        cx.request_redraw_area(self.around(area));
        let shown = &mut self.items[index];
        shown.cells = to;
        shown.area = area;
        if let Some(widget) = &mut shown.widget {
            let env = *cx.env();
            widget.place(&env, area);
            let host = self.host.as_ref().map(Id::as_str);
            let context = WidgetContext::new(SizeClass::of(&env, area)).host(host);
            widget.context(&env, &context);
        }
        if let Some(target) = self.targets.get_mut(index) {
            target.0 = area;
        }
        true
    }

    /// The item at a point, and whether at its corner (to size it).
    fn item_at(&self, point: Point) -> Option<(usize, bool)> {
        let index = self.items.iter().position(|s| s.area.contains(point))?;
        let corner = handle(self.items[index].area, &self.style).contains(point);
        Some((index, corner))
    }

    /// Whether the buttons are moving or sizing an item.
    fn buttons_mode(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.mode != Mode::Choose)
    }

    /// Ends moving or sizing with the buttons: the items as they are now,
    /// to keep.
    fn end_mode(&mut self, cx: &mut Cx<'_, '_>) -> GridResponse {
        if let Some(edit) = &mut self.edit {
            edit.mode = Mode::Choose;
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
        GridResponse::Changed(self.items())
    }

    /// An item's option chosen (the event given to the plugin's choice
    /// screen), or [`GridResponse::Ignored`] for other events.
    pub fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) -> GridResponse {
        let Some(picked) = self.chosen.read(event, params) else {
            return GridResponse::Ignored;
        };
        let Some(index) = picked
            .path
            .parse::<usize>()
            .ok()
            .filter(|i| *i < self.items.len())
        else {
            return GridResponse::Handled;
        };
        let env = *cx.env();
        // The screen's own: its answer.
        if self.extra.iter().any(|o| *o == picked.option.as_str()) {
            return GridResponse::Option {
                item: self.items[index].item.clone(),
                option: picked.option,
            };
        }
        let option = picked.option.strip_prefix(OPTIONS).unwrap_or_default();
        match option {
            option @ ("move" | "resize") => {
                if let Some(edit) = &mut self.edit {
                    edit.mode = if option == "move" {
                        Mode::Move
                    } else {
                        Mode::Resize
                    };
                }
                self.focus = index;
                self.focus_shown = true;
                let message = env
                    .i18n()
                    .tr(&format!("{OPTIONS}{option}.help"))
                    .to_string();
                cx.command(Command::Notify(message));
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            "settings" => {
                let What::Widget(id) = self.items[index].item.what.clone() else {
                    return GridResponse::Handled;
                };
                // Each placement has settings of its own: one placed before
                // there were placements gets its number first.
                let placed = if id.contains('@') {
                    id
                } else {
                    self.place(&env, &id)
                };
                let params = Params::new().with(params::ID, placed.clone());
                cx.command(Command::OpenSlot(slots::SETTINGS, params));
                if self.items[index].item.what != What::Widget(placed.clone()) {
                    self.items[index].item.what = What::Widget(placed);
                    return GridResponse::Changed(self.items());
                }
            }
            "remove" => {
                // What the placement set goes with it.
                if let What::Widget(id) = &self.items[index].item.what
                    && id.contains('@')
                {
                    for value in env.settings().placement_values(id) {
                        cx.command(Command::ResetSetting(value));
                    }
                }
                self.items.remove(index);
                // The focus goes to another item, which nobody chose to
                // move or size.
                if let Some(edit) = &mut self.edit {
                    edit.mode = Mode::Choose;
                }
                self.target(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                return GridResponse::Changed(self.items());
            }
            _ => {}
        }
        GridResponse::Handled
    }

    /// Buttons: the focus goes from target to target (left and right in
    /// order, up and down to the nearest), confirm runs it; the first
    /// press shows where the focus is. While an item is moved or sized,
    /// they move or size it. *Back* ends editing. The page buttons are
    /// left to the screen (except while an item is moved or sized, where
    /// they go up and down where there are no such buttons).
    pub fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> GridResponse {
        let a = action;
        if let Some(Edit { mode, .. }) = self.edit
            && mode != Mode::Choose
            && let Some(Target::Item(index)) = self.targets.get(self.focus).map(|t| t.1.clone())
        {
            return self.move_item(mode, index, a, cx);
        }
        if *a == actions::BACK && self.edit.is_some() {
            self.set_editing(false);
            return GridResponse::Editing(false);
        }
        // With the buttons as by touch: holding Select runs the focused
        // target's own long press where it has one (elsewhere it opens
        // the menu, the action's own).
        if *a == actions::LONG_CONFIRM
            && let Some(hold) = self.focused_hold()
        {
            cx.command(hold);
            return GridResponse::Handled;
        }
        // The page buttons turn the focused widget's pages (a list, a
        // calendar) where it takes them, as a swipe on it would; else the
        // screen's. Left and Right always go on through the targets, so a
        // widget that pages without end keeps the focus nowhere. Where its
        // targets are the only ones, the page buttons are the screen's
        // (its other pages always in reach) and Left and Right page it at
        // its first and last target.
        let next = *a == actions::NEXT_PAGE;
        if (next || *a == actions::PREV_PAGE)
            && self.focus_shown
            && self.edit.is_none()
            && let Some(item) = self.owners.get(self.focus).copied().flatten()
            && !self.alone(item)
        {
            let direction = if next {
                crate::gesture::Direction::Left
            } else {
                crate::gesture::Direction::Right
            };
            if self.tell(item, WidgetInput::Swipe(direction), cx) {
                self.retarget(item, cx);
                return GridResponse::Handled;
            }
        }
        let moves = [
            actions::UP,
            actions::DOWN,
            actions::LEFT,
            actions::RIGHT,
            actions::NEXT,
            actions::PREVIOUS,
            actions::CONFIRM,
        ];
        if !moves.contains(a) {
            return GridResponse::Ignored;
        }
        if !self.focus_shown {
            // The first press shows where the focus is.
            self.focus_shown = true;
            self.focus_on(self.focus, cx);
            return GridResponse::Handled;
        }
        let len = self.targets.len();
        let step = if *a == actions::RIGHT {
            1
        } else if *a == actions::LEFT {
            -1
        } else {
            0
        };
        if step != 0
            && self.edit.is_none()
            && let Some(item) = self.owners.get(self.focus).copied().flatten()
            && self.alone(item)
            && (step > 0 && self.focus + 1 == len || step < 0 && self.focus == 0)
        {
            let direction = if step > 0 {
                crate::gesture::Direction::Left
            } else {
                crate::gesture::Direction::Right
            };
            if self.tell(item, WidgetInput::Swipe(direction), cx) {
                self.retarget(item, cx);
                return GridResponse::Handled;
            }
        }
        let to = if *a == actions::CONFIRM {
            return self.run(self.focus, cx);
        } else if *a == actions::RIGHT || *a == actions::NEXT {
            ui::step_focus(self.focus, 1, len)
        } else if *a == actions::LEFT || *a == actions::PREVIOUS {
            ui::step_focus(self.focus, -1, len)
        } else {
            // The nearest above or below (in a widget too); nothing there,
            // it stays.
            match self.vertical(*a == actions::DOWN) {
                Some(to) => to,
                None => return GridResponse::Handled,
            }
        };
        self.focus_on(to, cx);
        GridResponse::Handled
    }

    /// Moving or sizing item `index` with the buttons.
    fn move_item(&mut self, mode: Mode, index: usize, a: &Id, cx: &mut Cx<'_, '_>) -> GridResponse {
        if *a == actions::CONFIRM || *a == actions::BACK {
            return self.end_mode(cx);
        }
        let Some(c) = self.items.get(index).map(|s| s.cells) else {
            return GridResponse::Handled;
        };
        let step = |n: u8, more: bool| {
            if more {
                n.saturating_add(1)
            } else {
                n.saturating_sub(1)
            }
        };
        let left = *a == actions::LEFT || *a == actions::PREVIOUS;
        let right = *a == actions::RIGHT || *a == actions::NEXT;
        // Up and down, or the page buttons where there are none (the
        // X4's side buttons).
        let up = *a == actions::UP || *a == actions::PREV_PAGE;
        let down = *a == actions::DOWN || *a == actions::NEXT_PAGE;
        let to = match mode {
            Mode::Move if left || right => Cells {
                col: step(c.col, right),
                ..c
            },
            Mode::Move if up || down => Cells {
                row: step(c.row, down),
                ..c
            },
            Mode::Resize if left || right => self.kept(
                index,
                Cells {
                    w: step(c.w, right).max(1),
                    ..c
                },
                true,
                cx.env(),
            ),
            Mode::Resize if up || down => self.kept(
                index,
                Cells {
                    h: step(c.h, down).max(1),
                    ..c
                },
                false,
                cx.env(),
            ),
            _ => return GridResponse::Ignored,
        };
        if to != c {
            if self.set_cells(index, to, cx) {
                cx.request_refresh(RefreshMode::Fast);
            } else if to.fits(self.geometry.columns, self.geometry.rows) {
                // Within the grid, but another item is in the way: said,
                // so the press does not seem lost.
                let env = *cx.env();
                cx.command(Command::Notify(
                    env.i18n().tr("core.grid.in-the-way").to_string(),
                ));
            }
        }
        GridResponse::Handled
    }

    /// While editing, the edges are the grid's: items at them are dragged
    /// and sized, not the screen left (for the screen's
    /// [`holds_edges`](crate::Screen::holds_edges)).
    pub fn holds_edges(&self) -> bool {
        self.edit.is_some()
    }

    /// While editing, a finger on an item drags it (by its corner: sizes
    /// it); lifted where it was, it offers the item's options.
    pub fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> GridResponse {
        if self.edit.is_none() {
            return GridResponse::Ignored;
        }
        match event.phase {
            // A finger while the buttons move or size an item is not
            // taken (the kernel then makes it a tap, see `on_gesture`).
            PointerPhase::Down if self.buttons_mode() => GridResponse::Ignored,
            PointerPhase::Down => {
                let Some((index, resize)) = self.item_at(event.point) else {
                    return GridResponse::Ignored;
                };
                let to = self.items[index].cells;
                if let Some(edit) = &mut self.edit {
                    edit.drag = Some(Drag {
                        index,
                        resize,
                        start: event.point,
                        to,
                        moved: false,
                    });
                }
                GridResponse::Handled
            }
            PointerPhase::Move => {
                let Some(mut drag) = self.edit.as_ref().and_then(|e| e.drag) else {
                    return GridResponse::Handled;
                };
                let Some(from) = self.items.get(drag.index).map(|s| s.cells) else {
                    return GridResponse::Handled;
                };
                let Some(to) = dragged(&self.geometry, from, drag.start, event.point, drag.resize)
                else {
                    return GridResponse::Handled;
                };
                let to = if drag.resize {
                    let wide = to.w != from.w;
                    self.kept(drag.index, to, wide, cx.env())
                } else {
                    to
                };
                // Drawn again only where it goes to other cells (or other
                // sizes): a finger trembling in a cell asks for nothing, as
                // a refresh takes most of a second on some panels.
                if to != drag.to {
                    cx.request_redraw_area(self.around(self.geometry.rect(drag.to)));
                    drag.to = to;
                    drag.moved |= to != from;
                    cx.request_redraw_area(self.around(self.geometry.rect(to)));
                    cx.request_refresh(RefreshMode::Fast);
                    if let Some(edit) = &mut self.edit {
                        edit.drag = Some(drag);
                    }
                }
                GridResponse::Handled
            }
            // Up, Cancel and phases of later releases end what goes on.
            _ => {
                let Some(drag) = self.edit.as_mut().and_then(|e| e.drag.take()) else {
                    return GridResponse::Handled;
                };
                if !drag.moved {
                    // A tap: the item gets the focus and offers its options.
                    let target = self
                        .targets
                        .iter()
                        .position(|t| t.1 == Target::Item(drag.index));
                    if let Some(target) = target {
                        self.focus = target;
                        self.focus_shown = true;
                    }
                    if event.phase == PointerPhase::Up {
                        self.options(drag.index, cx);
                    }
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Fast);
                    return GridResponse::Handled;
                }
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                if event.phase == PointerPhase::Up && self.set_cells(drag.index, drag.to, cx) {
                    return GridResponse::Changed(self.items());
                }
                GridResponse::Handled
            }
        }
    }

    /// A tap runs the target there; a long press begins editing.
    pub fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> GridResponse {
        match gesture.kind {
            // A tap while the buttons move or size an item ends that and
            // keeps what they did, so that a tap on the screen's own button
            // (e.g. to stop editing, which reads the layout again) loses
            // nothing; the next tap does what it does.
            GestureKind::Tap if self.buttons_mode() => self.end_mode(cx),
            GestureKind::Tap => match self
                .targets
                .iter()
                .position(|(r, _)| r.contains(gesture.start))
            {
                Some(index) => {
                    self.focus = index;
                    self.run(index, cx)
                }
                None => match self.touch.iter().find(|(r, ..)| r.contains(gesture.start)) {
                    Some((_, command, _)) => {
                        cx.command(command.clone());
                        GridResponse::Handled
                    }
                    None => GridResponse::Ignored,
                },
            },
            // A target's own long press, else editing.
            GestureKind::LongPress if self.edit.is_none() => {
                let on = |r: &Rect| r.contains(gesture.start);
                let hold = self
                    .targets
                    .iter()
                    .find_map(|(r, target)| match target {
                        Target::Run(_, Some(hold)) if on(r) => Some((**hold).clone()),
                        _ => None,
                    })
                    .or_else(|| {
                        self.touch
                            .iter()
                            .find(|(r, _, hold)| on(r) && hold.is_some())
                            .and_then(|(_, _, hold)| hold.as_deref().cloned())
                    });
                if let Some(hold) = hold {
                    cx.command(hold);
                    return GridResponse::Handled;
                }
                self.set_editing(true);
                GridResponse::Editing(true)
            }
            // A swipe that starts on a widget that takes it.
            GestureKind::Swipe(direction) if self.edit.is_none() => {
                let index = self
                    .items
                    .iter()
                    .position(|s| s.widget.is_some() && s.area.contains(gesture.start));
                match index {
                    Some(index) if self.tell(index, WidgetInput::Swipe(direction), cx) => {
                        self.retarget(index, cx);
                        GridResponse::Handled
                    }
                    _ => GridResponse::Ignored,
                }
            }
            _ => GridResponse::Ignored,
        }
    }
}

/// The corner of an item that sizes it when dragged.
fn handle(area: Rect, style: &UiStyle) -> Rect {
    let side = (style.row / 2).max(16) as u16;
    Rect::new(
        (area.x + area.width).saturating_sub(side),
        (area.y + area.height).saturating_sub(side),
        side,
        side,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_kit::Kit;
    use crate::{Output, PointerTool, SettingSpec, SettingValue};

    const CHOSEN: Event<Picked> = Event::new("test.chosen");

    /// Four columns and three rows of 100 pixels, a gap of 10: cell
    /// (c, r) is at (c * 100 + 5, r * 100 + 5), 90 pixels wide and high.
    fn geometry() -> Geometry {
        Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 10)
    }

    fn tool(id: &'static str) -> What {
        What::Tool(Id::new_static(id))
    }

    fn at(what: What, col: u8, row: u8, w: u8, h: u8) -> Item {
        Item::placed(what, Cells::new(col, row, w, h))
    }

    /// Tools `a` at 0/0, `b` at 1/0 and `c` at 0/1.
    fn three() -> Vec<Item> {
        alloc::vec![
            at(tool("t.a"), 0, 0, 1, 1),
            at(tool("t.b"), 1, 0, 1, 1),
            at(tool("t.c"), 0, 1, 1, 1),
        ]
    }

    /// A button below the grid, numbered 7.
    const BUTTON: (Rect, u16) = (Rect::new(0, 310, 100, 40), 7);

    fn laid_out(env: &Env<'_>, items: &[Item], buttons: &[(Rect, u16)]) -> GridView {
        let mut grid = GridView::asking(env, CHOSEN);
        grid.layout(env, geometry(), items, buttons);
        grid
    }

    /// Being edited, as the screen lays it out again once editing began.
    fn edited(env: &Env<'_>, items: &[Item]) -> GridView {
        let mut grid = laid_out(env, items, &[]);
        grid.set_editing(true);
        grid.layout(env, geometry(), items, &[]);
        grid
    }

    fn act(grid: &mut GridView, env: &Env<'_>, action: &Id) -> (GridResponse, Output) {
        let mut out = Output::default();
        let response = grid.on_action(action, &mut Cx::new(env, &mut out));
        (response, out)
    }

    /// Option `option` of item `index` chosen, as the choice screen says.
    fn pick(grid: &mut GridView, env: &Env<'_>, index: usize, option: &str) -> GridResponse {
        let picked = Picked {
            path: format!("{index}"),
            option: format!("{OPTIONS}{option}"),
        };
        let Command::Emit(event, params) = CHOSEN.emit(picked) else {
            unreachable!("an event is emitted");
        };
        let mut out = Output::default();
        grid.on_event(&event, &params, &mut Cx::new(env, &mut out))
    }

    fn pointer(
        grid: &mut GridView,
        env: &Env<'_>,
        phase: PointerPhase,
        x: i32,
        y: i32,
    ) -> (GridResponse, Output) {
        let event = PointerEvent::new(0, phase, Point::new(x, y), PointerTool::Finger, 1000);
        let mut out = Output::default();
        let response = grid.on_pointer(&event, &mut Cx::new(env, &mut out));
        (response, out)
    }

    fn gesture(
        grid: &mut GridView,
        env: &Env<'_>,
        kind: GestureKind,
        x: i32,
        y: i32,
    ) -> GridResponse {
        let point = Point::new(x, y);
        let gesture = Gesture {
            kind,
            start: point,
            end: point,
            tool: PointerTool::Finger,
        };
        let mut out = Output::default();
        grid.on_gesture(gesture, &mut Cx::new(env, &mut out))
    }

    /// Where item `index` is now.
    fn cells(grid: &GridView, index: usize) -> Cells {
        grid.items[index].cells
    }

    fn changed(response: GridResponse) -> Vec<Item> {
        match response {
            GridResponse::Changed(items) => items,
            other => panic!("not changed: {other:?}"),
        }
    }

    fn notified(out: &Output) -> bool {
        out.commands.iter().any(|c| matches!(c, Command::Notify(_)))
    }

    /// Whether the options of item `index` were offered.
    fn offered(out: &Output, index: usize) -> bool {
        let path = format!("{index}");
        out.commands.iter().any(|c| {
            matches!(c, Command::OpenSlot(slot, p)
                if *slot == crate::builtin::slots::CHOICE && p.get(params::PATH) == Some(path.as_str()))
        })
    }

    #[test]
    fn items_without_room_are_kept() {
        let kit = Kit::new(false);
        let env = kit.env();
        // Thirteen tools for twelve cells: the last has no room.
        let names = [
            "t.a", "t.b", "t.c", "t.d", "t.e", "t.f", "t.g", "t.h", "t.i", "t.j", "t.k", "t.l",
            "t.m",
        ];
        let items: Vec<Item> = names.iter().map(|n| Item::new(tool(n))).collect();
        let grid = laid_out(&env, &items, &[]);
        let kept = grid.items();
        assert_eq!(kept.len(), 13);
        assert!(kept[..12].iter().all(|i| i.at.is_some()));
        assert_eq!(kept[12], Item::new(tool("t.m")));
    }

    #[test]
    fn equal_items_without_room_are_kept() {
        let kit = Kit::new(false);
        let env = kit.env();
        // The same tool twice in one place: the second has no room, and
        // is kept all the same.
        let items = [at(tool("t.a"), 0, 0, 1, 1), at(tool("t.a"), 0, 0, 1, 1)];
        let grid = laid_out(&env, &items, &[]);
        assert_eq!(grid.items(), items);
        // So with places to come, in a grid of one cell.
        let mut one = GridView::asking(&env, CHOSEN);
        let items = [Item::new(tool("t.a")), Item::new(tool("t.a"))];
        let small = Geometry::of(Rect::new(0, 0, 100, 100), 1, 1, 0);
        one.layout(&env, small, &items, &[]);
        assert_eq!(
            one.items(),
            [at(tool("t.a"), 0, 0, 1, 1), Item::new(tool("t.a"))]
        );
    }

    #[test]
    fn a_long_press_begins_editing_and_back_ends_it() {
        let kit = Kit::new(true);
        let env = kit.env();
        let mut grid = laid_out(&env, &three(), &[]);
        assert!(!grid.editing() && !grid.holds_edges());
        // Not editing, a finger is the screen's.
        let (response, _) = pointer(&mut grid, &env, PointerPhase::Down, 50, 50);
        assert_eq!(response, GridResponse::Ignored);
        let response = gesture(&mut grid, &env, GestureKind::LongPress, 50, 50);
        assert_eq!(response, GridResponse::Editing(true));
        assert!(grid.editing() && grid.holds_edges());
        // Once editing, a long press does nothing more.
        let response = gesture(&mut grid, &env, GestureKind::LongPress, 50, 50);
        assert_eq!(response, GridResponse::Ignored);
        grid.layout(&env, geometry(), &three(), &[]);
        assert!(matches!(grid.hint(&actions::BACK, &env), Hint::Text(_)));
        let (response, _) = act(&mut grid, &env, &actions::BACK);
        assert_eq!(response, GridResponse::Editing(false));
        assert!(!grid.editing() && !grid.holds_edges());
        assert_eq!(grid.hint(&actions::BACK, &env), Hint::Default);
        // Not editing, *Back* is the screen's.
        let (response, _) = act(&mut grid, &env, &actions::BACK);
        assert_eq!(response, GridResponse::Ignored);
    }

    #[test]
    fn the_buttons_move_the_focus_and_run_tools() {
        let kit = Kit::new(false);
        let env = kit.env();
        let mut grid = laid_out(&env, &three(), &[]);
        // Without touch, the focus shows from the start.
        assert!(grid.focus_shown);
        assert_eq!(grid.focus, 0);
        let (response, out) = act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(response, GridResponse::Handled);
        assert_eq!(grid.focus, 1);
        assert!(out.redraw_area.is_some());
        // Down from `b`: `c`, the only one below.
        act(&mut grid, &env, &actions::DOWN);
        assert_eq!(grid.focus, 2);
        // Up from `c`: `a`, right above it.
        act(&mut grid, &env, &actions::UP);
        assert_eq!(grid.focus, 0);
        // Nothing above: the focus stays.
        act(&mut grid, &env, &actions::UP);
        assert_eq!(grid.focus, 0);
        // Before the first: round to the last.
        act(&mut grid, &env, &actions::PREVIOUS);
        assert_eq!(grid.focus, 2);
        act(&mut grid, &env, &actions::NEXT);
        assert_eq!(grid.focus, 0);
        let (response, out) = act(&mut grid, &env, &actions::CONFIRM);
        assert_eq!(response, GridResponse::Handled);
        assert_eq!(out.commands, [Command::run(Id::new_static("t.a"))]);
        // Other buttons are the screen's.
        let (response, _) = act(&mut grid, &env, &actions::NEXT_PAGE);
        assert_eq!(response, GridResponse::Ignored);
    }

    #[test]
    fn with_touch_the_first_press_shows_the_focus() {
        let kit = Kit::new(true);
        let env = kit.env();
        let mut grid = laid_out(&env, &three(), &[]);
        assert!(!grid.focus_shown);
        let (response, out) = act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(response, GridResponse::Handled);
        assert!(grid.focus_shown && out.commands.is_empty());
        assert_eq!(grid.focus, 0);
        act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(grid.focus, 1);
        // A tap runs the tool there and takes the focus.
        assert_eq!(
            gesture(&mut grid, &env, GestureKind::Tap, 50, 150),
            GridResponse::Handled
        );
        assert_eq!(grid.focus, 2);
        assert_eq!(
            gesture(&mut grid, &env, GestureKind::Tap, 350, 250),
            GridResponse::Ignored
        );
    }

    #[test]
    fn the_buttons_alone_move_and_size_an_item() {
        let kit = Kit::new(false);
        let env = kit.env();
        let mut grid = edited(&env, &three());
        // While editing, the targets are the items: confirm offers the
        // options of the one with the focus.
        let (_, out) = act(&mut grid, &env, &actions::RIGHT);
        assert!(out.commands.is_empty());
        let (_, out) = act(&mut grid, &env, &actions::CONFIRM);
        assert!(offered(&out, 1));
        assert_eq!(pick(&mut grid, &env, 1, "move"), GridResponse::Handled);
        // `b` a cell right, then down.
        act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(cells(&grid, 1), Cells::new(2, 0, 1, 1));
        act(&mut grid, &env, &actions::DOWN);
        assert_eq!(cells(&grid, 1), Cells::new(2, 1, 1, 1));
        // The page buttons go up and down too (the X4's side buttons).
        act(&mut grid, &env, &actions::PREV_PAGE);
        assert_eq!(cells(&grid, 1), Cells::new(2, 0, 1, 1));
        act(&mut grid, &env, &actions::NEXT_PAGE);
        act(&mut grid, &env, &actions::LEFT);
        // `c` is in the way: said, and nothing moves.
        let (_, out) = act(&mut grid, &env, &actions::LEFT);
        assert!(notified(&out));
        assert_eq!(cells(&grid, 1), Cells::new(1, 1, 1, 1));
        // At the grid's edge nothing is said.
        act(&mut grid, &env, &actions::DOWN);
        let (_, out) = act(&mut grid, &env, &actions::DOWN);
        assert!(!notified(&out));
        assert_eq!(cells(&grid, 1), Cells::new(1, 2, 1, 1));
        // Confirm ends moving and keeps the place.
        let items = changed(act(&mut grid, &env, &actions::CONFIRM).0);
        assert_eq!(items[1], at(tool("t.b"), 1, 2, 1, 1));
        assert!(grid.editing());
        // The buttons move the focus again.
        act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(grid.focus, 2);
        assert_eq!(cells(&grid, 1), Cells::new(1, 2, 1, 1));

        // `c` wider, then higher; never narrower than a cell.
        pick(&mut grid, &env, 2, "resize");
        act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(cells(&grid, 2), Cells::new(0, 1, 2, 1));
        // `b` below it is in the way.
        act(&mut grid, &env, &actions::DOWN);
        assert_eq!(cells(&grid, 2), Cells::new(0, 1, 2, 1));
        act(&mut grid, &env, &actions::RIGHT);
        act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(cells(&grid, 2), Cells::new(0, 1, 4, 1));
        for _ in 0..5 {
            act(&mut grid, &env, &actions::LEFT);
        }
        assert_eq!(cells(&grid, 2), Cells::new(0, 1, 1, 1));
        act(&mut grid, &env, &actions::DOWN);
        assert_eq!(cells(&grid, 2), Cells::new(0, 1, 1, 2));
        // *Back* ends sizing, not editing.
        let items = changed(act(&mut grid, &env, &actions::BACK).0);
        assert_eq!(items[2], at(tool("t.c"), 0, 1, 1, 2));
        assert!(grid.editing());
        assert_eq!(
            act(&mut grid, &env, &actions::BACK).0,
            GridResponse::Editing(false)
        );
    }

    #[test]
    fn items_are_moved_and_sized_by_a_finger() {
        let kit = Kit::new(true);
        let env = kit.env();
        let mut grid = edited(&env, &three());
        // `a` dragged a cell right and two down.
        pointer(&mut grid, &env, PointerPhase::Down, 50, 50);
        let (_, out) = pointer(&mut grid, &env, PointerPhase::Move, 150, 250);
        assert!(out.redraw_area.is_some());
        let items = changed(pointer(&mut grid, &env, PointerPhase::Up, 150, 250).0);
        assert_eq!(items[0], at(tool("t.a"), 1, 2, 1, 1));
        // Its corner (the bottom right) pulled two cells right.
        pointer(&mut grid, &env, PointerPhase::Down, 190, 290);
        pointer(&mut grid, &env, PointerPhase::Move, 350, 290);
        let items = changed(pointer(&mut grid, &env, PointerPhase::Up, 350, 290).0);
        assert_eq!(items[0], at(tool("t.a"), 1, 2, 3, 1));
        // `c` onto `b`: it stays where it was.
        pointer(&mut grid, &env, PointerPhase::Down, 50, 150);
        pointer(&mut grid, &env, PointerPhase::Move, 150, 50);
        let (response, _) = pointer(&mut grid, &env, PointerPhase::Up, 150, 50);
        assert_eq!(response, GridResponse::Handled);
        assert_eq!(cells(&grid, 2), Cells::new(0, 1, 1, 1));
        // Taken away (a palm): nothing moves.
        pointer(&mut grid, &env, PointerPhase::Down, 50, 150);
        pointer(&mut grid, &env, PointerPhase::Move, 350, 50);
        let (response, out) = pointer(&mut grid, &env, PointerPhase::Cancel, 350, 50);
        assert_eq!(response, GridResponse::Handled);
        assert!(out.commands.is_empty());
        assert_eq!(cells(&grid, 2), Cells::new(0, 1, 1, 1));
        // A tap offers the options and takes the focus.
        pointer(&mut grid, &env, PointerPhase::Down, 150, 50);
        let (response, out) = pointer(&mut grid, &env, PointerPhase::Up, 150, 50);
        assert_eq!(response, GridResponse::Handled);
        assert!(offered(&out, 1));
        assert_eq!(grid.focus, 1);
        assert!(grid.focus_shown);
        // Off the items: not the grid's.
        let (response, _) = pointer(&mut grid, &env, PointerPhase::Down, 350, 150);
        assert_eq!(response, GridResponse::Ignored);
    }

    #[test]
    fn items_come_back_as_they_were_left() {
        let kit = Kit::new(true);
        let env = kit.env();
        let mut grid = edited(&env, &three());
        pointer(&mut grid, &env, PointerPhase::Down, 150, 50);
        pointer(&mut grid, &env, PointerPhase::Move, 350, 250);
        let items = changed(pointer(&mut grid, &env, PointerPhase::Up, 350, 250).0);
        assert_eq!(items, grid.items());
        let text = super::super::write(&items);
        assert_eq!(text, "tool:t.a 0/0 1x1, tool:t.b 3/2 1x1, tool:t.c 0/1 1x1");
        let again = laid_out(&env, &super::super::parse(&text), &[]);
        assert_eq!(again.items(), items);
    }

    #[test]
    fn the_screens_buttons_follow_the_items() {
        let kit = Kit::new(false);
        let env = kit.env();
        let mut grid = laid_out(&env, &three(), &[BUTTON]);
        // Before the first item: the button, the last target.
        act(&mut grid, &env, &actions::LEFT);
        assert_eq!(grid.focus, 3);
        assert_eq!(
            act(&mut grid, &env, &actions::CONFIRM).0,
            GridResponse::Button(7)
        );
        // Down from `a`: `c`, then the button below it.
        grid.reset_focus();
        act(&mut grid, &env, &actions::DOWN);
        act(&mut grid, &env, &actions::DOWN);
        assert_eq!(grid.focus, 3);
        // A tap on it, editing or not.
        assert_eq!(
            gesture(&mut grid, &env, GestureKind::Tap, 50, 330),
            GridResponse::Button(7)
        );
        grid.set_editing(true);
        grid.layout(&env, geometry(), &three(), &[BUTTON]);
        assert_eq!(
            gesture(&mut grid, &env, GestureKind::Tap, 50, 330),
            GridResponse::Button(7)
        );
        // While editing, after the items too.
        grid.reset_focus();
        act(&mut grid, &env, &actions::LEFT);
        assert_eq!(grid.focus, 3);
        assert_eq!(
            act(&mut grid, &env, &actions::CONFIRM).0,
            GridResponse::Button(7)
        );
    }

    #[test]
    fn a_finger_during_a_move_by_buttons_keeps_it() {
        // A touch screen with buttons (the Boox): `b` moved by the
        // buttons, then a finger taps the screen's button.
        let kit = Kit::new(true);
        let env = kit.env();
        let mut grid = edited(&env, &three());
        grid.layout(&env, geometry(), &three(), &[BUTTON]);
        pick(&mut grid, &env, 1, "move");
        act(&mut grid, &env, &actions::DOWN);
        assert_eq!(cells(&grid, 1), Cells::new(1, 1, 1, 1));
        // As the kernel goes: the finger coming down is not taken (a
        // taken one would never become a tap) ...
        let (down, _) = pointer(&mut grid, &env, PointerPhase::Down, 50, 330);
        assert_eq!(down.handled(), Handled::No);
        // ... so it is a tap when lifted: that ends moving, the place kept.
        let items = changed(gesture(&mut grid, &env, GestureKind::Tap, 50, 330));
        assert_eq!(items[1], at(tool("t.b"), 1, 1, 1, 1));
        assert!(grid.editing());
        // The next tap does what it does.
        pointer(&mut grid, &env, PointerPhase::Down, 50, 330);
        pointer(&mut grid, &env, PointerPhase::Up, 50, 330);
        assert_eq!(
            gesture(&mut grid, &env, GestureKind::Tap, 50, 330),
            GridResponse::Button(7)
        );
    }

    #[test]
    fn removed_items_are_gone() {
        let kit = Kit::new(false);
        let env = kit.env();
        let mut grid = edited(&env, &three());
        let items = changed(pick(&mut grid, &env, 1, "remove"));
        let all = three();
        assert_eq!(items, [all[0].clone(), all[2].clone()]);
        assert_eq!(grid.targets.len(), 2);
        // Options of an item not there are not taken.
        assert_eq!(pick(&mut grid, &env, 5, "remove"), GridResponse::Handled);
        assert_eq!(grid.items().len(), 2);
    }

    #[test]
    fn removing_an_item_ends_moving() {
        let kit = Kit::new(true);
        let env = kit.env();
        let mut grid = edited(&env, &three());
        assert_eq!(pick(&mut grid, &env, 0, "move"), GridResponse::Handled);
        // `b` removed while `a` is moved (its options answered late).
        changed(pick(&mut grid, &env, 1, "remove"));
        // Moving ended: the buttons move the focus, neither `a` nor `c`.
        let (a, c) = (cells(&grid, 0), cells(&grid, 1));
        let (response, _) = act(&mut grid, &env, &actions::RIGHT);
        assert_eq!(response, GridResponse::Handled);
        assert_eq!((cells(&grid, 0), cells(&grid, 1)), (a, c));
    }

    #[test]
    fn placements_are_tried_a_bounded_number_of_times() {
        let kit = |values: u32| {
            let mut kit = Kit::new(false);
            kit.settings
                .define(SettingSpec::switch(
                    "test.w.face",
                    "face",
                    false,
                    crate::builtin::groups::OTHER,
                ))
                .unwrap();
            for n in 1..=values {
                let id = format!("test.w@{n}.face");
                kit.settings.set(&id, SettingValue::Bool(true)).unwrap();
            }
            kit
        };
        // The first placement neither an item nor a value uses.
        let few = kit(1);
        let env = few.env();
        let widget = What::Widget("test.w@2".into());
        let grid = laid_out(&env, &[at(widget, 0, 0, 1, 1)], &[]);
        assert_eq!(grid.place(&env, "test.w"), "test.w@3");
        // Every placement tried is used: the widget's own id.
        let many = kit(256);
        let env = many.env();
        let grid = laid_out(&env, &[], &[]);
        assert_eq!(grid.place(&env, "test.w"), "test.w");
    }
}
