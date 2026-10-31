//! The home screen's own widgets, which its pages are made of (any
//! plugin's widgets go on them too), and its tools:
//!
//! - status line: the time, Wi-Fi while in a network, and the battery's
//!   charge (above the grid by default, see `pages`);
//! - date: today in words;
//! - clock: the time large, with the date;
//! - continue reading: the book read last, with its cover and progress;
//! - recently read: the books before it, as a row of covers;
//! - tools: an action's icon and name, in a cell of the grid.
//!
//! Every widget has settings of its own (from the editor, *Settings* of
//! the widget): where its text sits, how large it is, and which of its
//! parts it shows. Widgets fill the cells they are given.
//!
//! Widgets draw what is stored (covers once looked up, see `covers`),
//! never opening books themselves, and read it when the home screen
//! updates them, not as they are drawn, so drawing stays quick: covers
//! are made small for their place once it is known ([`Widget::place`])
//! and drawn from memory in every band.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, screens};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::status::Place;
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, Drawn, Frame, TextSize, Widget, WidgetSpec, WidgetTarget, grid, line_height,
};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Env, FontHandle, Id, Params, Rect, RegisterError, Registrar,
    SettingExtra, SettingKind, SettingSpec, SettingValue, library, time,
};

use crate::pages::{CONTINUE, DATE, RECENT, STATUS};
use crate::recent;
use paperpaw_sdk::setting::{Setting, Text};

/// The clock with the date, large.
pub const CLOCK: &str = "home.clock";
/// Whether the clock shows the date below the time.
const CLOCK_DATE: Setting<bool> = Setting::new("home.clock.date");
/// Whether the continue card shows the cover, the progress.
const CARD_COVER: Setting<bool> = Setting::new("home.continue.cover");
const CARD_PROGRESS: Setting<bool> = Setting::new("home.continue.progress");
/// Whether the recent books show their heading and their titles, how many
/// there are at most.
const RECENT_HEADING: Setting<bool> = Setting::new("home.recent.heading");
const RECENT_TITLES: Setting<bool> = Setting::new("home.recent.titles");
const RECENT_COUNT: Setting<i64> = Setting::new("home.recent.count");
/// Whether tools show their names below their icons.
pub(crate) const TOOL_LABELS: Setting<bool> = Setting::new("home.tool-labels");
/// Whether the focus shows at once where there is no touch, or only
/// with the first press (the page seen as it is first).
pub(crate) const FOCUS_AT_ONCE: Setting<bool> = Setting::new("home.focus-at-once");
/// The status items of the status line's left, centre and right (ids
/// separated by commas).
const STATUS_LEFT: Setting<Text> = Setting::new("home.status.left");
const STATUS_CENTER: Setting<Text> = Setting::new("home.status.center");
const STATUS_RIGHT: Setting<Text> = Setting::new("home.status.right");
/// The status line shown: its items unless another is chosen.
const STATUS_LINE: Setting<Text> = Setting::new("home.status.line");
/// Where the status line's items show, above the grid and as a widget.
const PLACE: Place = Place::new(
    Id::new_static("home.status-line"),
    "home.widget.status",
    [
        Id::new_static(STATUS_LEFT.static_str()),
        Id::new_static(STATUS_CENTER.static_str()),
        Id::new_static(STATUS_RIGHT.static_str()),
    ],
)
.with_line(Id::new_static(STATUS_LINE.static_str()));
/// The status line's parts, each a list of status items chosen on their
/// picker: the time on the left; Wi-Fi and the battery on the right.
static STATUS_ITEMS: SettingExtra = SettingExtra::new().editor(screens::STATUS_ITEMS);
static STATUS_LINES: SettingExtra = SettingExtra::new().editor(screens::STATUS_LINES);
static STATUS_SETTINGS: [SettingSpec; 4] = [
    SettingSpec::new(
        paperpaw_sdk::Id::new_static(STATUS_LINE.static_str()),
        "home.status.line",
        SettingKind::List,
        SettingValue::Text(Cow::Borrowed("")),
        Id::new_static(STATUS),
    )
    .with_extra(&STATUS_LINES)
    .description("home.status.line.help"),
    status_part(
        STATUS_LEFT.static_str(),
        "home.status.left",
        "core.status.clock",
    ),
    status_part(STATUS_CENTER.static_str(), "home.status.center", ""),
    status_part(
        STATUS_RIGHT.static_str(),
        "home.status.right",
        "lock.touch-status, wifi.status, power.status, core.status.battery",
    ),
];

const fn status_part(id: &'static str, title: &'static str, default: &'static str) -> SettingSpec {
    SettingSpec::new(
        paperpaw_sdk::Id::new_static(id),
        title,
        SettingKind::List,
        SettingValue::Text(Cow::Borrowed(default)),
        Id::new_static(STATUS),
    )
    .with_extra(&STATUS_ITEMS)
    .description("home.status.part.help")
}

/// Recent books shown as covers at most.
const MAX_COVERS: usize = 8;

/// How many books the recent shelf shows at most.
static RECENT_SETTINGS: [SettingSpec; 1] = [SettingSpec::number(
    "home.recent.count",
    "home.setting.recent-count",
    1,
    MAX_COVERS as i64,
    6,
    Id::new_static(RECENT),
)
.description("home.setting.recent-count.help")];

