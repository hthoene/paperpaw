//! The widgets: *Pomodoro timer* (`pomodoro.timer`), *Pomodoro button*
//! (`pomodoro.button`) and *Pomodoro today* (`pomodoro.today`).
//!
//! Each reads one [`Snapshot`] of the shared state when it is updated and
//! draws from it in whatever room its place has (measured in pixels, not
//! cells, so a wide low place and a tall narrow one both read well): a
//! figure when small, with the phase, dots, bar and hint as the room allows. They are read
//! when the timer changes (the plugin says so) and each minute while it
//! runs, which is as often as the minutes they show change: no widget
//! asks for a redraw of its own.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, CellSpan, Frame, Padding, Widget, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Env, Id, Point, Rect, RegisterError, Registrar, SettingSpec,
};

use crate::phase::{self, Log, Phase};
use crate::state::{Shared, today};
use crate::{OPEN, TOGGLE, goal, lengths};

pub(crate) const TIMER: Id = Id::new_static(TIMER_NAME);
pub(crate) const BUTTON: Id = Id::new_static(BUTTON_NAME);
pub(crate) const TODAY: Id = Id::new_static(TODAY_NAME);
const TIMER_NAME: &str = "pomodoro.timer";
const BUTTON_NAME: &str = "pomodoro.button";
const TODAY_NAME: &str = "pomodoro.today";

/// The settings of the widgets, kept in the program: the look, and the
/// parts one may leave out.
static SETTINGS: [SettingSpec; 9] = [
    widgets::frame_spec("pomodoro.timer.frame", TIMER, Frame::None),
    widgets::padding_spec("pomodoro.timer.padding", TIMER, Padding::Auto),
    widgets::part_spec("pomodoro.timer.bar", TIMER, "pomodoro.part.bar", true),
    widgets::part_spec("pomodoro.timer.dots", TIMER, "pomodoro.part.dots", true),
    widgets::frame_spec("pomodoro.button.frame", BUTTON, Frame::Line),
    widgets::padding_spec("pomodoro.button.padding", BUTTON, Padding::Auto),
    widgets::frame_spec("pomodoro.today.frame", TODAY, Frame::None),
    widgets::padding_spec("pomodoro.today.padding", TODAY, Padding::Auto),
    widgets::part_spec("pomodoro.today.week", TODAY, "pomodoro.part.week", true),
];

pub(crate) fn register(r: &mut Registrar, shared: &Shared) -> Result<(), RegisterError> {
    for (id, title) in [
        (TIMER, "pomodoro.timer"),
        (BUTTON, "pomodoro.button"),
        (TODAY, "pomodoro.today"),
    ] {
        r.add_setting_group(widgets::settings_group(&id, title))?;
    }
    r.add_settings(&SETTINGS)?;
    let specs = [
        (TIMER, "pomodoro.timer", Kind::Timer, CellSpan::new(2, 2)),
        (BUTTON, "pomodoro.button", Kind::Button, CellSpan::new(1, 1)),
        (TODAY, "pomodoro.today", Kind::Today, CellSpan::new(2, 1)),
    ];
    for (id, title, kind, cells) in specs {
        let shared = shared.clone();
        r.add_widget(
            WidgetSpec::new(id, title, move |_| {
                Box::new(Pomo {
                    kind,
                    shared: shared.clone(),
                    snap: None,
                })
            })
            .category(widgets::categories::READING)
            .cells(cells),
        )?;
    }
    Ok(())
}

/// What the widgets show of the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub phase: Phase,
    pub running: bool,
    /// Whole minutes left, counted up.
    pub minutes: u64,
    /// Thousandths of the phase gone.
    pub gone: u16,
    pub done: u32,
    pub rounds: u32,
    pub log: Log,
    pub goal: Option<u32>,
}

impl Snapshot {
    fn read(env: &Env<'_>, shared: &Shared) -> Snapshot {
        let st = shared.borrow();
        let set = lengths(env);
        let whole = set.ms(st.phase);
        let left = st.remaining(env.now(), whole).min(whole);
        // The count of a day gone by is not today's.
        let mut log = st.log;
        log.roll(today(env));
        Snapshot {
            phase: st.phase,
            running: st.running,
            minutes: phase::minutes_left(left),
            gone: phase::gone(left, whole),
            done: st.done,
            rounds: set.rounds.max(2),
            log,
            goal: goal(env),
        }
    }

