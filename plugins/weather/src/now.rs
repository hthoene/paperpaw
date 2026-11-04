//! The widget "Weather now": a symbol, the temperature large, the
//! condition's name, today's highest and lowest, and what else the reader
//! chooses (how warm it feels, wind, chance of rain, humidity, sunrise and
//! sunset), with the place's name small above.
//!
//! The symbol sits beside the text where the place is wide, above it where
//! it is tall (or as set), or small before the temperature; with the size
//! "as large as it fits", the temperature takes the room the other lines
//! leave. Lines that do not fit are left out, the least important first.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{Arg, Canvas, Env, FontHandle, Id, Rect, RegisterError, Registrar, SettingSpec};

use crate::data::{self, Current, Day};
use crate::draw::{self, cap, digits_height, draw_digits};
use crate::symbol;
use crate::{Shared, Shown};

pub(crate) const ID: Id = Id::new_static("weather.now");

/// The values of the symbol's size.
const SYMBOLS: [&str; 3] = [
    "weather.symbol.large",
    "weather.symbol.small",
    "weather.symbol.none",
];
/// The values of where the symbol sits.
const PLACINGS: [&str; 3] = [
    "weather.placing.auto",
    "weather.placing.beside",
    "weather.placing.above",
];
/// Its settings, kept in the program: its look, its symbol, its parts.
static SETTINGS: [SettingSpec; 19] = [
    widgets::frame_spec("weather.now.frame", ID, Frame::None),
    widgets::padding_spec("weather.now.padding", ID, Padding::Auto),
    widgets::paper_spec("weather.now.paper", ID),
    widgets::ink_spec("weather.now.ink", ID),
    widgets::size_spec("weather.now.size", ID, TextSize::Fit, true),
    widgets::align_spec("weather.now.align", ID, Align::Center),
    SettingSpec::choice(
        "weather.now.symbol",
        "weather.setting.symbol",
        &SYMBOLS,
        SYMBOLS[0],
        ID,
    )
    .description("weather.setting.symbol.help"),
    SettingSpec::choice(
        "weather.now.placing",
        "weather.setting.placing",
        &PLACINGS,
        PLACINGS[0],
        ID,
    )
    .description("weather.setting.placing.help"),
    draw::style_spec("weather.now.style", ID),
    widgets::part_spec("weather.now.place", ID, "weather.part.place", true),
    widgets::part_spec("weather.now.condition", ID, "weather.part.condition", true),
    widgets::part_spec("weather.now.range", ID, "weather.part.range", true),
    widgets::part_spec("weather.now.feels", ID, "weather.part.feels", false),
    widgets::part_spec("weather.now.wind", ID, "weather.part.wind", false),
    widgets::part_spec("weather.now.rain", ID, "weather.part.rain", false),
    widgets::part_spec("weather.now.humidity", ID, "weather.part.humidity", false),
    widgets::part_spec("weather.now.sun", ID, "weather.part.sun", false),
    widgets::part_spec("weather.now.unit", ID, "weather.part.unit", false),
    widgets::part_spec("weather.now.age", ID, "weather.part.age", true),
];

pub(crate) fn register(r: &mut Registrar, state: &Shared) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "weather.widget.now");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    let state = state.clone();
    r.add_widget(
        WidgetSpec::new(ID, "weather.widget.now", move |env| {
            Box::new(Now {
                shown: Shown::read(env, &state),
                state: state.clone(),
                placed: None,
            })
        })
        .category(widgets::categories::WORLD),
    )
}

/// "Weather now": what it shows, read when updated.
struct Now {
    state: Shared,
    shown: Shown,
    /// Where it is drawn, once placed, and what it shows laid out there.
    placed: Option<(Rect, Option<Laid>)>,
}

/// The temperature and the lines laid out in a widget's area, drawn as
/// they are in every band.
struct Laid {
    temperature: String,
    plan: Plan,
}

