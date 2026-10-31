//! A to-do list: a few things to do, kept in `todo.txt` in the plugin's
//! folder (as todo.txt has them: an item a line, `x 2026-10-19 ` before
//! those done, a date before open ones that would read as done). The
//! widget shows the open ones with a box each; a tap on one ticks it
//! off: it shows struck through for the rest of the day (or goes at
//! once), then it goes. The list's own screen adds, ticks, edits,
//! moves and removes them.
//!
//! The list is written back only once it was read: a file that cannot be
//! read is left as it is, and the widget and the screen say so. Items
//! past the first [`MAX_ITEMS`] are not shown and kept as they are.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::time;
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetLook, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, FixedAction, FontHandle, Gesture, GestureKind, Handled,
    Id, Params, Point, Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec,
    SettingSpec, Storage, StorageError,
};

use super::{Changes, data, frame_height, today};

pub(super) const ID: Id = Id::new_static("widgets.todo");
/// Opens the list's own screen.
pub const OPEN: Id = Id::new_static("widgets.todo.open");
/// Items shown at most.
const COUNT: Setting<i64> = Setting::new("widgets.todo.count");
/// What becomes of those ticked off: one of [`DONE`].
const DONE_SETTING: Setting<Text> = Setting::new("widgets.todo.done");
/// Struck through for the rest of the day, or gone at once.
const DONE: [&str; 2] = ["widgets.todo.done.struck", "widgets.todo.done.hidden"];
/// The boxes before the items: one of [`BOXES`].
const BOX: Setting<Text> = Setting::new("widgets.todo.box");
const BOXES: [&str; 3] = [
    "widgets.todo.box.circle",
    "widgets.todo.box.square",
    "widgets.todo.box.none",
];
/// "To do" above the items.
const CAPTION: &str = "caption";
const SCREEN: Id = Id::new_static("widgets.todo.list");
const CHOSEN: Event<choice::Picked> = Event::new("widgets.todo.chosen");
/// An item tapped in the widget: its text ([`TEXT`]) and whether it was
/// done ([`WAS_DONE`]).
const TICKED: Event<Params> = Event::new("widgets.todo.ticked");
const TEXT: &str = "text";
const WAS_DONE: &str = "done";
/// A new item typed, an item's text changed (its index in [`INDEX`]).
const ADDED: Event<Typed> = Event::new("widgets.todo.added");
const EDITED: Event<Typed> = Event::new("widgets.todo.edited");
const INDEX: &str = "index";
const OPTION_LABELS: &str = "widgets.todo.option.";
/// Items shown and changed at most (those after them are kept as they
/// are), and their length.
const MAX_ITEMS: usize = 100;
const MAX_TEXT: usize = 200;

fn file() -> String {
    data("todo.txt")
}

/// The list's settings, kept in the program.
static SETTINGS: [SettingSpec; 8] = [
    SettingSpec::number(
        "widgets.todo.count",
        "widgets.todo.setting.count",
        1,
        12,
        5,
        ID,
    )
    .description("widgets.todo.count.help"),
    SettingSpec::choice(
        "widgets.todo.done",
        "widgets.todo.setting.done",
        &DONE,
        DONE[0],
        ID,
    )
    .description("widgets.todo.done.help"),
    SettingSpec::choice(
        "widgets.todo.box",
        "widgets.todo.setting.box",
        &BOXES,
        BOXES[0],
        ID,
    )
    .description("widgets.todo.box.help"),
    widgets::part_spec(
        "widgets.todo.caption",
        ID,
        "widgets.todo.setting.caption",
        true,
    ),
    widgets::size_spec("widgets.todo.size", ID, TextSize::Small, false),
    widgets::align_spec("widgets.todo.align", ID, Align::Left),
    widgets::frame_spec("widgets.todo.frame", ID, Frame::None),
    widgets::padding_spec("widgets.todo.padding", ID, Padding::Auto),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "widgets.todo")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ICONS: [(Id, &str); 1] = [(OPEN, "list-check")];

