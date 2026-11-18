//! What the picture home screen declares of itself (its settings, screens,
//! the `core.home` offer, the pages as a place of widgets and the status bar's
//! items), compared with the interface-6 registration by the declaration
//! tests. The screens themselves follow (`PORTING-7.md`).
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Kind, ScreenDecl,
    SlotOfferDecl, SpecDecl, StatusPlaceDecl, WidgetLayout, WidgetPlaceDecl,
};

/// Callback tokens: stable within this plugin.
pub const DASHBOARD: u32 = 10;
pub const ADD: u32 = 11;
pub const PICK: u32 = 12;
pub const STATUS_ITEMS: u32 = 13;
/// The minute tick: a bar that shows after a press goes, a bar that shows is
/// drawn anew.
pub const TICK: u32 = 14;
/// A widget of the page says it shows something new (`event.widget_changed`):
/// its cell is drawn anew.
pub const WIDGET_CHANGED: u32 = 15;
/// A picture the host prepared (`event.image_ready`, which the host sends only
/// to a plugin that subscribes): the backdrop or a picture cell is drawn.
pub const IMAGE_READY: u32 = 16;

/// Pages at most.
pub const MAX_PAGES: usize = 4;
/// The priority the offer of the home screen has (a plugin that replaces a
/// default).
pub const PREFERRED: i32 = 100;

const GROUP: &str = "picture-home.group";
const SHAPE: &str = "picture-home.bar.shape";
const COMPACT: &str = "picture-home.bar.compact";

const FITS: [&str; 5] = [
    "picture-home.fit.fill",
    "picture-home.fit.fit",
    "picture-home.fit.stretch",
    "picture-home.fit.original",
    "picture-home.fit.tile",
];
const ANCHORS: [&str; 9] = [
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
const BRIGHTNESSES: [&str; 5] = [
    "picture-home.brightness.much-darker",
    "picture-home.brightness.darker",
    "picture-home.brightness.normal",
    "picture-home.brightness.lighter",
    "picture-home.brightness.much-lighter",
];
const CONTRASTS: [&str; 3] = [
    "picture-home.contrast.lower",
    "picture-home.contrast.normal",
    "picture-home.contrast.higher",
];
/// What the widgets sit on: the SDK's names for the plates.
const PLATES: [&str; 3] = [
    paperpaw_sdk::widgets::PAPER_NORMAL,
    paperpaw_sdk::widgets::PAPER_LIGHT_GRAY,
    paperpaw_sdk::widgets::PAPER_NONE,
];
const RENDERS: [&str; 3] = [
    "picture-home.render.auto",
    "picture-home.render.grays",
    "picture-home.render.dither",
];
const BARS: [&str; 3] = [
    "picture-home.bar.off",
    "picture-home.bar.auto",
    "picture-home.bar.on",
];
const BAR_ATS: [&str; 2] = ["picture-home.bar.top", "picture-home.bar.bottom"];
const BAR_SHAPES: [&str; 2] = ["picture-home.bar.full", COMPACT];
const BAR_ALIGNS: [&str; 3] = [
    "picture-home.bar.left",
    "picture-home.bar.center",
    "picture-home.bar.right",
];
const BAR_LOOKS: [&str; 5] = [
    "picture-home.look.normal",
    "picture-home.look.inverted",
    "picture-home.look.light",
    "picture-home.look.dark",
    "picture-home.look.clear",
];

/// What the pages show at first: the clock and the book being read, then the
/// tools.
pub const DEFAULTS: [&str; MAX_PAGES] = [
    "home.clock, home.continue, home.recent",
    "home.tools",
    "",
    "",
];
const PAGE_NAMES: [&str; MAX_PAGES] = ["page-1", "page-2", "page-3", "page-4"];
const PAGE_TITLES: [&str; MAX_PAGES] = [
    "picture-home.setting.page-1",
    "picture-home.setting.page-2",
    "picture-home.setting.page-3",
    "picture-home.setting.page-4",
];

/// One setting that is a choice.
struct Choice {
    name: &'static str,
    title: &'static str,
    help: &'static str,
    choices: &'static [&'static str],
    default: usize,
    advanced: bool,
}

const fn choice(
    name: &'static str,
    title: &'static str,
    help: &'static str,
    choices: &'static [&'static str],
    default: usize,
) -> Choice {
    Choice {
        name,
        title,
        help,
        choices,
        default,
        advanced: false,
    }
}

/// A choice, then the extras it has.
fn declare_choice<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
    c: &Choice,
) -> Result<(), DeclareError> {
    let default = c.choices.get(c.default).copied().unwrap_or("");
    let mut spec = SpecDecl::new(
        c.name,
        Kind::Choice {
            default,
            choices: c.choices,
        },
        GROUP,
        c.title,
    );
    spec.help = c.help;
    spec.advanced = c.advanced;
    d.spec(spec)?;
    Ok(())
}

