//! What the home screen declares of itself (its pages, grid and footer
//! settings, screens, slot, actions and menu entries), compared with the
//! interface-6 registration by the declaration tests. The built-in widgets
//! and the help hints follow (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl,
    KeyBindingDecl, Kind, MenuEntryDecl, ScreenDecl, SlotOfferDecl, SpecDecl, WidgetLayout,
    WidgetPlaceDecl,
};

/// Callback tokens: stable within this plugin.
pub const DASHBOARD: u32 = 10;
pub const ADD: u32 = 11;
pub const DRAWER: u32 = 12;
pub const ALL_TOOLS: u32 = 1;
pub const EDIT_PAGES: u32 = 2;
pub const SETTING_CHANGED: u32 = 3;

/// Pages at most.
pub const MAX_PAGES: usize = 8;
const GROUP: &str = "home.group";

/// What the pages show at first: the clock, the book being read and the
/// recent ones, then the tools.
pub const DEFAULTS: [&str; MAX_PAGES] = [
    "home.clock, home.continue, home.recent",
    "home.tools",
    "",
    "",
    "",
    "",
    "",
    "",
];

const PAGE_NAMES: [&str; MAX_PAGES] = [
    "page-1", "page-2", "page-3", "page-4", "page-5", "page-6", "page-7", "page-8",
];
const PAGE_WIDE_NAMES: [&str; MAX_PAGES] = [
    "page-1.wide",
    "page-2.wide",
    "page-3.wide",
    "page-4.wide",
    "page-5.wide",
    "page-6.wide",
    "page-7.wide",
    "page-8.wide",
];
const PAGE_TITLES: [&str; MAX_PAGES] = [
    "home.setting.page-1",
    "home.setting.page-2",
    "home.setting.page-3",
    "home.setting.page-4",
    "home.setting.page-5",
    "home.setting.page-6",
    "home.setting.page-7",
    "home.setting.page-8",
];
const PAGE_WIDE_TITLES: [&str; MAX_PAGES] = [
    "home.setting.page-1-wide",
    "home.setting.page-2-wide",
    "home.setting.page-3-wide",
    "home.setting.page-4-wide",
    "home.setting.page-5-wide",
    "home.setting.page-6-wide",
    "home.setting.page-7-wide",
    "home.setting.page-8-wide",
];

/// The group (Settings, Customise, Home screen), the pages, the grid and
/// what shows above and below it.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "home.section");
    group.order = 220;
    d.group(group)?;
    for n in 0..MAX_PAGES {
        let mut page = SpecDecl::new(
            PAGE_NAMES[n],
            Kind::Text(DEFAULTS[n]),
            GROUP,
            PAGE_TITLES[n],
        );
        page.help = "home.setting.page.help";
        page.advanced = true;
        d.spec(page)?;
        let mut extra = ExtraDecl::new(PAGE_NAMES[n]);
        extra.editor = Some(EditorDecl::Screen("dashboard"));
        d.extra(extra)?;
    }
    for n in 0..MAX_PAGES {
        let mut page = SpecDecl::new(
            PAGE_WIDE_NAMES[n],
            Kind::Text(""),
            GROUP,
            PAGE_WIDE_TITLES[n],
        );
        page.help = "home.setting.page-wide.help";
        page.advanced = true;
        d.spec(page)?;
        let mut extra = ExtraDecl::new(PAGE_WIDE_NAMES[n]);
        extra.editor = Some(EditorDecl::Screen("dashboard"));
        d.extra(extra)?;
    }
    let numbers = [
        (
            "columns",
            3,
            2,
            6,
            "home.setting.columns",
            "home.setting.columns.help",
        ),
        (
            "rows",
            0,
            0,
            12,
            "home.setting.rows",
            "home.setting.rows.help",
        ),
    ];
    for (name, default, min, max, title, help) in numbers {
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
        spec.help = help;
        d.spec(spec)?;
    }
    let switches = [
        ("tool-labels", true, "home.setting.tool-labels"),
        ("focus-at-once", false, "home.setting.focus-at-once"),
        ("status-bar", true, "home.setting.status-bar"),
        ("footer.pencil", true, "home.setting.footer-pencil"),
        ("footer.dots", true, "home.setting.footer-dots"),
    ];
    for (name, default, title) in switches {
        let mut spec = SpecDecl::new(name, Kind::Bool(default), GROUP, title);
        spec.help = match name {
            "tool-labels" => "home.setting.tool-labels.help",
            "focus-at-once" => "home.setting.focus-at-once.help",
            "status-bar" => "home.setting.status-bar.help",
            "footer.pencil" => "home.setting.footer-pencil.help",
            _ => "home.setting.footer-dots.help",
        };
        d.spec(spec)?;
    }
    // Its picker is the status items' screen, which belongs to the host: no
    // editor can be named for it yet (an ignored acceptance case).
    let mut items = SpecDecl::new(
        "footer.items",
        Kind::List(""),
        GROUP,
        "home.setting.footer-items",
    );
    items.help = "home.setting.footer-items.help";
    d.spec(items)?;
    d.submit()
}

/// The dashboard, adding to a page, and every tool at once.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("dashboard", "home.title", DASHBOARD))?;
    d.screen(ScreenDecl::new("add-widget", "home.edit.add", ADD))?;
    d.screen(ScreenDecl::new("drawer", "home.drawer", DRAWER))?;
    d.submit()
}

/// The slot, the actions, their icons and menu places, the pages as a place
/// of widgets, and the swipe up that shows every tool.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new("core.home", "home.dashboard"))?;
    d.action(ActionDecl::new("all-tools", "home.drawer", ALL_TOOLS))?;
    d.action(ActionDecl::new("edit-pages", "home.edit-pages", EDIT_PAGES))?;
    d.icon("home.group", "home")?;
    d.icon("home.all-tools", "apps")?;
    d.icon("home.edit-pages", "pencil")?;
    let mut edit = MenuEntryDecl::new("core.main", "home.edit-pages");
    edit.order = 40;
    d.menu_entry(edit)?;
    let mut library = MenuEntryDecl::new("home.actions", "core.library");
    library.order = 10;
    d.menu_entry(library)?;
    d.key_binding(KeyBindingDecl::new(
        "core.home",
        "swipe.up",
        "home.all-tools",
    ))?;
    let pages = "home.page-1\nhome.page-2\nhome.page-3\nhome.page-4\nhome.page-5\nhome.page-6\nhome.page-7\nhome.page-8";
    let place = WidgetPlaceDecl::new("pages", "home.title", pages, WidgetLayout::Grid)
        .map_err(|_| DeclareError::Empty("widget place"))?;
    d.widget_place(place)?;
    d.menu_need("home.focus-at-once", paperpaw_sdk::MenuNeed::NoTouch)?;
    d.submit()
}

/// The grid's columns or rows changed: the pages are fitted to it.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.submit()
}
