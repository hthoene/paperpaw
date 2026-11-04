//! The settings of the picture home screen: the picture and how it is set,
//! the bars, the grid and the pages (each a layout of items, as the
//! built-in home screen keeps them).

use alloc::borrow::Cow;
use alloc::format;
use alloc::vec::Vec;

use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::widgets::grid;
use paperpaw_sdk::{Env, Id, SettingExtra, SettingSpec};

use crate::{GROUP, PICK_SCREEN, SCREEN};

/// Pages at most.
pub(crate) const MAX_PAGES: usize = 4;

// ---- The picture ----------------------------------------------------------

/// The folder on the card the pictures are in.
pub(crate) const FOLDER: Setting<Text> = Setting::new("picture-home.folder");
/// The picture's file name in that folder; none: no picture.
pub(crate) const PICTURE: Setting<Text> = Setting::new("picture-home.picture");
pub(crate) const FIT: Setting<Text> = Setting::new("picture-home.fit");
pub(crate) const ANCHOR: Setting<Text> = Setting::new("picture-home.anchor");
pub(crate) const ZOOM: Setting<i64> = Setting::new("picture-home.zoom");
pub(crate) const LIGHTEN: Setting<i64> = Setting::new("picture-home.lighten");
/// How light the picture is and how strongly its dark and light parts
/// differ (values of [`BRIGHTNESSES`] and [`CONTRASTS`]): panels differ.
pub(crate) const BRIGHTNESS: Setting<Text> = Setting::new("picture-home.brightness");
pub(crate) const CONTRAST: Setting<Text> = Setting::new("picture-home.contrast");

/// From much darker to much lighter, the middle as it is.
pub(crate) const BRIGHTNESSES: [&str; 5] = [
    "picture-home.brightness.much-darker",
    "picture-home.brightness.darker",
    "picture-home.brightness.normal",
    "picture-home.brightness.lighter",
    "picture-home.brightness.much-lighter",
];
/// Lower, as it is, higher.
pub(crate) const CONTRASTS: [&str; 3] = [
    "picture-home.contrast.lower",
    "picture-home.contrast.normal",
    "picture-home.contrast.higher",
];

/// How the picture is rendered (values of [`RENDERS`]).
pub(crate) const RENDER: Setting<Text> = Setting::new("picture-home.render");

/// Automatic is as the built-in home screen: the screen drawn in the
/// panel's grays where it has them to show, then dots after a change;
/// grey levels always; dots always.
pub(crate) const RENDERS: [&str; 3] = [
    "picture-home.render.auto",
    "picture-home.render.grays",
    "picture-home.render.dither",
];

/// How the picture fills the screen (values of [`FIT`]).
pub(crate) const FITS: [&str; 5] = [
    "picture-home.fit.fill",
    "picture-home.fit.fit",
    "picture-home.fit.stretch",
    "picture-home.fit.original",
    "picture-home.fit.tile",
];

/// Which part of the picture stays where it is cut, or where it sits where
/// it is smaller than the screen: the nine places, row by row (values of
/// [`ANCHOR`]).
pub(crate) const ANCHORS: [&str; 9] = [
    "picture-home.anchor.top-left",
    "picture-home.anchor.top",
    "picture-home.anchor.top-right",
    "picture-home.anchor.left",
    "picture-home.anchor.center",
    "picture-home.anchor.right",
    "picture-home.anchor.bottom-left",
    "picture-home.anchor.bottom",
    "picture-home.anchor.bottom-right",
];

// ---- The bars ---------------------------------------------------------------

/// The status bar: [`BARS`], its place, its shape and its look.
pub(crate) const BAR: Setting<Text> = Setting::new("picture-home.bar");
pub(crate) const BAR_AT: Setting<Text> = Setting::new("picture-home.bar.at");
pub(crate) const BAR_SHAPE: Setting<Text> = Setting::new("picture-home.bar.shape");
pub(crate) const BAR_ALIGN: Setting<Text> = Setting::new("picture-home.bar.align");
pub(crate) const BAR_LOOK: Setting<Text> = Setting::new("picture-home.bar.look");
/// The row of hints for the buttons below the screen (the X4's): [`BARS`].
pub(crate) const HINTS: Setting<Text> = Setting::new("picture-home.hints");

