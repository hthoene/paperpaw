//! *Reading streak*: the days read in a row (any reading, or the daily
//! goal reached), the longest streak so far, and the last 7 or 14 days
//! as marks: filled (with a check, if chosen) for the days that count,
//! an outline for the others, today's darker while it does not count yet.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetLook, WidgetTarget,
};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Point, Rect, RegisterError, Registrar, SettingSpec};

use super::{caption_room, chosen, frame_height, goal, line, middle};
use crate::fmt;
use crate::totals::{self, Streaks};

pub(super) const STREAK: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.streak";
/// Which days count: any reading, or the daily goal reached.
const COUNTS: [&str; 2] = ["stats.streak.any", "stats.streak.goal"];
/// How the days show: dots, or dots with a check.
const MARKS: [&str; 2] = ["stats.marks.dots", "stats.marks.checks"];
/// How many days show.
const DAYS: [&str; 2] = ["stats.marks.7", "stats.marks.14"];

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 16] = [
    widgets::frame_spec("stats.widget.streak.frame", STREAK, Frame::None),
    widgets::padding_spec("stats.widget.streak.padding", STREAK, Padding::Auto),
    widgets::paper_spec("stats.widget.streak.paper", STREAK),
    widgets::ink_spec("stats.widget.streak.ink", STREAK),
    widgets::corners_spec("stats.widget.streak.corners", STREAK),
    widgets::frame_line_spec("stats.widget.streak.frame-line", STREAK),
    widgets::shadow_spec("stats.widget.streak.drop-shadow", STREAK),
    widgets::size_spec("stats.widget.streak.size", STREAK, TextSize::Medium, true),
    widgets::align_spec("stats.widget.streak.align", STREAK, Align::Center),
    SettingSpec::choice(
        "stats.widget.streak.counts",
        "stats.setting.counts",
        &COUNTS,
        COUNTS[0],
        STREAK,
    )
    .description("stats.setting.counts.help"),
    SettingSpec::choice(
        "stats.widget.streak.style",
        "stats.setting.marks",
        &MARKS,
        MARKS[1],
        STREAK,
    )
    .description("stats.setting.marks.help"),
    SettingSpec::choice(
        "stats.widget.streak.days",
        "stats.setting.days",
        &DAYS,
        DAYS[0],
        STREAK,
    )
    .description("stats.setting.days.help"),
    widgets::part_spec(
        "stats.widget.streak.caption",
        STREAK,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec(
        "stats.widget.streak.longest",
        STREAK,
        "stats.part.longest",
        true,
    ),
    widgets::part_spec(
        "stats.widget.streak.marks",
        STREAK,
        "stats.part.marks",
        true,
    )
    .description("stats.part.marks.help"),
    widgets::part_spec(
        "stats.widget.streak.weekdays",
        STREAK,
        "stats.part.weekdays",
        true,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(STREAK, "stats.widget.streak");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    static WIDGET: [FixedWidget; 1] = [FixedWidget::new(STREAK, "stats.widget.streak", |_| {
        Box::new(Streak::default())
    })
    .category(widgets::categories::READING)];
    r.add_widgets(&WIDGET)
}

/// A day of the marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    weekday: u8,
    counts: bool,
    today: bool,
}

/// What the widget shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    streaks: Streaks,
    marks: Vec<Mark>,
}

/// The least seconds of a day that counts: the goal, if chosen and set.
fn least(env: &Env<'_>) -> u32 {
    if chosen(env, NAME, "counts") != COUNTS[1] {
        return 1;
    }
    goal(env).map_or(1, |g| g.min(u64::from(u32::MAX)) as u32)
}

fn shown(env: &Env<'_>) -> Option<Shown> {
    let (today, days) = super::days(env)?;
    let least = least(env);
    let count = if chosen(env, NAME, "days") == DAYS[1] {
        14
    } else {
        7
    };
    let marks = (today - count + 1..=today)
        .map(|day| Mark {
            weekday: fmt::date_of(day).weekday,
            counts: fmt::seconds_on(&days, day) >= least,
            today: day == today,
        })
        .collect();
    Some(Shown {
        streaks: totals::streaks(&days, today, least),
        marks,
    })
}

