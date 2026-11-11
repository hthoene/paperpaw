//! The statistics' widgets on interface 7: the host makes an instance when a
//! screen shows one (`event.widget_made`), asks it for its height and where it
//! sits through the `core.widget` stage, and calls the plugin to draw it
//! (`DrawPurpose::Widget`, `frame.id` the instance). Nothing of a callback is
//! kept: an instance is the widget's id with its placement, and what it shows
//! is read from the statistics' files when it is measured or drawn. A tap on
//! any of them opens the statistics.
//!
//! The pieces the widgets share (the figures, the shades, the captions) are
//! here; each widget is a module of its own.
use crate::files::DataFolder;
use crate::native_screens::{failed, utc_day};
use crate::store::{self, Day};
use crate::texts::Words;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, FrameRect, Input};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::reply::Reply;
use paperpaw_sdk::native::snapshot::capture_snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::ui::{Instance, WidgetInputKind, WidgetTarget, stages::WidgetCall};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Color, Rect};
use paperpaw_ui_kit::widget::layout::{caption, caption_height};
use paperpaw_ui_kit::widget::settings::text_of;
use paperpaw_ui_kit::widget::text::{Wrapped, largest_size};
use paperpaw_ui_kit::widget::{Cx, Look, TextSize};
use paperpaw_ui_kit::{Paint, geometry, paint};

use crate::declarations::{
    CALENDAR_WIDGET_TOKEN, RING_TOKEN, STREAK_TOKEN, TODAY_TOKEN, WEEK_TOKEN, YEAR_TOKEN,
};

/// The most bytes of a stage reply (a size, or one target).
const REPLY: usize = 256;
/// The target of a tap: the whole widget.
const WHOLE: u32 = 1;

/// What each widget does, by the token it was declared with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Behaviour {
    /// The natural height at a width; 0 shows nothing.
    pub height: fn(&mut Cx<'_>, u16) -> Result<u16, Error>,
    /// Lays the widget out in a rectangle.
    pub draw: fn(&mut Cx<'_>, &mut Vec<Paint>, Rect, &Slot) -> Result<(), Error>,
}

static TODAY: Behaviour = Behaviour {
    height: crate::native_widget_today::height,
    draw: crate::native_widget_today::draw,
};
static WEEK: Behaviour = Behaviour {
    height: crate::native_widget_week::height,
    draw: crate::native_widget_week::draw,
};

static RING: Behaviour = Behaviour {
    height: crate::native_widget_ring::height,
    draw: crate::native_widget_ring::draw,
};

static STREAK: Behaviour = Behaviour {
    height: crate::native_widget_streak::height,
    draw: crate::native_widget_streak::draw,
};

static CALENDAR: Behaviour = Behaviour {
    height: crate::native_widget_calendar::height,
    draw: crate::native_widget_calendar::draw,
};

static YEAR: Behaviour = Behaviour {
    height: crate::native_widget_year::height,
    draw: crate::native_widget_year::draw,
};

/// The behaviour of the widget declared with `token`.
fn behaviour(token: u32) -> Option<&'static Behaviour> {
    match token {
        TODAY_TOKEN => Some(&TODAY),
        WEEK_TOKEN => Some(&WEEK),
        RING_TOKEN => Some(&RING),
        STREAK_TOKEN => Some(&STREAK),
        CALENDAR_WIDGET_TOKEN => Some(&CALENDAR),
        YEAR_TOKEN => Some(&YEAR),
        _ => None,
    }
}

/// The token of the widget `widget` (its id, with a placement perhaps).
fn token_of(widget: &str) -> Option<u32> {
    match paperpaw_ui_kit::widget::settings::base(widget) {
        "stats.widget.today" => Some(TODAY_TOKEN),
        "stats.widget.week" => Some(WEEK_TOKEN),
        "stats.widget.goal" => Some(RING_TOKEN),
        "stats.widget.streak" => Some(STREAK_TOKEN),
        "stats.widget.calendar" => Some(CALENDAR_WIDGET_TOKEN),
        "stats.widget.year" => Some(YEAR_TOKEN),
        _ => None,
    }
}

