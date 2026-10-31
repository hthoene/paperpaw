//! The reader's status lines above and below the text.
//!
//! For each of the two places the user chooses status lines to switch
//! between (setting [`LINES`]); a tap on the line shows the next one, and
//! the choice is kept. Lines come from any plugin
//! (`Registrar::add_status_line`); the reader brings three:
//!
//! - *Text and symbols* (the kernel's `core.status-line.items`, which
//!   answers to the reader's `reader.status-line.items` too): a left,
//!   centre and right part, each with the status items chosen for it
//!   (`core.status-items`: the kernel's time and battery, the reader's
//!   title, chapter, page and progress, and whatever other plugins add);
//! - *Progress bar* ([`lines::PROGRESS`]): the book as a bar, with a mark
//!   where each chapter begins, and the page;
//! - *Empty* ([`lines::EMPTY`]): nothing, until the next tap.
//!
//! A place without lines takes no space. A place's height depends only
//! on the text size of status lines ([`SIZE`]), so switching lines never
//! changes the pages.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{fonts, params, screens};
use paperpaw_sdk::status::{Place, StatusFrame, StatusTarget, StatusValue, context, item_ids};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::{
    Arg, Canvas, Env, Id, Params, RegisterError, Registrar, SettingExtra, SettingKind, SettingSpec,
    SettingValue, StatusItemSpec, StatusLineSpec, Storage,
};

use crate::PLUGIN;
use paperpaw_sdk::setting::Setting;

/// The settings group of the status lines (the kernel's).
pub const GROUP: Id = paperpaw_sdk::builtin::groups::STATUS_LINES;

/// The battery's charge in percent beside its symbol.
pub const PERCENT: Setting<bool> = Setting::new("reader.status.battery-percent");
/// Text size of the status lines, in points.
pub const SIZE: Setting<i64> = Setting::new("reader.status.size");
/// How far the status lines sit from the edge of the screen, in tenths of
/// a millimetre: closer than the text's margin, so the text has the room.
pub const EDGE: Setting<i64> = Setting::new("reader.status.edge");
/// The status lines to switch between, above and below the text (ids
/// separated by commas; the first shows at first).
pub const LINES: [Id; 2] = [TOP_LINES, BOTTOM_LINES];
const TOP_LINES: Id = Id::new_static("reader.status.top-lines");
const BOTTOM_LINES: Id = Id::new_static("reader.status.bottom-lines");
/// The settings' sections: the line above and the line below.
const SECTION_TOP: &str = "reader.status.section.top";
const SECTION_BOTTOM: &str = "reader.status.section.bottom";
const TOP_PLACE: &str = "reader.status.top";
const BOTTOM_PLACE: &str = "reader.status.bottom";
/// Where status items show: above the text and below it.
pub const PLACES: [Place; 2] = [
    Place::new(
        Id::new_static(TOP_PLACE),
        SECTION_TOP,
        [TOP_LEFT, TOP_CENTER, TOP_RIGHT],
    ),
    Place::new(
        Id::new_static(BOTTOM_PLACE),
        SECTION_BOTTOM,
        [BOTTOM_LEFT, BOTTOM_CENTER, BOTTOM_RIGHT],
    ),
];

/// The reader's status lines.
pub mod lines {
    use paperpaw_sdk::Id;

    pub const PROGRESS: Id = Id::new_static("reader.status-line.progress");
    pub const EMPTY: Id = Id::new_static("reader.status-line.empty");
}

const TOP_LEFT: Id = Id::new_static("reader.status.top-left");
const TOP_CENTER: Id = Id::new_static("reader.status.top-center");
const TOP_RIGHT: Id = Id::new_static("reader.status.top-right");
const BOTTOM_LEFT: Id = Id::new_static("reader.status.bottom-left");
const BOTTOM_CENTER: Id = Id::new_static("reader.status.bottom-center");
const BOTTOM_RIGHT: Id = Id::new_static("reader.status.bottom-right");

/// The reader's status items.
pub mod items {
    use paperpaw_sdk::Id;

