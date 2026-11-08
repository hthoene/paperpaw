//! The statistics' widgets for the home screen (or any screen showing
//! widgets), each in the shared look of widgets (its frame, size and
//! alignment) with settings of its own for what it shows:
//!
//! - *Read today* (`today`): today's reading time and pages, the way to
//!   the daily goal ([`GOAL`]) and the week or the streak;
//! - *Reading per day* (`week`): the last days as bars;
//! - *Daily goal* (`ring`): today's reading as a ring towards the goal;
//! - *Reading calendar* (`calendar`): this month, or the last weeks as a
//!   heat map, each day shaded by how long one read;
//! - *Books this year* (`year`): the books read to the end this year,
//!   towards a yearly goal;
//! - *Reading streak* (`streak`): the days read in a row, the longest
//!   streak and the last days as marks.
//!
//! They read the statistics when they are updated, never as they are
//! drawn, and keep only the few numbers they show. Asked each minute (a
//! new day changes them), they read again only when the day or the files
//! of the statistics changed ([`Stamp`]).

mod calendar;
mod ring;
mod streak;
mod today;
mod week;
mod year;

use alloc::borrow::Cow;
use alloc::vec::Vec;

use paperpaw_sdk::round::point_at;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{self, TextSize, WidgetLook, WidgetTarget};
use paperpaw_sdk::{
    Canvas, Color, Command, Commands, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar,
    SettingSpec, SettingValue,
};

use crate::store::{self, Day};
use crate::{OPEN, fmt};
use paperpaw_sdk::setting::{Setting, Text};

pub(crate) use today::TODAY;

/// Setting: minutes a day to read, or none.
pub(crate) const GOAL: Setting<Text> = Setting::new("stats.goal");
/// Choices of [`GOAL`]: minutes, `off` for none.
const GOALS: [&str; 12] = [
    "stats.goal.off",
    "stats.goal.10",
    "stats.goal.15",
    "stats.goal.20",
    "stats.goal.30",
    "stats.goal.45",
    "stats.goal.60",
    "stats.goal.90",
    "stats.goal.120",
    "stats.goal.180",
    "stats.goal.240",
    "stats.goal.300",
];

/// The number at the end of a choice's value (`stats.goal.30`: 30).
fn number_of(value: &str) -> Option<u32> {
    value.rsplit('.').next()?.parse().ok()
}

/// The daily goal in seconds, if one is set.
pub(crate) fn goal(env: &Env<'_>) -> Option<u64> {
    number_of(env.settings().text(GOAL.as_str()))
        .filter(|&m| m > 0)
        .map(|m| u64::from(m) * 60)
}

/// The plugin's settings for the widgets, kept in the program.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::choice(
    "stats.goal",
    "stats.setting.goal",
    &GOALS,
    GOALS[0],
    crate::GROUP,
)
.description("stats.setting.goal.help")];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&SETTINGS)?;
    r.on(
        paperpaw_sdk::builtin::events::SETTING_CHANGED,
        |env, changed, out| {
            if changed.id == GOAL && goal(env).is_some() {
                offer_widget(env, out);
            }
        },
    )?;
    today::register(r)?;
    week::register(r)?;
    ring::register(r)?;
    calendar::register(r)?;
    year::register(r)?;
    streak::register(r)
}

/// A goal set shows on the home screen: the first time, *Read today*
/// goes onto its first page, after the book being read, if no page shows
/// it yet. Offered once only (kept as a file of the plugin), so that a
/// widget the user removed stays away.
fn offer_widget(env: &Env<'_>, out: &mut Commands) {
    let offered = alloc::format!(
        "{}/goal-widget-offered",
        Env::plugin_dir(crate::PLUGIN.as_str())
    );
    if env.storage().exists(&offered) {
        return;
    }
    let pages: [&str; 4] = ["home.page-1", "home.page-2", "home.page-3", "home.page-4"];
    let shown = pages.iter().any(|page| {
        // Also placed in a grid: `stats.today 0/2 4x1`.
        paperpaw_sdk::status::item_ids(env.settings().text(page))
            .any(|item| item.split_whitespace().next() == Some(TODAY.as_str()))
    });
    let first = match env.settings().get(pages[0]) {
        Some(SettingValue::Text(list)) => Some(list),
        _ => None,
    };
    // Nothing to offer it on (another home screen), or not kept as
    // offered (it would come back at every change of the goal).
    if (first.is_none() && !shown) || env.storage().write(&offered, b"").is_err() {
        return;
    }
    if let Some(list) = first.filter(|_| !shown) {
        let mut ids: Vec<&str> = paperpaw_sdk::status::item_ids(list).collect();
        let at = ids
            .iter()
            .position(|item| item.split_whitespace().next() == Some("home.continue"))
            .map_or(ids.len(), |i| i + 1);
        let today = TODAY;
        ids.insert(at, today.as_str());
        out.push(Command::SetSetting(
            Id::new_static("home.page-1"),
            SettingValue::Text(Cow::Owned(ids.join(", "))),
        ));
    }
}