/// What a tap on a widget does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tap {
    /// Opens the statistics.
    Statistics,
    /// Opens the statistics' settings, where the goal is set.
    Goal,
    /// Opens the books.
    Books,
}

/// One widget made for a screen.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when it was last asked about, for the tick.
    shown: Option<u64>,
    /// What a widget keeps between its draws.
    slot: Slot,
}

/// What a widget keeps between its draws (the figures it worked out, as
/// interface 6 kept them per widget, not the log they come from).
#[derive(Debug, Default)]
pub(crate) struct Slot {
    pub streak: crate::native_widget_streak::Cache,
    pub calendar: crate::native_widget_calendar::Cache,
    pub year: crate::native_widget_year::Cache,
}

/// The widgets made for screens, by the host's instance.
#[derive(Debug, Default)]
pub(crate) struct Widgets {
    items: Vec<Item>,
}

impl Widgets {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    /// A screen shows the widget `widget`: it gets an instance. A widget this
    /// plugin does not make is none of its business.
    pub(crate) fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let Some(token) = token_of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: paperpaw_ui_kit::text::copy(widget)?,
            shown: None,
            slot: Slot::default(),
        });
        Ok(())
    }

    /// The screen dropped the instance.
    pub(crate) fn dropped(&mut self, instance: Instance) {
        self.items.retain(|item| item.instance != instance);
    }

    /// Answers a call of the stage `core.widget`: the height, the place kept,
    /// the one target (the whole widget).
    pub(crate) fn stage(
        &self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = paperpaw_ui_kit::mem::buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let height = (behaviour.height)(&mut cx, width)?;
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_targets(&[WidgetTarget::new(WHOLE, rect)], &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Draws the instance `frame.id` is.
    pub(crate) fn draw(&self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = area_of(frame.rect);
        let mut out = Vec::new();
        (behaviour.draw)(&mut cx, &mut out, area, &item.slot)?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    /// What `input` is, if a tap on one of the widgets: the ring asks for a
    /// goal where there is none, the year opens the books, the others open the
    /// statistics.
    ///
    /// # Errors
    /// Host refusals.
    pub(crate) fn tapped(&self, host: &Host<'_>, input: &Input<'_>) -> Result<Option<Tap>, Error> {
        let Input::Widget {
            instance,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(None);
        };
        let Some(item) = self.find(instance.get()) else {
            return Ok(None);
        };
        Ok(Some(if item.token == RING_TOKEN && goal(host)?.is_none() {
            Tap::Goal
        } else if item.token == YEAR_TOKEN {
            Tap::Books
        } else {
            Tap::Statistics
        }))
    }

    /// Tells the makers whose widget shows something new since the last
    /// minute: the day changed, or there is more reading. One that fails is
    /// left for the next tick and costs the others nothing.
    pub(crate) fn tick(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = paperpaw_sdk::native::commands::CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let Ok(now) = stamp(host) else {
                continue;
            };
            let instance = item.instance;
            if item.shown.is_some_and(|before| before != now)
                && commands.widget_changed(instance).is_err()
            {
                continue;
            }
            item.shown = Some(now);
        }
    }
}

/// The rectangle of a frame as the drawing area.
fn area_of(rect: FrameRect) -> Rect {
    Rect::new(
        geometry::clamp_u16(i64::from(rect.x)),
        geometry::clamp_u16(i64::from(rect.y)),
        geometry::clamp_u16(i64::from(rect.width)),
        geometry::clamp_u16(i64::from(rect.height)),
    )
}

/// What the widgets read their numbers for: today, and the sizes of the log
/// and of the books. It changes when there is a new day or more reading.
///
/// # Errors
/// Host refusals.
pub(crate) fn stamp(host: &Host<'_>) -> Result<u64, Error> {
    let today = today_of(host)?.map_or(0, |day| i64::from(day) + 1);
    let storage = DataStorage::new(host);
    let (log, books) = store::sizes(&DataFolder(&storage));
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for part in [today as u64, log, books] {
        hash = (hash ^ part).wrapping_mul(0x0100_0000_01b3);
    }
    Ok(hash)
}

/// The day now (UTC until the system zone comes with the snapshot), if the
/// device knows the time.
///
/// # Errors
/// Host refusals.
pub(crate) fn today_of(host: &Host<'_>) -> Result<Option<i32>, Error> {
    Ok(capture_snapshot(host)?
        .unix_seconds()
        .and_then(|unix| u32::try_from(unix).ok())
        .map(utc_day))
}

/// Today and the days read, if the device knows the time.
///
/// # Errors
/// Host refusals and [`Error::NoMemory`].
pub(crate) fn days(cx: &Cx<'_>) -> Result<Option<(i32, Vec<Day>)>, Error> {
    let Some(today) = cx
        .snapshot
        .unix_seconds()
        .and_then(|unix| u32::try_from(unix).ok())
        .map(utc_day)
    else {
        return Ok(None);
    };
    let storage = DataStorage::new(cx.host);
    // A log that cannot be read shows as no days (as interface 6 did).
    let days = match store::days(&DataFolder(&storage), utc_day) {
        Ok(days) => days,
        Err(error) if failed(error) == Error::NoMemory => return Err(Error::NoMemory),
        Err(_) => Vec::new(),
    };
    Ok(Some((today, days)))
}

/// The catalog of a widget's context as the plugin's texts.
impl Words for Cx<'_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.tr(key)
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        Cx::format(self, key, args)
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        self.messages.digits(self.host, text)
    }
}