/// Whether a bar shows: always, never, or for a few seconds after a press.
pub(crate) const BARS: [&str; 3] = [
    "picture-home.bar.off",
    "picture-home.bar.auto",
    "picture-home.bar.on",
];
pub(crate) const BAR_ATS: [&str; 2] = ["picture-home.bar.top", "picture-home.bar.bottom"];
pub(crate) const BAR_SHAPES: [&str; 2] = ["picture-home.bar.full", "picture-home.bar.compact"];
pub(crate) const BAR_ALIGNS: [&str; 3] = [
    "picture-home.bar.left",
    "picture-home.bar.center",
    "picture-home.bar.right",
];
/// What the bar is on: the page, the page inverted, grays, nothing.
pub(crate) const BAR_LOOKS: [&str; 5] = [
    "picture-home.look.normal",
    "picture-home.look.inverted",
    "picture-home.look.light",
    "picture-home.look.dark",
    "picture-home.look.clear",
];
/// The status items on the bar's left, centre and right.
pub(crate) const BAR_LEFT: Setting<Text> = Setting::new("picture-home.bar.left-items");
pub(crate) const BAR_CENTER: Setting<Text> = Setting::new("picture-home.bar.center-items");
pub(crate) const BAR_RIGHT: Setting<Text> = Setting::new("picture-home.bar.right-items");

// ---- The grid ---------------------------------------------------------------

pub(crate) const COLUMNS: Setting<i64> = Setting::new("picture-home.columns");
pub(crate) const ROWS: Setting<i64> = Setting::new("picture-home.rows");
pub(crate) const DOTS: Setting<bool> = Setting::new("picture-home.dots");
pub(crate) const PENCIL: Setting<bool> = Setting::new("picture-home.pencil");
/// Whether the focus shows at once where there is no touch, or only with
/// the first press (the page seen as it is first).
pub(crate) const FOCUS_AT_ONCE: Setting<bool> = Setting::new("picture-home.focus-at-once");

/// What the pages show at first: the clock and the book being read, then
/// the tools.
pub(crate) const DEFAULTS: [&str; MAX_PAGES] = [
    "home.clock, home.continue, home.recent",
    "home.tools",
    "",
    "",
];

/// The setting of page `index` (0 the first).
pub(crate) fn page_setting(index: usize) -> Id {
    Id::parse(&format!("picture-home.page-{}", index + 1))
        .unwrap_or(Id::new_static("picture-home.page-1"))
}

pub(crate) const PAGE_SETTINGS: &[Id] = &[
    Id::new_static("picture-home.page-1"),
    Id::new_static("picture-home.page-2"),
    Id::new_static("picture-home.page-3"),
    Id::new_static("picture-home.page-4"),
];

static IN_PLUGIN: SettingExtra = SettingExtra::new();
/// A page's items, edited on the home screen itself.
static PAGE: SettingExtra = SettingExtra::new().editor(SCREEN);
/// The picture is chosen from the folder's pictures.
static PICKED: SettingExtra = SettingExtra::new().editor(PICK_SCREEN);
static FOLDER_EXTRA: SettingExtra =
    SettingExtra::new().editor_slot(paperpaw_sdk::builtin::slots::CHOOSE_FOLDER);
/// The status items, each part chosen on its picker.
static ITEMS: SettingExtra =
    SettingExtra::new().editor(paperpaw_sdk::builtin::screens::STATUS_ITEMS);
/// Only where the bar is compact.
static COMPACT_ONLY: SettingExtra =
    SettingExtra::new().shown_if(Id::new_static(BAR_SHAPE.static_str()), BAR_SHAPES[1]);

const fn page(id: &'static str, title: &'static str, default: &'static str) -> SettingSpec {
    SettingSpec::text(id, title, default, GROUP)
        .with_extra(&PAGE)
        .description("picture-home.setting.page.help")
        .advanced()
}

const fn items(id: &'static str, title: &'static str, default: &'static str) -> SettingSpec {
    SettingSpec::new(
        Id::new_static(id),
        title,
        paperpaw_sdk::SettingKind::List,
        paperpaw_sdk::SettingValue::Text(Cow::Borrowed(default)),
        GROUP,
    )
    .with_extra(&ITEMS)
    .description("picture-home.setting.items.help")
}

