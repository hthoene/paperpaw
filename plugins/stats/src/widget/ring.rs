//! *Daily goal*: today's reading time as a ring that fills towards the
//! daily goal ([`GOAL`](super::GOAL)), the minutes in the middle and the
//! streak below ("5 days in a row"), or what is left to read today.
//!
//! Without a goal the ring stays empty around the minutes read, and below
//! it the widget asks for a goal: choosing it opens the statistics'
//! settings, where the goal is set (it is not measured against a made-up
//! goal, which would tell of a progress nobody asked for).
//!
//! In a place much wider than high the ring sits on the left, the time
//! and the note beside it.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetLook, WidgetTarget,
};
use paperpaw_sdk::{
    Arg, Canvas, Command, Env, Id, Params, Rect, RegisterError, Registrar, SettingSpec,
};

use super::{caption_room, chosen, fit_size, frame_height, goal, line, middle};
use crate::{fmt, totals};

pub(super) const RING: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.goal";
/// A whole ring, or three quarters of one open at the bottom (a gauge).
const STYLES: [&str; 2] = ["stats.ring.full", "stats.ring.gauge"];
/// How thick the ring is.
const LINES: [&str; 3] = ["stats.line.thin", "stats.line.medium", "stats.line.thick"];

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 14] = [
    widgets::frame_spec("stats.widget.goal.frame", RING, Frame::None),
    widgets::padding_spec("stats.widget.goal.padding", RING, Padding::Auto),
    widgets::paper_spec("stats.widget.goal.paper", RING),
    widgets::ink_spec("stats.widget.goal.ink", RING),
    widgets::corners_spec("stats.widget.goal.corners", RING),
    widgets::frame_line_spec("stats.widget.goal.frame-line", RING),
    widgets::shadow_spec("stats.widget.goal.drop-shadow", RING),
    widgets::size_spec("stats.widget.goal.size", RING, TextSize::Fit, true),
    widgets::align_spec("stats.widget.goal.align", RING, Align::Center),
    SettingSpec::choice(
        "stats.widget.goal.style",
        "stats.setting.ring",
        &STYLES,
        STYLES[0],
        RING,
    )
    .description("stats.setting.ring.help"),
    SettingSpec::choice(
        "stats.widget.goal.line",
        "stats.setting.line",
        &LINES,
        LINES[1],
        RING,
    )
    .description("stats.setting.line.help"),
    widgets::part_spec(
        "stats.widget.goal.caption",
        RING,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec("stats.widget.goal.unit", RING, "stats.part.unit", true),
    widgets::part_spec(
        "stats.widget.goal.note",
        RING,
        "stats.part.streak-left",
        true,
    )
    .description("stats.part.streak-left.help"),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(RING, "stats.widget.goal");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    static WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(RING, "stats.widget.goal", |_| Box::new(Ring::default()))
                .category(widgets::categories::READING),
        ];
    r.add_widgets(&WIDGET)
}

/// What the ring shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Numbers {
    today: u64,
    /// Days in a row the goal was reached (any reading without a goal).
    streak: u32,
}

/// Today's goal ring, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Ring {
    numbers: Option<Numbers>,
    read_for: Option<super::Stamp>,
}

impl Widget for Ring {
    fn update(&mut self, env: &Env<'_>) -> bool {
        if !super::stale(env, &mut self.read_for) {
            return false;
        }
        let numbers = super::days(env).map(|(today, days)| {
            let least = goal(env).map_or(1, |g| g.min(u64::from(u32::MAX)) as u32);
            Numbers {
                today: u64::from(fmt::seconds_on(&days, today)),
                streak: totals::streaks(&days, today, least).current,
            }
        });
        let changed = numbers != self.numbers;
        self.numbers = numbers;
        changed
    }

