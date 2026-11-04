//! "Library in numbers": the books, those read, being read and not read
//! yet, as large numbers in a row with small names below, and a bar of
//! the share read.
//!
//! The books are counted in the catalog (or the library folder); which
//! are read the details of the books opened lately tell (a hundred at
//! most, as the library remembers them), read again only for the book
//! being read and for books not seen before.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::library;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetLook, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{Canvas, Color, Command, Env, FontHandle, Id, Rect, RegisterError, Registrar};

use super::{Generation, add_settings, catalog_stamp, each_book, frame_height, path_hash};
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.library";
/// The numbers that can show, in order: each a part and the key of its
/// name.
const PARTS: [&str; 4] = ["books", "finished", "reading", "unread"];
/// The names below the numbers.
const LABELS: [&str; 4] = [
    "widgets.library.books",
    "widgets.library.finished",
    "widgets.library.reading",
    "widgets.library.unread",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 15] = [
    widgets::align_spec("widgets.library.align", ID, Align::Center),
    widgets::frame_spec("widgets.library.frame", ID, Frame::None),
    widgets::padding_spec("widgets.library.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.library.paper", ID),
    widgets::ink_spec("widgets.library.ink", ID),
    widgets::corners_spec("widgets.library.corners", ID),
    widgets::frame_line_spec("widgets.library.frame-line", ID),
    widgets::shadow_spec("widgets.library.drop-shadow", ID),
    widgets::size_spec("widgets.library.size", ID, TextSize::Fit, true),
    widgets::part_spec(
        "widgets.library.books",
        ID,
        "widgets.books.part.books",
        true,
    ),
    widgets::part_spec(
        "widgets.library.finished",
        ID,
        "widgets.books.part.finished",
        true,
    ),
    widgets::part_spec(
        "widgets.library.reading",
        ID,
        "widgets.books.part.reading",
        true,
    ),
    widgets::part_spec(
        "widgets.library.unread",
        ID,
        "widgets.books.part.unread",
        true,
    ),
    widgets::part_spec("widgets.library.bar", ID, "widgets.books.part.bar", true),
    widgets::part_spec(
        "widgets.library.caption",
        ID,
        "widgets.books.part.caption",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar, generation: &Generation) -> Result<(), RegisterError> {
    add_settings(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.library");
            &SPEC
        },
        &SETTINGS,
    )?;
    let kept: Kept = Rc::default();
    let generation = generation.clone();
    r.add_widget(
        WidgetSpec::new(ID, "widgets.library", move |_| {
            Box::new(Library {
                kept: kept.clone(),
                generation: generation.clone(),
                numbers: None,
                area: None,
                laid: None,
            })
        })
        .category(widgets::categories::BOOKS),
    )
}

/// The library counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Numbers {
    pub books: usize,
    pub read: usize,
    pub reading: usize,
    pub unread: usize,
}

impl Numbers {
    /// `books` books, of which those opened are as far as `progress`
    /// says (percent, `None` not begun).
    pub(crate) fn count(books: usize, progress: impl Iterator<Item = Option<u8>>) -> Numbers {
        let (mut read, mut reading) = (0, 0);
        for percent in progress {
            match percent.unwrap_or(0) {
                100.. => read += 1,
                0 => {}
                _ => reading += 1,
            }
        }
        // Books opened outside the library still count as books.
        let books = books.max(read + reading);
        Numbers {
            books,
            read,
            reading,
            unread: books - read - reading,
        }
    }

    /// The number of part `name` ([`PARTS`]).
    pub(crate) fn of(&self, name: &str) -> usize {
        match name {
            "books" => self.books,
            "finished" => self.read,
            "reading" => self.reading,
            _ => self.unread,
        }
    }
}

/// What was counted: for which generation and catalog the books, and the
/// progress of the books opened (by their path's hash).
#[derive(Debug, Default)]
struct Counted {
    key: (u32, u64),
    books: usize,
    progress: Vec<(u64, Option<u8>)>,
}

type Kept = Rc<RefCell<Option<Counted>>>;

struct Library {
    kept: Kept,
    generation: Generation,
    numbers: Option<Numbers>,
    /// Where it is drawn, once placed, and the numbers laid out there.
    area: Option<Rect>,
    laid: Option<Laid>,
}