pub(super) fn register(r: &mut Registrar, changes: &Changes) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.todo");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    let changed = changes.clone();
    r.on(TICKED, move |env, params, out| {
        let Some(text) = params.get(TEXT) else {
            return;
        };
        let today = today(env);
        let Ok(mut list) = load(env.storage(), today) else {
            out.push(Command::Notify(
                env.i18n().tr("widgets.todo.unreadable").into(),
            ));
            return;
        };
        let was_done = params.get(WAS_DONE).is_some();
        if let Some(item) = list
            .items
            .iter_mut()
            .find(|i| i.text == text && i.done.is_some() == was_done)
        {
            item.done = if was_done { None } else { Some(today) };
            if save(env.storage(), &list).is_err() {
                out.push(Command::Notify(
                    env.i18n().tr("widgets.todo.not-saved").into(),
                ));
            }
        }
        changed.changed(out, &ID);
    })?;
    r.add_screen(ScreenSpec::new(
        SCREEN,
        "widgets.todo",
        Box::new({
            let changes = changes.clone();
            move |env, _| Box::new(TodoScreen::new(env, changes.clone()))
        }),
    ))?;
    r.add_actions(&ACTIONS)?;
    r.add_icons(&ICONS)?;
    r.add_widget(
        WidgetSpec::new(ID, "widgets.todo", {
            let changes = changes.clone();
            move |_| {
                Box::new(TodoWidget {
                    changes: changes.clone(),
                    ..TodoWidget::default()
                })
            }
        })
        .category(widgets::categories::PERSONAL),
    )
}

// ---- The list -----------------------------------------------------------

/// A thing to do, and the day it was done (days since 1970).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    text: String,
    done: Option<i64>,
}

/// `2026-10-19` of a day since 1970.
fn date_of(day: i64) -> String {
    let (year, month, day) = time::civil_from_days(day);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The day since 1970 of `2026-10-19`.
fn day_of(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day))
        .then(|| time::days_from_civil(year, month, day))
}

/// The list as kept: its first items, and the lines after them as they
/// are, written back after them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct List {
    items: Vec<Item>,
    rest: String,
}

/// The items of a list as kept, but those done before `today`; past
/// [`MAX_ITEMS`], the rest of the lines as they are.
fn parse(text: &str, today: i64) -> List {
    let mut items = Vec::new();
    let mut lines = text.lines();
    while items.len() < MAX_ITEMS {
        let Some(line) = lines.next() else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let item = match line.strip_prefix("x ") {
            Some(rest) => {
                let rest = rest.trim_start();
                let (date, text) = rest.split_once(' ').unwrap_or((rest, ""));
                match day_of(date) {
                    Some(day) => Item {
                        text: String::from(text.trim()),
                        done: Some(day),
                    },
                    // Done, but when is not said: long ago.
                    None => Item {
                        text: String::from(rest),
                        done: Some(i64::MIN),
                    },
                }
            }
            // An open item kept with a date in front (see `dated`): the
            // date dropped.
            None => match line.split_once(' ') {
                Some((date, text)) if dated(line) && day_of(date).is_some() => Item {
                    text: String::from(text.trim_start()),
                    done: None,
                },
                _ => Item {
                    text: String::from(line),
                    done: None,
                },
            },
        };
        let gone = item.done.is_some_and(|day| day < today);
        if !gone && !item.text.is_empty() {
            items.push(item);
        }
    }
    let mut rest = String::new();
    for line in lines {
        rest.push_str(line);
        rest.push('\n');
    }
    List { items, rest }
}

/// The date before an open item whose text would read as one done.
const MADE: &str = "1970-01-01";

/// Whether an open item's text would read as one done ("x ray at 3"),
/// also once dates before it are dropped: such an item is kept with a
/// date in front ([`MADE`]), where todo.txt has the day an item was made,
/// so that it reads as open there and here, and loses nothing.
fn dated(text: &str) -> bool {
    let mut rest = text;
    loop {
        if rest.starts_with("x ") {
            return true;
        }
        match rest.split_once(' ') {
            Some((word, after)) if day_of(word).is_some() => rest = after.trim_start(),
            _ => return false,
        }
    }
}

