//! *Books this year*: the books read to the end this year, towards a
//! yearly goal (off, 6, 12, 24, 36 or 52 books) as a bar, a ring or a dot
//! a book, with how the year goes ("2 ahead of plan") and the book
//! finished last.
//!
//! A book counts in the year it was last read to the end: the statistics
//! keep when that was (`finished` in `books.ini`, from
//! [`BOOK_FINISHED`](paperpaw_sdk::builtin::events::BOOK_FINISHED)).
//! Books finished before the statistics kept that, or marked as read in
//! the library, count in the year they were last opened, if the library
//! has them read to the end.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetLook, WidgetTarget, Wrapped,
};
use paperpaw_sdk::{
    Arg, Canvas, Command, Env, Id, Params, Point, Rect, RegisterError, Registrar, SettingSpec,
};

use super::{caption_room, chosen, fit_size, frame_height, line, middle, shade};
use crate::store::{self, Book};
use crate::{BOOKS, fmt};

pub(super) const YEAR: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.year";
/// Books a year to read, or none.
const GOALS: [&str; 6] = [
    "stats.books.goal.off",
    "stats.books.goal.6",
    "stats.books.goal.12",
    "stats.books.goal.24",
    "stats.books.goal.36",
    "stats.books.goal.52",
];
/// How the way to the goal shows.
const STYLES: [&str; 3] = ["stats.year.bar", "stats.year.ring", "stats.year.dots"];
/// Books looked up in the library at most (those opened last), for books
/// without a date of their own.
const LIBRARY_LOOKUPS: usize = 24;
/// Dots drawn at most.
const MOST_DOTS: u32 = 60;

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 11] = [
    widgets::frame_spec("stats.widget.year.frame", YEAR, Frame::None),
    widgets::padding_spec("stats.widget.year.padding", YEAR, Padding::Auto),
    widgets::paper_spec("stats.widget.year.paper", YEAR),
    widgets::ink_spec("stats.widget.year.ink", YEAR),
    widgets::size_spec("stats.widget.year.size", YEAR, TextSize::Medium, true),
    widgets::align_spec("stats.widget.year.align", YEAR, Align::Left),
    SettingSpec::choice(
        "stats.widget.year.goal",
        "stats.setting.books-goal",
        &GOALS,
        GOALS[2],
        YEAR,
    )
    .description("stats.setting.books-goal.help"),
    SettingSpec::choice(
        "stats.widget.year.style",
        "stats.setting.year-style",
        &STYLES,
        STYLES[0],
        YEAR,
    )
    .description("stats.setting.year-style.help"),
    widgets::part_spec(
        "stats.widget.year.caption",
        YEAR,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec("stats.widget.year.plan", YEAR, "stats.part.plan", true)
        .description("stats.part.plan.help"),
    widgets::part_spec("stats.widget.year.last", YEAR, "stats.part.last-book", true),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(YEAR, "stats.widget.year");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    static WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(YEAR, "stats.widget.year", |_| Box::new(Year::default()))
                .category(widgets::categories::READING),
        ];
    r.add_widgets(&WIDGET)
}

/// The yearly goal, if one is set.
fn books_goal(env: &Env<'_>) -> Option<u32> {
    super::number_of(chosen(env, NAME, "goal")).filter(|&g| g > 0)
}

/// Whether `year` has 366 days.
fn leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The day of the year of `day` (1 on 1 January) and the year's days.
pub(super) fn day_of_year(day: i32) -> (u32, u32) {
    const BEFORE: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let d = fmt::date_of(day);
    let extra = u32::from(leap(d.year) && d.month > 2);
    let ordinal = BEFORE[usize::from(d.month.clamp(1, 12) - 1)] + extra + u32::from(d.day);
    (ordinal, if leap(d.year) { 366 } else { 365 })
}

/// How far ahead of the plan (behind, if less than 0) `done` books are
/// on day `ordinal` of a year of `length` days, `goal` books spread
/// evenly over it.
pub(super) fn plan(done: u32, goal: u32, ordinal: u32, length: u32) -> i32 {
    let expected = u64::from(goal) * u64::from(ordinal) / u64::from(length.max(1));
    done as i32 - expected as i32
}