/// The widgets' settings: where each sits, its frame, size and parts.
static SETTINGS: [SettingSpec; 21] = {
    use widgets::{Padding, align_spec, frame_spec, padding_spec, part_spec, size_spec};
    const STATUS_ID: Id = Id::new_static(STATUS);
    const DATE_ID: Id = Id::new_static(DATE);
    const CLOCK_ID: Id = Id::new_static(CLOCK);
    const CONTINUE_ID: Id = Id::new_static(CONTINUE);
    const RECENT_ID: Id = Id::new_static(RECENT);
    [
        frame_spec("home.status.frame", STATUS_ID, Frame::None),
        padding_spec("home.status.padding", STATUS_ID, Padding::Auto),
        align_spec("home.date.align", DATE_ID, Align::Center),
        frame_spec("home.date.frame", DATE_ID, Frame::None),
        padding_spec("home.date.padding", DATE_ID, Padding::Auto),
        size_spec("home.date.size", DATE_ID, TextSize::Medium, false),
        align_spec("home.clock.align", CLOCK_ID, Align::Center),
        frame_spec("home.clock.frame", CLOCK_ID, Frame::None),
        padding_spec("home.clock.padding", CLOCK_ID, Padding::Auto),
        size_spec("home.clock.size", CLOCK_ID, TextSize::Medium, true),
        part_spec("home.clock.date", CLOCK_ID, "home.setting.clock-date", true),
        align_spec("home.continue.align", CONTINUE_ID, Align::Left),
        frame_spec("home.continue.frame", CONTINUE_ID, Frame::Line),
        padding_spec("home.continue.padding", CONTINUE_ID, Padding::Auto),
        part_spec(
            "home.continue.cover",
            CONTINUE_ID,
            "home.setting.card-cover",
            true,
        ),
        part_spec(
            "home.continue.progress",
            CONTINUE_ID,
            "home.setting.card-progress",
            true,
        ),
        align_spec("home.recent.align", RECENT_ID, Align::Center),
        frame_spec("home.recent.frame", RECENT_ID, Frame::None),
        padding_spec("home.recent.padding", RECENT_ID, Padding::Auto),
        part_spec(
            "home.recent.heading",
            RECENT_ID,
            "home.setting.recent-heading",
            true,
        ),
        part_spec(
            "home.recent.titles",
            RECENT_ID,
            "home.setting.recent-titles",
            true,
        ),
    ]
};

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    use widgets::categories;
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(STATUS),
            "home.widget.status",
            Drawn::new(status_widget_height, draw_status_widget)
                .shows(status_signature)
                .targets(status_widget_targets),
        )
        .category(categories::DEVICE),
    )?;
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(DATE),
            "home.widget.date",
            Drawn::new(date_height, draw_date).shows(date_text),
        )
        .category(categories::TIME),
    )?;
    r.add_widget(
        WidgetSpec::new(Id::new_static(CONTINUE), "home.widget.continue", |_| {
            Box::new(Card::default())
        })
        .category(categories::BOOKS),
    )?;
    r.add_widget(
        WidgetSpec::new(Id::new_static(RECENT), "home.widget.recent", |_| {
            Box::new(Shelf::default())
        })
        .category(categories::BOOKS),
    )?;
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(CLOCK),
            "home.widget.clock",
            Drawn::new(clock_height, draw_clock).shows(clock_shows),
        )
        .category(categories::TIME),
    )?;

    // The status line's parts: the time on the left; Wi-Fi and the
    // battery on the right.
    let status = Id::new_static(STATUS);
    r.add_setting_group(widgets::settings_group(&status, "home.widget.status"))?;
    r.add_settings(&STATUS_SETTINGS)?;
    r.add_status_place(PLACE)?;
    // Each other widget's settings, in its own group.
    for (id, title) in [
        (DATE, "home.widget.date"),
        (CLOCK, "home.widget.clock"),
        (CONTINUE, "home.widget.continue"),
        (RECENT, "home.widget.recent"),
    ] {
        r.add_setting_group(widgets::settings_group(&Id::new_static(id), title))?;
    }
    r.add_settings(&SETTINGS)?;
    r.add_settings(&RECENT_SETTINGS)?;
    Ok(())
}

/// The height `widget`'s frame takes (the SDK's).
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    paperpaw_sdk::widgets::frame_height(env, widget)
}

/// `height` pixels placed in the middle of `area`, top to bottom.
fn centred(area: Rect, height: u16) -> u16 {
    area.y + area.height.saturating_sub(height) / 2
}

// ---- Status line --------------------------------------------------------

/// The height of the status line above the grid: without a frame, flush
/// with the screen's margins (its spacing setting is for the widget).
pub(crate) fn status_height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    let frame = if widgets::frame(env, STATUS) == Frame::None {
        0
    } else {
        frame_height(env, STATUS)
    };
    line_height(env, style.small()) + style.gap() / 2 + frame
}

/// The status line above the grid ([`status_height`]): the status items
/// chosen for its left, centre and right ([`PLACE`]), any
/// plugin's, as the reader's lines have them.
pub(crate) fn draw_status(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, STATUS, area);
    let inner = if look.frame == Frame::None {
        area
    } else {
        look.area
    };
    draw_status_line(canvas, env, &look, inner);
}

/// The status line as a widget in the grid: inside its spacing.
fn status_widget_height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    line_height(env, style.small()) + style.gap() / 2 + frame_height(env, STATUS)
}

fn draw_status_widget(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, STATUS, area);
    draw_status_line(canvas, env, &look, look.area);
}

/// The status items in `area`, as many as fit whole: where all do not,
/// the battery's percentage is left out first, then the items that
/// matter least (the status line leaves them out), so that nothing is
/// cut or drawn outside.
fn draw_status_line(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &widgets::WidgetLook,
    area: Rect,
) {
    let style = UiStyle::new(env);
    let color = if look.frame == Frame::Dark {
        look.faint
    } else {
        style.faint(ui::Ink::NOTE)
    };
    let context = Params::new();
    PLACE.draw(canvas, env, &status_frame(env, area, color, &context));
}