/// The list as kept: an item a line, those done after `x` and the day,
/// then the lines after the first items as they were.
fn format(list: &List) -> String {
    let mut text = String::new();
    for item in &list.items {
        let line = item.text.replace(['\n', '\r'], " ");
        match item.done {
            Some(day) => text.push_str(&format!("x {} {line}\n", date_of(day))),
            None if dated(&line) => text.push_str(&format!("{MADE} {line}\n")),
            None => text.push_str(&format!("{line}\n")),
        }
    }
    text.push_str(&list.rest);
    text
}

/// The list kept, empty where there is none; an error where it is there
/// but cannot be read (too large, not text, the card failing), so that
/// it is not written over.
fn load(storage: &dyn Storage, today: i64) -> Result<List, StorageError> {
    let text = paperpaw_sdk::files::load_text_for_update(storage, &file(), 32 * 1024)?;
    Ok(text.map_or_else(List::default, |text: String| parse(&text, today)))
}

fn save(storage: &dyn Storage, list: &List) -> Result<(), StorageError> {
    storage.write(&file(), format(list).as_bytes())
}

/// `text` as an item's: one line, not too long.
fn cleaned(text: &str) -> String {
    let text = text.replace(['\n', '\r', '\t'], " ");
    text.trim().chars().take(MAX_TEXT).collect()
}

// ---- The widget ---------------------------------------------------------

/// The list's first items, read when updated.
#[derive(Default)]
struct TodoWidget {
    /// The items shown.
    items: Vec<Item>,
    /// Open ones in all.
    open: usize,
    /// Items not shown for want of room or count.
    more: usize,
    /// The day and the changes it was read at.
    read: Option<(i64, u32)>,
    /// The list could not be read.
    unreadable: bool,
    changes: Changes,
}

impl Widget for TodoWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        // Read each minute (a new day lets those done go), but read
        // anew only then or after a change.
        let day = today(env);
        let read = Some((day, self.changes.count()));
        if read == self.read {
            return false;
        }
        self.read = read;
        let hide = env.settings().read(&DONE_SETTING) == DONE[1];
        let loaded = load(env.storage(), day);
        let unreadable = loaded.is_err();
        let all: Vec<Item> = loaded
            .unwrap_or_default()
            .items
            .into_iter()
            .filter(|i| !(hide && i.done.is_some()))
            .collect();
        let count = env.settings().read(&COUNT).clamp(1, 12) as usize;
        let open = all.iter().filter(|i| i.done.is_none()).count();
        let more = all.len().saturating_sub(count);
        let items: Vec<Item> = all.into_iter().take(count).collect();
        let changed = items != self.items
            || open != self.open
            || more != self.more
            || unreadable != self.unreadable;
        (self.items, self.open, self.more) = (items, open, more);
        self.unreadable = unreadable;
        changed
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let rows = self.items.len().max(1) as u16 + u16::from(self.more > 0);
        let caption = if widgets::shows(env, ID.as_str(), CAPTION) {
            widgets::caption_height(env)
        } else {
            0
        };
        caption + row_height(env) * rows + frame_height(env, ID.as_str())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return;
        };
        let mut ui = ui::Ui::new(canvas, env, &style);
        let align = widgets::align(env, ID.as_str());
        if self.unreadable {
            let text = env.i18n().tr("widgets.todo.unreadable");
            super::draw_empty(ui.canvas(), env, &look, text);
            return;
        }
        if self.items.is_empty() {
            // The whole place to say how to add some, small.
            let key = if env.device().input.touch {
                "widgets.todo.none.touch"
            } else {
                "widgets.todo.none"
            };
            let text = env.i18n().tr(key);
            super::draw_empty(ui.canvas(), env, &look, text);
            return;
        }
        let places = places(env, look.area, self);
        if places.caption.is_some() {
            let text = caption_text(env, self.open);
            widgets::draw_caption(ui.canvas(), env, ID.as_str(), &look, &text);
        }
        let boxes = env.settings().read(&BOX);
        let size = items_size(env, &font, &places.rows, &self.items, boxes);
        for (row, item) in places.rows.iter().zip(&self.items) {
            draw_item(&mut ui, size, *row, item, boxes, align, &look);
        }
        if let Some((row, more)) = places.more {
            let text = env
                .i18n()
                .format("widgets.todo.more", &[("count", Arg::Int(more as i64))]);
            let text = ui::fit(&font, style.small(), &text, f32::from(row.width));
            let x = align.x(row, font.measure(style.small(), &text));
            ui::Label::new(&text)
                .px(style.small())
                .color(look.faint)
                .draw_from(&mut ui, x, row);
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let look = widgets::widget_look(env, ID.as_str(), area);
        let places = places(env, look.area, self);
        let open = Command::OpenScreen(SCREEN, Params::new());
        if self.items.is_empty() {
            return alloc::vec![WidgetTarget::new(area, open)];
        }
        let mut targets = Vec::new();
        if let Some(caption) = places.caption {
            targets.push(WidgetTarget::new(caption, open.clone()));
        }
        for (row, item) in places.rows.iter().zip(&self.items) {
            let mut params = Params::new().with(TEXT, item.text.clone());
            if item.done.is_some() {
                params.set(WAS_DONE, "1");
            }
            targets.push(WidgetTarget::new(*row, TICKED.emit(params)));
        }
        if let Some((row, _)) = places.more {
            targets.push(WidgetTarget::new(row, open));
        }
        targets
    }
}