fn number<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
    (name, default, min, max): (&str, i64, i32, i32),
    title: &str,
    help: &str,
) -> Result<(), DeclareError> {
    let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
    spec.help = help;
    d.spec(spec)?;
    Ok(())
}

fn switch<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
    (name, default): (&str, bool),
    title: &str,
    help: &str,
) -> Result<(), DeclareError> {
    let mut spec = SpecDecl::new(name, Kind::Bool(default), GROUP, title);
    spec.help = help;
    d.spec(spec)?;
    Ok(())
}

/// The group (Settings, Customise, Picture home screen) and its 28 settings,
/// in the order they are listed.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "picture-home.title");
    group.order = 221;
    d.group(group)?;
    picture(d)?;
    bars(d)?;
    grid(d)?;
    pages(d)?;
    d.submit()
}

/// The picture: the folder (any plugin's chooser), the file (its own
/// picker) and how it is set.
fn picture<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    // The picture: the folder (any plugin's chooser), the file (its own
    // picker) and how it is set.
    let mut folder = SpecDecl::new(
        "folder",
        Kind::Text("wallpapers"),
        GROUP,
        "picture-home.setting.folder",
    );
    folder.help = "picture-home.setting.folder.help";
    d.spec(folder)?;
    let mut extra = ExtraDecl::new("folder");
    extra.editor = Some(EditorDecl::Slot("core.choose-folder"));
    d.extra(extra)?;
    let mut picture = SpecDecl::new(
        "picture",
        Kind::Text(""),
        GROUP,
        "picture-home.setting.picture",
    );
    picture.help = "picture-home.setting.picture.help";
    d.spec(picture)?;
    let mut extra = ExtraDecl::new("picture");
    extra.editor = Some(EditorDecl::Screen("pick"));
    d.extra(extra)?;
    declare_choice(
        d,
        &choice(
            "fit",
            "picture-home.setting.fit",
            "picture-home.setting.fit.help",
            &FITS,
            0,
        ),
    )?;
    declare_choice(
        d,
        &choice(
            "anchor",
            "picture-home.setting.anchor",
            "picture-home.setting.anchor.help",
            &ANCHORS,
            4,
        ),
    )?;
    number(
        d,
        ("zoom", 100, 100, 300),
        "picture-home.setting.zoom",
        "picture-home.setting.zoom.help",
    )?;
    number(
        d,
        ("lighten", 15, 0, 60),
        "picture-home.setting.lighten",
        "picture-home.setting.lighten.help",
    )?;
    declare_choice(
        d,
        &choice(
            "brightness",
            "picture-home.setting.brightness",
            "picture-home.setting.brightness.help",
            &BRIGHTNESSES,
            2,
        ),
    )?;
    declare_choice(
        d,
        &choice(
            "contrast",
            "picture-home.setting.contrast",
            "picture-home.setting.contrast.help",
            &CONTRASTS,
            1,
        ),
    )?;
    declare_choice(
        d,
        &choice(
            "plate",
            "picture-home.setting.plate",
            "picture-home.setting.plate.help",
            &PLATES,
            0,
        ),
    )?;
    declare_choice(
        d,
        &choice(
            "render",
            "picture-home.setting.render",
            "picture-home.setting.render.help",
            &RENDERS,
            0,
        ),
    )?;
    Ok(())
}

/// The bar and the row of hints.
fn bars<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>) -> Result<(), DeclareError> {
    declare_choice(
        d,
        &choice(
            "bar",
            "picture-home.setting.bar",
            "picture-home.setting.bar.help",
            &BARS,
            2,
        ),
    )?;
    declare_choice(
        d,
        &choice(
            "bar.at",
            "picture-home.setting.bar-at",
            "picture-home.setting.bar-at.help",
            &BAR_ATS,
            0,
        ),
    )?;
    declare_choice(
        d,
        &choice(
            "bar.shape",
            "picture-home.setting.bar-shape",
            "picture-home.setting.bar-shape.help",
            &BAR_SHAPES,
            0,
        ),
    )?;
    declare_choice(
        d,
        &choice(
            "bar.align",
            "picture-home.setting.bar-align",
            "picture-home.setting.bar-align.help",
            &BAR_ALIGNS,
            2,
        ),
    )?;
    let mut extra = ExtraDecl::new("bar.align");
    extra.shown_if = Some((SHAPE, COMPACT));
    d.extra(extra)?;
    declare_choice(
        d,
        &choice(
            "bar.look",
            "picture-home.setting.bar-look",
            "picture-home.setting.bar-look.help",
            &BAR_LOOKS,
            0,
        ),
    )?;
    let mut hints = choice(
        "hints",
        "picture-home.setting.hints",
        "picture-home.setting.hints.help",
        &BARS,
        2,
    );
    hints.advanced = true;
    declare_choice(d, &hints)?;
    // The status items on the bar's left, centre and right. Each part is
    // picked on the plugin's own screen, built on the kit's status items
    // editor (`paperpaw_ui_kit::status_items`); the host has none.
    for (name, title, default) in [
        (
            "bar.left-items",
            "picture-home.setting.bar-left",
            "core.status.clock",
        ),
        ("bar.center-items", "picture-home.setting.bar-center", ""),
        (
            "bar.right-items",
            "picture-home.setting.bar-right",
            "lock.touch-status, wifi.status, power.status, core.status.battery",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::List(default), GROUP, title);
        spec.help = "picture-home.setting.items.help";
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen("status-items"));
        d.extra(extra)?;
    }
    Ok(())
}

