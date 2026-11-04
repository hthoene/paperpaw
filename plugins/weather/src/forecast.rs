//! The widget "Forecast": the next days, each with its weekday, a symbol
//! and its highest and lowest temperature (and the chance of rain, if
//! chosen); in a row of columns where the place is wide, as a list where
//! it is tall (or as set). In the list, a bar shows each day's range
//! within the week's.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetLook, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{
    Arg, Canvas, Color, Env, FontHandle, Id, Rect, RegisterError, Registrar, SettingSpec,
};

use crate::data::{self, Day, Temperature};
use crate::draw::{self, digits_height, draw_digits};
use crate::symbol::Style;
use crate::{Shared, Shown};

pub(crate) const ID: Id = Id::new_static("weather.forecast");
/// How many days it shows.
const DAYS: Setting<i64> = Setting::new("weather.forecast.days");
/// The values of the first day shown.
const STARTS: [&str; 2] = ["weather.start.today", "weather.start.tomorrow"];
/// The values of its layout.
const LAYOUTS: [&str; 3] = [
    "weather.layout.auto",
    "weather.layout.row",
    "weather.layout.list",
];
/// The values of how weekdays are named.
const NAMES: [&str; 3] = [
    "weather.names.short",
    "weather.names.initial",
    "weather.names.long",
];
/// Its look, kept in the program.
static LOOK: [SettingSpec; 9] = [
    widgets::frame_spec("weather.forecast.frame", ID, Frame::None),
    widgets::padding_spec("weather.forecast.padding", ID, Padding::Auto),
    widgets::paper_spec("weather.forecast.paper", ID),
    widgets::ink_spec("weather.forecast.ink", ID),
    widgets::corners_spec("weather.forecast.corners", ID),
    widgets::frame_line_spec("weather.forecast.frame-line", ID),
    widgets::shadow_spec("weather.forecast.drop-shadow", ID),
    widgets::size_spec("weather.forecast.size", ID, TextSize::Fit, true),
    widgets::align_spec("weather.forecast.align", ID, Align::Center),
];
/// The rest of its settings after the days, kept in the program: what
/// it shows and how.
/// How many days, counted with their unit.
static IN_DAYS: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().unit("weather.unit.days");
static DAYS_SETTING: [SettingSpec; 1] =
    [
        SettingSpec::number(DAYS.static_str(), "weather.setting.days", 3, 7, 5, ID)
            .with_extra(&IN_DAYS)
            .description("weather.setting.days.help"),
    ];

static SETTINGS: [SettingSpec; 11] = [
    SettingSpec::choice(
        "weather.forecast.start",
        "weather.setting.start",
        &STARTS,
        STARTS[0],
        ID,
    )
    .description("weather.setting.start.help"),
    SettingSpec::choice(
        "weather.forecast.layout",
        "weather.setting.layout",
        &LAYOUTS,
        LAYOUTS[0],
        ID,
    )
    .description("weather.setting.layout.help"),
    SettingSpec::choice(
        "weather.forecast.names",
        "weather.setting.names",
        &NAMES,
        NAMES[0],
        ID,
    )
    .description("weather.setting.names.help"),
    draw::style_spec("weather.forecast.style", ID),
    widgets::part_spec("weather.forecast.symbol", ID, "weather.part.symbol", true),
    widgets::part_spec("weather.forecast.low", ID, "weather.part.low", true),
    widgets::part_spec("weather.forecast.rain", ID, "weather.part.rain-days", false),
    widgets::part_spec("weather.forecast.bars", ID, "weather.part.bars", true),
    widgets::part_spec("weather.forecast.today", ID, "weather.part.today", true),
    widgets::part_spec("weather.forecast.place", ID, "weather.part.place", false),
    widgets::part_spec("weather.forecast.lines", ID, "weather.part.lines", false),
];