/// "To do", or "To do · 3" with open items.
fn caption_text(env: &Env<'_>, open: usize) -> String {
    let title = env.i18n().tr("widgets.todo");
    if open == 0 {
        String::from(title)
    } else {
        format!("{title} · {open}")
    }
}

/// The height of a row of the widget: its line with room around, at
/// least as high as a finger needs on touch screens.
fn row_height(env: &Env<'_>) -> u16 {
    let line = widgets::line_height(env, text_size(env));
    let least = if env.device().input.touch {
        env.metrics().mm(6) as u16
    } else {
        0
    };
    (line + line / 3).max(least)
}

/// The size of the items' text: a list is denser than a widget's one
/// line, so a step smaller than [`TextSize::text`] (small is the size of
/// notes, medium the body's).
fn text_size(env: &Env<'_>) -> f32 {
    let style = UiStyle::new(env);
    match widgets::size(env, ID.as_str()) {
        TextSize::Small => style.small(),
        TextSize::Medium | TextSize::Fit => style.text(),
        TextSize::Large => style.title(),
        _ => style.text(),
    }
}

/// Where the widget's parts go in `area` (inside its frame).
struct Places {
    caption: Option<Rect>,
    rows: Vec<Rect>,
    /// "3 more", where not all are shown, and how many.
    more: Option<(Rect, usize)>,
}

fn places(env: &Env<'_>, area: Rect, widget: &TodoWidget) -> Places {
    let mut rest = area;
    let mut caption = None;
    let row = row_height(env).max(1);
    // The caption where an item fits below it.
    let height = widgets::caption_height(env);
    if widgets::shows(env, ID.as_str(), CAPTION) && rest.height >= height + row {
        caption = Some(Rect::new(rest.x, rest.y, rest.width, height));
        rest = Rect::new(rest.x, rest.y + height, rest.width, rest.height - height);
    }
    let fits = usize::from(rest.height / row).max(1);
    let row = row.min(rest.height.max(1));
    let mut shown = widget.items.len().min(fits);
    let mut more = widget.more;
    // Not all fit: the last row says how many more there are (where
    // two items show at least).
    if shown < widget.items.len() && shown > 2 {
        more += widget.items.len() - shown + 1;
        shown -= 1;
    }
    let rows: Vec<Rect> = (0..shown)
        .map(|n| Rect::new(rest.x, rest.y + row * n as u16, rest.width, row))
        .collect();
    let more = (more > 0 && rows.len() < fits).then(|| {
        let place = Rect::new(rest.x, rest.y + row * rows.len() as u16, rest.width, row);
        (place, more)
    });
    Places {
        caption,
        rows,
        more,
    }
}