/// The setting: minutes a day to read, or none.
const GOAL: &str = "stats.goal";

/// The daily goal in seconds, if one is set.
///
/// # Errors
/// Host refusals.
pub(crate) fn goal(host: &Host<'_>) -> Result<Option<u64>, Error> {
    text_of(host, GOAL, |value| {
        value
            .rsplit('.')
            .next()
            .and_then(|minutes| minutes.parse::<u64>().ok())
            .filter(|&minutes| minutes > 0)
            .map(|minutes| minutes * 60)
    })
}

/// The first day of the week, 0 (Sunday) or 1 (Monday), and the day's
/// weekday for a local day.
pub(crate) fn weekday_of(day: i32) -> u8 {
    crate::texts::date_of(day).weekday
}

/// How many pixels `tenths` of a millimetre take on this display.
pub(crate) fn mm(cx: &Cx<'_>, tenths: u32) -> u16 {
    let dpi = u32::from(cx.snapshot.display_dpi());
    geometry::clamp_u16(i64::from((tenths * dpi + 127) / 254))
}

/// The height a widget's caption takes, if it shows one (the part `caption`).
///
/// # Errors
/// Font and host refusals.
pub(crate) fn caption_room(cx: &Cx<'_>) -> Result<u16, Error> {
    if cx.settings.shows("caption")? {
        caption_height(&cx.font, cx.style().small, cx.style().gap)
    } else {
        Ok(0)
    }
}

/// The first of `texts` (the full one, then shorter ones) that fits the width
/// whole, where the widget shows its caption and there is room for a line of
/// text below it as well; else none: a caption is left out whole rather than
/// cut, and the content goes first.
///
/// # Errors
/// Font and host refusals.
pub(crate) fn pick_caption<'t>(
    cx: &Cx<'_>,
    area: Rect,
    texts: &[&'t str],
) -> Result<Option<&'t str>, Error> {
    let room = caption_room(cx)?;
    let least = geometry::line_px(&cx.font, cx.style().text)?;
    if room == 0 || room.saturating_add(least) > area.height {
        return Ok(None);
    }
    let width = f32::from(area.width);
    for text in texts {
        if cx.font.measure_text(cx.style().small, text)? <= width {
            return Ok(Some(text));
        }
    }
    Ok(None)
}

