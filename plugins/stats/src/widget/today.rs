//! *Read today*: today's reading time and pages, a bar towards the daily
//! goal ([`GOAL`](super::GOAL)), and below them the week or the goal:
//! "7 min to your daily goal", "Daily goal reached, 3 days in a row".

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetTarget,
};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec};

use super::{caption_room, frame_height, goal, line, shade};
use crate::fmt::{self, seconds_on};
use crate::{overview, totals};

pub(crate) const TODAY: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.today";

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 10] = [
    widgets::frame_spec("stats.widget.today.frame", TODAY, Frame::None),
    widgets::padding_spec("stats.widget.today.padding", TODAY, Padding::Auto),
    widgets::paper_spec("stats.widget.today.paper", TODAY),
    widgets::ink_spec("stats.widget.today.ink", TODAY),
    widgets::size_spec("stats.widget.today.size", TODAY, TextSize::Medium, true),
    widgets::align_spec("stats.widget.today.align", TODAY, Align::Left),
    widgets::part_spec(
        "stats.widget.today.caption",
        TODAY,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec("stats.widget.today.pages", TODAY, "stats.part.pages", true),
    widgets::part_spec("stats.widget.today.bar", TODAY, "stats.part.goal-bar", true),
    widgets::part_spec("stats.widget.today.note", TODAY, "stats.part.note", true),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(TODAY, "stats.widget.today");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    static WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(TODAY, "stats.widget.today", |_| Box::new(Today::default()))
                .category(widgets::categories::READING),
        ];
    r.add_widgets(&WIDGET)
}

/// What the widget shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Numbers {
    today: u64,
    pages: u32,
    week: u64,
    /// Days in a row the goal was reached, up to today (or yesterday,
    /// while today is not done yet).
    streak: u32,
}

fn numbers(env: &Env<'_>) -> Option<Numbers> {
    let (today, days) = super::days(env)?;
    let weekday = fmt::date_of(today).weekday;
    let since = today - i32::from((7 + weekday - fmt::week_start(env)) % 7);
    let streak = goal(env).map_or(0, |goal| {
        let least = goal.min(u64::from(u32::MAX)) as u32;
        totals::streaks(&days, today, least).current
    });
    let pages = if widgets::shows(env, NAME, "pages") {
        overview::distinct_pages(env, today)
    } else {
        0
    };
    Some(Numbers {
        today: u64::from(seconds_on(&days, today)),
        pages,
        week: (since..=today)
            .map(|day| u64::from(seconds_on(&days, day)))
            .sum(),
        streak,
    })
}

/// `text` kept on one line where the note is broken into two.
fn unbroken(text: String) -> String {
    text.replace(' ', "\u{a0}")
}

/// "23 min · 41 pages", and below it the week or the goal: "7 min to
/// your daily goal", "Goal reached, 3 days in a row".
fn lines(env: &Env<'_>, numbers: &Numbers) -> (String, String) {
    let time = fmt::duration(env, numbers.today);
    let first = if widgets::shows(env, NAME, "pages") {
        let pages = env
            .i18n()
            .plural("stats.pages", i64::from(numbers.pages), &[]);
        format!("{time} · {pages}")
    } else {
        time
    };
    let second = match goal(env) {
        None => env.i18n().format(
            "stats.widget.this-week",
            &[(
                "time",
                Arg::Str(&unbroken(fmt::duration(env, numbers.week))),
            )],
        ),
        Some(goal) if numbers.today < goal => {
            let left = unbroken(fmt::duration(env, (goal - numbers.today).max(60)));
            let line = env
                .i18n()
                .format("stats.goal.left", &[("time", Arg::Str(&left))]);
            match numbers.streak {
                0 => line,
                days => format!(
                    "{line} · {}",
                    env.i18n().plural("stats.goal.streak", i64::from(days), &[])
                ),
            }
        }
        Some(_) => match numbers.streak {
            0 | 1 => env.i18n().tr("stats.goal.reached").to_string(),
            days => env
                .i18n()
                .plural("stats.goal.reached-days", i64::from(days), &[]),
        },
    };
    (first, second)
}

/// "Read today": its numbers, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Today {
    numbers: Option<Numbers>,
    read_for: Option<super::Stamp>,
}