/// The settings, in the order they are listed.
pub(crate) static SETTINGS: [SettingSpec; 27] = [
    SettingSpec::text(
        FOLDER.static_str(),
        "picture-home.setting.folder",
        "wallpapers",
        GROUP,
    )
    .with_extra(&FOLDER_EXTRA)
    .description("picture-home.setting.folder.help"),
    SettingSpec::text(
        PICTURE.static_str(),
        "picture-home.setting.picture",
        "",
        GROUP,
    )
    .with_extra(&PICKED)
    .description("picture-home.setting.picture.help"),
    SettingSpec::choice(
        FIT.static_str(),
        "picture-home.setting.fit",
        &FITS,
        FITS[0],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.fit.help"),
    SettingSpec::choice(
        ANCHOR.static_str(),
        "picture-home.setting.anchor",
        &ANCHORS,
        ANCHORS[4],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.anchor.help"),
    SettingSpec::number(
        ZOOM.static_str(),
        "picture-home.setting.zoom",
        100,
        300,
        100,
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.zoom.help"),
    SettingSpec::number(
        LIGHTEN.static_str(),
        "picture-home.setting.lighten",
        0,
        60,
        15,
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.lighten.help"),
    SettingSpec::choice(
        BRIGHTNESS.static_str(),
        "picture-home.setting.brightness",
        &BRIGHTNESSES,
        BRIGHTNESSES[2],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.brightness.help"),
    SettingSpec::choice(
        CONTRAST.static_str(),
        "picture-home.setting.contrast",
        &CONTRASTS,
        CONTRASTS[1],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.contrast.help"),
    SettingSpec::choice(
        RENDER.static_str(),
        "picture-home.setting.render",
        &RENDERS,
        RENDERS[0],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.render.help"),
    SettingSpec::choice(
        BAR.static_str(),
        "picture-home.setting.bar",
        &BARS,
        BARS[2],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.bar.help"),
    SettingSpec::choice(
        BAR_AT.static_str(),
        "picture-home.setting.bar-at",
        &BAR_ATS,
        BAR_ATS[0],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.bar-at.help"),
    SettingSpec::choice(
        BAR_SHAPE.static_str(),
        "picture-home.setting.bar-shape",
        &BAR_SHAPES,
        BAR_SHAPES[0],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.bar-shape.help"),
    SettingSpec::choice(
        BAR_ALIGN.static_str(),
        "picture-home.setting.bar-align",
        &BAR_ALIGNS,
        BAR_ALIGNS[2],
        GROUP,
    )
    .with_extra(&COMPACT_ONLY)
    .description("picture-home.setting.bar-align.help"),
    SettingSpec::choice(
        BAR_LOOK.static_str(),
        "picture-home.setting.bar-look",
        &BAR_LOOKS,
        BAR_LOOKS[0],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.bar-look.help"),
    SettingSpec::choice(
        HINTS.static_str(),
        "picture-home.setting.hints",
        &BARS,
        BARS[2],
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.hints.help")
    .advanced(),
    items(
        BAR_LEFT.static_str(),
        "picture-home.setting.bar-left",
        "core.status.clock",
    ),
    items(
        BAR_CENTER.static_str(),
        "picture-home.setting.bar-center",
        "",
    ),
    items(
        BAR_RIGHT.static_str(),
        "picture-home.setting.bar-right",
        "lock.touch-status, wifi.status, power.status, core.status.battery",
    ),
    SettingSpec::number(
        COLUMNS.static_str(),
        "picture-home.setting.columns",
        2,
        grid::MAX_COLUMNS as i64,
        3,
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.columns.help"),
    SettingSpec::number(
        ROWS.static_str(),
        "picture-home.setting.rows",
        0,
        grid::MAX_ROWS as i64,
        0,
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.rows.help"),
    SettingSpec::switch(DOTS.static_str(), "picture-home.setting.dots", true, GROUP)
        .with_extra(&IN_PLUGIN)
        .description("picture-home.setting.dots.help"),
    SettingSpec::switch(
        PENCIL.static_str(),
        "picture-home.setting.pencil",
        true,
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.pencil.help"),
    SettingSpec::switch(
        FOCUS_AT_ONCE.static_str(),
        "picture-home.setting.focus-at-once",
        false,
        GROUP,
    )
    .with_extra(&IN_PLUGIN)
    .description("picture-home.setting.focus-at-once.help"),
    page(
        "picture-home.page-1",
        "picture-home.setting.page-1",
        DEFAULTS[0],
    ),
    page(
        "picture-home.page-2",
        "picture-home.setting.page-2",
        DEFAULTS[1],
    ),
    page(
        "picture-home.page-3",
        "picture-home.setting.page-3",
        DEFAULTS[2],
    ),
    page(
        "picture-home.page-4",
        "picture-home.setting.page-4",
        DEFAULTS[3],
    ),
];

/// The picture's path on the card, if one is chosen.
pub(crate) fn picture_path(env: &Env<'_>) -> Option<alloc::string::String> {
    let name = env.settings().text(PICTURE.as_str());
    if name.is_empty() {
        return None;
    }
    let folder = env.settings().text(FOLDER.as_str());
    Some(paperpaw_sdk::files::join(folder.trim_matches('/'), name))
}

/// The pages' items as they are kept: the first always, the others where
/// they hold something.
pub(crate) fn pages(env: &Env<'_>) -> Vec<(usize, Vec<grid::Item>)> {
    let mut pages = Vec::new();
    for slot in 0..MAX_PAGES {
        let items = grid::parse(env.settings().text(page_setting(slot).as_str()));
        if slot == 0 || !items.is_empty() {
            pages.push((slot, items));
        }
    }
    pages
}