/// The frame of the status line in `area`, in the interface's font.
fn status_frame<'a>(
    env: &Env<'_>,
    area: Rect,
    color: Color,
    context: &'a Params,
) -> paperpaw_sdk::StatusFrame<'a> {
    let style = UiStyle::new(env);
    paperpaw_sdk::StatusFrame::new(area, true, style.small(), color, context)
        .font(paperpaw_sdk::builtin::fonts::UI.static_str())
}

/// Where a tap on the status line in `area` does something (the items
/// that do something when tapped): by touch alone, the buttons reaching
/// what they do by the actions bound to them, so that the focus never
/// stops on an item that comes and goes (Wi-Fi).
fn status_targets(env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
    let context = Params::new();
    PLACE
        .targets(env, &status_frame(env, area, Color::BLACK, &context))
        .into_iter()
        .map(|t| WidgetTarget::touch(t.area, t.command))
        .collect()
}

/// Where a tap on the status line above the grid ([`draw_status`]) does
/// something.
pub(crate) fn status_bar_targets(env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
    let look = widgets::widget_look(env, STATUS, area);
    let inner = if look.frame == Frame::None {
        area
    } else {
        look.area
    };
    status_targets(env, inner)
}

/// Where a tap on the status line widget in `area` does something.
fn status_widget_targets(env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
    status_targets(env, widgets::widget_look(env, STATUS, area).area)
}

/// What the status line shows now, to draw it again when it changes.
fn status_signature(env: &Env<'_>) -> String {
    format!("{}", PLACE.signature(env, &Params::new()))
}

// ---- Date ---------------------------------------------------------------

/// The date in words: "Thursday, 15 October".
pub(crate) fn date_text(env: &Env<'_>) -> String {
    time::date_text(env).unwrap_or_default()
}

/// Today as long as fits `width` pixels whole at a size from `max` down
/// to `min`: in full ("Thursday, 15 October"), else short ("Thu 15
/// Oct"); `None` where not even the short one does.
fn fitting_date(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    max: f32,
    min: f32,
    width: f32,
) -> Option<(f32, String)> {
    [time::date_text(env), time::short_date_text(env)]
        .into_iter()
        .flatten()
        .find_map(|text| {
            widgets::largest_size(max, min, |size| font.measure(size, &text) <= width)
                .map(|size| (size, text))
        })
}

fn date_height(env: &Env<'_>, _: u16) -> u16 {
    if time::local_time(env).is_none() {
        return 0;
    }
    let style = UiStyle::new(env);
    line_height(env, widgets::size(env, DATE).text(&style)) + frame_height(env, DATE)
}

fn draw_date(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = UiStyle::new(env);
    let look = widgets::draw_frame(canvas, env, DATE, area);
    let area = look.area;
    let Some(font) = style.font(env) else {
        return;
    };
    // As large as set, smaller down to the body's size rather than cut
    // short, then in short words; where not even those fit a line, in
    // lines ("Wed" above "14 Oct"), down to the small size.
    let width = f32::from(area.width);
    let max = widgets::size(env, DATE).text(&style);
    let align = widgets::align(env, DATE);
    if let Some((size, text)) = fitting_date(env, &font, max, style.text().min(max), width) {
        widgets::draw_line(canvas, env, area, align, size, size, &text, look.ink);
    } else if let Some(short) = time::short_date_text(env) {
        // The weekday above the day and the month.
        let short = match short.split_once(' ') {
            Some((weekday, rest)) => format!("{}\n{rest}", weekday.trim_end_matches(',')),
            None => short,
        };
        let lines =
            widgets::Wrapped::fitting(env, &short, max, style.small(), area.width, area.height);
        lines.draw(canvas, env, area, align, look.ink);
    }
}

// ---- Clock ----------------------------------------------------------------

/// What the clock shows: the time and the date.
fn clock_shows(env: &Env<'_>) -> String {
    format!(
        "{}|{}",
        time::clock_text(env).unwrap_or_default(),
        date_text(env)
    )
}

/// How wide the time is by the clock's size setting, as a share of the
/// screen's width: small, medium (the first home screen's, on every
/// device) and large clearly apart, the same wherever it is placed (as
/// far as the place holds it); as large as fits ([`TextSize::Fit`]) fills
/// the place instead.
fn set_share(size: TextSize) -> f32 {
    match size {
        TextSize::Small => 0.15,
        TextSize::Large => 0.32,
        _ => 0.21,
    }
}

/// The date's share of the time's size: it grows with the time, never
/// smaller than small text.
const DATE_SHARE: f32 = 0.24;

/// Where the time's digits end and what follows them begins ("AM"): the
/// digits (or the dashes of a clock not yet set) are drawn large, from a
/// pack's large sizes where it has them (see [`FontHandle::display`]),
/// the rest beside them at the date's size.
fn split_time(clock: &str) -> (&str, &str) {
    let end = clock
        .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '-'))
        .unwrap_or(clock.len());
    (&clock[..end], clock[end..].trim())
}

/// How the clock is laid out in `width` by `height` pixels: the time's
/// size, and the date's with its text where it shows.
struct ClockLayout {
    size: f32,
    /// The size of the date and of what follows the digits ("AM").
    small: f32,
    date: Option<String>,
    /// The ink of the digits and of the date, from their baselines.
    digits_ink: (i32, i32),
    date_ink: (i32, i32),
}