/// What `shown` shows laid out in widget area `area`: where the symbol
/// goes, how large the temperature is, which lines fit; `None` while
/// there is no weather to show.
fn lay_out(env: &Env<'_>, area: Rect, shown: &Shown) -> Option<Laid> {
    let weather = shown.weather.as_ref()?;
    let current = weather.current?;
    let today = weather.days_from(shown.today).first().copied();
    let font = UiStyle::new(env).font(env)?;
    let area = widgets::widget_look(env, ID.as_str(), area).area;
    let unit = crate::temperature_unit(env);
    let temperature = data::degrees_text(
        unit,
        current.temperature,
        widgets::shows(env, ID.as_str(), "unit"),
    );
    let all = lines(env, shown, current, today);
    let plan = plan(env, &font, area, &temperature, &all);
    Some(Laid { temperature, plan })
}

impl Widget for Now {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = Shown::read(env, &self.state);
        let changed = shown != self.shown;
        self.shown = shown;
        if changed && let Some((area, laid)) = &mut self.placed {
            *laid = lay_out(env, *area, &self.shown);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.placed.as_ref().is_none_or(|(at, _)| *at != area) {
            self.placed = Some((area, lay_out(env, area, &self.shown)));
        }
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let probe = Rect::new(0, 0, 100, 100);
        let frame = 100 - widgets::widget_look(env, ID.as_str(), probe).area.height;
        let size = temperature_size(env, &style).unwrap_or(style.title() * 2.4);
        digits_height(size) + widgets::line_height(env, style.small()) * 3 + frame
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let Some(current) = self.shown.weather.as_ref().and_then(|w| w.current) else {
            draw::draw_waiting(canvas, env, ID.as_str(), (&look, area), &self.shown);
            return;
        };
        // Laid out when placed; drawn elsewhere, laid out for that.
        let made;
        let laid = match &self.placed {
            Some((at, Some(laid))) if *at == area => laid,
            _ => match lay_out(env, area, &self.shown) {
                Some(laid) => {
                    made = laid;
                    &made
                }
                None => return,
            },
        };
        draw_now(canvas, env, &look, current, laid);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget::new(area, crate::tap(&self.shown))]
    }
}

/// The temperature's size as set; `None` for "as large as it fits".
fn temperature_size(env: &Env<'_>, style: &UiStyle) -> Option<f32> {
    match widgets::size(env, ID.as_str()) {
        TextSize::Small => Some(style.title() * 1.6),
        TextSize::Medium => Some(style.title() * 2.4),
        TextSize::Large => Some(style.title() * 3.4),
        TextSize::Fit => None,
        _ => None,
    }
}

/// A line of text: what it says, how large, in which colour, and how
/// soon it is left out where there is no room (lower first).
#[derive(Debug, Clone)]
struct Line {
    text: String,
    size: f32,
    faint: bool,
    rank: u8,
}