impl Library {
    /// The numbers laid out where it is placed.
    fn lay_out(&mut self, env: &Env<'_>) {
        self.laid = match (self.area, &self.numbers) {
            (Some(area), Some(numbers)) => {
                let look = widgets::widget_look(env, WIDGET, area);
                lay_out(env, super::below_caption(env, WIDGET, &look), numbers)
            }
            _ => None,
        };
    }

    fn count(&self, env: &Env<'_>) -> Numbers {
        let key = (self.generation.get(), catalog_stamp(env));
        let mut kept = self.kept.borrow_mut();
        if kept.as_ref().is_none_or(|counted| counted.key != key) {
            let mut books = 0;
            each_book(env, &mut |_| books += 1);
            *kept = Some(Counted {
                key,
                books,
                progress: Vec::new(),
            });
        }
        let counted = kept.get_or_insert_with(Counted::default);
        let opened = library::opened_books(env.storage());
        let mut progress = Vec::with_capacity(opened.len());
        for (i, path) in opened.iter().enumerate() {
            let hash = path_hash(path);
            let known = counted.progress.iter().find(|(h, _)| *h == hash);
            // The book being read anew, the others as known.
            let percent = match known {
                Some((_, percent)) if i > 0 => Some(*percent),
                _ => match library::book(env.storage(), path) {
                    Some(book) => Some(book.progress),
                    None if env.storage().exists(path) => Some(None),
                    None => None,
                },
            };
            if let Some(percent) = percent {
                progress.push((hash, percent));
            }
        }
        counted.progress = progress;
        Numbers::count(counted.books, counted.progress.iter().map(|(_, p)| *p))
    }
}

impl Widget for Library {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let numbers = Some(self.count(env));
        let changed = numbers != self.numbers;
        self.numbers = numbers;
        if changed {
            self.lay_out(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.lay_out(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let big = widgets::line_height(env, style.title() * 2.0);
        big + widgets::line_height(env, style.small()) * 2 + style.gap() + frame_height(env, WIDGET)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        let Some(numbers) = &self.numbers else {
            return;
        };
        let caption = env.i18n().tr("widgets.library.caption");
        let rest = super::caption(canvas, env, WIDGET, &look, caption);
        // Laid out when placed; drawn elsewhere, laid out for that.
        let laid = match self.laid {
            Some(laid) if self.area == Some(area) => Some(laid),
            _ => lay_out(env, rest, numbers),
        };
        if let Some(laid) = laid {
            draw(canvas, env, rest, &look, numbers, &laid);
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget::new(area, Command::run(actions::LIBRARY))]
    }
}

/// How the numbers are laid out in a place: the first `shown` of them,
/// in `columns`, the numbers `size` large, with the bar or not; or, where
/// no number fits above its name, each beside its name on a line.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Laid {
    shown: usize,
    columns: u16,
    size: f32,
    bar: bool,
    /// The numbers beside their names, small, on one line.
    beside: bool,
}

/// The parts shown and their names.
fn parts<'e>(env: &Env<'e>) -> Vec<(&'static str, &'e str)> {
    PARTS
        .iter()
        .zip(LABELS)
        .filter(|(p, _)| widgets::shows(env, WIDGET, p))
        .map(|(p, label)| (*p, env.i18n().tr(label)))
        .collect()
}

/// The height of the digits' ink at `size`, for a font whose digits are
/// `digits` of its ascent high.
fn digits_height(font: &FontHandle<'_>, size: f32, digits: f32) -> u16 {
    (font.line_metrics(size).ascent * digits + 0.99) as u16
}

/// The digits' ink height for the ascent, and the names' ink (its top
/// above the baseline and its height), measured once.
fn inks(font: &FontHandle<'_>, style: &UiStyle, labels: &str) -> (f32, (i32, u16)) {
    let ascent = font.line_metrics(style.text()).ascent.max(1.0);
    let digits = font
        .ink(style.text(), "0123456789")
        .map_or(0.72, |(top, bottom)| (bottom - top) as f32 / ascent);
    let names = font.ink(style.small(), labels).map_or(
        (-(style.small() as i32), style.small() as u16),
        |(top, bottom)| (top, (bottom - top) as u16),
    );
    (digits, names)
}