/// The books finished in `year` (`year_of` gives a moment's year) and
/// the one finished last; `read_to_end` says for books without a date
/// whether the library has them read to the end, counted in the year
/// they were last opened (asked for the most recently opened only).
pub(super) fn finished_in<'b>(
    books: &'b [Book],
    year: i32,
    year_of: impl Fn(i64) -> i32,
    mut read_to_end: impl FnMut(&Book) -> bool,
) -> (u32, Option<&'b Book>) {
    let mut count = 0;
    let mut last: Option<(&Book, i64)> = None;
    let mut found = |book: &'b Book, at: i64| {
        count += 1;
        if last.is_none_or(|(_, when)| at > when) {
            last = Some((book, at));
        }
    };
    for book in books.iter().filter(|b| b.finished > 0) {
        if year_of(book.finished) == year {
            found(book, book.finished);
        }
    }
    let mut undated: Vec<&Book> = books
        .iter()
        .filter(|b| b.finished <= 0 && b.last_open > 0 && !b.path.is_empty())
        .filter(|b| year_of(b.last_open) == year)
        .collect();
    undated.sort_by_key(|b| core::cmp::Reverse(b.last_open));
    for book in undated.into_iter().take(LIBRARY_LOOKUPS) {
        if read_to_end(book) {
            found(book, book.last_open);
        }
    }
    (count, last.map(|(book, _)| book))
}

/// What the widget shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    year: i32,
    done: u32,
    /// The title of the book finished last.
    last: Option<String>,
    /// Today's day of the year, and the year's days.
    ordinal: u32,
    length: u32,
}

fn shown(env: &Env<'_>) -> Option<Shown> {
    let today = fmt::today(env)?;
    let year = fmt::date_of(today).year;
    let books = store::books(env.storage());
    let year_of = |unix: i64| {
        let unix = unix.clamp(0, i64::from(u32::MAX)) as u32;
        fmt::date_of(fmt::day_of(env, unix)).year
    };
    let read_to_end = |book: &Book| {
        paperpaw_sdk::library::book(env.storage(), &book.path).and_then(|b| b.progress) == Some(100)
    };
    let (done, last) = finished_in(&books, year, year_of, read_to_end);
    let (ordinal, length) = day_of_year(today);
    Some(Shown {
        year,
        done,
        last: last.map(|b| b.title.clone()),
        ordinal,
        length,
    })
}

/// Books this year, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Year {
    shown: Option<Shown>,
    read_for: Option<super::Stamp>,
}