/// Draws `text` as the caption at the top of `area`; the area below it.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub(crate) fn draw_caption(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    text: &str,
) -> Result<Rect, Error> {
    caption(
        out,
        &cx.font,
        (cx.style().small, cx.style().gap),
        (area, cx.settings.align()?, look.faint),
        text,
    )
}

/// The room `caption` takes at the top of an area (none without one).
///
/// # Errors
/// Font refusals.
pub(crate) fn caption_used(cx: &Cx<'_>, caption: Option<&str>, area: Rect) -> Result<u16, Error> {
    if caption.is_none() {
        return Ok(0);
    }
    Ok(caption_height(&cx.font, cx.style().small, cx.style().gap)?.min(area.height))
}

/// Places content `height` high with the `caption` (if any) above it, the two
/// together in the middle of `look`'s area, and draws the caption; where the
/// content goes. Where the two do not fit together, the caption is left out.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub(crate) fn captioned(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    caption: Option<&str>,
    height: u16,
) -> Result<Rect, Error> {
    let cap = caption_height(&cx.font, cx.style().small, cx.style().gap)?;
    let caption = caption.filter(|_| cap.saturating_add(height) <= look.area.height);
    let cap = if caption.is_some() { cap } else { 0 };
    let block = middle(look.area, cap.saturating_add(height));
    if let Some(text) = caption {
        draw_caption(cx, out, look, block, text)?;
    }
    Ok(Rect::new(
        block.x,
        block.y.saturating_add(cap),
        block.width,
        height.min(block.height.saturating_sub(cap)),
    ))
}

/// The largest size from `max` down to `least` at which `text` fits `width`
/// and a line of it `height`; where it is too wide even then, the largest
/// whose line fits the height (down to the small size), at which it is
/// shortened.
///
/// # Errors
/// Font refusals.
pub(crate) fn sized(
    cx: &Cx<'_>,
    text: &str,
    (width, height): (u16, u16),
    (max, least): (f32, f32),
) -> Result<f32, Error> {
    let small = cx.style().small;
    let wide = f32::from(width);
    let tall =
        |size: f32| -> Result<bool, Error> { Ok(geometry::line_px(&cx.font, size)? <= height) };
    let found = largest_size(max, least.min(max), |size| {
        Ok(tall(size)? && cx.font.measure_text(size, text)? <= wide)
    })?;
    if let Some(size) = found {
        return Ok(size);
    }
    Ok(largest_size(max.min(least), small, tall)?.unwrap_or(small.min(max)))
}

/// The text size the widget's size setting gives, smaller where `text` is
/// wider than `width` (down to the body's size, below which it is shortened)
/// or a line of it higher than `height` (down to the small size); "as large as
/// fits" fits it into both, as large as they hold.
///
/// # Errors
/// Font and host refusals.
pub(crate) fn text_size(cx: &Cx<'_>, text: &str, width: u16, height: u16) -> Result<f32, Error> {
    let style = cx.style();
    let max = match cx.settings.size()? {
        TextSize::Fit => f32::from(height).max(style.text),
        size => size.text(style),
    };
    sized(cx, text, (width, height), (max, style.text))
}

/// `text` small in lines `width` wide, as many as fit `height` (`most` at
/// most): whole where it fits, else ending in an ellipsis where there is no room
/// for more; none where not even a line fits.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub(crate) fn note_lines(
    cx: &Cx<'_>,
    text: &str,
    (width, height): (u16, u16),
    most: u16,
) -> Result<Option<Wrapped>, Error> {
    let small = cx.style().small;
    let line = geometry::line_px(&cx.font, small)?.max(1);
    let height = height.min(line.saturating_mul(most));
    if text.is_empty() || height < line {
        return Ok(None);
    }
    Ok(Some(Wrapped::fitting(
        &cx.font,
        text,
        (small, small),
        (width, height),
        false,
    )?))
}