    /// A phase not begun: the full length, nothing gone.
    fn fresh(&self) -> bool {
        !self.running && self.gone == 0
    }
}

/// "3 rounds today", or "3 of 8 rounds today" with a goal.
pub(crate) fn count_line(env: &Env<'_>, log: &Log) -> String {
    let count = i64::from(log.today());
    match goal(env) {
        Some(goal) => env.i18n().plural(
            "pomodoro.today.goal",
            i64::from(goal),
            &[
                ("count", Arg::Int(count)),
                ("goal", Arg::Int(i64::from(goal))),
            ],
        ),
        None => env
            .i18n()
            .plural("pomodoro.today", count, &[("count", Arg::Int(count))]),
    }
}

/// "14 this week", when the week holds more than today.
pub(crate) fn week_line(env: &Env<'_>, log: &Log) -> Option<String> {
    let week = i64::from(log.week());
    (week > i64::from(log.today())).then(|| {
        env.i18n()
            .plural("pomodoro.week", week, &[("count", Arg::Int(week))])
    })
}

/// The dots of a set, `done` of `every` filled, centred in `area`.
pub(crate) fn dots(canvas: &mut Canvas<'_>, area: Rect, every: u32, done: u32, ink: Color) {
    let radius = i32::from((area.height * 3 / 10).clamp(3, 14));
    let mut step = radius * 3;
    let most = i32::from(area.width) / every.max(1) as i32;
    step = step.min(most.max(radius * 2 + 2));
    let width = step * (every as i32 - 1);
    let x0 = i32::from(area.x) + i32::from(area.width) / 2 - width / 2;
    let y = i32::from(area.y) + i32::from(area.height) / 2;
    for n in 0..every {
        let c = Point::new(x0 + step * n as i32, y);
        if n < done {
            canvas.fill_circle(c, radius as u32, ink);
        } else {
            canvas.stroke_circle(c, radius as u32, 2, ink);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Timer,
    Button,
    Today,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Timer => TIMER_NAME,
            Kind::Button => BUTTON_NAME,
            Kind::Today => TODAY_NAME,
        }
    }
}

struct Pomo {
    kind: Kind,
    shared: Shared,
    snap: Option<Snapshot>,
}

/// The pieces of a widget, top to bottom, and the room they ask for.
struct Bands {
    next: u16,
    end: u16,
}

impl Bands {
    fn new(area: Rect) -> Self {
        Bands {
            next: area.y,
            end: area.y + area.height,
        }
    }

    fn left(&self) -> u16 {
        self.end.saturating_sub(self.next)
    }

    /// The next `height` pixels of `area`.
    fn take(&mut self, area: Rect, height: u16) -> Rect {
        let band = Rect::new(area.x, self.next, area.width, height.min(self.left()));
        self.next += band.height;
        band
    }
}

impl Pomo {
    fn snap(&self) -> Option<&Snapshot> {
        self.snap.as_ref()
    }