impl Widget for Year {
    fn update(&mut self, env: &Env<'_>) -> bool {
        if !super::stale(env, &mut self.read_for) {
            return false;
        }
        let shown = shown(env);
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn live(&self) -> bool {
        // Asked each minute for a new day; read again only then, or
        // when there is more reading.
        true
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = NAME;
        let size = widgets::size(env, id).text(&style);
        let notes = self
            .shown
            .as_ref()
            .map_or(0, |s| notes(env, s).len() as u16);
        let band = if books_goal(env).is_some() {
            match chosen(env, id, "style") {
                s if s == STYLES[1] => line(&font, style.title()) * 2,
                s if s == STYLES[2] => line(&font, style.small()) * 2,
                _ => env.metrics().mm(3) as u16,
            }
        } else {
            0
        };
        frame_height(env, id)
            + caption_room(env, id)
            + line(&font, size)
            + band
            + notes * line(&font, style.small())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        match &self.shown {
            Some(shown) => draw(canvas, env, &look, shown),
            None => widgets::draw_message(canvas, env, &look, env.i18n().tr("stats.no-clock")),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget::new(
            area,
            Command::OpenScreen(BOOKS, Params::new())
        )]
    }
}

/// "3 of 12 books", or "3 books read" without a goal.
fn headline(env: &Env<'_>, shown: &Shown) -> String {
    match books_goal(env) {
        Some(goal) => env.i18n().plural(
            "stats.widget.year.of",
            i64::from(goal),
            &[("done", Arg::Int(i64::from(shown.done)))],
        ),
        None => env
            .i18n()
            .plural("stats.widget.year.read", i64::from(shown.done), &[]),
    }
}

/// The notes below: how the year goes, the book finished last; each
/// with whether it may be shortened where it is too long (a book's
/// title), else it is left out.
fn notes(env: &Env<'_>, shown: &Shown) -> Vec<(String, bool)> {
    let id = NAME;
    let mut notes = Vec::new();
    if let Some(goal) = books_goal(env).filter(|_| widgets::shows(env, id, "plan")) {
        let ahead = plan(shown.done, goal, shown.ordinal, shown.length);
        let note = if shown.done >= goal {
            env.i18n().tr("stats.widget.year.reached").to_string()
        } else if ahead > 0 {
            env.i18n()
                .plural("stats.widget.year.ahead", i64::from(ahead), &[])
        } else if ahead < 0 {
            env.i18n()
                .plural("stats.widget.year.behind", i64::from(-ahead), &[])
        } else {
            env.i18n().tr("stats.widget.year.on-plan").to_string()
        };
        notes.push((note, false));
    }
    if let Some(title) = shown
        .last
        .as_ref()
        .filter(|_| widgets::shows(env, id, "last"))
    {
        let note = env
            .i18n()
            .format("stats.widget.year.last", &[("title", Arg::Str(title))]);
        notes.push((note, true));
    }
    notes
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, shown: &Shown) {
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    let title = env.i18n().format(
        "stats.widget.year.caption",
        &[("year", Arg::Int(i64::from(shown.year)))],
    );
    // "Books in 2026", or where that is too long the year alone.
    let year = alloc::format!("{}", shown.year);
    let mut caption = super::caption(env, id, look.area, &[&title, &year]);
    let mut area = super::below_caption(env, caption, look.area);
    let goal = books_goal(env);
    let progress = goal.map(|g| shown.done.min(g) * 1000 / g.max(1));
    let style_set = chosen(env, id, "style");
    let small = line(&font, style.small());
    let align = widgets::align(env, id);
    if style_set == STYLES[1] && goal.is_some() {
        // Beside or below the ring, the notes that fit a line whole (the
        // last book's title may be shortened).
        let width = f32::from(area.width);
        let notes: Vec<String> = notes(env, shown)
            .into_iter()
            .filter(|(note, cut)| *cut || font.measure(style.small(), note) <= width)
            .map(|(note, _)| note)
            .collect();
        draw_ring(&mut ui, (look, caption), area, shown, progress, &notes);
        return;
    }
    // The notes on two lines where they need them (shortened where even
    // two do not hold them), the last book's title on one.
    let mut notes: Vec<Wrapped> = notes(env, shown)
        .into_iter()
        .filter_map(|(note, cut)| {
            if cut {
                Some(Wrapped::new(env, &note, style.small(), area.width, small))
            } else {
                super::note_lines(env, &note, area.width, small * 2, 2)
            }
        })
        .collect();
    let height_of = |notes: &[Wrapped]| notes.iter().map(|n| n.height(env)).sum::<u16>();
    let headline = headline(env, shown);
    // Notes as fit below the rest, the plan first.
    let dots = style_set == STYLES[2];
    let band = match goal {
        Some(_) if dots => small,
        Some(_) => env.metrics().mm(3) as u16,
        None => 0,
    };
    let least = line(&font, style.text());
    // As many notes as fit `height` with the rest.
    let kept = |height: u16| {
        let mut kept = 0;
        let mut used = band + least;
        for note in &notes {
            used += note.height(env);
            if used > height {
                break;
            }
            kept += 1;
        }
        kept
    };
    // The caption goes before a note.
    if caption.is_some() && kept(look.area.height) > kept(area.height) {
        caption = None;
        area = look.area;
    }
    notes.truncate(kept(area.height));
    let notes_h = height_of(&notes);
    let room = area.height.saturating_sub(band + notes_h);
    let size = super::text_size(env, &font, id, &headline, area.width, room);
    let big = line(&font, size);
    let band = if dots {
        // The dots take the room left, up to three lines of text.
        area.height.saturating_sub(big + notes_h).min(small * 3)
    } else {
        band
    };
    let block = super::captioned(ui.canvas(), env, id, look, caption, big + band + notes_h);
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    // Too wide: "3 / 12", or the number alone.
    let short = goal.map(|g| alloc::format!("{} / {g}", shown.done));
    let headline = match short {
        Some(short) if font.measure(size, &headline) > f32::from(area.width) => short,
        _ => headline,
    };
    let headline = super::number_if_narrow(&font, size, &headline, area.width);
    let headline = ui::fit(&font, size, headline, f32::from(area.width));
    let x = align.x(area, font.measure(size, &headline));
    ui::Label::new(&headline)
        .px(size)
        .color(look.ink)
        .draw_from(&mut ui, x, row(block.y, big));
    let mut y = block.y + big;
    if let Some(goal) = goal {
        if dots {
            draw_dots(&mut ui, look, row(y, band), shown.done, goal);
        } else {
            let height = env.metrics().mm(1).max(4) as u16;
            let track = Rect::new(area.x, y + (band - height) / 2, area.width, height);
            let radius = u32::from(height / 2);
            ui.canvas().fill_round_shaded(track, radius, shade(look, 1));
            let filled = (u32::from(area.width) * progress.unwrap_or(0) / 1000) as u16;
            if filled > 0 {
                let bar = Rect::new(track.x, track.y, filled.max(height), height);
                ui.canvas().fill_round(bar, radius, look.ink);
            }
        }
        y += band;
    }
    for note in &notes {
        let h = note.height(env);
        note.draw(ui.canvas(), env, row(y, h), align, look.faint);
        y += h;
    }
}

/// The goal as a ring with the books read inside, the notes beside it
/// (or below, where it is not wide).
fn draw_ring(
    ui: &mut ui::Ui<'_>,
    (look, caption): (&WidgetLook, Option<&str>),
    area: Rect,
    shown: &Shown,
    progress: Option<u32>,
    notes: &[String],
) {
    let env = ui.env();
    let style = *ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    let small = line(&font, style.small());
    let align = widgets::align(env, NAME);
    let wide = area.width >= area.height * 2;
    let notes_h = if wide { 0 } else { notes.len() as u16 * small };
    let d = if wide {
        area.height.min(area.width / 3)
    } else {
        area.width.min(area.height.saturating_sub(notes_h))
    };
    let block = super::captioned(ui.canvas(), env, NAME, look, caption, d + notes_h);
    let gap = style.gap();
    // Wide: the ring and the notes beside it placed together, as the
    // alignment says.
    let beside = if wide {
        let room = f32::from(area.width.saturating_sub(d + gap));
        let widest = notes
            .iter()
            .map(|n| font.measure(style.small(), n).min(room))
            .fold(0.0, f32::max);
        widest as u16
    } else {
        0
    };
    let x = if wide && beside > 0 {
        align.x(area, f32::from(d + gap + beside)) as u16
    } else {
        align.x(area, f32::from(d)) as u16
    };
    let at = Rect::new(x, block.y, d, d);
    let thick = (u32::from(d) / 12).max(3);
    super::draw_arc(ui.canvas(), env, look, at, progress, thick, false);
    // The count inside, "of 12" below it.
    let inner = d.saturating_sub(2 * thick as u16 + 4);
    let count = alloc::format!("{}", shown.done);
    let size = fit_size(&font, &style, "00", inner / 2, inner * 2 / 5).max(style.text());
    let big = line(&font, size);
    let of = books_goal(env).map(|g| {
        env.i18n().format(
            "stats.widget.year.of-goal",
            &[("goal", Arg::Int(i64::from(g)))],
        )
    });
    let of_h = if of.is_some() && big + small <= inner {
        small
    } else {
        0
    };
    let inside = middle(at, big + of_h);
    let cx = f32::from(at.x) + f32::from(d) / 2.0;
    let w = font.measure(size, &count);
    let r = Rect::new(at.x, inside.y, d, big);
    ui::Label::new(&count)
        .px(size)
        .color(look.ink)
        .draw_from(ui, cx - w / 2.0, r);
    if let Some(of) = of.filter(|_| of_h > 0) {
        let w = font.measure(style.small(), &of);
        let r = Rect::new(at.x, inside.y + big, d, small);
        ui::Label::new(&of)
            .px(style.small())
            .color(look.faint)
            .draw_from(ui, cx - w / 2.0, r);
    }
    // The notes.
    let (text, top) = if wide {
        let text = Rect::new(
            x + d + gap,
            area.y,
            (area.x + area.width).saturating_sub(x + d + gap),
            area.height,
        );
        let top = middle(text, notes.len() as u16 * small).y;
        (text, top)
    } else {
        (area, block.y + d)
    };
    for (n, note) in notes.iter().enumerate() {
        let note = ui::fit(&font, style.small(), note, f32::from(text.width));
        let x = if wide {
            f32::from(text.x)
        } else {
            align.x(text, font.measure(style.small(), &note))
        };
        let r = Rect::new(text.x, top + n as u16 * small, text.width, small);
        ui::Label::new(&note)
            .px(style.small())
            .color(look.faint)
            .draw_from(ui, x, r);
    }
}

/// A dot for each book of the goal in `band`, filled for those read, in
/// as few rows as let them be large.
fn draw_dots(ui: &mut ui::Ui<'_>, look: &WidgetLook, band: Rect, done: u32, goal: u32) {
    let env = ui.env();
    let style = *ui.style();
    let n = goal.clamp(1, MOST_DOTS);
    let (w, h) = (u32::from(band.width), u32::from(band.height));
    // The rows that give the largest dots.
    let (rows, size) = (1..=n.min(6))
        .map(|rows| {
            let cols = n.div_ceil(rows);
            (rows, (w / cols).min(h / rows))
        })
        .max_by_key(|(_, size)| *size)
        .unwrap_or((1, 0));
    if size < 4 {
        return;
    }
    let cols = n.div_ceil(rows);
    let radius = (size * 2 / 5).max(2);
    let thin = u32::from(style.line()).max(1);
    let align = widgets::align(env, NAME);
    let left = align.x(band, (cols * size) as f32) as i32;
    let top = i32::from(band.y) + ((h - rows * size) / 2) as i32;
    for i in 0..n {
        let (row, col) = (i / cols, i % cols);
        let center = Point::new(
            left + (col * size + size / 2) as i32,
            top + (row * size + size / 2) as i32,
        );
        if i < done {
            ui.canvas().fill_circle(center, radius, look.ink);
        } else {
            ui.canvas().stroke_circle(center, radius, thin, look.faint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_spreads_the_goal_over_the_year() {
        // 12 books: one a month, so by mid-October nine.
        let (ordinal, length) = day_of_year(20_740);
        assert_eq!((ordinal, length), (287, 365));
        assert_eq!(plan(9, 12, ordinal, length), 0);
        assert_eq!(plan(3, 12, ordinal, length), -6);
        assert_eq!(plan(11, 12, ordinal, length), 2);
        // On the first day nothing is expected yet.
        assert_eq!(plan(0, 52, 1, 365), 0);
        // 29 February counts in a leap year.
        let leap_day = (19_000..22_000)
            .find(|&d| {
                let t = fmt::date_of(d);
                (t.year, t.month, t.day) == (2028, 3, 1)
            })
            .unwrap_or(0);
        assert_eq!(day_of_year(leap_day), (61, 366));
    }

    fn book(id: u16, finished: i64, last_open: i64) -> Book {
        Book {
            id,
            title: alloc::format!("Book {id}"),
            path: alloc::format!("books/{id}.epub"),
            finished,
            last_open,
            ..Book::default()
        }
    }

    #[test]
    fn books_count_in_the_year_they_were_finished() {
        // A "year" of 1000 seconds, for short numbers.
        let year_of = |unix: i64| (unix / 1000) as i32;
        let books = [
            book(1, 2100, 2100),
            book(2, 2900, 2950),
            // Last year's.
            book(3, 1500, 2500),
            // Without a date: the library has it read to the end.
            book(4, 0, 2950),
            // Without a date, not read to the end.
            book(5, 0, 2990),
            // Without a date, opened last year.
            book(6, 0, 1990),
        ];
        let read = |b: &Book| b.id == 4 || b.id == 6;
        let (count, last) = finished_in(&books, 2, year_of, read);
        assert_eq!(count, 3);
        assert_eq!(last.map(|b| b.id), Some(4));
        let (count, last) = finished_in(&books, 1, year_of, read);
        assert_eq!((count, last.map(|b| b.id)), (2, Some(6)));
        assert_eq!(finished_in(&books, 5, year_of, read).0, 0);
    }
}