    pub const TITLE: Id = Id::new_static("reader.status.title");
    /// The chapter, or the title in books without a table of contents.
    pub const CHAPTER: Id = Id::new_static("reader.status.chapter");
    /// `12 / 340`, or a percentage while pages are being counted.
    pub const PAGE: Id = Id::new_static("reader.status.page");
    /// Reading progress in percent.
    pub const PROGRESS: Id = Id::new_static("reader.status.progress");
    /// A speaker while the book is read aloud, a clock while its pages
    /// turn by themselves.
    pub const ALOUD: Id = Id::new_static("reader.status.aloud");
    /// The page of the printed edition, where the book names it.
    pub const PRINTED_PAGE: Id = Id::new_static("reader.status.printed-page");
    /// The page in the chapter: `5 / 25`, once the pages are counted.
    pub const CHAPTER_PAGE: Id = Id::new_static("reader.status.chapter-page");
    /// The pages after this one to the chapter's end.
    pub const CHAPTER_LEFT: Id = Id::new_static("reader.status.chapter-left");
    /// How far into the chapter the page is, as a short bar.
    pub const CHAPTER_BAR: Id = Id::new_static("reader.status.chapter-bar");
}

/// Context: the printed edition's page.
pub(crate) const PRINTED_CONTEXT: &str = "reader.printed-page";

/// Context: the page in the chapter (from 1) and the chapter's pages.
pub(crate) const CHAPTER_PAGE_CONTEXT: &str = "reader.chapter-page";
pub(crate) const CHAPTER_PAGES_CONTEXT: &str = "reader.chapter-pages";

/// Context: the icon of what turns the pages by itself (reading aloud,
/// turning automatically).
pub(crate) const ALOUD_CONTEXT: &str = "reader.aloud";

static PT: SettingExtra = SettingExtra::new().unit("core.unit.pt");
static TENTH_MM_5: SettingExtra = SettingExtra::new().unit("core.unit.tenth-mm").step(5);
/// The lines of a place, chosen on their editor, in the place's section.
static TOP_LINES_EXTRA: SettingExtra = SettingExtra::new()
    .section(SECTION_TOP)
    .editor(screens::STATUS_LINES);
static BOTTOM_LINES_EXTRA: SettingExtra = SettingExtra::new()
    .section(SECTION_BOTTOM)
    .editor(screens::STATUS_LINES);
/// A part's status items, chosen on their picker, in its place's section.
static TOP_ITEMS: SettingExtra = SettingExtra::new()
    .section(SECTION_TOP)
    .editor(screens::STATUS_ITEMS);
static BOTTOM_ITEMS: SettingExtra = SettingExtra::new()
    .section(SECTION_BOTTOM)
    .editor(screens::STATUS_ITEMS);

const fn list(
    id: Id,
    title: &'static str,
    default: &'static str,
    extra: &'static SettingExtra,
) -> SettingSpec {
    SettingSpec::new(
        paperpaw_sdk::Id::new_static(id.static_str()),
        title,
        SettingKind::List,
        SettingValue::Text(Cow::Borrowed(default)),
        GROUP,
    )
    .with_extra(extra)
}

/// A part of *Text and symbols*: its status items.
const fn part(
    id: Id,
    title: &'static str,
    default: &'static str,
    extra: &'static SettingExtra,
) -> SettingSpec {
    list(id, title, default, extra).description("reader.status.part.help")
}