pub(crate) fn register(r: &mut Registrar, state: &Shared) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "weather.widget.forecast");
        &SPEC
    })?;
    r.add_settings(&LOOK)?;
    r.add_settings(&DAYS_SETTING)?;
    r.add_settings(&SETTINGS)?;
    let state = state.clone();
    r.add_widget(
        WidgetSpec::new(ID, "weather.widget.forecast", move |env| {
            Box::new(Forecast {
                shown: Shown::read(env, &state),
                state: state.clone(),
            })
        })
        .category(widgets::categories::WORLD),
    )
}

/// "Forecast": what it shows, read when updated.
struct Forecast {
    state: Shared,
    shown: Shown,
}

impl Widget for Forecast {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = Shown::read(env, &self.state);
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let probe = Rect::new(0, 0, 100, 100);
        let frame = 100 - widgets::widget_look(env, ID.as_str(), probe).area.height;
        let small = widgets::line_height(env, style.small());
        small * 3 + digits_height(style.title()) * 2 + frame
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let days = self
            .shown
            .weather
            .as_ref()
            .map(|w| shown_days(env, w.days_from(self.shown.today)))
            .unwrap_or_default();
        if days.is_empty() {
            draw::draw_waiting(canvas, env, ID.as_str(), (&look, area), &self.shown);
            return;
        }
        draw_forecast(canvas, env, &look, &self.shown, &days);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget::new(area, crate::tap(&self.shown))]
    }
}

/// The days shown: from today or tomorrow, as many as set.
fn shown_days(env: &Env<'_>, days: &[Day]) -> Vec<Day> {
    let skip = usize::from(env.settings().text(&format!("{ID}.start")) == STARTS[1]);
    let count = env.settings().read(&DAYS).clamp(1, 7) as usize;
    days.iter().skip(skip).take(count).copied().collect()
}

/// A weekday's name as set: `long` "Monday", short "Mon", or its initial.
fn day_name(env: &Env<'_>, day: &Day, today: Option<i32>, long: bool, initial: bool) -> String {
    let weekday = data::weekday(day.date);
    if today == Some(day.date) && widgets::shows(env, ID.as_str(), "today") && !initial {
        return env.i18n().tr("weather.today").to_string();
    }
    if long {
        return env
            .i18n()
            .tr(&format!("core.weekday.{weekday}"))
            .to_string();
    }
    let key = format!("weather.weekday.{weekday}");
    let short = env.i18n().tr(&key);
    if initial {
        short.chars().next().map(String::from).unwrap_or_default()
    } else {
        short.to_string()
    }
}

/// How the weekdays are named: (long, initial).
fn naming(env: &Env<'_>) -> (bool, bool) {
    let names = env.settings().text(&format!("{ID}.names"));
    (names == NAMES[2], names == NAMES[1])
}

/// Whether the days go in a list, one below the other.
fn as_list(env: &Env<'_>, area: Rect, count: usize) -> bool {
    match LAYOUTS
        .iter()
        .position(|l| *l == env.settings().text(&format!("{ID}.layout")))
    {
        Some(1) => false,
        Some(2) => true,
        // Tall, or too narrow for the columns.
        _ => {
            u32::from(area.height) * 10 > u32::from(area.width) * 12
                || u32::from(area.width) < count as u32 * 40
        }
    }
}

/// The temperatures' size as set; `None` for "as large as it fits".
fn temperature_size(env: &Env<'_>, style: &UiStyle) -> Option<f32> {
    match widgets::size(env, ID.as_str()) {
        TextSize::Small => Some(style.text()),
        TextSize::Medium => Some(style.title()),
        TextSize::Large => Some(style.title() * 1.4),
        TextSize::Fit => None,
        _ => None,
    }
}

