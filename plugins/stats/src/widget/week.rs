//! *Reading per day*: the last days (7, 14 or 28, or this week) as bars,
//! today's marked, the daily goal as a dashed line across, each day's
//! initial below.

use alloc::boxed::Box;
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, Widget, WidgetLook, WidgetTarget,
};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec};

use super::{caption_room, chosen, frame_height, goal, line, shade};
use crate::fmt::{self, seconds_on};

pub(super) const WEEK: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.week";
/// Which days it shows: the last 7, this week, the last 14 or 28.
const PERIODS: [&str; 4] = [
    "stats.period.7",
    "stats.period.week",
    "stats.period.14",
    "stats.period.28",
];
/// The bars' ends: round or square.
const SHAPES: [&str; 2] = ["stats.shape.round", "stats.shape.square"];

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 11] = [
    widgets::frame_spec("stats.widget.week.frame", WEEK, Frame::None),
    widgets::padding_spec("stats.widget.week.padding", WEEK, Padding::Auto),
    widgets::paper_spec("stats.widget.week.paper", WEEK),
    widgets::ink_spec("stats.widget.week.ink", WEEK),
    widgets::align_spec("stats.widget.week.align", WEEK, Align::Left),
    SettingSpec::choice(
        "stats.widget.week.period",
        "stats.setting.period",
        &PERIODS,
        PERIODS[0],
        WEEK,
    )
    .description("stats.setting.period.help"),
    SettingSpec::choice(
        "stats.widget.week.shape",
        "stats.setting.bars",
        &SHAPES,
        SHAPES[0],
        WEEK,
    )
    .description("stats.setting.bars.help"),
    widgets::part_spec(
        "stats.widget.week.caption",
        WEEK,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec(
        "stats.widget.week.labels",
        WEEK,
        "stats.part.weekdays",
        true,
    ),
    widgets::part_spec("stats.widget.week.goal", WEEK, "stats.part.goal-line", true),
    widgets::part_spec("stats.widget.week.today", WEEK, "stats.part.today", true),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(WEEK, "stats.widget.week");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    static WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(WEEK, "stats.widget.week", |_| Box::new(Week::default()))
                .category(widgets::categories::READING),
        ];
    r.add_widgets(&WIDGET)
}

/// A day shown: its weekday, seconds read, and whether it is still to
/// come (later this week).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    weekday: u8,
    seconds: u32,
    to_come: bool,
    today: bool,
}

/// The days shown, oldest first.
fn days(env: &Env<'_>) -> Vec<Shown> {
    let Some((today, days)) = super::days(env) else {
        return Vec::new();
    };
    let period = chosen(env, NAME, "period");
    let range = match PERIODS.iter().position(|p| *p == period) {
        Some(1) => {
            let weekday = fmt::date_of(today).weekday;
            let start = today - i32::from((7 + weekday - fmt::week_start(env)) % 7);
            start..=start + 6
        }
        Some(2) => today - 13..=today,
        Some(3) => today - 27..=today,
        _ => today - 6..=today,
    };
    range
        .map(|day| Shown {
            weekday: fmt::date_of(day).weekday,
            seconds: seconds_on(&days, day),
            to_come: day > today,
            today: day == today,
        })
        .collect()
}

/// The last days as bars, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Week {
    days: Vec<Shown>,
    read_for: Option<super::Stamp>,
}

impl Widget for Week {
    fn update(&mut self, env: &Env<'_>) -> bool {
        if !super::stale(env, &mut self.read_for) {
            return false;
        }
        let days = days(env);
        let changed = days != self.days;
        self.days = days;
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
        frame_height(env, id)
            + caption_room(env, id)
            + labels_height(env, &font, &style)
            + (style.row() as f32 * 1.4) as u16
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        if self.days.is_empty() {
            widgets::draw_message(canvas, env, &look, env.i18n().tr("stats.no-clock"));
            return;
        }
        draw_week(canvas, env, &look, &self.days);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        super::open_statistics(area)
    }
}

/// The height of the row of weekdays below the bars, if it shows.
fn labels_height(env: &Env<'_>, font: &paperpaw_sdk::FontHandle<'_>, style: &UiStyle) -> u16 {
    if widgets::shows(env, NAME, "labels") {
        line(font, style.small()) + style.gap() / 2
    } else {
        0
    }
}

/// The time read in `days`: "3 h 20 min".
fn total(env: &Env<'_>, days: &[Shown]) -> alloc::string::String {
    let total: u64 = days.iter().map(|d| u64::from(d.seconds)).sum();
    fmt::duration(env, total)
}

/// "Last 7 days: 3 h 20 min" or "This week: 2 h 05 min".
fn label(env: &Env<'_>, days: &[Shown]) -> alloc::string::String {
    let time = total(env, days);
    if days.iter().any(|d| d.to_come) || chosen(env, NAME, "period") == PERIODS[1] {
        env.i18n()
            .format("stats.widget.this-week", &[("time", Arg::Str(&time))])
    } else {
        env.i18n().format(
            "stats.widget.days-label",
            &[
                ("count", Arg::Int(days.len() as i64)),
                ("time", Arg::Str(&time)),
            ],
        )
    }
}