/// Draws an item in `row`: its box, ticked if done, and its text, struck
/// through if done.
fn draw_item(
    ui: &mut ui::Ui<'_>,
    size: f32,
    row: Rect,
    item: &Item,
    boxes: &str,
    align: Align,
    look: &WidgetLook,
) {
    let env = ui.env();
    let Some(font) = ui.font() else {
        return;
    };
    let font = &font;
    let style = UiStyle::new(env);
    let done = item.done.is_some();
    let side = (size * 0.8) as u16;
    let with_box = boxes != BOXES[2];
    let lead = lead(size, boxes);
    let room = f32::from(row.width.saturating_sub(lead));
    let text = ui::fit(font, size, &item.text, room);
    let width = font.measure(size, &text);
    let x = align.x(row, f32::from(lead) + width) as u16;
    let color = if done { look.faint } else { look.ink };
    if with_box {
        let mid = row.y + row.height / 2;
        let place = Rect::new(x, mid - side / 2, side, side);
        draw_box(ui.canvas(), env, place, boxes == BOXES[1], done, look);
    }
    let label = Rect::new(x + lead, row.y, row.width.saturating_sub(lead), row.height);
    ui::Label::new(&text)
        .px(size)
        .color(color)
        .draw_from(ui, f32::from(label.x), label);
    if done {
        // A line through the middle of the small letters.
        let metrics = font.line_metrics(size);
        let baseline =
            f32::from(row.y) + (f32::from(row.height) + metrics.ascent - metrics.descent) / 2.0;
        let y = (baseline - size * 0.27) as u16;
        let line = u32::from(style.line()).max(1) as u16;
        ui.canvas()
            .fill(Rect::new(label.x, y, width as u16, line), color);
    }
}

/// How far an item's text begins from its row's edge at `size`: the
/// box and a space after it, where there are boxes.
fn lead(size: f32, boxes: &str) -> u16 {
    if boxes == BOXES[2] {
        0
    } else {
        (size * 0.8) as u16 + (size * 0.55) as u16
    }
}

/// The size of the items in `rows`: the list's own, or smaller (down to
/// the small size) where an item would be shortened at it, the same for
/// all so that they line up.
fn items_size(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    rows: &[Rect],
    items: &[Item],
    boxes: &str,
) -> f32 {
    let size = text_size(env);
    let least = UiStyle::new(env).small().min(size);
    rows.iter()
        .zip(items)
        .map(|(row, item)| {
            let room = f32::from(row.width.saturating_sub(lead(size, boxes)));
            widgets::fit_line(font, size, least, &item.text, room).0
        })
        .fold(size, f32::min)
}

/// A box in `place`: round or square, filled and ticked when done.
fn draw_box(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    place: Rect,
    square: bool,
    done: bool,
    look: &WidgetLook,
) {
    let style = UiStyle::new(env);
    let line = (u32::from(place.width) / 10)
        .max(u32::from(style.line()))
        .max(1);
    let r = u32::from(place.width / 2);
    let center = Point::new(i32::from(place.x) + r as i32, i32::from(place.y) + r as i32);
    match (square, done) {
        (false, false) => canvas.stroke_circle(center, r, line, look.ink),
        (false, true) => canvas.fill_circle(center, r, look.faint),
        (true, false) => canvas.stroke_round(place, line, r / 3, look.ink),
        (true, true) => canvas.fill_round(place, r / 3, look.faint),
    }
    if done {
        // A tick in the paper's colour.
        let s = place.width as i32;
        let (x, y) = (i32::from(place.x), i32::from(place.y));
        let width = (s as u32 / 9).max(2);
        let a = Point::new(x + s * 28 / 100, y + s * 50 / 100);
        let b = Point::new(x + s * 44 / 100, y + s * 66 / 100);
        let c = Point::new(x + s * 72 / 100, y + s * 36 / 100);
        canvas.draw_line(a, b, width, look.paper);
        canvas.draw_line(b, c, width, look.paper);
    }
}