/// The value of widget `widget`'s choice `key`.
fn chosen<'e>(env: &Env<'e>, widget: &str, key: &str) -> &'e str {
    env.settings().text(&alloc::format!("{widget}.{key}"))
}

/// The height `widget`'s frame takes (the SDK's).
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    paperpaw_sdk::widgets::frame_height(env, widget)
}

/// `height` pixels placed in the middle of `area`, top to bottom.
fn middle(area: Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    Rect::new(
        area.x,
        area.y + (area.height - height) / 2,
        area.width,
        height,
    )
}

/// The caption's height, if `widget` shows it.
fn caption_room(env: &Env<'_>, widget: &str) -> u16 {
    if widgets::shows(env, widget, "caption") {
        widgets::caption_height(env)
    } else {
        0
    }
}

/// The caption `widget` shows above content in `area`: the first of
/// `texts` (the full one, then shorter ones) that fits the width whole,
/// where the widget shows its caption and there is room for a line of
/// text below it as well. Else none: a caption is left out whole rather
/// than cut, and the content goes first.
fn caption<'t>(env: &Env<'_>, widget: &str, area: Rect, texts: &[&'t str]) -> Option<&'t str> {
    let cap = caption_room(env, widget);
    let style = UiStyle::new(env);
    let least = widgets::line_height(env, style.text());
    if cap == 0 || cap + least > area.height {
        return None;
    }
    let font = style.font(env)?;
    let width = f32::from(area.width);
    texts
        .iter()
        .copied()
        .find(|text| font.measure(style.small(), text) <= width)
}

/// The room below `caption` in `area` (all of it without one).
fn below_caption(env: &Env<'_>, caption: Option<&str>, area: Rect) -> Rect {
    let cap = if caption.is_some() {
        widgets::caption_height(env).min(area.height)
    } else {
        0
    };
    Rect::new(area.x, area.y + cap, area.width, area.height - cap)
}

/// Places content `height` high with `widget`'s `caption` (if any)
/// above it, the two together in the middle of `look`'s area, and draws
/// the caption; where the content goes. Where the two do not fit
/// together, the caption is left out.
fn captioned(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    look: &WidgetLook,
    caption: Option<&str>,
    height: u16,
) -> Rect {
    let cap = widgets::caption_height(env);
    let caption = caption.filter(|_| cap + height <= look.area.height);
    let cap = if caption.is_some() { cap } else { 0 };
    let block = middle(look.area, cap + height);
    if let Some(text) = caption {
        let at = look.within(block);
        widgets::draw_caption(canvas, env, widget, &at, text);
    }
    Rect::new(
        block.x,
        block.y + cap,
        block.width,
        height.min(block.height - cap),
    )
}

/// The colour between `look`'s paper and its ink at `level` of 4 (4: the
/// ink), for shades drawn with `fill_shaded` (dithered on panels with few
/// grays): light on light paper, the other way round on a dark card.
fn shade(look: &WidgetLook, level: u8) -> Color {
    const LIGHT: [u8; 5] = [255, 205, 160, 95, 0];
    let gray = LIGHT[usize::from(level.min(4))];
    if look.paper.luma() < 128 {
        Color::gray(255 - gray)
    } else {
        Color::gray(gray)
    }
}

/// Whether text on a shade of `level` is drawn in the paper's colour.
fn on_dark(level: u8) -> bool {
    level >= 3
}

/// How dark a day of `seconds` reading is shaded, 0 (none) to 4: with a
/// daily goal by its share (a third, two thirds, nearly, reached), else
/// by minutes (under 15, under 30, under 60, an hour or more).
pub(crate) fn level(seconds: u32, goal: Option<u64>) -> u8 {
    let seconds = u64::from(seconds);
    if seconds == 0 {
        return 0;
    }
    match goal.filter(|&g| g > 0) {
        Some(goal) if seconds >= goal => 4,
        Some(goal) => 1 + (seconds * 3 / goal) as u8,
        None => match seconds / 60 {
            0..=14 => 1,
            15..=29 => 2,
            30..=59 => 3,
            _ => 4,
        },
    }
}

/// The text size widget `widget`'s size setting gives, smaller where
/// `text` is wider than `width` (down to the body's size, below which it
/// is shortened) or a line of it higher than `height` (down to the small
/// size); "as large as fits" fits it into both, as large as they hold.
fn text_size(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    widget: &str,
    text: &str,
    width: u16,
    height: u16,
) -> f32 {
    let style = UiStyle::new(env);
    let max = match widgets::size(env, widget) {
        TextSize::Fit => f32::from(height).max(style.text()),
        size => size.text(&style),
    };
    sized(font, &style, text, (width, height), (max, style.text()))
}

/// The largest size (three times a title's at most, the small size at
/// least) at which `text` fits `width` and a line of it `height`.
fn fit_size(font: &FontHandle<'_>, style: &UiStyle, text: &str, width: u16, height: u16) -> f32 {
    sized(
        font,
        style,
        text,
        (width, height),
        (style.title() * 3.0, style.small()),
    )
}

/// The largest size from `max` down to `least` at which `text` fits
/// `width` and a line of it `height`, measured as drawn; where it is
/// too wide even then, the largest whose line fits the height (down to
/// the small size), at which it is shortened.
fn sized(
    font: &FontHandle<'_>,
    style: &UiStyle,
    text: &str,
    (width, height): (u16, u16),
    (max, least): (f32, f32),
) -> f32 {
    let wide = f32::from(width);
    let tall = |size: f32| line(font, size) <= height;
    widgets::largest_size(max, least.min(max), |size| {
        tall(size) && font.measure(size, text) <= wide
    })
    .or_else(|| widgets::largest_size(max.min(least), style.small(), tall))
    .unwrap_or(style.small().min(max))
}

/// `text`, or where it is wider than `width` at `size`, the number it
/// begins with alone ("5 days": "5").
fn number_if_narrow<'t>(font: &FontHandle<'_>, size: f32, text: &'t str, width: u16) -> &'t str {
    if font.measure(size, text) <= f32::from(width) {
        return text;
    }
    let digits = text
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map_or(text.len(), |(at, _)| at);
    if digits == 0 { text } else { &text[..digits] }
}

/// `text` small in lines `width` wide, as many as fit `height` (`most`
/// at most): whole where it fits, else ending in "…" where there is no
/// room for more ([`Wrapped::whole`](widgets::Wrapped::whole) tells);
/// none where not even a line fits.
fn note_lines(
    env: &Env<'_>,
    text: &str,
    width: u16,
    height: u16,
    most: u16,
) -> Option<widgets::Wrapped> {
    let style = UiStyle::new(env);
    let small = widgets::line_height(env, style.small()).max(1);
    let height = height.min(small * most);
    if text.is_empty() || height < small {
        return None;
    }
    Some(widgets::Wrapped::fitting(
        env,
        text,
        style.small(),
        style.small(),
        width,
        height,
    ))
}

/// The height of a line of text `size` pixels large.
fn line(font: &FontHandle<'_>, size: f32) -> u16 {
    font.line_metrics(size).line_height as u16
}

/// What a widget read its numbers for: today, and the sizes of the log
/// and of the books. Read again only when that changed, so that being
/// asked each minute (for a new day) costs little; screens make widgets
/// anew when a setting changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Stamp(Option<i32>, (u64, u64));

/// Whether what a widget read for `last` is out of date; `last` becomes
/// what it is read for now.
fn stale(env: &Env<'_>, last: &mut Option<Stamp>) -> bool {
    let now = Some(Stamp(fmt::today(env), store::sizes(env.storage())));
    if *last == now {
        return false;
    }
    *last = now;
    true
}

/// Today and the days read, if the device knows the time.
fn days(env: &Env<'_>) -> Option<(i32, Vec<Day>)> {
    let today = fmt::today(env)?;
    Some((
        today,
        store::days(env.storage(), |unix| fmt::day_of(env, unix)).unwrap_or_default(),
    ))
}

/// The first letter of a weekday's short name (0 being Sunday).
fn initial<'a>(env: &Env<'a>, weekday: u8) -> &'a str {
    let name = fmt::weekday_short(env, weekday);
    name.char_indices()
        .nth(1)
        .map_or(name, |(at, _)| &name[..at])
}