/// The numbers laid out in `area`: all of them where they fit with their
/// names whole, else fewer (the last left out first); as many in a row
/// as makes them largest, the bar kept where it costs little.
fn lay_out(env: &Env<'_>, area: Rect, numbers: &Numbers) -> Option<Laid> {
    let style = UiStyle::new(env);
    // Numbers drawn large alone: at a pack's large sizes for digits too.
    let font = style.font(env)?.display();
    let id = WIDGET;
    let gap = style.gap();
    let parts = parts(env);
    let count = parts.len();
    if count == 0 {
        return None;
    }
    let labels: String = parts.iter().map(|(_, l)| *l).collect();
    let (digits, (_, names_h)) = inks(&font, &style, &labels);
    let bar_room = (style.line() * 3).max(4) + gap;
    let wanted_bar = widgets::shows(env, id, "bar") && numbers.books > 0;
    let max = match widgets::size(env, id) {
        // As large as the place holds, its height checked below.
        TextSize::Fit => f32::from(area.height).max(style.small()),
        other => other.text(&style) * 1.4,
    };
    let mut best: Option<(f32, Laid)> = None;
    let shown = count;
    let widest_label = parts
        .iter()
        .map(|(_, l)| font.measure(style.small(), l))
        .fold(0.0, f32::max);
    let widest = parts
        .iter()
        .map(|(p, _)| numbers.of(p).to_string())
        .max_by_key(String::len)
        .unwrap_or_default();
    let mut columns_tried = Vec::with_capacity(3);
    for columns in [shown, shown.div_ceil(2), 1] {
        let columns = columns as u16;
        if columns_tried.contains(&columns) {
            continue;
        }
        columns_tried.push(columns);
        let column = area.width / columns;
        // The names whole, a little room between neighbours, else
        // shortened: that counts against it, the more the shorter.
        let between = if columns > 1 { gap / 2 } else { 0 };
        let room = f32::from(column.saturating_sub(between));
        let names = if widest_label <= room {
            1.0
        } else {
            0.5 * room / widest_label.max(1.0)
        };
        let rows = (shown as u16).div_ceil(columns);
        for bar in [wanted_bar, false] {
            let fixed = if bar { bar_room } else { 0 } + gap * (rows - 1);
            let row = area.height.saturating_sub(fixed) / rows;
            let size = widgets::largest_size(max, style.small(), |s| {
                // A pixel or two to spare: the ink is measured from the
                // ascent here, as drawn there.
                let fits = |height: u16| height + gap / 2 + names_h + 2 <= row;
                // The estimate first; where it fits, the ink as drawn (the
                // digits' ink does not grow quite with the size).
                fits(digits_height(&font, s, digits))
                    && font.measure(s, &widest) <= f32::from(column) * 0.8
                    && font
                        .ink(s, "0123456789")
                        .is_none_or(|(top, bottom)| fits((bottom - top) as u16))
            });
            let Some(size) = size else {
                continue;
            };
            // Names shortened cost much, the bar a little.
            let mut score = size * names;
            if bar != wanted_bar {
                score *= 0.9;
            }
            if best.is_none_or(|(b, _)| score > b) {
                let laid = Laid {
                    shown,
                    columns,
                    size,
                    bar,
                    beside: false,
                };
                best = Some((score, laid));
            }
        }
    }
    if let Some((_, laid)) = best {
        return Some(laid);
    }
    // No number fits above its name: each beside it on a line (or the
    // number alone where the name does not fit).
    Some(Laid {
        shown,
        columns: shown as u16,
        size: style.small(),
        bar: false,
        beside: true,
    })
}