    fn live(&self) -> bool {
        // Asked each minute for a new day; read again only then, or
        // when there is more reading.
        true
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = NAME;
        let ring = largest(env, &style, &font).min(width);
        frame_height(env, id) + caption_room(env, id) + ring + note_height(env, &font, &style)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        match self.numbers {
            Some(numbers) => draw(canvas, env, &look, numbers),
            None => widgets::draw_message(canvas, env, &look, env.i18n().tr("stats.no-clock")),
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        if goal(env).is_some() {
            return super::open_statistics(area);
        }
        // Without a goal: where it is set.
        alloc::vec![WidgetTarget::new(
            area,
            Command::OpenSlot(
                slots::SETTINGS,
                Params::new().with(params::ID, crate::GROUP.as_str()),
            )
        )]
    }
}

/// The ring's largest diameter by the size setting (as large as fits:
/// what a medium one has, for the height it asks for).
fn largest(env: &Env<'_>, style: &UiStyle, font: &paperpaw_sdk::FontHandle<'_>) -> u16 {
    let title = line(font, style.title());
    match widgets::size(env, NAME) {
        TextSize::Small => title * 3,
        TextSize::Medium | TextSize::Fit => title * 9 / 2,
        TextSize::Large => title * 6,
        _ => title * 9 / 2,
    }
}

fn note_height(env: &Env<'_>, font: &paperpaw_sdk::FontHandle<'_>, style: &UiStyle) -> u16 {
    if widgets::shows(env, NAME, "note") || goal(env).is_none() {
        line(font, style.small()) + style.gap() / 2
    } else {
        0
    }
}

/// The note: the streak, what is left today, or the goal reached; without
/// a goal, the request for one.
fn note(env: &Env<'_>, numbers: Numbers) -> Option<String> {
    let Some(goal) = goal(env) else {
        return Some(env.i18n().tr("stats.widget.goal.set").to_string());
    };
    if !widgets::shows(env, NAME, "note") {
        return None;
    }
    Some(if numbers.streak > 0 {
        env.i18n()
            .plural("stats.goal.streak", i64::from(numbers.streak), &[])
    } else if numbers.today >= goal {
        env.i18n().tr("stats.goal.reached").to_string()
    } else {
        let left = fmt::duration(env, (goal - numbers.today).max(60));
        env.i18n()
            .format("stats.goal.to-go", &[("time", Arg::Str(&left))])
    })
}

/// The least diameter of a ring with the minutes inside it: smaller, the
/// widget shows the time as text.
fn least_ring(font: &paperpaw_sdk::FontHandle<'_>, style: &UiStyle) -> u16 {
    line(font, style.text()) * 3 / 2
}

/// "4 min", or with a goal "4 of 30 min".
fn time_text(env: &Env<'_>, numbers: Numbers) -> String {
    let time = fmt::duration(env, numbers.today);
    match goal(env) {
        Some(goal) => env.i18n().format(
            "stats.widget.goal.of",
            &[
                ("time", Arg::Str(&time)),
                ("goal", Arg::Str(&fmt::duration(env, goal))),
            ],
        ),
        None => time,
    }
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, numbers: Numbers) {
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    let title = env.i18n().tr("stats.widget.goal.caption");
    let caption = super::caption(env, id, look.area, &[title]);
    let area = super::below_caption(env, caption, look.area);
    let note = note(env, numbers);
    let limit = if widgets::size(env, id) == TextSize::Fit {
        u16::MAX
    } else {
        largest(env, &style, &font)
    };
    let align = widgets::align(env, id);
    let progress = goal(env).map(|goal| (numbers.today * 1000 / goal.max(1)).min(1000) as u32);
    let small = line(&font, style.small());
    let least = least_ring(&font, &style);
    if area.width >= area.height.saturating_mul(2) && area.height >= least {
        // Wide: the ring, the time and the note beside it, placed
        // together as the alignment says.
        let area = super::captioned(ui.canvas(), env, id, look, caption, area.height);
        let d = area.height.min(area.width * 2 / 5).min(limit);
        let gap = style.gap();
        let room = area.width.saturating_sub(d + gap);
        let first = time_text(env, numbers);
        let size = super::fit_size(&font, &style, &first, room, area.height / 2).min(style.title());
        let big = line(&font, size);
        let first = ui::fit(&font, size, &first, f32::from(room));
        // The note where it fits whole below the time; else, where it
        // fits whole on two lines, the note instead of the time (the
        // ring shows the minutes).
        let (note, instead) = match note {
            Some(n)
                if big + small <= area.height
                    && font.measure(style.small(), &n) <= f32::from(room) =>
            {
                (Some(n), None)
            }
            Some(n) => (
                None,
                super::note_lines(env, &n, room, area.height, 2).filter(|l| l.whole()),
            ),
            None => (None, None),
        };
        if let Some(lines) = instead {
            let wide = lines
                .lines()
                .iter()
                .map(|l| font.measure(style.small(), l))
                .fold(0.0, f32::max);
            let group = d + gap + ((wide + 0.99) as u16).min(room);
            let left = align.x(area, f32::from(group)) as u16;
            let ring = middle(Rect::new(left, area.y, d, area.height), d);
            draw_ring(&mut ui, look, ring, progress, numbers.today, true);
            let text = Rect::new(left + d + gap, area.y, room, area.height);
            lines.draw(ui.canvas(), env, text, Align::Left, look.faint);
            return;
        }
        let wide = note
            .as_ref()
            .map_or(0.0, |n| font.measure(style.small(), n))
            .max(font.measure(size, &first));
        let group = d + gap + ((wide + 0.99) as u16).min(room);
        let left = align.x(area, f32::from(group)) as u16;
        let ring = middle(Rect::new(left, area.y, d, area.height), d);
        draw_ring(&mut ui, look, ring, progress, numbers.today, false);
        let text = Rect::new(left + d + gap, area.y, room, area.height);
        draw_beside(&mut ui, look, text, (&*first, size), note.as_deref());
        return;
    }
    // The ring with the note below, together in the middle; where the
    // note would leave the ring small, the ring alone. The note on two
    // lines where it needs them and they fit whole, else left out.
    let gap = style.gap() / 2;
    let ring_in = |note_h: u16| {
        area.width
            .min(area.height.saturating_sub(note_h))
            .min(limit)
    };
    let lines = note.and_then(|n| super::note_lines(env, &n, area.width, area.height, 2));
    let with_note = lines.as_ref().map(|l| l.height(env) + gap).filter(|&h| {
        let small_ring = progress.is_some() && ring_in(h) < area.width * 2 / 5;
        !small_ring && ring_in(h) >= least
    });
    let note_h = with_note.unwrap_or(0);
    let d = ring_in(note_h);
    if d < least {
        // No room for a ring: the time as text, the note below it.
        draw_time(&mut ui, (look, caption), area, numbers, lines);
        return;
    }
    let note = lines.filter(|_| note_h > 0);
    let block = super::captioned(ui.canvas(), env, id, look, caption, d + note_h);
    let x = align.x(area, f32::from(d)) as u16;
    let ring = Rect::new(x, block.y, d, d);
    draw_ring(&mut ui, look, ring, progress, numbers.today, true);
    if let Some(lines) = note {
        let row = Rect::new(area.x, block.y + d + gap, area.width, note_h - gap);
        lines.draw(ui.canvas(), env, row, align, look.faint);
    }
}

/// The time `size` pixels large in `text` beside the ring, the note below
/// it, together in the middle of its height.
fn draw_beside(
    ui: &mut ui::Ui<'_>,
    look: &WidgetLook,
    text: Rect,
    (first, size): (&str, f32),
    note: Option<&str>,
) {
    let style = *ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    let (big, small) = (line(&font, size), line(&font, style.small()));
    let note_h = if note.is_some() { small } else { 0 };
    let block = middle(text, big + note_h);
    let row = Rect::new(text.x, block.y, text.width, big);
    let x = f32::from(text.x);
    ui::Label::new(first)
        .px(size)
        .color(look.ink)
        .draw_from(ui, x, row);
    if let Some(note) = note {
        let row = Rect::new(text.x, block.y + big, text.width, small);
        ui::Label::new(note)
            .px(style.small())
            .color(look.faint)
            .draw_from(ui, x, row);
    }
}

/// The time read as text where there is no room for a ring, as large as
/// fits, with the note below it where that fits too.
fn draw_time(
    ui: &mut ui::Ui<'_>,
    (look, caption): (&WidgetLook, Option<&str>),
    area: Rect,
    numbers: Numbers,
    note: Option<widgets::Wrapped>,
) {
    let env = ui.env();
    let style = *ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    let first = time_text(env, numbers);
    let note_h = note.as_ref().map_or(0, |n| n.height(env));
    // The time's size in `area` with the note below it (if `with`), and
    // whether the note keeps: not where it leaves no room or makes the
    // time smaller than the body's size.
    let lay = |area: Rect, with: bool| {
        let fits = |note_h: u16| {
            let room = area.height.saturating_sub(note_h);
            let size = super::text_size(env, &font, id, &first, area.width, room);
            (size, line(&font, size))
        };
        let (size, big) = fits(note_h);
        let alone = fits(0);
        if !with || big + note_h > area.height || (size < style.text() && alone.0 > size) {
            (alone.0, alone.1, false)
        } else {
            (size, big, true)
        }
    };
    let (mut size, mut big, mut keep) = lay(area, note.is_some());
    let (mut area, mut caption) = (area, caption);
    // The caption goes before the note.
    if note.is_some() && !keep && caption.is_some() {
        let without = lay(look.area, true);
        if without.2 {
            (size, big, keep) = without;
            (area, caption) = (look.area, None);
        }
    }
    let note = note.filter(|_| keep);
    let note_h = note.as_ref().map_or(0, |n| n.height(env));
    let align = widgets::align(env, id);
    let block = super::captioned(ui.canvas(), env, id, look, caption, big + note_h);
    let width = f32::from(area.width);
    let shown = super::number_if_narrow(&font, size, &first, area.width);
    let shown = ui::fit(&font, size, shown, width);
    let x = align.x(area, font.measure(size, &shown));
    let row = Rect::new(area.x, block.y, area.width, big);
    ui::Label::new(&shown)
        .px(size)
        .color(look.ink)
        .draw_from(ui, x, row);
    if let Some(note) = note {
        let row = Rect::new(area.x, block.y + big, area.width, note_h);
        note.draw(ui.canvas(), env, row, align, look.faint);
    }
}

/// Draws the ring into the square `at`: a thin track all round (three
/// quarters for a gauge), over it the share of the goal reached
/// (`progress` in thousandths) with round ends, and the minutes read
/// inside (with "min" below them if `unit` and the unit shows).
fn draw_ring(
    ui: &mut ui::Ui<'_>,
    look: &WidgetLook,
    at: Rect,
    progress: Option<u32>,
    seconds: u64,
    unit: bool,
) {
    let env = ui.env();
    let style = *ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    let id = NAME;
    let d = u32::from(at.width.min(at.height));
    if d < 8 {
        return;
    }
    let center_y = at.y + (d / 2) as u16;
    let thick = match LINES.iter().position(|l| *l == chosen(env, id, "line")) {
        Some(0) => d / 20,
        Some(2) => d / 8,
        _ => d / 12,
    }
    // Small rings thicker, to tell the share from the track.
    .max(3)
    .max(if d < 80 { d / 8 } else { 0 });
    let gauge = chosen(env, id, "style") == STYLES[1];
    super::draw_arc(ui.canvas(), env, look, at, progress, thick, gauge);
    let track = u32::from(style.line()).max(1).min(thick);
    // The minutes inside.
    let inner = d.saturating_sub(2 * thick + 2 * track) as u16;
    let minutes = format!("{}", seconds / 60);
    let unit = unit && widgets::shows(env, id, "unit");
    let unit_h = if unit { line(&font, style.small()) } else { 0 };
    let room = (u32::from(inner) * 45 / 100) as u16;
    let size = fit_size(&font, &style, "000", inner * 3 / 5, room);
    let big = line(&font, size);
    let unit_h = if unit_h + big <= inner { unit_h } else { 0 };
    let shown = middle(
        Rect::new(at.x, center_y.saturating_sub(inner / 2), at.width, inner),
        big + unit_h,
    );
    let w = font.measure(size, &minutes);
    if w > f32::from(inner) * 0.9 || big > inner {
        // A ring too small for the minutes: the ring alone.
        return;
    }
    let row = Rect::new(at.x, shown.y, at.width, big);
    let cx = f32::from(at.x) + d as f32 / 2.0;
    ui::Label::new(&minutes)
        .px(size)
        .color(look.ink)
        .draw_from(ui, cx - w / 2.0, row);
    if unit_h > 0 {
        let text = env.i18n().tr("stats.unit.minutes");
        let w = font.measure(style.small(), text);
        let row = Rect::new(at.x, shown.y + big, at.width, unit_h);
        ui::Label::new(text)
            .px(style.small())
            .color(look.faint)
            .draw_from(ui, cx - w / 2.0, row);
    }
}