/// A tap on the whole widget opens the statistics.
fn open_statistics(area: Rect) -> Vec<WidgetTarget> {
    alloc::vec![WidgetTarget::new(area, Command::run(OPEN))]
}

/// Draws a ring into the square `at`, `thick` pixels wide: a thin track
/// all round (three quarters open at the bottom for a `gauge`), over it
/// `progress` (thousandths, if any) with round ends.
fn draw_arc(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    at: Rect,
    progress: Option<u32>,
    thick: u32,
    gauge: bool,
) {
    let style = UiStyle::new(env);
    let d = u32::from(at.width.min(at.height));
    let radius = d / 2;
    if radius < 4 {
        return;
    }
    let thick = thick.clamp(1, radius);
    let center = Point::new(
        i32::from(at.x) + radius as i32,
        i32::from(at.y) + radius as i32,
    );
    let (from, sweep) = if gauge { (225, 270) } else { (0, 360) };
    // The track, thin, in the middle of where the ring is drawn.
    let track = u32::from(style.line()).max(1).min(thick);
    let track_radius = radius - (thick - track) / 2;
    // Halfway between paper and ink where the panel has grays to show
    // it, else faint (black or white, told apart by its thinness).
    let grays = env
        .device()
        .display
        .format
        .gray_levels()
        .is_none_or(|g| g > 4);
    let color = if grays { shade(look, 2) } else { look.faint };
    canvas.fill_ring(center, track_radius, track, from, sweep, color);
    if let Some(progress) = progress.filter(|p| *p > 0) {
        let done = (sweep * progress.min(1000) / 1000).max(1);
        canvas.fill_ring(center, radius, thick, from, done, look.ink);
        // Round ends, but for a whole ring.
        if done < 360 {
            let middle = (radius - thick / 2) as i32;
            for deg in [from, from + done as i32] {
                canvas.fill_circle(point_at(center, middle, deg), thick / 2, look.ink);
            }
        }
    }
}