/// The grid.
fn grid<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>) -> Result<(), DeclareError> {
    number(
        d,
        ("columns", 3, 2, 6),
        "picture-home.setting.columns",
        "picture-home.setting.columns.help",
    )?;
    number(
        d,
        ("rows", 0, 0, 12),
        "picture-home.setting.rows",
        "picture-home.setting.rows.help",
    )?;
    switch(
        d,
        ("dots", true),
        "picture-home.setting.dots",
        "picture-home.setting.dots.help",
    )?;
    switch(
        d,
        ("pencil", true),
        "picture-home.setting.pencil",
        "picture-home.setting.pencil.help",
    )?;
    switch(
        d,
        ("focus-at-once", false),
        "picture-home.setting.focus-at-once",
        "picture-home.setting.focus-at-once.help",
    )?;
    Ok(())
}

/// The pages, edited on the home screen itself.
fn pages<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>) -> Result<(), DeclareError> {
    for n in 0..MAX_PAGES {
        let mut page = SpecDecl::new(
            PAGE_NAMES[n],
            Kind::Text(DEFAULTS[n]),
            GROUP,
            PAGE_TITLES[n],
        );
        page.help = "picture-home.setting.page.help";
        page.advanced = true;
        d.spec(page)?;
        let mut extra = ExtraDecl::new(PAGE_NAMES[n]);
        extra.editor = Some(EditorDecl::Screen("dashboard"));
        d.extra(extra)?;
    }
    Ok(())
}

/// The dashboard, adding to a page, choosing the picture and the bar items'
/// editor.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new(
        "dashboard",
        "picture-home.title",
        DASHBOARD,
    ))?;
    d.screen(ScreenDecl::new("add", "picture-home.add", ADD))?;
    d.screen(ScreenDecl::new(
        "pick",
        "picture-home.setting.picture",
        PICK,
    ))?;
    d.screen(ScreenDecl::new(
        "status-items",
        "picture-home.setting.bar",
        STATUS_ITEMS,
    ))?;
    d.submit()
}

/// The icon, the home screen offer, the pages as a place of widgets, the
/// bar's place of status items and the need.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.icon("picture-home.group", "layout-grid")?;
    let mut offer = SlotOfferDecl::new("core.home", "picture-home.dashboard");
    offer.priority = PREFERRED;
    d.slot_offer(offer)?;
    let pages =
        "picture-home.page-1\npicture-home.page-2\npicture-home.page-3\npicture-home.page-4";
    let place = WidgetPlaceDecl::new("pages", "picture-home.title", pages, WidgetLayout::Grid)
        .map_err(|_| DeclareError::Empty("widget place"))?;
    d.widget_place(place)?;
    let bar = StatusPlaceDecl::new(
        "bar",
        "picture-home.setting.bar",
        "picture-home.bar.left-items",
        "picture-home.bar.center-items",
        "picture-home.bar.right-items",
    )
    .map_err(|_| DeclareError::Empty("status place"))?;
    d.status_place(bar)?;
    d.menu_need("picture-home.focus-at-once", MenuNeed::NoTouch)?;
    d.submit()
}

/// The minute tick, for the status bar, a widget that says it shows
/// something new (`event.widget_changed`, which the host sends only to a maker
/// that subscribes), for its cell, and a picture the host prepared
/// (`event.image_ready`, likewise only to a subscriber), for the backdrop.
///
/// # Errors
/// The declaration is refused.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Tick, TICK)?;
    d.subscribe_event(EventKind::WidgetChanged, WIDGET_CHANGED)?;
    d.subscribe_event(EventKind::ImageReady, IMAGE_READY)?;
    d.submit()
}