/// The text size and place of the lines, which lines show above and
/// below the text (below: the items, the progress bar, nothing; above:
/// none), what each part of *Text and symbols* shows (the chapter, the
/// time and the battery with the page, and Wi-Fi while it is in use),
/// and the battery's percent.
static SETTINGS: [SettingSpec; 11] = [
    SettingSpec::number(SIZE.static_str(), "reader.status.size", 5, 16, 6, GROUP)
        .with_extra(&PT)
        .description("reader.status.size.help"),
    SettingSpec::number(EDGE.static_str(), "reader.status.edge", 0, 150, 15, GROUP)
        .advanced()
        .with_extra(&TENTH_MM_5)
        .description("reader.status.edge.help"),
    list(TOP_LINES, "reader.status.top-lines", "", &TOP_LINES_EXTRA)
        .description("reader.status.lines.help"),
    list(
        BOTTOM_LINES,
        "reader.status.bottom-lines",
        "core.status-line.items, reader.status-line.progress, reader.status-line.empty",
        &BOTTOM_LINES_EXTRA,
    )
    .description("reader.status.lines.help"),
    part(TOP_LEFT, "reader.status.top-left", "", &TOP_ITEMS),
    part(TOP_CENTER, "reader.status.top-center", "", &TOP_ITEMS),
    part(TOP_RIGHT, "reader.status.top-right", "", &TOP_ITEMS),
    part(
        BOTTOM_LEFT,
        "reader.status.bottom-left",
        "reader.status.aloud, reader.status.chapter",
        &BOTTOM_ITEMS,
    ),
    part(
        BOTTOM_CENTER,
        "reader.status.bottom-center",
        "core.status.clock",
        &BOTTOM_ITEMS,
    ),
    part(
        BOTTOM_RIGHT,
        "reader.status.bottom-right",
        "lock.touch-status, wifi.status, power.status, core.status.battery, reader.status.page",
        &BOTTOM_ITEMS,
    ),
    SettingSpec::switch(
        PERCENT.static_str(),
        "reader.status.battery-percent",
        true,
        GROUP,
    )
    .description("reader.status.battery-percent.help"),
];

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&SETTINGS)?;
    for place in PLACES {
        r.add_status_place(place)?;
    }
    r.add_status_line(
        StatusLineSpec::new(
            lines::PROGRESS,
            "reader.status.line.progress",
            Box::new(draw_progress),
        )
        .signature(Box::new(|_, cx| {
            let mut hash = paperpaw_sdk::digest::Fnv1a64::new();
            hash.feed(cx.get(params::PROGRESS).unwrap_or_default().as_bytes());
            hash.feed(b"|");
            hash.feed(cx.get(context::PAGE).unwrap_or_default().as_bytes());
            let chapters = cx.get(context::CHAPTERS).unwrap_or_default().len();
            hash.feed(&chapters.to_le_bytes());
            hash.value()
        })),
    )?;
    r.add_status_line(
        StatusLineSpec::new(
            lines::EMPTY,
            "reader.status.line.empty",
            Box::new(|_, _, _| {}),
        )
        .signature(Box::new(|_, _| 0)),
    )?;
    let text = |value: Option<&str>| value.map(|v| StatusValue::Text(v.to_string()));
    // The book's and the chapter's titles give way first where a line
    // has no room, the page last.
    r.add_status_item(
        StatusItemSpec::new(
            items::TITLE,
            "reader.status.item.title",
            Box::new(move |_, cx| text(cx.get(params::TITLE))),
        )
        .priority(64),
    )?;
    r.add_status_item(
        StatusItemSpec::new(
            items::CHAPTER,
            "reader.status.item.chapter",
            Box::new(move |_, cx| text(cx.get(context::CHAPTER).or(cx.get(params::TITLE)))),
        )
        .priority(64),
    )?;
    r.add_status_item(StatusItemSpec::new(
        items::ALOUD,
        "reader.status.item.aloud",
        Box::new(|_, cx| {
            cx.get(ALOUD_CONTEXT)
                .map(|icon| StatusValue::Icon(Cow::Owned(icon.to_string())))
        }),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        items::PRINTED_PAGE,
        "reader.status.item.printed-page",
        Box::new(|env, cx| {
            let page = cx.get(PRINTED_CONTEXT)?;
            Some(StatusValue::Text(
                env.i18n()
                    .format("reader.printed-page", &[("page", Arg::Str(page))]),
            ))
        }),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        items::CHAPTER_PAGE,
        "reader.status.item.chapter-page",
        Box::new(|env, cx| {
            let (page, pages) = chapter_pages(cx)?;
            Some(StatusValue::Text(env.i18n().format(
                "reader.page",
                &[("page", Arg::Int(page)), ("pages", Arg::Int(pages))],
            )))
        }),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        items::CHAPTER_BAR,
        "reader.status.item.chapter-bar",
        Box::new(|_, cx| {
            let (page, pages) = chapter_pages(cx)?;
            let permille = (page * 1000 / pages.max(1)).clamp(0, 1000) as u16;
            Some(StatusValue::Bar { permille })
        }),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        items::CHAPTER_LEFT,
        "reader.status.item.chapter-left",
        Box::new(|env, cx| {
            let (page, pages) = chapter_pages(cx)?;
            let left = pages - page;
            Some(StatusValue::Text(env.i18n().plural(
                "reader.chapter-left",
                left,
                &[("count", Arg::Int(left))],
            )))
        }),
    ))?;
    r.add_status_item(
        StatusItemSpec::new(
            items::PAGE,
            "reader.status.item.page",
            Box::new(move |_, cx| text(cx.get(context::PAGE))),
        )
        .priority(224),
    )?;
    r.add_status_item(StatusItemSpec::new(
        items::PROGRESS,
        "reader.status.item.progress",
        Box::new(|env, cx| {
            let percent: f32 = cx.get(params::PROGRESS)?.parse().ok()?;
            Some(StatusValue::Text(env.i18n().format(
                "reader.percent",
                &[("percent", Arg::Int(percent as i64))],
            )))
        }),
    ))?;
    Ok(())
}