fn draw_forecast(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    shown: &Shown,
    days: &[Day],
) {
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let Some(font) = ui.font() else {
        return;
    };
    let mut caption = Vec::new();
    if widgets::shows(env, ID.as_str(), "place") && !shown.place.is_empty() {
        caption.push(shown.place.clone());
    }
    if let Some(age) = shown.age_text(env) {
        caption.push(age);
    }
    let area = if caption.is_empty() {
        look.area
    } else {
        widgets::draw_caption(ui.canvas(), env, ID.as_str(), look, &caption.join(" · "))
    };
    let days = if as_list(env, area, days.len()) {
        days
    } else {
        // As many columns as have room.
        let least = font.measure(style.small(), "-88°") * 1.4;
        let fit = ((f32::from(area.width) / least) as usize).max(1);
        &days[..days.len().min(fit)]
    };
    let context = Context {
        env,
        font: &font,
        style,
        look,
        unit: crate::temperature_unit(env),
        symbols: draw::style(env, ID.as_str()),
        today: shown.today,
    };
    if as_list(env, area, days.len()) {
        context.list(&mut ui, area, days);
    } else {
        context.row(&mut ui, area, days);
    }
}

/// What drawing the days needs.
struct Context<'a, 'e> {
    env: &'a Env<'e>,
    font: &'a FontHandle<'e>,
    style: UiStyle,
    look: &'a WidgetLook,
    unit: Temperature,
    symbols: Style,
    today: Option<i32>,
}

impl Context<'_, '_> {
    fn shows(&self, part: &str) -> bool {
        widgets::shows(self.env, ID.as_str(), part)
    }

    /// Draws a light line in `area`: a gray, dithered where the panel has
    /// few grays; on a dark card the faint colour.
    fn rule(&self, canvas: &mut Canvas<'_>, area: Rect) {
        if self.look.paper.luma() < 128 {
            canvas.fill(area, self.look.faint);
        } else {
            canvas.fill_shaded(area, Color::gray(150));
        }
    }

    fn degrees(&self, tenths: i32) -> String {
        data::degrees_text(self.unit, tenths, false)
    }

    fn rain(&self, day: &Day) -> Option<String> {
        let rain = day.rain.filter(|_| self.shows("rain"))?;
        Some(
            self.env
                .i18n()
                .format("weather.percent", &[("percent", Arg::Int(i64::from(rain)))]),
        )
    }