/// The lines below the temperature and the caption above it, as set.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn lines(env: &Env<'_>, shown: &Shown, current: Current, today: Option<Day>) -> Vec<Line> {
    let style = UiStyle::new(env);
    let part = |name: &str| widgets::shows(env, ID.as_str(), name);
    let unit = crate::temperature_unit(env);
    let with_unit = part("unit");
    let degrees = |tenths| data::degrees_text(unit, tenths, with_unit);
    let mut lines = Vec::new();
    let mut caption = Vec::new();
    if part("place") && !shown.place.is_empty() {
        caption.push(shown.place.clone());
    }
    if part("age")
        && let Some(age) = shown.age_text(env)
    {
        caption.push(age);
    }
    if !caption.is_empty() {
        lines.push(Line {
            text: caption.join(" · "),
            size: style.small(),
            faint: true,
            rank: 3,
        });
    }
    if part("condition") {
        lines.push(Line {
            text: env.i18n().tr(data::condition(current.code).0).to_string(),
            size: style.text(),
            faint: false,
            rank: 5,
        });
    }
    let mut details: Vec<(String, u8)> = Vec::new();
    if part("range")
        && let Some(day) = today
    {
        let text = env.i18n().format(
            "weather.range",
            &[
                ("high", Arg::Str(&degrees(day.high))),
                ("low", Arg::Str(&degrees(day.low))),
            ],
        );
        details.push((text, 4));
    }
    if part("feels")
        && let Some(feels) = current.feels
    {
        let text = env
            .i18n()
            .format("weather.feels", &[("degrees", Arg::Str(&degrees(feels)))]);
        details.push((text, 2));
    }
    if part("wind")
        && let Some(wind) = current.wind
    {
        let wind_unit = crate::wind_unit(env);
        let speed = env.i18n().format(
            wind_unit.key(),
            &[("speed", Arg::Int(i64::from(wind_unit.speed(wind))))],
        );
        let text = env
            .i18n()
            .format("weather.wind", &[("speed", Arg::Str(&speed))]);
        details.push((text, 2));
    }
    if part("rain")
        && let Some(rain) = today.and_then(|d| d.rain)
    {
        let text = env
            .i18n()
            .format("weather.rain", &[("percent", Arg::Int(i64::from(rain)))]);
        details.push((text, 2));
    }
    if part("humidity")
        && let Some(humidity) = current.humidity
    {
        let text = env.i18n().format(
            "weather.humidity",
            &[("percent", Arg::Int(i64::from(humidity)))],
        );
        details.push((text, 1));
    }
    if part("sun")
        && let Some((rise, set)) = today.and_then(|d| d.sunrise.zip(d.sunset))
    {
        let text = env.i18n().format(
            "weather.sun",
            &[
                ("rise", Arg::Str(&draw::clock(env, rise))),
                ("set", Arg::Str(&draw::clock(env, set))),
            ],
        );
        details.push((text, 1));
    }
    for (text, rank) in details {
        lines.push(Line {
            text,
            size: style.small(),
            faint: rank < 4,
            rank,
        });
    }
    lines
}

/// `lines` without those ranked below `least`, the details beside each
/// other where they fit `width` (" · " between them).
fn packed(font: &FontHandle<'_>, lines: &[Line], least: u8, width: f32) -> Vec<Line> {
    let small = lines.iter().map(|l| l.size).fold(f32::MAX, f32::min);
    let mut out: Vec<Line> = Vec::new();
    for line in lines.iter().filter(|l| l.rank >= least) {
        if let Some(last) = out.last_mut()
            && last.rank < 5
            && last.size == line.size
            && last.faint == line.faint
            && line.rank < 3
            && last.rank < 3
        {
            let joined = alloc::format!("{} · {}", last.text, line.text);
            if font.measure(line.size, &joined) <= width {
                last.text = joined;
                last.rank = last.rank.max(line.rank);
                continue;
            }
        }
        let mut line = line.clone();
        // The condition smaller rather than cut short.
        if line.rank == 5 && font.measure(line.size, &line.text) > width {
            line.size = small;
        }
        // A line that does not fit even so is left out whole.
        if font.measure(line.size, &line.text) <= width {
            out.push(line);
        }
    }
    out
}

/// Where the symbol goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placing {
    Beside,
    Above,
    /// Small, before the temperature.
    Inline,
    None,
}

/// What is drawn where: the symbol's square, the temperature's size, the
/// lines kept, and the text's place.
#[derive(Debug, Clone)]
struct Plan {
    placing: Placing,
    symbol: Rect,
    size: f32,
    caption: Option<Line>,
    lines: Vec<Line>,
    text: Rect,
    /// The temperature fits the text's width.
    wide_enough: bool,
}

fn line_height(env: &Env<'_>, line: &Line) -> u16 {
    widgets::line_height(env, line.size)
}

/// The height of the text: the caption, the temperature, the lines.
fn text_height(env: &Env<'_>, size: f32, lines: &[Line]) -> u16 {
    digits_height(size) + lines.iter().map(|l| line_height(env, l)).sum::<u16>()
}