/// The page in the chapter and the chapter's pages, from the context.
fn chapter_pages(cx: &Params) -> Option<(i64, i64)> {
    let number = |key| cx.get(key)?.parse::<i64>().ok();
    Some((
        number(CHAPTER_PAGE_CONTEXT)?,
        number(CHAPTER_PAGES_CONTEXT)?,
    ))
}

/// The frame of the line at the top (or bottom) in `area`, in its place.
pub(crate) fn frame<'a>(
    env: &Env<'_>,
    area: paperpaw_sdk::Rect,
    top: bool,
    context: &'a Params,
) -> StatusFrame<'a> {
    let place = if top { TOP_PLACE } else { BOTTOM_PLACE };
    StatusFrame::new(area, top, size(env), color(env), context)
        .place(place)
        .percent(env.settings().read(&PERCENT))
}

/// Where a tap on the line shown at the top (or bottom) in `frame` does
/// something.
pub(crate) fn targets(env: &Env<'_>, id: &str, frame: &StatusFrame<'_>) -> Vec<StatusTarget> {
    env.status_items().line_targets(id, env, frame)
}

/// The text size of status lines in pixels.
pub(crate) fn size(env: &Env<'_>) -> f32 {
    env.metrics()
        .pt(env.settings().read(&SIZE).clamp(5, 16) as f32)
}

/// The height of a status line's place.
pub(crate) fn height(env: &Env<'_>) -> f32 {
    (size(env) * 2.1 + 0.5) as u32 as f32
}

/// The lines to switch between at the top (or bottom).
pub(crate) fn rotation<'e>(env: &Env<'e>, top: bool) -> Vec<&'e str> {
    item_ids(env.settings().text(LINES[usize::from(!top)].as_str())).collect()
}

/// The line showing at the top (or bottom): the one chosen last if it is
/// still among the lines to switch between, else the first.
pub(crate) fn current<'e>(env: &Env<'e>, top: bool, chosen: &str) -> &'e str {
    let rotation = rotation(env, top);
    rotation
        .iter()
        .find(|id| **id == chosen)
        .or(rotation.first())
        .copied()
        .unwrap_or("")
}

/// The line after `chosen` at the top (or bottom).
pub(crate) fn next<'e>(env: &Env<'e>, top: bool, chosen: &str) -> &'e str {
    let rotation = rotation(env, top);
    let at = rotation
        .iter()
        .position(|id| *id == current(env, top, chosen));
    match at {
        Some(at) => rotation[(at + 1) % rotation.len()],
        None => "",
    }
}