impl ClockLayout {
    /// The time as large as its size setting says and the place holds: as
    /// large as fits by [`TextSize::Fit`], at most that else; the date below
    /// it in proportion (in short words where the full ones do not fit),
    /// left out only where not even the time at the body's size and the
    /// date at the small size fit together.
    fn of(env: &Env<'_>, style: &UiStyle, font: &FontHandle<'_>, width: f32, height: f32) -> Self {
        let clock = time::clock_text(env).unwrap_or_else(|| String::from("--:--"));
        let (digits, rest) = split_time(&clock);
        let display = font.display();
        let gap = style.gap() as i32;
        let wants_date = env.settings().read(&CLOCK_DATE) && time::local_time(env).is_some();
        // The most it may be: as set, or (as large as fits) up to the
        // width's, its height checked as it is laid out.
        let size_for = |wide: f32| 100.0 * wide / display.measure(100.0, digits).max(1.0);
        let most = match widgets::size(env, CLOCK) {
            TextSize::Fit => size_for(width),
            size => size_for(set_share(size) * f32::from(env.device().display.width)),
        };
        let lay = |size: f32, with_date: bool| {
            // In proportion to the size the digits are drawn at (a pack's
            // largest at most), so the date never outgrows them.
            let small = (display.size_for(size, digits) * DATE_SHARE).max(style.small());
            let rest_width = if rest.is_empty() {
                0.0
            } else {
                font.measure(small, " ") + font.measure(small, rest)
            };
            let wide = display.measure(size, digits) + rest_width;
            let digits_ink = ink_of(&display, size, digits);
            let date = with_date
                .then(|| fitting_date(env, font, small, style.small(), width))
                .flatten();
            let date_ink = date
                .as_ref()
                .map_or((0, 0), |(size, text)| ink_of(font, *size, text));
            let high = digits_ink.1 - digits_ink.0
                + date.as_ref().map_or(0, |_| gap + date_ink.1 - date_ink.0);
            let layout = ClockLayout {
                size,
                small: date.as_ref().map_or(small, |(size, _)| *size),
                date: date.map(|(_, text)| text),
                digits_ink,
                date_ink,
            };
            (wide <= width + 1.0 && high <= height as i32, layout)
        };
        let fitting = |with_date: bool| {
            let mut found = None;
            widgets::largest_size(most, style.text().min(most), |size| {
                let (fits, layout) = lay(size, with_date);
                if fits {
                    found = Some(layout);
                }
                fits
            });
            found
        };
        fitting(wants_date)
            .or_else(|| fitting(false))
            .unwrap_or_else(|| lay(style.text(), false).1)
    }

    /// The height of its ink, the date's and the gap between included.
    fn height(&self, gap: i32) -> i32 {
        let date = match self.date {
            Some(_) => gap + self.date_ink.1 - self.date_ink.0,
            None => 0,
        };
        self.digits_ink.1 - self.digits_ink.0 + date
    }
}

fn clock_height(env: &Env<'_>, width: u16) -> u16 {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return line_height(env, style.title()) + frame_height(env, CLOCK);
    };
    // As high as the time and the date need at their size: as large as
    // fits, that is as wide as the place.
    let frame = widgets::frame(env, CLOCK);
    let pad = widgets::padding(env, CLOCK)
        .pixels(&style, frame)
        .min(width / 4);
    let inner = f32::from(width.saturating_sub(2 * pad));
    let layout = ClockLayout::of(env, &style, &font, inner, f32::MAX);
    // The room above and below the ink a line of text has.
    let room = line_height(env, style.small()) / 2;
    layout.height(style.gap() as i32) as u16 + 2 * room + 2 * pad + frame_height(env, CLOCK)
}

/// The top and bottom of the ink of `text` at `size` from its baseline
/// (the top negative); the line's where it draws nothing.
fn ink_of(font: &FontHandle<'_>, size: f32, text: &str) -> (i32, i32) {
    font.ink(size, text).unwrap_or_else(|| {
        let metrics = font.line_metrics(size);
        (-(metrics.ascent as i32), -(metrics.descent as i32))
    })
}

fn draw_clock(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let look = widgets::draw_frame(canvas, env, CLOCK, area);
    let area = look.area;
    let width = f32::from(area.width);
    let clock = time::clock_text(env).unwrap_or_else(|| String::from("--:--"));
    let (digits, rest) = split_time(&clock);
    let display = font.display();
    // A gap from ink to ink: the date's small letters need the room a
    // gap between lines of text has, or they cling to the digits.
    let gap = style.gap() as i32;
    let layout = ClockLayout::of(env, &style, &font, width, f32::from(area.height));
    let size = layout.size;
    let (top, bottom) = layout.digits_ink;
    // Together in the middle by their ink.
    let total = layout.height(gap);
    let y = i32::from(area.y) + (i32::from(area.height) - total) / 2;
    let align = widgets::align(env, CLOCK);
    let rest_width = if rest.is_empty() {
        0.0
    } else {
        font.measure(layout.small, " ") + font.measure(layout.small, rest)
    };
    // Shortened only where not even the body's size fits (a sliver).
    let shown = ui::fit(&display, size, digits, width - rest_width);
    let digits_width = display.measure(size, &shown);
    let x = align.x(area, digits_width + rest_width);
    let high = (bottom - top) as u16;
    let at = Rect::new(x.max(0.0) as u16, y.max(0) as u16, area.width, high);
    canvas.describe(paperpaw_sdk::semantics::Role::Text, &clock, at);
    let baseline = y - top;
    display.draw(canvas, size, x, baseline, &shown, look.ink);
    if !rest.is_empty() {
        let x = x + digits_width + font.measure(layout.small, " ");
        font.draw(canvas, layout.small, x, baseline, rest, look.ink);
    }
    if let Some(text) = &layout.date {
        let (date_top, date_bottom) = layout.date_ink;
        let y = y + (bottom - top) + gap;
        let x = align.x(area, font.measure(layout.small, text));
        font.draw(canvas, layout.small, x, y - date_top, text, look.faint);
        let high = (date_bottom - date_top) as u16;
        let at = Rect::new(x.max(0.0) as u16, y.max(0) as u16, area.width, high);
        canvas.describe(paperpaw_sdk::semantics::Role::Text, text, at);
    }
}