// ---- The list's screen --------------------------------------------------

/// The list: *Add* first, then the items; a tap ticks one off or on,
/// holding it (or the menu button) offers to edit or remove it.
struct TodoScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    items: Vec<Item>,
    /// The lines after the items, kept as they are.
    rest: String,
    /// The list could not be read: it is shown empty and not changed.
    unreadable: bool,
    changes: Changes,
}

impl TodoScreen {
    fn new(env: &Env<'_>, changes: Changes) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let loaded = load(env.storage(), today(env));
        let unreadable = loaded.is_err();
        let List { items, rest } = loaded.unwrap_or_default();
        Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            style,
            header,
            items,
            rest,
            unreadable,
            changes,
        }
    }

    fn len(&self) -> usize {
        self.items.len() + 1
    }

    fn changed(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let list = List {
            items: core::mem::take(&mut self.items),
            rest: core::mem::take(&mut self.rest),
        };
        if save(env.storage(), &list).is_err() {
            cx.command(Command::Notify(
                env.i18n().tr("widgets.todo.not-saved").into(),
            ));
        }
        (self.items, self.rest) = (list.items, list.rest);
        let mut out = paperpaw_sdk::Commands::new();
        self.changes.changed(&mut out, &ID);
        for command in out {
            cx.command(command);
        }
        self.list.set_focus(self.list.focus().min(self.len() - 1));
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Row `row` chosen: *Add* asks for an item, an item is ticked off or
    /// on.
    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.unreadable {
            cx.command(Command::Notify(
                env.i18n().tr("widgets.todo.unreadable").into(),
            ));
            return;
        }
        if row == 0 {
            cx.command(ask::text(env.i18n().tr("widgets.todo.add"), &ADDED).command());
            return;
        }
        let today = today(&env);
        if let Some(item) = self.items.get_mut(row - 1) {
            item.done = match item.done {
                Some(_) => None,
                None => Some(today),
            };
            self.changed(cx);
        }
    }

    /// What can be done with item `row`.
    fn offer(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let Some(item) = row.checked_sub(1).and_then(|i| self.items.get(i)) else {
            return;
        };
        let tick = if item.done.is_some() {
            "untick"
        } else {
            "tick"
        };
        cx.command(
            paperpaw_sdk::ask::choice(&item.text, &CHOSEN)
                .about(format!("{}", row - 1))
                .options(OPTION_LABELS, &[tick, "edit", "up", "down", "remove"])
                .command(),
        );
    }
}