/// The caption, then a bar a day with its weekday below; today's bar in
/// the ink and the others a shade lighter (if today is marked); the
/// daily goal, if set, as a dashed line across.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw_week(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, days: &[Shown]) {
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    // The caption, or where it is too long the time alone.
    let (full, time) = (label(env, days), total(env, days));
    let caption = super::caption(env, id, look.area, &[&full, &time]);
    let area = super::below_caption(env, caption, look.area);
    let area = super::captioned(ui.canvas(), env, id, look, caption, area.height);
    let small = line(&font, style.small());
    let labels = labels_height(env, &font, &style);
    // Where the bars would be low, the weekdays go.
    let labels = if area.height.saturating_sub(labels) < small * 2 {
        0
    } else {
        labels
    };
    let thin = u32::from(style.line()).max(1);
    // Room above the bars for the goal's line and rounded tops.
    let top = thin as u16 * 2;
    let chart = Rect::new(
        area.x,
        area.y + top,
        area.width,
        area.height.saturating_sub(labels + top),
    );
    let goal = goal(env).filter(|_| widgets::shows(env, id, "goal"));
    let most = days
        .iter()
        .map(|d| u64::from(d.seconds))
        .max()
        .unwrap_or(0)
        .max(goal.unwrap_or(0))
        .max(60);
    let round = chosen(env, id, "shape") != SHAPES[1];
    let mark_today = widgets::shows(env, id, "today");
    let n = days.len() as u32;
    let width = u32::from(area.width);
    let short = |weekday: u8| fmt::weekday_short(env, weekday);
    let column_of = |i: u32| (width * i / n, width * (i + 1) / n - width * i / n);
    // Two letters where every column has room for them, else one.
    let (_, narrowest) = column_of(0);
    let two = (0..7).all(|w| font.measure(style.small(), short(w)) * 1.4 <= narrowest as f32);
    // Where even one letter a day is crowded, only the weeks' first days
    // and today are named.
    let every = (0..7)
        .all(|w| font.measure(style.small(), super::initial(env, w)) * 1.6 <= narrowest as f32);
    let first_day = fmt::week_start(env);
    let base = i32::from(chart.y + chart.height);
    let few_grays = env
        .device()
        .display
        .format
        .gray_levels()
        .is_some_and(|g| g <= 4);
    for (i, day) in days.iter().enumerate() {
        let (left, column) = column_of(i as u32);
        let cx = i32::from(area.x) + (left + column / 2) as i32;
        let bar_w = (column * 2 / 5).clamp(2, env.metrics().mm(3).max(2));
        let bx = cx - (bar_w / 2) as i32;
        let radius = if round { bar_w / 4 } else { 0 };
        if day.seconds > 0 {
            let h = (u64::from(chart.height) * u64::from(day.seconds) / most) as u32;
            let h = h.max(if round { radius * 2 } else { thin * 2 }).max(2);
            let bar = Rect::new(bx as u16, (base - h as i32) as u16, bar_w as u16, h as u16);
            let color = if mark_today && !day.today {
                shade(look, 3)
            } else {
                look.ink
            };
            if color == look.ink {
                ui.canvas().fill_round(bar, radius.min(h / 4), color);
            } else {
                ui.canvas().fill_round_shaded(bar, radius.min(h / 4), color);
            }
        } else if !day.to_come {
            // A day without reading: a light stub on the base; where the
            // panel has few grays a thin line in the faint colour, since
            // a light shade there is only a few scattered dots.
            if few_grays {
                let stub = Rect::new(
                    bx as u16,
                    (base - thin as i32) as u16,
                    bar_w as u16,
                    thin as u16,
                );
                ui.canvas().fill(stub, look.faint);
            } else {
                let h = (thin * 2).max(3);
                let stub = Rect::new(bx as u16, (base - h as i32) as u16, bar_w as u16, h as u16);
                ui.canvas()
                    .fill_round_shaded(stub, radius.min(h / 2), shade(look, 1));
            }
        }
        if labels > 0 && (every || day.today || day.weekday == first_day) {
            let name = if two {
                short(day.weekday)
            } else {
                super::initial(env, day.weekday)
            };
            let w = font.measure(style.small(), name);
            // Under its bar, but inside the area (the first and last
            // columns' names may be wider than half a column).
            let right = f32::from(area.x + area.width) - w;
            let at = (cx as f32 - w / 2.0).min(right).max(f32::from(area.x));
            let row = Rect::new(
                area.x,
                chart.y + chart.height + labels - small,
                area.width,
                small,
            );
            let color = if day.today && mark_today {
                look.ink
            } else {
                look.faint
            };
            ui::Label::new(name)
                .px(style.small())
                .color(color)
                .draw_from(&mut ui, at, row);
            if day.today && mark_today {
                // Underlined: seen also where faint text is drawn black.
                let y = i32::from(row.y + row.height) - thin as i32;
                ui.canvas()
                    .fill_rect(at as i32, y, w as u32, thin, look.ink);
            }
        }
    }
    if let Some(goal) = goal {
        let y = base - (u64::from(chart.height) * goal / most) as i32;
        let end = i32::from(area.x + area.width);
        super::dashed(ui.canvas(), i32::from(area.x), end, y, thin, look.faint);
    }
}