// ---- Continue reading -----------------------------------------------------

fn card_height(env: &Env<'_>, _: u16) -> u16 {
    (UiStyle::new(env).row() as f32 * 3.0) as u16
}

/// A book a widget shows, and its cover if it has one (looked up already).
#[derive(Debug, Clone, PartialEq)]
struct Book {
    book: recent::Recent,
    cover: Option<ImageRef>,
}

impl Book {
    fn of(env: &Env<'_>, book: recent::Recent) -> Self {
        Self {
            cover: book.cover.clone().filter(|i| env.storage().exists(&i.path)),
            book,
        }
    }

    fn open(&self) -> Command {
        paperpaw_sdk::readers::open(self.book.path.clone())
    }
}

/// Where `image` goes, fitted into `place`: `x` and `y` say where in it,
/// from 0 (left, top) to 1 (right, bottom).
fn fitted(image: &ImageRef, place: Rect, x: f32, y: f32) -> Rect {
    let (w, h) = ui::fit_image(image, place.width, place.height);
    let left = f32::from(place.width - w) * x;
    let top = f32::from(place.height - h) * y;
    Rect::new(place.x + left as u16, place.y + top as u16, w, h)
}

/// `book`'s cover `image` made small to fit `place` (as [`fitted`] puts
/// it there), as the panel shows it: kept beside the book as `variant`,
/// so it is made once, not again each time home is shown; read once,
/// then drawn from memory.
fn small_cover(
    env: &Env<'_>,
    book: &Book,
    image: &ImageRef,
    place: Rect,
    variant: &str,
) -> Option<Thumbnail> {
    let format = env.device().display.format;
    let mut known = library::Book::default().cover(image.clone());
    known.size = book.book.size;
    let (w, h) = (place.width, place.height);
    library::cover_variant(
        env.storage(),
        &book.book.path,
        &known,
        w,
        h,
        format,
        variant,
    )
}

/// Draws `image` at `at`: from `small` where it was made for that size,
/// otherwise from its file (where there was no memory to make it small).
fn draw_cover(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    image: &ImageRef,
    small: Option<&Thumbnail>,
    at: Rect,
) {
    match small {
        Some(small) if (small.width(), small.height()) == (at.width, at.height) => {
            small.draw(canvas, at.x, at.y);
        }
        _ => ui::draw_image(canvas, env.storage(), image, at),
    }
}

/// The book read last; the library before any was read.
#[derive(Debug, Default)]
struct Card {
    book: Option<Book>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    /// Where its parts go there, worked out with the place.
    parts: CardParts,
    /// The book's cover, made small for that place.
    cover: Option<Thumbnail>,
}

/// Where the parts of the continue card go: the cover, and beside it
/// the column of text with the caption, the title and the progress,
/// each where it fits.
#[derive(Debug, Default, Clone, PartialEq)]
struct CardParts {
    /// Where the cover goes, if it shows.
    cover: Option<Rect>,
    /// The place it was fitted into there.
    cover_place: Rect,
    /// The column of text; none where the cover alone fits.
    text: Option<Rect>,
    /// "Continue reading" above the title.
    caption: bool,
    /// The title, in one line or two.
    title: widgets::Wrapped,
    /// The percentage and the bar below.
    progress: bool,
    /// Between the rows: half a gap, or none where that keeps the
    /// caption.
    gap: u16,
}

impl CardParts {
    /// The parts of `book`'s card in `area`: the cover on the left where
    /// the text keeps room enough beside it (else the text alone, or the
    /// cover alone where not even the text has room); the title as large
    /// as fits in up to two lines, then the progress and the caption
    /// where there is room for them too (the caption is left out first).
    fn of(env: &Env<'_>, area: Rect, book: &Book) -> Self {
        let style = UiStyle::new(env);
        let r = widgets::widget_look(env, CONTINUE, area).area;
        let gap = style.gap();
        // A few words of a title at the body's size.
        let least = (style.text() * 4.0) as u16;
        let mut parts = Self::default();
        let image = book
            .cover
            .as_ref()
            .filter(|_| env.settings().read(&CARD_COVER));
        let mut text = r;
        if let Some(image) = image {
            let place = Rect::new(r.x, r.y, r.width / 3, r.height);
            let at = fitted(image, place, 0.0, 0.5);
            let beside = r.width.saturating_sub(at.width + gap);
            if beside >= least {
                parts.cover = Some(at);
                parts.cover_place = place;
                text = Rect::new(r.x + at.width + gap, r.y, beside, r.height);
            } else if r.width < least {
                // No room for text: the cover alone, in the middle.
                parts.cover = Some(fitted(image, r, 0.5, 0.5));
                parts.cover_place = r;
                return parts;
            }
        }
        let small = line_height(env, style.small());
        let line = line_height(env, style.text());
        let caption = env.i18n().tr("home.continue");
        let caption_fits = style
            .font(env)
            .is_some_and(|font| font.measure(style.small(), caption) <= f32::from(text.width));
        // Half a gap between the rows; none where only that keeps all
        // three (the small rows have room above their letters anyway).
        let all = |gap: u16| caption_fits && line + 2 * (gap + small) <= text.height;
        let half = if all(gap / 2) || !all(0) { gap / 2 } else { 0 };
        parts.gap = half;
        let mut room = text.height;
        parts.progress = env.settings().read(&CARD_PROGRESS) && room >= line + half + small;
        if parts.progress {
            room -= half + small;
        }
        parts.caption = caption_fits && room >= line + half + small;
        if parts.caption {
            room -= half + small;
        }
        // Two lines at most; in a flat card, down to the small size.
        let two = room.min(
            line_height(env, style.title()) + widgets::heading_line_height(env, style.title()),
        );
        let least = if room >= line {
            style.text()
        } else {
            style.small()
        };
        let title = &book.book.title;
        parts.title = widgets::Wrapped::heading(env, title, style.title(), least, text.width, two);
        parts.text = Some(text);
        parts
    }