fn choice_path() -> String {
    format!("{}/status-lines.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The lines chosen last, top then bottom.
pub(crate) fn load_choice(storage: &dyn Storage) -> [String; 2] {
    let data = paperpaw_sdk::read_file(storage, &choice_path(), 1024).unwrap_or_default();
    let text = String::from_utf8_lossy(&data);
    let mut lines = text.lines().map(|l| l.trim().to_string());
    [
        lines.next().unwrap_or_default(),
        lines.next().unwrap_or_default(),
    ]
}

pub(crate) fn save_choice(storage: &dyn Storage, chosen: &[String; 2]) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "which status line the reader chose; not kept, the first shows next time"
    )]
    let _ = storage.write(
        &choice_path(),
        format!("{}\n{}\n", chosen[0], chosen[1]).as_bytes(),
    );
}

/// The colour of status lines: gray, or black for strong contrast.
pub(crate) fn color(env: &Env<'_>) -> paperpaw_sdk::Color {
    UiStyle::new(env).faint(paperpaw_sdk::ui::Ink::DETAIL)
}

/// The book as a bar from the left, filled up to where the page is, with
/// a mark where each chapter begins; the page on the right.
fn draw_progress(canvas: &mut Canvas<'_>, env: &Env<'_>, frame: &StatusFrame<'_>) {
    let Some(font) = env.fonts().role(fonts::STATUS.as_str()) else {
        return;
    };
    let area = frame.area;
    let progress = frame
        .context
        .get(params::PROGRESS)
        .and_then(|p| p.parse::<f32>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 100.0)
        / 100.0;
    let label = frame.context.get(context::PAGE).unwrap_or_default();
    let label_width = font.measure(frame.size, label);
    let gap = if label.is_empty() { 0.0 } else { frame.size };
    let x0 = f32::from(area.x);
    let width = (f32::from(area.width) - label_width - gap).max(1.0);
    let thickness = (frame.size * 0.3).max(3.0) as u32;
    let mid = i32::from(area.y) + i32::from(area.height) / 2;
    let top = mid - thickness as i32 / 2;
    let line = (frame.size / 12.0).max(1.0) as u32;
    canvas.stroke_rect(x0 as i32, top, width as u32, thickness, line, frame.color);
    let filled = (width * progress) as u32;
    canvas.fill_rect(x0 as i32, top, filled, thickness, frame.color);
    // Chapter marks stand out above and below the bar.
    let reach = thickness as i32;
    for mark in frame
        .context
        .get(context::CHAPTERS)
        .unwrap_or_default()
        .split(',')
        .filter_map(|m| m.trim().parse::<u32>().ok())
        .filter(|&m| m > 0 && m < 1000)
    {
        let x = x0 as i32 + (width * mark as f32 / 1000.0) as i32;
        canvas.fill_rect(
            x,
            top - reach / 2,
            line,
            thickness + reach as u32,
            frame.color,
        );
    }
    if !label.is_empty() {
        let metrics = font.line_metrics(frame.size);
        let baseline = i32::from(area.y)
            + ((f32::from(area.height) + metrics.ascent - metrics.descent) / 2.0) as i32;
        let x = x0 + f32::from(area.width) - label_width;
        font.draw(canvas, frame.size, x, baseline, label, frame.color);
    }
}

/// Whether there is a line at the top (or bottom): some status line to
/// show there.
pub(crate) fn shown(env: &Env<'_>, top: bool) -> bool {
    !rotation(env, top).is_empty()
}

/// What the lines shown now show, to notice when they need a redraw.
pub(crate) fn signature(env: &Env<'_>, context: &Params, chosen: &[String; 2]) -> u64 {
    let mut hash = paperpaw_sdk::digest::Fnv1a64::new();
    for top in [true, false] {
        let id = current(env, top, &chosen[usize::from(!top)]);
        hash.feed(id.as_bytes());
        hash.feed(
            &env.status_items()
                .line_signature(id, env, context)
                .to_le_bytes(),
        );
    }
    hash.value()
}

#[cfg(test)]
mod choice_tests;