/// Draws a dashed line across from `x` to `end` at `y`.
fn dashed(canvas: &mut Canvas<'_>, x: i32, end: i32, y: i32, thickness: u32, color: Color) {
    let dash = (thickness * 4).max(4) as i32;
    let mut at = x;
    while at < end {
        canvas.fill_rect(at, y, dash.min(end - at) as u32, thickness, color);
        at += dash * 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::widgets::Frame;

    /// Every widget's settings have ids of their own, named after their
    /// widget (its group): the plugin loads, the widgets read them.
    #[test]
    fn the_widgets_register() {
        use paperpaw_sdk::Contribution;
        let mut r = Registrar::new(crate::PLUGIN);
        assert!(register(&mut r).is_ok());
        let mut ids: Vec<&str> = r
            .contributions()
            .iter()
            .flat_map(|c| match c {
                Contribution::Setting(spec) => core::slice::from_ref(&**spec),
                Contribution::FixedSettings(list) => list,
                _ => &[],
            })
            .map(|spec| {
                let (id, group) = (spec.id.as_str(), spec.group.as_str());
                let widget = group.starts_with("stats.widget.");
                assert!(
                    !widget || id.starts_with(&alloc::format!("{group}.")),
                    "{id}"
                );
                id
            })
            .collect();
        let all = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all);
        assert!(all > 30);
    }

    #[test]
    fn goals_are_minutes_or_off() {
        let minutes: Vec<Option<u32>> = GOALS.iter().map(|g| number_of(g)).collect();
        assert_eq!(minutes[0], None);
        assert!(minutes[1..].iter().all(Option::is_some));
    }

    #[test]
    fn days_are_shaded_by_time_or_the_goal() {
        // By minutes.
        let by_minutes: Vec<u8> = [0, 30, 14 * 60, 15 * 60, 45 * 60, 60 * 60, 5 * 3600]
            .into_iter()
            .map(|s| level(s, None))
            .collect();
        assert_eq!(by_minutes, [0, 1, 1, 2, 3, 4, 4]);
        // By the goal's share: 30 minutes.
        let goal = Some(1800);
        let by_goal: Vec<u8> = [0, 60, 600, 1200, 1799, 1800, 9000]
            .into_iter()
            .map(|s| level(s, goal))
            .collect();
        assert_eq!(by_goal, [0, 1, 2, 3, 3, 4, 4]);
    }

    #[test]
    fn shades_turn_round_on_a_dark_card() {
        let area = Rect::new(0, 0, 10, 10);
        let light = WidgetLook::new(
            area,
            Color::BLACK,
            Color::gray(90),
            Color::WHITE,
            Frame::None,
        );
        let dark = WidgetLook::new(
            area,
            Color::WHITE,
            Color::gray(90),
            Color::BLACK,
            Frame::Dark,
        );
        assert_eq!(shade(&light, 4), Color::BLACK);
        assert_eq!(shade(&dark, 4), Color::WHITE);
        assert!(shade(&light, 1).luma() > shade(&light, 3).luma());
        assert!(shade(&dark, 1).luma() < shade(&dark, 3).luma());
    }
}