/// The streak, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Streak {
    shown: Option<Shown>,
    read_for: Option<super::Stamp>,
}

impl Widget for Streak {
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
        let small = line(&font, style.small());
        frame_height(env, id)
            + caption_room(env, id)
            + line(&font, size)
            + small
            + marks_height(env, small, 0, widgets::shows(env, id, "weekdays"))
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        match &self.shown {
            Some(shown) => draw(canvas, env, &look, shown),
            None => widgets::draw_message(canvas, env, &look, env.i18n().tr("stats.no-clock")),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        super::open_statistics(area)
    }
}

/// The marks' place across `width` (as large as they may be for 0): how
/// wide each one's column is and how large the mark.
fn mark_size(env: &Env<'_>, width: u16, count: usize) -> (u32, u32) {
    let most = env.metrics().mm(4);
    // A mark and a half for each, at most: together, not spread out.
    let column = match width {
        0 => most * 3 / 2,
        w => (u32::from(w) / count.max(1) as u32).min(most * 3 / 2),
    };
    (column, (column * 3 / 4).min(most))
}

/// The height of the marks across `width`, with the room above them and
/// their weekdays below if `weekdays`; 0 where they do not show.
fn marks_height(env: &Env<'_>, small: u16, width: u16, weekdays: bool) -> u16 {
    if !widgets::shows(env, NAME, "marks") {
        return 0;
    }
    let style = UiStyle::new(env);
    let count = if chosen(env, NAME, "days") == DAYS[1] {
        14
    } else {
        7
    };
    let (_, d) = mark_size(env, width, count);
    d as u16 + style.gap() / 2 + if weekdays { small } else { 0 }
}

/// "5 days", and below it "in a row, longest 12" (or how to begin one).
fn texts(env: &Env<'_>, streaks: Streaks) -> (String, String) {
    let first = env
        .i18n()
        .plural("stats.days", i64::from(streaks.current), &[]);
    let second = if streaks.current == 0 {
        String::from(env.i18n().tr("stats.widget.streak.start"))
    } else if widgets::shows(env, NAME, "longest") {
        env.i18n().format(
            "stats.widget.streak.longest",
            &[("count", Arg::Int(i64::from(streaks.longest)))],
        )
    } else {
        String::from(env.i18n().tr("stats.in-a-row"))
    };
    (first, second)
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, shown: &Shown) {
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    let title = env.i18n().tr("stats.widget.streak.caption");
    let (first, second) = texts(env, shown.streaks);
    let mut caption = super::caption(env, id, look.area, &[title]);
    let mut area = super::below_caption(env, caption, look.area);
    // The caption goes before the note below the streak.
    let note = |area: Rect| lay_texts(env, area, (&first, &second)).second.is_some();
    if caption.is_some() && !note(area) && note(look.area) {
        caption = None;
        area = look.area;
    }
    let small = line(&font, style.small());
    let gap = style.gap() / 2;
    let weekdays = widgets::shows(env, id, "weekdays");
    let align = widgets::align(env, id);
    let headline = match widgets::size(env, id) {
        TextSize::Fit => style.text(),
        size => size.text(&style),
    };
    let least = line(&font, headline) + small;
    let roomy = u32::from(area.width) * 3 / 5 >= env.metrics().mm(3) * shown.marks.len() as u32;
    if area.width >= area.height * 3 && roomy {
        // Wide and low: the marks beside the text, if there is room, the
        // two placed together as the alignment says.
        // The text two fifths (more where the streak needs it), the marks
        // the rest; the gap between them taken from the marks where the
        // two would not fit together otherwise.
        let space = style.gap();
        let needs = font.measure(headline, &first) as u16 + 1;
        let least_marks = (env.metrics().mm(3) * shown.marks.len() as u32) as u16;
        let side = needs
            .max(area.width * 2 / 5)
            .min(area.width.saturating_sub(least_marks + space));
        let rest = area.width.saturating_sub(side);
        let fits = |weekdays| {
            let h = marks_height(env, small, rest, weekdays);
            (h > 0 && h - gap <= area.height).then_some((h - gap, weekdays))
        };
        if let Some((marks, weekdays)) = fits(weekdays).or_else(|| fits(false)) {
            let area = super::captioned(ui.canvas(), env, id, look, caption, area.height);
            let side_area = Rect::new(area.x, area.y, side, area.height);
            let laid = lay_texts(env, side_area, (&first, &second));
            let (mut column, mut d) = mark_size(env, rest, shown.marks.len());
            let spans = |column: u32, d: u32| column * shown.marks.len() as u32 - (column - d);
            if u32::from(laid.width) + u32::from(space) + spans(column, d) > u32::from(area.width) {
                (column, d) = mark_size(env, rest.saturating_sub(space), shown.marks.len());
            }
            let inset = (column - d) / 2;
            let span = column * shown.marks.len() as u32 - 2 * inset;
            let space = u32::from(space);
            let group = u32::from(laid.width) + space + span;
            let left = align.x(area, group as f32) as u16;
            let text = Rect::new(left, area.y, laid.width, area.height);
            draw_texts(&mut ui, look, text, &laid, Align::Left);
            let x = left + laid.width + space as u16;
            let width = (column * shown.marks.len() as u32) as u16;
            let at = middle(Rect::new(x, area.y, width, area.height), marks);
            draw_marks(&mut ui, look, at, &shown.marks, (Align::Left, weekdays));
            return;
        }
    }
    // The marks below where there is room for them besides the text,
    // without the weekdays if need be, and without the caption rather
    // than without the marks.
    let fits = |area: Rect, weekdays| {
        let h = marks_height(env, small, area.width, weekdays);
        (h > 0 && h + least <= area.height).then_some((h, weekdays))
    };
    let in_area = |area: Rect| fits(area, weekdays).or_else(|| fits(area, false));
    if caption.is_some() && in_area(area).is_none() && in_area(look.area).is_some() {
        caption = None;
        area = look.area;
    }
    let (marks, weekdays) = in_area(area).unwrap_or((0, false));
    let room = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(marks),
    );
    let laid = lay_texts(env, room, (&first, &second));
    // The text and the marks below it, together in the middle.
    let block = super::captioned(ui.canvas(), env, id, look, caption, laid.height + marks);
    let at = Rect::new(area.x, block.y, area.width, laid.height);
    draw_texts(&mut ui, look, at, &laid, align);
    if marks > 0 {
        let at = Rect::new(area.x, block.y + laid.height + gap, area.width, marks - gap);
        draw_marks(&mut ui, look, at, &shown.marks, (align, weekdays));
    }
}