fn draw(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    look: &WidgetLook,
    numbers: &Numbers,
    laid: &Laid,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env).map(|font| font.display()) else {
        return;
    };
    let id = WIDGET;
    let gap = style.gap();
    let parts = parts(env);
    let shown = &parts[..laid.shown.min(parts.len())];
    let column = area.width / laid.columns.max(1);
    let used = column * laid.columns;
    let left = super::place_x(area, used, widgets::align(env, id));
    if laid.beside {
        for (i, (part, label)) in shown.iter().enumerate() {
            let cell = Rect::new(left + i as u16 * column, area.y, column, area.height);
            let number = numbers.of(part).to_string();
            let text = alloc::format!("{number} {label}");
            if font.measure(style.small(), &text) > f32::from(cell.width) {
                // Not even one with its name: the number alone, smaller
                // where it must be, nothing where even that is too wide.
                let small = style.small();
                let least = small * 0.5;
                if font.measure(least, &number) <= f32::from(cell.width) {
                    widgets::draw_line(
                        canvas,
                        env,
                        cell,
                        Align::Center,
                        small,
                        least,
                        &number,
                        look.ink,
                    );
                }
                continue;
            }
            let x = Align::Center.x(cell, font.measure(style.small(), &text));
            let baseline = widgets::ink_baseline(&font, style.small(), &text, cell);
            canvas.describe(Role::Text, &text, cell);
            font.draw(canvas, style.small(), x, baseline, &number, look.ink);
            let x = x + font.measure(style.small(), &alloc::format!("{number} "));
            font.draw(canvas, style.small(), x, baseline, label, look.faint);
        }
        return;
    }
    let labels: String = parts.iter().map(|(_, l)| *l).collect();
    let (_, (names_top, names_h)) = inks(&font, &style, &labels);
    // The names in their columns, a little room from their neighbours;
    // none where one would keep less than three letters (a row of "B…"
    // says nothing), the numbers alone then.
    let between = if laid.columns > 1 { gap / 2 } else { 0 };
    let room = f32::from(column.saturating_sub(between));
    let names: Vec<String> = shown
        .iter()
        .map(|(_, label)| paperpaw_sdk::ui::fit(&font, style.small(), label, room))
        .collect();
    let named = shown
        .iter()
        .zip(&names)
        .all(|((_, label), name)| name == label || name.chars().count() > 3);
    let names_h = if named { names_h } else { 0 };
    let size = laid.size;
    // Each row: the digits' ink, half a gap, the names' ink; a gap
    // between rows and above the bar; all of it in the middle.
    let digits = font.ink(size, "0123456789").unwrap_or((-(size as i32), 0));
    let digits_h = (digits.1 - digits.0) as u16;
    let row_h = digits_h + if named { gap / 2 + names_h } else { 0 };
    let rows = (laid.shown as u16).div_ceil(laid.columns.max(1));
    let bar_h = (style.line() * 3).max(4);
    let total = row_h * rows + gap * (rows - 1) + if laid.bar { gap + bar_h } else { 0 };
    let top = area.y + area.height.saturating_sub(total) / 2;
    for (i, (part, label)) in shown.iter().enumerate() {
        let (col, row) = (i as u16 % laid.columns, i as u16 / laid.columns);
        let y = top + row * (row_h + gap);
        // A last row with fewer in the middle under the others.
        let in_row = (shown.len() as u16 - row * laid.columns).min(laid.columns);
        let shift = (laid.columns - in_row) * column / 2;
        let cell = Rect::new(left + shift + col * column, y, column, row_h);
        let number = numbers.of(part).to_string();
        let said = alloc::format!("{number} {label}");
        canvas.describe(Role::Text, &said, cell);
        let x = Align::Center.x(cell, font.measure(size, &number));
        font.draw(canvas, size, x, i32::from(y) - digits.0, &number, look.ink);
        let label = &names[i];
        let label_w = font.measure(style.small(), label);
        if named && label_w <= f32::from(column) {
            let x = Align::Center.x(cell, label_w);
            let baseline = i32::from(y + digits_h + gap / 2) - names_top;
            font.draw(canvas, style.small(), x, baseline, label, look.faint);
        }
    }
    if laid.bar {
        let inset = column / 6;
        let line = Rect::new(
            left + inset,
            top + row_h * rows + gap * (rows - 1) + gap,
            used.saturating_sub(2 * inset),
            bar_h,
        );
        draw_share(canvas, env, look, line, numbers);
    }
}

/// A bar of the books: those read dark, those being read shaded, the
/// rest a fine line.
fn draw_share(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    area: Rect,
    numbers: &Numbers,
) {
    let style = UiStyle::new(env);
    let fine = (style.line()).clamp(1, area.height);
    canvas.fill(
        Rect::new(area.x, area.y + (area.height - fine) / 2, area.width, fine),
        look.ink,
    );
    let share = |n: usize| (u32::from(area.width) * n as u32 / numbers.books.max(1) as u32) as u16;
    let read = share(numbers.read);
    let reading = share(numbers.reading);
    if reading > 0 {
        let at = Rect::new(
            area.x + read,
            area.y,
            reading.min(area.width - read),
            area.height,
        );
        let shade = if look.paper.luma() < 128 {
            Color::gray(140)
        } else {
            Color::gray(150)
        };
        canvas.fill_round_shaded(at, u32::from(area.height / 2), shade);
    }
    if read > 0 {
        let at = Rect::new(
            area.x,
            area.y,
            read.max(area.height).min(area.width),
            area.height,
        );
        canvas.fill_round(at, u32::from(area.height / 2), look.ink);
    }
}