/// The largest temperature that fits `width` and `height` with `lines`,
/// as set or as large as fits (at most `most`).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn fitting(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    text: &str,
    lines: &[Line],
    width: f32,
    height: u16,
    most: f32,
    inline: bool,
) -> f32 {
    let style = UiStyle::new(env);
    if let Some(size) = temperature_size(env, &style) {
        return size;
    }
    let rest = lines.iter().map(|l| line_height(env, l)).sum::<u16>();
    let by_height = draw::digits_size(f32::from(height.saturating_sub(rest)));
    let probe = style.title();
    let gap = style.gap();
    let room = width - if inline { f32::from(gap / 2) } else { 0.0 };
    let wide = (temperature_width(font, probe, text, inline, 0)).max(1.0) / probe;
    let by_width = room / wide;
    by_height
        .min(by_width)
        .min(most)
        .max(style.small().min(by_width))
}

/// The small symbol before a temperature `size` pixels large: its side
/// and the room after it (`gap` and some).
fn inline_symbol(size: f32, gap: u16) -> (u16, u16) {
    let side = (cap(size) * 1.25) as u16;
    (side, gap / 2 + side / 8)
}

/// How wide the temperature is, with the small symbol before it if
/// `inline`.
fn temperature_width(font: &FontHandle<'_>, size: f32, text: &str, inline: bool, gap: u16) -> f32 {
    let (side, after) = if inline {
        inline_symbol(size, gap)
    } else {
        (0, 0)
    };
    font.measure(size, text) + f32::from(side + after)
}