    /// Phase, minutes left, the bar and dots, and a hint what a tap does:
    /// as many as the place has room for.
    fn draw_timer(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, snap: &Snapshot) {
        let look = widgets::draw_frame(canvas, env, TIMER_NAME, area);
        let inner = look.area;
        let small = UiStyle::new(env).small();
        let line = widgets::line_height(env, small);
        let i18n = env.i18n();
        let hint_key = if snap.fresh() {
            "pomodoro.hint.start"
        } else {
            "pomodoro.hint.resume"
        };

        // What the place has room for, most wanted first; the time keeps
        // at least two lines.
        let mut room = inner.height.saturating_sub(2 * line);
        let mut want = |height: u16, wanted: bool| {
            let fits = wanted && room >= height;
            if fits {
                room -= height;
            }
            fits
        };
        let name = want(line, true);
        let dots_h = line;
        let dots_wide = inner.width >= (small as u16).saturating_mul(snap.rounds as u16);
        let dots_on = want(dots_h, dots_wide && widgets::shows(env, TIMER_NAME, "dots"));
        let bar_h = (line / 3).max(6);
        let tall = inner.height >= 6 * line;
        let bar_on = want(bar_h, tall && widgets::shows(env, TIMER_NAME, "bar"));
        let hint = want(
            line,
            !snap.running && fits(env, small, i18n.tr(hint_key), inner.width),
        );

        let mut bands = Bands::new(inner);
        if name {
            let long = i18n.tr(snap.phase.key());
            let text = if fits(env, small, long, inner.width) {
                long
            } else {
                i18n.tr(snap.phase.short_key())
            };
            let band = bands.take(inner, line);
            put_line(canvas, env, band, small, small * 0.6, text, look.faint);
        }
        let below =
            u16::from(dots_on) * dots_h + u16::from(bar_on) * bar_h + u16::from(hint) * line;
        let time = bands.take(inner, bands.left().saturating_sub(below));
        put_line(
            canvas,
            env,
            time,
            f32::from(time.height),
            small,
            &minutes_text(env, snap, inner.width),
            look.ink,
        );
        if bar_on {
            let band = bands.take(inner, bar_h);
            let style = UiStyle::new(env);
            ui::Ui::new(canvas, env, &style).progress(band, snap.gone, look.ink);
        }
        if dots_on {
            let band = bands.take(inner, dots_h);
            dots(canvas, band, snap.rounds, snap.done, look.ink);
        }
        if hint {
            let band = bands.take(inner, line);
            put_line(
                canvas,
                env,
                band,
                small,
                small * 0.6,
                i18n.tr(hint_key),
                look.faint,
            );
        }
    }

    /// Start or Pause in a button, and the time left beneath it where
    /// there is room.
    fn draw_button(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, snap: &Snapshot) {
        let look = widgets::draw_frame(canvas, env, BUTTON_NAME, area);
        let inner = look.area;
        let style = UiStyle::new(env);
        let small = style.small();
        let line = widgets::line_height(env, small);
        let i18n = env.i18n();
        let label = i18n.tr(if snap.running {
            "pomodoro.pause"
        } else if snap.fresh() {
            "pomodoro.start"
        } else {
            "pomodoro.resume"
        });
        // No room for the word: the sign of what a tap does.
        if !fits(env, style.text(), label, inner.width) {
            glyph(canvas, inner, snap.running, look.ink);
            canvas.describe(Role::Button, label, inner);
            return;
        }
        let two = inner.height >= 3 * line;
        let mut bands = Bands::new(inner);
        let main = bands.take(
            inner,
            if two {
                inner.height - line
            } else {
                inner.height
            },
        );
        put_line(canvas, env, main, style.text(), small, label, look.ink);
        if two {
            let band = bands.take(inner, line);
            let text = alloc::format!(
                "{} · {}",
                i18n.tr(snap.phase.short_key()),
                minutes_text(env, snap, inner.width)
            );
            put_line(canvas, env, band, small, small * 0.6, &text, look.faint);
        }
    }

    /// The day's count, large, the goal beside it, and the week below.
    fn draw_today(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, snap: &Snapshot) {
        let look = widgets::draw_frame(canvas, env, TODAY_NAME, area);
        let inner = look.area;
        let style = UiStyle::new(env);
        let small = style.small();
        let line = widgets::line_height(env, small);
        let week = if inner.height < 3 * line || !widgets::shows(env, TODAY_NAME, "week") {
            None
        } else {
            week_line(env, &snap.log)
        };
        let below = if week.is_some() { line } else { 0 };
        let mut bands = Bands::new(inner);
        let main = bands.take(inner, inner.height.saturating_sub(below));
        let said = count_line(env, &snap.log);
        let text = if fits(env, small, &said, inner.width) {
            said
        } else {
            alloc::format!("{}", snap.log.today())
        };
        put_line(
            canvas,
            env,
            main,
            f32::from(main.height),
            small,
            &text,
            look.ink,
        );
        if let Some(week) = week {
            let band = bands.take(inner, line);
            put_line(canvas, env, band, small, small * 0.6, &week, look.faint);
        }
    }
}