impl Screen for TodoScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("widgets.todo"),
            None,
            self.list.paging(len),
        );
        if ui.font().is_none() {
            return;
        }
        let items = &self.items;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let (ink, paper) = (style.focus_ink(focused), style.focus_paper(focused));
            let margin = style.margin();
            let inner = Rect::new(
                rect.x + margin,
                rect.y,
                rect.width.saturating_sub(2 * margin),
                rect.height,
            );
            let look = WidgetLook::new(
                inner,
                ink,
                if focused && style.dark_focus() {
                    Color::gray(200)
                } else {
                    style.faint(ui::Ink::MUTED)
                },
                paper,
                widgets::Frame::None,
            );
            if row == 0 {
                let icon = style.text() * 1.1;
                let width = ui.icon("plus", icon, f32::from(inner.x), inner, ink);
                let x = f32::from(inner.x)
                    + if width > 0.0 {
                        width + style.gap() as f32
                    } else {
                        0.0
                    };
                let text = env.i18n().tr("widgets.todo.add");
                ui::Label::new(text)
                    .px(style.text())
                    .color(ink)
                    .draw_from(ui, x, inner);
                return;
            }
            if let Some(item) = items.get(row - 1) {
                let boxes = BOXES[0];
                draw_item(ui, style.text(), inner, item, boxes, Align::Left, &look);
            }
        });
        if self.items.is_empty() {
            let key = if self.unreadable {
                "widgets.todo.unreadable"
            } else {
                "widgets.todo.empty"
            };
            let row = self.list.row_rect(1);
            let area = Rect::new(
                row.x + style.margin(),
                row.y,
                row.width.saturating_sub(2 * style.margin()),
                style.row() * 2,
            );
            let look = WidgetLook::new(
                area,
                style.faint(ui::Ink::MUTED),
                style.faint(ui::Ink::MUTED),
                Color::WHITE,
                widgets::Frame::None,
            );
            widgets::draw_message(ui.canvas(), env, &look, env.i18n().tr(key));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(action, rows) {
            delta
        } else if *action == actions::CONFIRM {
            self.open(self.list.focus(), cx);
            return Handled::Yes;
        } else if *action == actions::MENU {
            self.offer(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        let change = self.list.move_focus(delta, len);
        change.request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.len();
        if gesture.kind == GestureKind::Tap
            && let Some(action) =
                self.header
                    .tap(&env, &self.style, gesture.start, self.list.paging(len))
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(row) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(row);
        match gesture.kind {
            GestureKind::Tap => self.open(row, cx),
            GestureKind::LongPress => self.offer(row, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(typed) = ADDED.read(event, params) {
            let text = cleaned(&typed.text);
            if text.is_empty() || self.unreadable {
                return;
            }
            if self.items.len() >= MAX_ITEMS {
                let full = env.i18n().format(
                    "widgets.todo.full",
                    &[("count", Arg::Int(MAX_ITEMS as i64))],
                );
                cx.command(Command::Notify(full));
                return;
            }
            self.items.push(Item { text, done: None });
            // The new item in sight.
            self.list.set_focus(self.items.len());
            self.changed(cx);
        } else if let Some(typed) = EDITED.read(event, params) {
            let index = typed
                .context
                .get(INDEX)
                .and_then(|i| i.parse::<usize>().ok());
            let text = cleaned(&typed.text);
            if let Some(item) = index.and_then(|i| self.items.get_mut(i))
                && !text.is_empty()
            {
                item.text = text;
                self.changed(cx);
            }
        } else if let Some(picked) = CHOSEN.read(event, params) {
            let Some(index) = picked
                .path
                .parse::<usize>()
                .ok()
                .filter(|i| *i < self.items.len())
            else {
                return;
            };
            match picked.option.as_str() {
                "tick" | "untick" => self.open(index + 1, cx),
                "edit" => {
                    let question = ask::text(env.i18n().tr("widgets.todo.edit"), &EDITED)
                        .initial(self.items[index].text.clone())
                        .with(INDEX, index.to_string());
                    cx.command(question.command());
                }
                "up" if index > 0 => {
                    self.items.swap(index, index - 1);
                    self.list.set_focus(index);
                    self.changed(cx);
                }
                "down" if index + 1 < self.items.len() => {
                    self.items.swap(index, index + 1);
                    self.list.set_focus(index + 2);
                    self.changed(cx);
                }
                "remove" => {
                    self.items.remove(index);
                    self.changed(cx);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn its_settings_are_those_read() {
        let ids: alloc::vec::Vec<&str> = super::SETTINGS.iter().map(|s| s.id.as_str()).collect();
        for read in [
            super::COUNT.as_str(),
            super::DONE_SETTING.as_str(),
            super::BOX.as_str(),
            "widgets.todo.caption",
        ] {
            assert!(ids.contains(&read), "{read}");
        }
        for look in ["size", "align", "frame"] {
            let id = alloc::format!("{}.{look}", super::ID);
            assert!(ids.contains(&id.as_str()), "{id}");
        }
        assert!(super::SETTINGS.iter().all(|s| s.group == super::ID));
    }
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn days_and_dates_agree() {
        assert_eq!(time::days_from_civil(1970, 1, 1), 0);
        assert_eq!(time::days_from_civil(2000, 3, 1), 11_017);
        let day = time::days_from_civil(2026, 10, 19);
        assert_eq!(date_of(day), "2026-10-19");
        assert_eq!(day_of("2026-10-19"), Some(day));
        assert_eq!(
            day_of("2024-02-29").map(date_of).as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(day_of("2026-13-01"), None);
        assert_eq!(day_of("yesterday"), None);
    }

    #[test]
    fn the_list_is_kept() {
        let storage = MemoryStorage::new();
        let today = time::days_from_civil(2026, 10, 19);
        let items = [
            Item {
                text: String::from("Return the library book"),
                done: None,
            },
            Item {
                text: String::from("Call Anna\nback"),
                done: Some(today),
            },
        ];
        let list = List {
            items: items.to_vec(),
            rest: String::new(),
        };
        save(&storage, &list).unwrap();
        let text = String::from_utf8(paperpaw_sdk::read_file(&storage, &file(), 1024).unwrap());
        assert_eq!(
            text.unwrap(),
            "Return the library book\nx 2026-10-19 Call Anna back\n"
        );
        let loaded = load(&storage, today).unwrap().items;
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], items[0]);
        assert_eq!(loaded[1].text, "Call Anna back");
        assert_eq!(loaded[1].done, Some(today));
        // The next day, what was done is gone.
        assert_eq!(
            load(&storage, today + 1).unwrap().items,
            alloc::vec![items[0].clone()]
        );
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn the_list_of_version_1_is_read() {
        let today = time::days_from_civil(2026, 10, 19);
        let items = parse(include_str!("../../testdata/todo.txt.v1"), today).items;
        let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["Buy bread", "Water the plants", "Read chapter 3"]);
        assert_eq!(items[1].done, Some(today));
        assert_eq!(items[0].done, None);
    }

    /// A list longer than is shown keeps its items past the first
    /// hundred when it is changed.
    #[test]
    fn a_long_list_keeps_what_is_not_shown() {
        let storage = MemoryStorage::new();
        let text: String = (1..=130).map(|n| format!("Item {n}\n")).collect();
        storage.write(&file(), text.as_bytes()).unwrap();
        let mut list = load(&storage, 0).unwrap();
        assert_eq!(list.items.len(), MAX_ITEMS);
        list.items[0].text = String::from("First");
        save(&storage, &list).unwrap();
        let kept = String::from_utf8(paperpaw_sdk::read_file(&storage, &file(), 4096).unwrap());
        let kept = kept.unwrap();
        assert_eq!(kept.lines().count(), 130);
        assert!(kept.starts_with("First\nItem 2\n"), "{kept}");
        assert!(kept.ends_with("Item 129\nItem 130\n"), "{kept}");
    }

    /// A list that is there but cannot be read is an error, not an empty
    /// list that the next change would write over it.
    #[test]
    fn an_unreadable_list_is_not_empty() {
        let storage = MemoryStorage::new();
        assert_eq!(load(&storage, 0), Ok(List::default()));
        // Saved by another program in another encoding.
        storage.write(&file(), b"Br\xf6tchen holen\n").unwrap();
        assert!(load(&storage, 0).is_err());
        // Larger than is read.
        storage.write(&file(), &[b'a'; 40 * 1024]).unwrap();
        assert!(load(&storage, 0).is_err());
        // Saved by a program that marks UTF-8 at the start: read.
        storage.write(&file(), b"\xef\xbb\xbfBuy bread\n").unwrap();
        let items = load(&storage, 0).unwrap().items;
        assert_eq!(items[0].text, "Buy bread");
    }

    #[test]
    fn items_are_cleaned() {
        assert_eq!(cleaned("  a\tb\nc  "), "a b c");
        assert_eq!(cleaned(&"x".repeat(300)).len(), MAX_TEXT);
    }
}

#[cfg(test)]
mod roundtrip_tests;