impl Widget for Today {
    fn update(&mut self, env: &Env<'_>) -> bool {
        if !super::stale(env, &mut self.read_for) {
            return false;
        }
        let numbers = numbers(env);
        let changed = numbers != self.numbers;
        self.numbers = numbers;
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
        let note = if widgets::shows(env, id, "note") {
            line(&font, style.small())
        } else {
            0
        };
        frame_height(env, id) + caption_room(env, id) + line(&font, size) + bar_band(env) + note
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        match &self.numbers {
            Some(numbers) => draw(canvas, env, &look, numbers),
            None => widgets::draw_message(canvas, env, &look, env.i18n().tr("stats.no-clock")),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        super::open_statistics(area)
    }
}

/// The height of the goal's bar with the room around it, if it shows.
fn bar_band(env: &Env<'_>) -> u16 {
    if goal(env).is_some() && widgets::shows(env, NAME, "bar") {
        env.metrics().mm(3) as u16
    } else {
        0
    }
}

/// How full the goal's bar is, in thousandths.
fn progress(env: &Env<'_>, numbers: &Numbers) -> u64 {
    goal(env).map_or(0, |goal| (numbers.today * 1000 / goal.max(1)).min(1000))
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &widgets::WidgetLook, numbers: &Numbers) {
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    let label = env.i18n().tr("stats.today");
    let mut caption = super::caption(env, id, look.area, &[label]);
    let mut area = super::below_caption(env, caption, look.area);
    let (first, second) = lines(env, numbers);
    // Where they do not all fit, the note goes, then the bar.
    let least = line(&font, style.text());
    // The caption goes before the note: where the note is whole only
    // without it.
    let whole = |area: Rect| {
        let room = area.height.saturating_sub(least + bar_band(env));
        super::note_lines(env, &second, area.width, room, 3).is_some_and(|n| n.whole())
    };
    if caption.is_some() && widgets::shows(env, id, "note") && !whole(area) && whole(look.area) {
        caption = None;
        area = look.area;
    }
    let band = if least + bar_band(env) > area.height {
        0
    } else {
        bar_band(env)
    };
    // The note on as many lines as it needs (three at most, as narrow
    // places break "This week: 4 min" into), shortened where they do
    // not hold it.
    let mut note = if widgets::shows(env, id, "note") {
        let room = area.height.saturating_sub(least + band);
        super::note_lines(env, &second, area.width, room, 3)
    } else {
        None
    };
    let mut note_h = note.as_ref().map_or(0, |n| n.height(env));
    let room = area.height.saturating_sub(band + note_h);
    let mut size = super::text_size(env, &font, id, &first, area.width, room);
    // The note goes where it makes the numbers smaller than the body's
    // size.
    let alone = super::text_size(env, &font, id, &first, area.width, area.height - band);
    if size < style.text() && alone > size {
        note = None;
        note_h = 0;
        size = alone;
    }
    let big = line(&font, size);
    // Too wide with the pages: the time alone.
    let width = f32::from(area.width);
    let first = if font.measure(size, &first) > width {
        fmt::duration(env, numbers.today)
    } else {
        first
    };
    // The lines together in the middle, the caption above them.
    let block = super::captioned(ui.canvas(), env, id, look, caption, big + band + note_h);
    let align = widgets::align(env, id);
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    let first = ui::fit(&font, size, &first, width);
    let x = align.x(area, font.measure(size, &first));
    ui::Label::new(&first)
        .px(size)
        .color(look.ink)
        .draw_from(&mut ui, x, row(block.y, big));
    let mut y = block.y + big;
    if band > 0 {
        // A bar filled up to today's share of the goal, its ends round.
        let height = env.metrics().mm(1).max(4) as u16;
        let track = Rect::new(area.x, y + (band - height) / 2, area.width, height);
        let radius = u32::from(height / 2);
        ui.canvas().fill_round_shaded(track, radius, shade(look, 1));
        let filled = (u64::from(area.width) * progress(env, numbers) / 1000) as u16;
        if filled > 0 {
            let filled = filled.max(height);
            let bar = Rect::new(track.x, track.y, filled, height);
            ui.canvas().fill_round(bar, radius, look.ink);
        }
        y += band;
    }
    if let Some(note) = note {
        note.draw(ui.canvas(), env, row(y, note_h), align, look.faint);
    }
}