    /// The days side by side, each a column: its name, symbol, highest,
    /// lowest and chance of rain, one below the other.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn row(&self, ui: &mut ui::Ui<'_>, area: Rect, days: &[Day]) {
        let style = self.style;
        let font = self.font;
        let count = days.len().max(1) as u16;
        let column = area.width / count;
        let gap = style.gap() / 2;
        let with_symbol = self.shows("symbol");
        let mut with_low = self.shows("low");
        let with_rain = days.iter().any(|d| self.rain(d).is_some());
        let rain_h = if with_rain {
            widgets::line_height(self.env, style.small())
        } else {
            0
        };
        // The temperatures as large as the column allows, as set, and
        // smaller where the column is low; the lowest left out where even
        // the smallest do not fit.
        let widest = (font.measure(style.title(), "-88°") / style.title()).max(0.1);
        let fit = (f32::from(column) * 0.85 / widest).min(style.title() * 1.5);
        let mut high = temperature_size(self.env, &style)
            .unwrap_or(fit)
            .min(fit)
            .max(style.small());
        let name_of = |high: f32| {
            if high > style.title() * 1.2 {
                style.text()
            } else {
                style.small()
            }
        };
        let low_of = |high: f32| (high * 0.8).max(style.small());
        let words_of = |high: f32, low: bool| {
            widgets::line_height(self.env, name_of(high))
                + digits_height(draw::effective(font, high))
                + if low {
                    digits_height(draw::effective(font, low_of(high)))
                } else {
                    0
                }
                + rain_h
        };
        while words_of(high, with_low) > area.height && high > style.small() * 0.8 {
            high -= 1.0;
        }
        // The symbols rather than the largest digits: smaller digits where
        // that leaves room for them.
        if with_symbol {
            // As the symbol is placed below: at least a name's height.
            let room = |high: f32| {
                let name = widgets::line_height(self.env, name_of(high));
                area.height
                    .saturating_sub(words_of(high, with_low) + gap * 2)
                    >= name
            };
            let mut smaller = high;
            while !room(smaller) && smaller > style.text() {
                smaller -= 1.0;
            }
            if room(smaller) {
                high = smaller;
            }
        }
        if with_low && words_of(high, true) > area.height {
            with_low = false;
        }
        // Without room below, the lowest beside the highest.
        let low_beside = !with_low && self.shows("low");
        let high = draw::effective(font, high);
        let low = draw::effective(font, low_of(high));
        let name = name_of(high);
        let name_h = widgets::line_height(self.env, name);
        let high_h = digits_height(high);
        let low_h = if with_low { digits_height(low) } else { 0 };
        let words = name_h + high_h + low_h + rain_h;
        let side = if with_symbol {
            let room = area.height.saturating_sub(words + gap * 2);
            room.min(column * 3 / 4)
        } else {
            0
        };
        let side = if side < name_h { 0 } else { side };
        let total = words + side + if side > 0 { gap * 2 } else { 0 };
        let top = area.y + area.height.saturating_sub(total) / 2;
        let (long, initial) = naming(self.env);
        for (i, day) in days.iter().enumerate() {
            let cell = Rect::new(area.x + column * i as u16, area.y, column, area.height);
            let centre = |w: f32| f32::from(cell.x) + (f32::from(cell.width) - w) / 2.0;
            let mut y = top;
            // The name: shorter where it does not fit.
            let mut label = day_name(self.env, day, self.today, long, initial);
            if font.measure(name, &label) > f32::from(column) {
                label = day_name(self.env, day, None, false, false);
            }
            if font.measure(name, &label) > f32::from(column) {
                label = day_name(self.env, day, None, false, true);
            }
            let color = if self.today == Some(day.date) {
                self.look.ink
            } else {
                self.look.faint
            };
            let row = Rect::new(cell.x, y, cell.width, name_h);
            ui::Label::new(&label).px(name).color(color).draw_from(
                ui,
                centre(font.measure(name, &label)),
                row,
            );
            y += name_h;
            if side > 0 {
                y += gap;
                let x = cell.x + (cell.width - side) / 2;
                let (_, symbol) = data::condition(day.code);
                let at = Rect::new(x, y, side, side);
                draw::draw_symbol(ui.canvas(), at, (symbol, true), self.symbols, self.look.ink);
                y += side + gap;
            }
            let text = self.degrees(day.high);
            let row = Rect::new(cell.x, y, cell.width, high_h);
            let high_w = font.measure(high, &text);
            let beside = self.degrees(day.low);
            let space = f32::from(gap);
            let beside_w = font.measure(low, &beside);
            if low_beside && high_w + space + beside_w <= f32::from(column) * 0.95 {
                let x = centre(high_w + space + beside_w);
                draw_digits(ui, font, high, x, row, &text, self.look.ink);
                let x = x + high_w + space;
                draw_digits(ui, font, low, x, row, &beside, self.look.faint);
            } else {
                draw_digits(ui, font, high, centre(high_w), row, &text, self.look.ink);
            }
            y += high_h;
            if with_low {
                let text = self.degrees(day.low);
                let row = Rect::new(cell.x, y, cell.width, low_h);
                let x = centre(font.measure(low, &text));
                draw_digits(ui, font, low, x, row, &text, self.look.faint);
                y += low_h;
            }
            if let Some(rain) = self.rain(day) {
                let row = Rect::new(cell.x, y, cell.width, rain_h);
                let x = centre(font.measure(style.small(), &rain));
                ui::Label::new(&rain)
                    .px(style.small())
                    .color(self.look.faint)
                    .draw_from(ui, x, row);
            }
            if self.shows("lines") && i > 0 {
                let inset = total / 8;
                let line = Rect::new(
                    cell.x,
                    top + inset,
                    u32::from(style.line()).max(1) as u16,
                    total - 2 * inset,
                );
                self.rule(ui.canvas(), line);
            }
        }
    }

    /// The days one below the other, each a row: its name, symbol, chance
    /// of rain, lowest, a bar of its range within the week's, highest.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn list(&self, ui: &mut ui::Ui<'_>, area: Rect, days: &[Day]) {
        let style = self.style;
        let font = self.font;
        let env = self.env;
        // As many days as have rows a line of small text high.
        let least = widgets::line_height(env, style.small()).max(1);
        let days = &days[..days.len().min(usize::from(area.height / least).max(1))];
        let count = days.len().max(1) as u16;
        let height = area.height / count;
        let gap = style.gap();
        let highs: Vec<String> = days.iter().map(|d| self.degrees(d.high)).collect();
        let lows: Vec<String> = days.iter().map(|d| self.degrees(d.low)).collect();
        let rains: Vec<Option<String>> = days.iter().map(|d| self.rain(d)).collect();
        let widest = |texts: &mut dyn Iterator<Item = &String>, size: f32| {
            draw::ceil(texts.map(|t| font.measure(size, t)).fold(0.0, f32::max))
        };
        let with_low = self.shows("low");
        let mut side = if self.shows("symbol") {
            ((f32::from(height) * 0.8) as u16).min(area.width / 6)
        } else {
            0
        };
        let mut rain_w = widest(&mut rains.iter().flatten(), style.small());
        // The text as large as the rows allow, as set, and as the width
        // allows: the names shorter where they would not fit.
        let by_height = draw::digits_size(f32::from(height) * 0.55).min(style.title() * 1.1);
        let wanted = temperature_size(env, &style)
            .unwrap_or(by_height)
            .min(by_height.max(style.small()));
        let probe = style.title();
        let fixed = f32::from(side + rain_w + gap * 4);
        let (long, initial) = naming(env);
        let mut size = style.small();
        let mut names = Vec::new();
        for (long, initial) in [(long, initial), (false, initial), (false, true)] {
            names = days
                .iter()
                .map(|d| day_name(env, d, self.today, long, initial))
                .collect::<Vec<String>>();
            let at_probe = f32::from(widest(&mut names.iter(), probe))
                + f32::from(widest(&mut highs.iter(), probe))
                + if with_low {
                    f32::from(widest(&mut lows.iter(), probe))
                } else {
                    0.0
                };
            let by_width = (f32::from(area.width) - fixed) * probe / at_probe.max(1.0);
            size = wanted.min(by_width);
            if size >= style.small() {
                break;
            }
        }
        let size = draw::effective(font, size.max(style.small() * 0.8));
        let small = style.small().min(size);
        let name_w = widest(&mut names.iter(), size);
        let high_w = widest(&mut highs.iter(), size);
        let mut low_w = if with_low {
            widest(&mut lows.iter(), size)
        } else {
            0
        };
        // Where the parts do not fit across, the symbol goes, then the
        // chance of rain, then the lowest.
        let after = |w: u16| if w > 0 { w + gap } else { 0 };
        let needs = |side: u16, rain_w: u16, low_w: u16| {
            name_w + gap + after(side) + after(rain_w) + after(low_w) + high_w
        };
        if needs(side, rain_w, low_w) > area.width {
            side = 0;
        }
        if needs(side, rain_w, low_w) > area.width {
            rain_w = 0;
        }
        if needs(side, rain_w, low_w) > area.width {
            low_w = 0;
        }
        // From the left: name, symbol, rain; from the right: highest, bar,
        // lowest.
        let mut x = area.x + name_w + gap;
        let symbol_x = x;
        if side > 0 {
            x += side + gap;
        }
        let rain_x = x;
        if rain_w > 0 {
            x += rain_w + gap;
        }
        let right = area.x + area.width;
        let high_x = right.saturating_sub(high_w);
        let room = high_x.saturating_sub(x + low_w + gap * 2);
        let bars =
            self.shows("bars") && low_w > 0 && room >= (gap * 2).max(env.metrics().mm(6) as u16);
        let (bar_x, bar_w) = if bars {
            (x + low_w + gap, room)
        } else {
            (0, 0)
        };
        let low_x = if bars {
            x
        } else {
            high_x.saturating_sub(low_w + gap)
        };
        // The week's range, for the bars.
        let coldest = days.iter().map(|d| d.low).min().unwrap_or(0);
        let warmest = days
            .iter()
            .map(|d| d.high)
            .max()
            .unwrap_or(1)
            .max(coldest + 1);
        let place = |t: i32| {
            i64::from(bar_x)
                + i64::from(bar_w) * i64::from(t - coldest) / i64::from(warmest - coldest)
        };
        for (i, day) in days.iter().enumerate() {
            let row = Rect::new(area.x, area.y + height * i as u16, area.width, height);
            let color = if self.today == Some(day.date) {
                self.look.ink
            } else {
                self.look.faint
            };
            ui::Label::new(&names[i])
                .px(size)
                .color(color)
                .draw_from(ui, f32::from(area.x), row);
            if side > 0 {
                let at = Rect::new(symbol_x, row.y + (height - side) / 2, side, side);
                let (_, symbol) = data::condition(day.code);
                draw::draw_symbol(ui.canvas(), at, (symbol, true), self.symbols, self.look.ink);
            }
            if let Some(rain) = rains[i].as_ref().filter(|_| rain_w > 0) {
                let w = font.measure(small, rain);
                let x = f32::from(rain_x + rain_w) - w;
                ui::Label::new(rain)
                    .px(small)
                    .color(self.look.faint)
                    .draw_from(ui, x, row);
            }
            if low_w > 0 {
                let w = font.measure(size, &lows[i]);
                let x = f32::from(low_x + low_w) - w;
                draw_digits(ui, font, size, x, row, &lows[i], self.look.faint);
            }
            let w = font.measure(size, &highs[i]);
            let x = f32::from(high_x + high_w) - w;
            draw_digits(ui, font, size, x, row, &highs[i], self.look.ink);
            if bars {
                let mid = i32::from(row.y) + i32::from(row.height) / 2;
                let track = u32::from(style.line()).max(1) as u16;
                let y = (mid - i32::from(track) / 2) as u16;
                self.rule(ui.canvas(), Rect::new(bar_x, y, bar_w, track));
                let thick = (draw::cap(size) * 0.25).max(3.0) as u32;
                let from = place(day.low) as i32;
                let to = (place(day.high) as i32).max(from + thick as i32);
                let bar = Rect::new(
                    from as u16,
                    (mid - thick as i32 / 2) as u16,
                    (to - from) as u16,
                    thick as u16,
                );
                ui.canvas().fill_round(bar, thick / 2, self.look.ink);
            }
            if self.shows("lines") && i > 0 {
                let line = Rect::new(
                    area.x,
                    row.y,
                    area.width,
                    u32::from(style.line()).max(1) as u16,
                );
                self.rule(ui.canvas(), line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calm_parts_at_first() {
        use paperpaw_sdk::SettingValue;
        for spec in LOOK.iter().chain(&SETTINGS) {
            assert!(
                spec.id.as_str().starts_with("weather.forecast."),
                "{}",
                spec.id
            );
            assert_eq!(spec.group, ID);
        }
        let shown: Vec<&str> = SETTINGS
            .iter()
            .filter(|s| s.default == SettingValue::Bool(true))
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(
            shown,
            [
                "weather.forecast.symbol",
                "weather.forecast.low",
                "weather.forecast.bars",
                "weather.forecast.today"
            ]
        );
    }
}