/// The streak and the note below it laid out for `area`.
struct Texts {
    size: f32,
    first: String,
    /// The note, where it has room below the streak.
    second: Option<String>,
    /// How wide and high the two are.
    width: u16,
    height: u16,
}

/// The streak as large as the size says (or fits) in `area`, the note
/// below it where there is room for both (else the streak alone), each
/// shortened where it is too wide.
fn lay_texts(env: &Env<'_>, area: Rect, (first, second): (&str, &str)) -> Texts {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return Texts {
            size: style.text(),
            first: String::new(),
            second: None,
            width: 0,
            height: 0,
        };
    };
    let small = line(&font, style.small());
    let width = f32::from(area.width);
    let room = area.height.saturating_sub(small);
    let with_note = super::text_size(env, &font, NAME, first, area.width, room);
    let alone = super::text_size(env, &font, NAME, first, area.width, area.height);
    // The note where there is room for it below the streak, and where it
    // does not make the streak smaller than the body's size.
    let note = line(&font, with_note) + small <= area.height
        && (with_note >= style.text() || with_note >= alone);
    let size = if note { with_note } else { alone };
    let shown = super::number_if_narrow(&font, size, first, area.width);
    let shown = ui::fit(&font, size, shown, width);
    // The longest streak left out where the note is too long; how to
    // begin one in short.
    let second = note
        .then(|| {
            let in_a_row = env.i18n().tr("stats.in-a-row");
            let start = env.i18n().tr("stats.widget.streak.start");
            let wide = font.measure(style.small(), second) > width;
            if wide && second.starts_with(in_a_row) {
                in_a_row
            } else if wide && second == start {
                env.i18n().tr("stats.widget.streak.start-short")
            } else {
                second
            }
        })
        // Left out where not even that fits whole: a note shortened says
        // little.
        .filter(|text| font.measure(style.small(), text) <= width)
        .map(String::from);
    let wide = font.measure(size, &shown).max(
        second
            .as_ref()
            .map_or(0.0, |s| font.measure(style.small(), s)),
    );
    let height = line(&font, size) + if second.is_some() { small } else { 0 };
    Texts {
        size,
        first: shown,
        second,
        width: ((wide + 0.99) as u16).min(area.width),
        height: height.min(area.height),
    }
}