/// Where the symbol goes as set; where it does not fit, the places to try
/// after it.
fn placings(env: &Env<'_>, area: Rect) -> &'static [Placing] {
    let symbol = env.settings().text(&alloc::format!("{ID}.symbol"));
    let placing = env.settings().text(&alloc::format!("{ID}.placing"));
    match SYMBOLS.iter().position(|s| *s == symbol) {
        Some(2) => &[Placing::None],
        Some(1) => &[Placing::Inline, Placing::None],
        _ => match PLACINGS.iter().position(|p| *p == placing) {
            Some(1) => &[
                Placing::Beside,
                Placing::Above,
                Placing::Inline,
                Placing::None,
            ],
            Some(2) => &[Placing::Above, Placing::Inline, Placing::None],
            _ if u32::from(area.width) * 10 >= u32::from(area.height) * 13 => &[
                Placing::Beside,
                Placing::Above,
                Placing::Inline,
                Placing::None,
            ],
            _ => &[
                Placing::Above,
                Placing::Beside,
                Placing::Inline,
                Placing::None,
            ],
        },
    }
}

/// The plan: the first place of the symbol where the temperature is not
/// too small, with as many lines as fit.
fn plan(env: &Env<'_>, font: &FontHandle<'_>, area: Rect, temperature: &str, all: &[Line]) -> Plan {
    let style = UiStyle::new(env);
    // The least a temperature should be before lines (or the large
    // symbol) are left out; less where the font has no larger sizes.
    let largest = draw::effective(font, 400.0);
    let least = (style.title() * 1.3).min(largest) * 0.9;
    let mut last = None;
    for &placing in placings(env, area) {
        let plan = plan_for(env, font, area, temperature, all, placing, least);
        // A small symbol before the temperature, even smaller, rather
        // than none.
        let enough = if placing == Placing::Inline {
            least * 0.5
        } else {
            least * 0.95
        };
        // A large symbol at least a line of text high, else not so.
        let seen = match placing {
            Placing::Beside | Placing::Above => {
                plan.symbol.width >= widgets::line_height(env, style.text())
            }
            _ => true,
        };
        if seen
            && ((plan.size >= enough && plan.wide_enough)
                || temperature_size(env, &style).is_some())
        {
            return plan;
        }
        last = Some(plan);
    }
    last.unwrap_or_else(|| plan_for(env, font, area, temperature, all, Placing::None, least))
}

/// The plan with the symbol placed so, as many lines left out as needed
/// for a temperature of at least `least`.
fn plan_for(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    area: Rect,
    temperature: &str,
    all: &[Line],
    placing: Placing,
    least: f32,
) -> Plan {
    let style = UiStyle::new(env);
    let gap = style.gap();
    let fixed = temperature_size(env, &style);
    // Whether lines are left to leave out from `rank` on.
    let more = |rank: u8| all.iter().any(|l| l.rank >= rank && l.rank > 0) && rank < 6;
    let mut rank = 0u8;
    loop {
        let (side, text_area) = match placing {
            Placing::Beside => {
                let side = match fixed {
                    None => area.height.min(area.width * 2 / 5),
                    Some(size) => area
                        .height
                        .min(area.width * 2 / 5)
                        .min(digits_height(size) * 2),
                };
                let shift = side + gap;
                let text = Rect::new(
                    area.x + shift,
                    area.y,
                    area.width.saturating_sub(shift),
                    area.height,
                );
                (side, text)
            }
            Placing::Above => {
                let lines = packed(font, all, rank, f32::from(area.width));
                let words = text_height(env, fixed.unwrap_or(least), &lines);
                // What the symbol may take; too little: fewer lines.
                let room = area.height.saturating_sub(words + gap);
                if room < area.height / 4 && more(rank) {
                    rank += 1;
                    continue;
                }
                let side = match fixed {
                    None => room.min(area.height * 11 / 20),
                    Some(size) => room.min(digits_height(size) * 2),
                }
                .min(area.width * 3 / 4);
                let shift = side + gap;
                let text = Rect::new(
                    area.x,
                    area.y + shift,
                    area.width,
                    area.height.saturating_sub(shift),
                );
                (side, text)
            }
            Placing::Inline | Placing::None => (0, area),
        };
        let lines = packed(font, all, rank, f32::from(text_area.width));
        let (caption, rest): (Vec<Line>, Vec<Line>) = lines.into_iter().partition(|l| l.rank == 3);
        let caption = caption.into_iter().next();
        let mut kept: Vec<Line> = caption.iter().cloned().collect();
        kept.extend(rest.iter().cloned());
        // Not larger than the symbol beside it, to keep them in balance.
        let most = match placing {
            Placing::Beside => (f32::from(side) * 0.55).max(least),
            Placing::Above => (f32::from(side) * 0.6).max(least),
            // Large, not huge.
            _ => (style.title() * 3.5).min(f32::from(text_area.height)),
        };
        let size = fitting(
            env,
            font,
            temperature,
            &kept,
            f32::from(text_area.width),
            text_area.height,
            most,
            placing == Placing::Inline,
        );
        let size = draw::effective(font, size);
        let wide = temperature_width(font, size, temperature, placing == Placing::Inline, gap);
        let fits = text_height(env, size, &kept) <= text_area.height
            && wide <= f32::from(text_area.width)
            && (fixed.is_some() || size >= least * 0.95);
        if fits || !more(rank) {
            let symbol = match placing {
                Placing::Beside => Rect::new(
                    area.x,
                    area.y + area.height.saturating_sub(side) / 2,
                    side,
                    side,
                ),
                Placing::Above => Rect::new(area.x, area.y, side, side),
                _ => Rect::new(0, 0, 0, 0),
            };
            return Plan {
                placing,
                symbol,
                size,
                caption,
                lines: rest,
                text: text_area,
                wide_enough: wide <= f32::from(text_area.width),
            };
        }
        rank += 1;
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw_now(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &widgets::WidgetLook,
    current: Current,
    laid: &Laid,
) {
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let Some(font) = ui.font() else {
        return;
    };
    let area = look.area;
    let gap = style.gap();
    let (temperature, plan) = (&laid.temperature, &laid.plan);
    let align = widgets::align(env, ID.as_str());
    let symbol_style = draw::style(env, ID.as_str());
    let (_, symbol) = data::condition(current.code);
    let mark = (symbol, current.day);
    let color = |line: &Line| if line.faint { look.faint } else { look.ink };

    // The rows of text, from the top.
    let (inline, inline_gap) = if plan.placing == Placing::Inline {
        inline_symbol(plan.size, gap)
    } else {
        (0, 0)
    };
    let temperature_width = font.measure(plan.size, temperature) + f32::from(inline + inline_gap);
    let mut rows: Vec<(u16, f32, Option<&Line>)> = Vec::new();
    if let Some(caption) = &plan.caption {
        rows.push((
            line_height(env, caption),
            font.measure(caption.size, &caption.text),
            Some(caption),
        ));
    }
    rows.push((digits_height(plan.size), temperature_width, None));
    for line in &plan.lines {
        rows.push((
            line_height(env, line),
            font.measure(line.size, &line.text),
            Some(line),
        ));
    }
    let total: u16 = rows.iter().map(|(h, _, _)| *h).sum();
    let widest = rows
        .iter()
        .map(|(_, w, _)| *w)
        .fold(0.0, f32::max)
        .min(f32::from(plan.text.width));

    // Beside: the symbol and the text as one group, placed as set; the
    // text starts at one edge.
    let (text, group_x) = match plan.placing {
        Placing::Beside => {
            // Placed by the symbol's ink, not its square.
            let side = plan.symbol.width;
            let (ink_x, ink_y, ink_w, ink_h) = draw::ink_of(symbol, current.day, side);
            let group = f32::from(ink_w + gap) + widest;
            let x = align.x(area, group) as i32;
            let y = i32::from(area.y) + (i32::from(area.height) - i32::from(ink_h)) / 2;
            let at = Rect::new(
                (x - ink_x).max(0) as u16,
                (y - ink_y).max(0) as u16,
                side,
                side,
            );
            symbol::draw(ui.canvas(), at, symbol, current.day, symbol_style, look.ink);
            let text_x = x as u16 + ink_w + gap;
            (
                Rect::new(text_x, plan.text.y, draw::ceil(widest), plan.text.height),
                Some(f32::from(text_x)),
            )
        }
        Placing::Above => {
            // Placed by the symbol's ink, not its square.
            let side = plan.symbol.width;
            let (ink_x, ink_y, ink_w, ink_h) = draw::ink_of(symbol, current.day, side);
            let total = ink_h + gap + total;
            let top = area.y + area.height.saturating_sub(total) / 2;
            let x = align.x(area, f32::from(ink_w)) as i32;
            let at = Rect::new(
                (x - ink_x).max(0) as u16,
                (i32::from(top) - ink_y).max(0) as u16,
                side,
                side,
            );
            symbol::draw(ui.canvas(), at, symbol, current.day, symbol_style, look.ink);
            let below = top + ink_h + gap;
            (
                Rect::new(
                    area.x,
                    below,
                    area.width,
                    area.height.saturating_sub(below - area.y),
                ),
                None,
            )
        }
        _ => (plan.text, None),
    };
    let mut y = match plan.placing {
        Placing::Above => text.y,
        _ => text.y + text.height.saturating_sub(total) / 2,
    };
    for (height, width, line) in rows {
        let row = Rect::new(text.x, y, text.width.max(1), height);
        let x = group_x.unwrap_or_else(|| align.x(text, width));
        match line {
            Some(line) => {
                ui::Label::new(&line.text)
                    .px(line.size)
                    .color(color(line))
                    .draw_from(&mut ui, x, row);
            }
            None => {
                if inline > 0 {
                    let top = row.y + row.height.saturating_sub(inline) / 2;
                    let at = Rect::new(x as u16, top, inline, inline);
                    draw::draw_symbol(ui.canvas(), at, mark, symbol_style, look.ink);
                }
                let x = x + f32::from(inline + inline_gap);
                draw_digits(&mut ui, &font, plan.size, x, row, temperature, look.ink);
            }
        }
        y += height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_the_widgets() {
        use paperpaw_sdk::SettingValue;
        for spec in &SETTINGS {
            assert!(spec.id.as_str().starts_with("weather.now."), "{}", spec.id);
            assert_eq!(spec.group, ID);
        }
        // Calm at first: the place, the condition, today's range.
        let shown: Vec<&str> = SETTINGS
            .iter()
            .filter(|s| s.default == SettingValue::Bool(true))
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(
            shown,
            [
                "weather.now.place",
                "weather.now.condition",
                "weather.now.range",
                "weather.now.age"
            ]
        );
    }
}