/// The sign of a button in `area`: two bars (pause) while it runs, else a
/// triangle (start).
fn glyph(canvas: &mut Canvas<'_>, area: Rect, running: bool, ink: Color) {
    let side = i32::from(area.width.min(area.height)) / 2;
    if side < 4 {
        return;
    }
    let x0 = i32::from(area.x) + (i32::from(area.width) - side) / 2;
    let y0 = i32::from(area.y) + (i32::from(area.height) - side) / 2;
    if running {
        let bar = (side / 3) as u32;
        canvas.fill_rect(x0, y0, bar, side as u32, ink);
        canvas.fill_rect(x0 + side - bar as i32, y0, bar, side as u32, ink);
        return;
    }
    // Pointing right, one pixel row at a time.
    let half = side / 2;
    for row in 0..side {
        let rise = half - (row - half).abs();
        let width = (side * rise / half.max(1)).max(1) as u32;
        canvas.fill_rect(x0, y0 + row, width, 1, ink);
    }
}

/// `text` centred on one line in `band`, from `max` down to `min` pixels
/// and then shortened ([`widgets::fit_line`]); the whole band is what a
/// screen reader gets, so the text may change with the language.
fn put_line(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    band: Rect,
    max: f32,
    min: f32,
    text: &str,
    ink: Color,
) {
    let Some(font) = UiStyle::new(env).font(env) else {
        return;
    };
    let (size, shown) = widgets::fit_line(&font, max, min, text, f32::from(band.width));
    let x = Align::Center.x(band, font.measure(size, &shown));
    canvas.describe(Role::Text, text, band);
    let baseline = widgets::ink_baseline(&font, size, &shown, band);
    font.draw(canvas, size, x, baseline, &shown, ink);
}

/// Whether `text` fits `width` pixels on one line at `px`.
fn fits(env: &Env<'_>, px: f32, text: &str, width: u16) -> bool {
    UiStyle::new(env)
        .font(env)
        .is_none_or(|font| font.measure(px, text) <= f32::from(width))
}

/// "24 min" (just "24" where that does not fit `width`): the last minute
/// reads "1 min", never "0".
fn minutes_text(env: &Env<'_>, snap: &Snapshot, width: u16) -> String {
    let n = i64::try_from(snap.minutes).unwrap_or(0);
    let long = env
        .i18n()
        .plural("pomodoro.widget.time", n, &[("count", Arg::Int(n))]);
    if fits(env, UiStyle::new(env).small(), &long, width) {
        long
    } else {
        alloc::format!("{n}")
    }
}

impl Widget for Pomo {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let now = Snapshot::read(env, &self.shared);
        let changed = self.snap.as_ref() != Some(&now);
        self.snap = Some(now);
        changed
    }

    fn live(&self) -> bool {
        // Minutes pass while it runs; a new day starts the count over.
        self.kind == Kind::Today || self.snap().is_none_or(|s| s.running)
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        let frame = widgets::frame_height(env, self.kind.name());
        let style = UiStyle::new(env);
        let line = widgets::line_height(env, style.small());
        frame
            + match self.kind {
                Kind::Timer => (width / 2).max(4 * line),
                Kind::Button => 3 * line,
                Kind::Today => 3 * line,
            }
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let Some(snap) = self.snap() else {
            return;
        };
        match self.kind {
            Kind::Timer => self.draw_timer(canvas, env, area, snap),
            Kind::Button => self.draw_button(canvas, env, area, snap),
            Kind::Today => self.draw_today(canvas, env, area, snap),
        }
    }

    fn targets(&self, _env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        // A tap starts or pauses; the timer and the count open the screen
        // on a long press (the button is only a button).
        let tap = match self.kind {
            Kind::Today => Command::run(OPEN),
            _ => Command::run(TOGGLE),
        };
        let target = WidgetTarget::new(area, tap);
        alloc::vec![if self.kind == Kind::Button {
            target
        } else {
            target.hold(Command::run(OPEN))
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_are_the_widgets() {
        for spec in &SETTINGS {
            let id = spec.id.as_str();
            let group = spec.group.as_str();
            assert!(id.starts_with(group), "{id} is not in {group}");
        }
    }
}