/// Draws the streak and the note below it, as laid out, in the middle
/// of `area` as `align` places them.
fn draw_texts(ui: &mut ui::Ui<'_>, look: &WidgetLook, area: Rect, laid: &Texts, align: Align) {
    let style = *ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    let small = line(&font, style.small());
    let big = line(&font, laid.size);
    let block = middle(area, laid.height);
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    let x = align.x(area, font.measure(laid.size, &laid.first));
    ui::Label::new(&laid.first)
        .px(laid.size)
        .color(look.ink)
        .draw_from(ui, x, row(block.y, big));
    if let Some(second) = &laid.second {
        let x = align.x(area, font.measure(style.small(), second));
        let at = row(block.y + big, small);
        ui::Label::new(second)
            .px(style.small())
            .color(look.faint)
            .draw_from(ui, x, at);
    }
}

/// A mark a day in `area`, placed as `align` says, the weekdays below if
/// `weekdays`.
fn draw_marks(
    ui: &mut ui::Ui<'_>,
    look: &WidgetLook,
    area: Rect,
    marks: &[Mark],
    (align, weekdays): (Align, bool),
) {
    let env = ui.env();
    let style = *ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    let small = line(&font, style.small());
    let n = marks.len().max(1) as u32;
    let (column, d) = mark_size(env, area.width, marks.len());
    // The weekdays where their letters have room.
    let weekdays = weekdays
        && (0..7)
            .all(|w| font.measure(style.small(), super::initial(env, w)) * 1.3 <= column as f32);
    let labels = if weekdays { small } else { 0 };
    let d = d.min(u32::from(area.height.saturating_sub(labels)));
    if d < 4 {
        return;
    }
    let checks = chosen(env, id, "style") == MARKS[1];
    let thin = u32::from(style.line()).max(1);
    // Flush with the text: the outer marks' edges on its margins.
    let inset = (column - d) / 2;
    let span = (column * n - 2 * inset) as f32;
    let left = align.x(area, span) as i32 - inset as i32;
    let radius = d / 2;
    for (i, mark) in marks.iter().enumerate() {
        let cx = left + (column * i as u32 + column / 2) as i32;
        let center = Point::new(cx, i32::from(area.y) + radius as i32);
        if mark.counts {
            ui.canvas().fill_circle(center, radius, look.ink);
            if checks && d >= 10 {
                draw_check(ui.canvas(), center, radius, look);
            }
        } else if mark.today {
            ui.canvas()
                .stroke_circle(center, radius, (thin * 2).min(radius / 3).max(1), look.ink);
        } else {
            ui.canvas().stroke_circle(center, radius, thin, look.faint);
        }
        if weekdays {
            let name = super::initial(env, mark.weekday);
            let w = font.measure(style.small(), name);
            let row = Rect::new(area.x, area.y + d as u16, area.width, labels);
            let color = if mark.today { look.ink } else { look.faint };
            ui::Label::new(name)
                .px(style.small())
                .color(color)
                .draw_from(ui, cx as f32 - w / 2.0, row);
        }
    }
}

/// A check in the paper's colour on a filled mark.
fn draw_check(canvas: &mut Canvas<'_>, center: Point, radius: u32, look: &WidgetLook) {
    let r = radius as i32;
    let at = |x: i32, y: i32| Point::new(center.x + x * r / 100, center.y + y * r / 100);
    let width = (radius / 4).max(1);
    let (a, b, c) = (at(-42, 2), at(-12, 32), at(44, -30));
    canvas.draw_line(a, b, width, look.paper);
    canvas.draw_line(b, c, width, look.paper);
}