    /// The height its column of text takes.
    fn height(&self, env: &Env<'_>) -> u16 {
        let style = UiStyle::new(env);
        let small = line_height(env, style.small()) + self.gap;
        self.title.height(env)
            + if self.caption { small } else { 0 }
            + if self.progress { small } else { 0 }
    }
}

impl Card {
    fn make_parts(&mut self, env: &Env<'_>) {
        self.parts = match (self.area, &self.book) {
            (Some(area), Some(book)) => CardParts::of(env, area, book),
            _ => CardParts::default(),
        };
        self.cover = match (self.parts.cover, &self.book) {
            (
                Some(_),
                Some(
                    book @ Book {
                        cover: Some(image), ..
                    },
                ),
            ) => small_cover(env, book, image, self.parts.cover_place, "home-card"),
            _ => None,
        };
    }
}

impl Widget for Card {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let book = recent::load(env.storage(), 1)
            .into_iter()
            .next()
            .map(|book| Book::of(env, book));
        let changed = book != self.book;
        self.book = book;
        if changed {
            self.make_parts(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_parts(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        card_height(env, width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let book = self.book.as_ref();
        if self.area == Some(area) {
            draw_card(canvas, env, area, book, &self.parts, self.cover.as_ref());
        } else {
            // Drawn where it was not placed (e.g. while dragged).
            let parts = book
                .map(|b| CardParts::of(env, area, b))
                .unwrap_or_default();
            draw_card(canvas, env, area, book, &parts, None);
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .book
            .as_ref()
            .map_or(Command::run(actions::LIBRARY), Book::open);
        alloc::vec![WidgetTarget::new(area, command)]
    }
}

fn draw_card(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    shown: Option<&Book>,
    parts: &CardParts,
    small: Option<&Thumbnail>,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let mut ui = ui::Ui::new(canvas, env, &style);
    let look = widgets::draw_frame(ui.canvas(), env, CONTINUE, area);
    let Some(Book { book, cover }) = shown else {
        // Nothing read yet: where the books are (a tap opens the library).
        ui.empty(
            look.area,
            "books",
            env.i18n().tr("home.start.title"),
            env.i18n().tr("home.start.text"),
        );
        return;
    };
    // The cover on the left, where there is one.
    if let (Some(at), Some(image)) = (parts.cover, cover) {
        draw_cover(ui.canvas(), env, image, small, at);
    }
    let Some(r) = parts.text else {
        return;
    };
    // The lines of text, together in the middle.
    let label_h = line_height(env, style.small());
    let gap = parts.gap;
    let mut y = centred(r, parts.height(env));
    let align = widgets::align(env, CONTINUE);
    let row = |y: u16, h: u16| Rect::new(r.x, y, r.width, h);
    if parts.caption {
        let label = env.i18n().tr("home.continue");
        ui::Label::new(label)
            .px(style.small())
            .color(look.faint)
            .draw_from(
                &mut ui,
                align.x(r, font.measure(style.small(), label)),
                row(y, label_h),
            );
        y += label_h + gap;
    }
    let title_h = parts.title.height(env);
    parts
        .title
        .draw(ui.canvas(), env, row(y, title_h), align, look.ink);
    y += title_h + gap;
    if !parts.progress {
        return;
    }
    let percent = env.i18n().format(
        "home.percent",
        &[("percent", Arg::Int((book.progress + 0.5) as i64))],
    );
    let x = f32::from(r.x);
    ui::Label::new(&percent)
        .px(style.small())
        .color(look.faint)
        .draw_from(&mut ui, x, row(y, label_h));
    // The progress bar, beside it.
    let bar_h = (u32::from(style.line()) * 4).max(3);
    let percent_w = font.measure(style.small(), &percent) + style.gap() as f32;
    let bx = (x + percent_w) as i32;
    let by = i32::from(y) + i32::from(label_h) / 2 - bar_h as i32 / 2;
    let w = (f32::from(r.width) - percent_w).max(1.0) as u32;
    let canvas = ui.canvas();
    canvas.stroke_rect(bx, by, w, bar_h, u32::from(style.line()).max(1), look.ink);
    let filled = (w as f32 * (book.progress / 100.0).clamp(0.0, 1.0)) as u32;
    canvas.fill_rect(bx, by, filled, bar_h, look.ink);
}

// ---- Recently read ----------------------------------------------------------

/// The recent books after the current one, as many as set.
#[derive(Debug, Default)]
struct Shelf {
    books: Vec<Book>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    /// Where its parts go there.
    parts: ShelfParts,
    /// The books' covers, made small for their places.
    covers: Vec<Option<Thumbnail>>,
    /// The lines of the titles of books without a cover, in their frames.
    names: Vec<Vec<String>>,
}

/// Where the parts of the shelf go: the heading, and each book's cover
/// with its title below; together in the middle of the place.
#[derive(Debug, Default, Clone, PartialEq)]
struct ShelfParts {
    /// The heading's row, where it shows.
    heading: Option<Rect>,
    /// Each book's place for its cover, and the row of its title where
    /// titles show.
    places: Vec<(Rect, Option<Rect>)>,
}

impl ShelfParts {
    /// The parts of a shelf of `books` in `area` (inside its spacing):
    /// covers 3:4 as large as the columns and the height allow, the
    /// heading above and the titles below; where the covers would be
    /// smaller than two lines of text, the titles alone (shown even when
    /// set off, as something must say which books they are), with the
    /// heading where there is room for it.
    fn of(env: &Env<'_>, area: Rect, books: &[Book]) -> Self {
        if books.is_empty() {
            return Self::default();
        }
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return Self::default();
        };
        let (fit, _) = shelf_columns(env, area.width);
        let columns = books.len().clamp(1, fit) as u16;
        let gap = style.gap();
        let label = line_height(env, style.small());
        let heading_fits =
            font.measure(style.small(), env.i18n().tr("home.recent")) <= f32::from(area.width);
        let (mut heading, mut title) = shelf_labels(env);
        if !heading_fits {
            heading = 0;
        }
        let least = 2 * label;
        let covers = area.height.saturating_sub(heading + title) >= least;
        if !covers {
            title = label;
            if area.height < heading + title {
                heading = 0;
            }
        }
        let room = area.height.saturating_sub(heading + title);
        // Covers 3:4, as wide as the columns or as high as the room allows.
        let column = area.width / columns;
        let (w, h) = if covers {
            let w = column.saturating_sub(gap).min(room * 3 / 4);
            (w, w * 4 / 3)
        } else {
            (column.saturating_sub(gap), 0)
        };
        let total = heading + h + title;
        let top = area.y + area.height.saturating_sub(total) / 2;
        let left = area.x + area.width.saturating_sub(column * columns) / 2;
        let heading = (heading > 0).then(|| Rect::new(area.x, top, area.width, label));
        let top = top + heading.map_or(0, |_| label + gap / 2);
        let places = (0..columns)
            .map(|i| {
                let start = left + i * column;
                let cover = Rect::new(start + (column - w) / 2, top, w, h);
                let row = Rect::new(start + gap / 2, top + h, column.saturating_sub(gap), title);
                (cover, (title > 0).then_some(row))
            })
            .collect();
        Self { heading, places }
    }
}

impl Shelf {
    fn make_covers(&mut self, env: &Env<'_>) {
        let Some(area) = self.area else {
            self.parts = ShelfParts::default();
            self.covers.clear();
            self.names.clear();
            return;
        };
        let inner = widgets::widget_look(env, RECENT, area).area;
        self.parts = ShelfParts::of(env, inner, &self.books);
        let shown = self.parts.places.iter().zip(&self.books);
        self.covers = shown
            .clone()
            .map(|((place, _), book)| {
                let image = book.cover.as_ref().filter(|_| place.height > 0)?;
                small_cover(env, book, image, *place, "home-shelf")
            })
            .collect();
        self.names = shown
            .map(|((place, _), book)| match book.cover {
                Some(_) => Vec::new(),
                None if place.height == 0 => Vec::new(),
                None => placeholder_lines(env, *place, &book.book.title),
            })
            .collect();
    }
}

impl Widget for Shelf {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let count = shelf_count(env);
        let books: Vec<Book> = recent::load(env.storage(), count + 1)
            .into_iter()
            .skip(1)
            .take(count)
            .map(|book| Book::of(env, book))
            .collect();
        let changed = books != self.books;
        self.books = books;
        if changed {
            self.make_covers(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_covers(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        shelf_height(env, width, &self.books)
    }

    /// Covers a third as high, where there is less room.
    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        let (heading, title) = shelf_labels(env);
        let full = shelf_height(env, width, &self.books);
        let covers = full.saturating_sub(heading + title);
        if full == 0 {
            0
        } else {
            heading + title + covers / 3
        }
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        if self.area == Some(area) {
            draw_shelf(canvas, env, area, self);
        } else {
            // Drawn where it was not placed (e.g. while dragged).
            let mut moved = Shelf {
                books: self.books.clone(),
                area: Some(area),
                ..Shelf::default()
            };
            let inner = widgets::widget_look(env, RECENT, area).area;
            moved.parts = ShelfParts::of(env, inner, &moved.books);
            draw_shelf(canvas, env, area, &moved);
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let parts = if self.area == Some(area) {
            self.parts.clone()
        } else {
            let inner = widgets::widget_look(env, RECENT, area).area;
            ShelfParts::of(env, inner, &self.books)
        };
        parts
            .places
            .iter()
            .zip(&self.books)
            .map(|((cover, title), book)| {
                WidgetTarget::new(title.map_or(*cover, |t| cover.union(&t)), book.open())
            })
            .collect()
    }
}

/// How many books the row shows at most: as many as set, but no more
/// than fit across the screen (looking each up takes time).
fn shelf_count(env: &Env<'_>) -> usize {
    shelf_columns(env, env.device().display.width).0
}

/// The covers a row holds at `width`, and how wide each one's place is.
fn shelf_columns(env: &Env<'_>, width: u16) -> (usize, u16) {
    let count = env
        .settings()
        .read(&RECENT_COUNT)
        .clamp(1, MAX_COVERS as i64) as u32;
    let columns = (u32::from(width) / env.metrics().mm(18).max(1)).clamp(1, count);
    (columns as usize, width / columns as u16)
}

/// Heights of the heading and of a title below a cover (0 where not
/// shown).
fn shelf_labels(env: &Env<'_>) -> (u16, u16) {
    let style = UiStyle::new(env);
    let label = line_height(env, style.small());
    let heading = if env.settings().read(&RECENT_HEADING) {
        label + style.gap() / 2
    } else {
        0
    };
    let title = if env.settings().read(&RECENT_TITLES) {
        label
    } else {
        0
    };
    (heading, title)
}

fn shelf_height(env: &Env<'_>, width: u16, books: &[Book]) -> u16 {
    if books.is_empty() {
        return 0;
    }
    let style = UiStyle::new(env);
    let (_, cell) = shelf_columns(env, width);
    let (heading, title) = shelf_labels(env);
    let cover = (cell.saturating_sub(style.gap())) * 4 / 3;
    heading + cover + title + frame_height(env, RECENT)
}

fn draw_shelf(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, shelf: &Shelf) {
    let books = &shelf.books;
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    // Nothing read before the current book: nothing to show.
    if books.is_empty() {
        return;
    }
    let mut ui = ui::Ui::new(canvas, env, &style);
    let look = widgets::draw_frame(ui.canvas(), env, RECENT, area);
    let align = widgets::align(env, RECENT);
    if let Some(row) = shelf.parts.heading {
        let text = env.i18n().tr("home.recent");
        let x = align.x(row, font.measure(style.small(), text));
        ui::Label::new(text)
            .px(style.small())
            .color(look.faint)
            .draw_from(&mut ui, x, row);
    }
    for (i, ((cover, row), Book { book, cover: image })) in
        shelf.parts.places.iter().zip(books).enumerate()
    {
        match image {
            // Covers too small to tell: the titles alone.
            _ if cover.height == 0 => {}
            // In the middle of its place, at the bottom.
            Some(image) => {
                let at = fitted(image, *cover, 0.5, 1.0);
                let small = shelf.covers.get(i).and_then(Option::as_ref);
                draw_cover(ui.canvas(), env, image, small, at);
            }
            None => {
                let lines = shelf.names.get(i).map_or(&[][..], Vec::as_slice);
                draw_placeholder(&mut ui, *cover, lines, look.ink);
            }
        }
        if let Some(row) = row {
            let title = ui::fit(&font, style.small(), &book.title, f32::from(row.width));
            let x = align.x(*row, font.measure(style.small(), &title));
            ui::Label::new(&title)
                .px(style.small())
                .color(look.ink)
                .draw_from(&mut ui, x, *row);
        }
    }
}

/// The lines of `title` in the frame of a book without a cover at
/// `area`: broken between words, as many as fit; where a word is wider
/// than a line, or more is left than fits, the last line shown ends in
/// "…" and nothing follows it. None where the frame is too small.
fn placeholder_lines(env: &Env<'_>, area: Rect, title: &str) -> Vec<String> {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return Vec::new();
    };
    let pad = style.gap() as f32 / 2.0;
    let width = f32::from(area.width) - 2.0 * pad;
    let line = font.line_metrics(style.small()).line_height;
    let fits = ((f32::from(area.height) - 2.0 * pad) / line) as usize;
    if width < style.small() * 2.0 || fits == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = title.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    for (n, word) in words.iter().enumerate() {
        let joined = lines.last().map(|last| format!("{last} {word}"));
        match joined {
            Some(joined) if font.measure(style.small(), &joined) <= width => {
                if let Some(last) = lines.last_mut() {
                    *last = joined;
                }
            }
            _ if lines.len() == fits || font.measure(style.small(), word) > width => {
                // No line left, or a word too wide: the rest shortened
                // on the last line there is, and no more.
                let rest = words[n..].join(" ");
                let full = lines.len() == fits;
                match lines.last_mut() {
                    Some(last) if full => {
                        *last = ui::fit(&font, style.small(), &format!("{last} {rest}"), width);
                    }
                    _ => lines.push(ui::fit(&font, style.small(), &rest, width)),
                }
                break;
            }
            _ => lines.push(String::from(*word)),
        }
    }
    lines
}

/// A book without a cover: a frame with its title (`lines`).
fn draw_placeholder(ui: &mut ui::Ui<'_>, area: Rect, lines: &[String], ink: Color) {
    let style = ui.style();
    ui.canvas()
        .stroke(area, u32::from(style.line()).max(2), ink);
    let Some(font) = ui.font() else {
        return;
    };
    let pad = style.gap() as f32 / 2.0;
    let line = font.line_metrics(style.small()).line_height;
    for (n, text) in lines.iter().enumerate() {
        let y = area.y + pad as u16 + (n as f32 * line) as u16;
        let row = Rect::new(area.x, y, area.width, line as u16);
        ui::Label::new(text).px(style.small()).color(ink).draw_from(
            ui,
            f32::from(area.x) + pad,
            row,
        );
    }
}

// ---- Tools ----------------------------------------------------------------

/// Draws the tool of `action` in `area`, its name if tools show names,
/// with room for a name of `lines` lines (see
/// [`grid::draw_tool_lines`](paperpaw_sdk::widgets::grid::draw_tool_lines)):
/// where its icon and name have ink.
pub(crate) fn draw_tool_lines(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    action: &Id,
    lines: usize,
) -> Rect {
    let labels = env.settings().read(&TOOL_LABELS);
    paperpaw_sdk::widgets::grid::draw_tool_lines(canvas, env, area, action, labels, lines)
}

/// How many lines the name of the tool of `action` takes in `area`.
pub(crate) fn tool_name_lines(env: &Env<'_>, area: Rect, action: &Id) -> usize {
    let labels = env.settings().read(&TOOL_LABELS);
    paperpaw_sdk::widgets::grid::tool_name_lines(env, area, action, labels)
}

/// How many of the recent books the widgets in `items` show: the current
/// book, and the ones after it on the shelf.
pub(crate) fn books_shown(env: &Env<'_>, items: &[grid::Item]) -> usize {
    items
        .iter()
        .map(|item| match &item.what {
            // Each shelf as many as its placement shows.
            grid::What::Widget(id) if widgets::base(id) == RECENT => {
                env.settings().with_placement(Some(id), || shelf_count(env)) + 1
            }
            grid::What::Widget(id) if widgets::base(id) == CONTINUE => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// The first `books` recent books whose covers were not looked up yet.
pub(crate) fn missing_covers(env: &Env<'_>, books: usize) -> Vec<String> {
    recent::load(env.storage(), books)
        .into_iter()
        .filter(|b| !b.indexed)
        .map(|b| b.path)
        .collect()
}