/// `text`, or where it is wider than `width` at `size`, the number it begins
/// with alone ("5 days": "5").
///
/// # Errors
/// Font refusals.
pub(crate) fn number_if_narrow<'t>(
    cx: &Cx<'_>,
    size: f32,
    text: &'t str,
    width: u16,
) -> Result<&'t str, Error> {
    if cx.font.measure_text(size, text)? <= f32::from(width) {
        return Ok(text);
    }
    let digits = text
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map_or(text.len(), |(at, _)| at);
    Ok(if digits == 0 {
        text
    } else {
        text.get(..digits).unwrap_or(text)
    })
}

/// `height` pixels in the middle of `area`, top to bottom.
pub(crate) fn middle(area: Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    Rect::new(
        area.x,
        area.y + (area.height - height) / 2,
        area.width,
        height,
    )
}

/// The colour between `paper` and its ink at `level` of 4 (4: the ink): light
/// on light paper, the other way round on a dark card.
pub(crate) fn shade(paper: Color, level: u8) -> Color {
    const LIGHT: [u8; 5] = [255, 205, 160, 95, 0];
    let gray = LIGHT[usize::from(level.min(4))];
    if paper.luma() < 128 {
        Color::gray(255 - gray)
    } else {
        Color::gray(gray)
    }
}

/// A dashed line across from `x` to `end` at `y`, `thickness` thick.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn dashed(
    out: &mut Vec<Paint>,
    (x, end, y): (i32, i32, i32),
    thickness: u32,
    color: Color,
) -> Result<(), Error> {
    let dash = (thickness * 4).max(4) as i32;
    let mut at = x;
    while at < end {
        let width = dash.min(end - at).max(0) as u16;
        let rect = Rect::new(
            geometry::clamp_u16(i64::from(at)),
            geometry::clamp_u16(i64::from(y)),
            width,
            geometry::clamp_u16(i64::from(thickness)),
        );
        paint::push(out, Paint::Fill(rect, color))?;
        at += dash * 2;
    }
    Ok(())
}

/// The widget's own message in its frame (no clock).
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub(crate) fn message(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    key: &str,
) -> Result<(), Error> {
    let text = cx.tr(key)?;
    let size = cx.style().small;
    paperpaw_ui_kit::widget::layout::label(
        out,
        &cx.font,
        (size, paperpaw_ui_kit::widget::Align::Left),
        look.area,
        &text,
        look.faint,
    )
}

/// Draws `text` at `x` with its line centred in `row`, `size` pixels large.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub(crate) fn text_at(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    text: &str,
    (size, color): (f32, Color),
    (x, row): (f32, Rect),
) -> Result<(), Error> {
    let (ascent, descent, _) = geometry::line_metrics(&cx.font, size)?;
    let baseline =
        i32::from(row.y).saturating_add(((f32::from(row.height) + ascent - descent) / 2.0) as i32);
    paint::push(
        out,
        Paint::Text {
            text: paperpaw_ui_kit::text::copy(text)?,
            size,
            origin: paperpaw_sdk::native::fonts::TextOrigin::new(x, baseline),
            color,
            clip: None,
        },
    )
}

/// The widget *Read today*, as the home screen's list names it.
const TODAY_ID: &str = "stats.widget.today";
/// The pages of the home screen, the first of them first.
const HOME_PAGES: [&str; 4] = ["home.page-1", "home.page-2", "home.page-3", "home.page-4"];
/// The file that says the widget was offered (a widget the reader removed stays
/// away).
const OFFERED: &str = "goal-widget-offered";

/// A goal set shows on the home screen: the first time, *Read today* goes onto
/// its first page, after the book being read, if no page shows it yet. Offered
/// once only (kept as a file of the plugin), so that a widget the reader
/// removed stays away.
///
/// # Errors
/// Host refusals and [`Error::NoMemory`].
pub(crate) fn offer_today(host: &Host<'_>) -> Result<(), Error> {
    if goal(host)?.is_none() {
        return Ok(());
    }
    let storage = DataStorage::new(host);
    if paperpaw_sdk::Storage::exists(&storage, OFFERED) {
        return Ok(());
    }
    let mut scratch = paperpaw_ui_kit::mem::buffer(2048)?;
    let mut first: Option<String> = None;
    let mut shown = false;
    for (n, page) in HOME_PAGES.iter().enumerate() {
        let list = match paperpaw_sdk::native::settings::read_setting(host, page, &mut scratch) {
            Ok(paperpaw_sdk::native::settings::SettingValue::Text(list)) => list,
            _ => continue,
        };
        shown |= paperpaw_sdk::status::item_ids(list)
            .any(|item| item.split_whitespace().next() == Some(TODAY_ID));
        if n == 0 {
            first = Some(paperpaw_ui_kit::text::copy(list)?);
        }
    }
    // Nothing to offer it on (another home screen), or it is shown already.
    let Some(list) = first.filter(|_| !shown) else {
        return Ok(());
    };
    let mut ids: Vec<&str> = Vec::new();
    ids.try_reserve(paperpaw_sdk::status::item_ids(&list).count() + 1)
        .map_err(|_| Error::NoMemory)?;
    ids.extend(paperpaw_sdk::status::item_ids(&list));
    let at = ids
        .iter()
        .position(|item| item.split_whitespace().next() == Some("home.continue"))
        .map_or(ids.len(), |i| i + 1);
    ids.insert(at, TODAY_ID);
    let mut joined = String::new();
    joined
        .try_reserve_exact(ids.iter().map(|id| id.len() + 2).sum())
        .map_err(|_| Error::NoMemory)?;
    for (n, id) in ids.iter().enumerate() {
        if n > 0 {
            joined.push_str(", ");
        }
        joined.push_str(id);
    }
    let mut bytes = paperpaw_ui_kit::mem::buffer(joined.len() + 256)?;
    paperpaw_sdk::native::commands::CommandSender::new(host, &mut bytes).set_setting(
        HOME_PAGES[0],
        paperpaw_sdk::native::settings::SettingValue::Text(&joined),
    )?;
    // Kept as offered only once the change is staged, so that a refused one
    // is tried again at the next change of the goal. A marker that cannot be
    // written costs a second offer at most, and none while the widget is on
    // the page (`shown`).
    paperpaw_sdk::Storage::write(&storage, OFFERED, b"").unwrap_or(());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shades_turn_round_on_a_dark_card() {
        assert_eq!(shade(Color::WHITE, 4), Color::BLACK);
        assert_eq!(shade(Color::BLACK, 4), Color::WHITE);
        assert!(shade(Color::WHITE, 1).luma() > shade(Color::WHITE, 3).luma());
        assert!(shade(Color::BLACK, 1).luma() < shade(Color::BLACK, 3).luma());
    }

    #[test]
    fn a_dashed_line_is_dashes_with_gaps_inside_its_span() {
        let mut out = Vec::new();
        dashed(&mut out, (10, 50, 7), 2, Color::BLACK).unwrap();
        // Dashes of 8 pixels every 16: at 10, 26 and 42 (the last cut at 50).
        let widths: Vec<(u16, u16)> = out
            .iter()
            .map(|op| match op {
                Paint::Fill(rect, _) => (rect.x, rect.width),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(widths, [(10, 8), (26, 8), (42, 8)]);
        let mut none = Vec::new();
        dashed(&mut none, (30, 10, 0), 1, Color::BLACK).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn a_goal_is_the_minutes_of_its_value() {
        let minutes = |value: &str| {
            value
                .rsplit('.')
                .next()
                .and_then(|minutes| minutes.parse::<u64>().ok())
                .filter(|&minutes| minutes > 0)
                .map(|minutes| minutes * 60)
        };
        assert_eq!(minutes("stats.goal.off"), None);
        assert_eq!(minutes("stats.goal.30"), Some(1800));
        assert_eq!(minutes("stats.goal.0"), None);
        for value in crate::declarations::GOALS {
            assert_eq!(minutes(value).is_some(), value != "stats.goal.off");
        }
    }
}
