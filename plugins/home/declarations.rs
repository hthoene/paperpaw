//! What the home screen declares of itself (its pages, grid and footer
//! settings, screens, slot, actions and menu entries), compared with the
//! interface-6 registration by the declaration tests, and the first-time help
//! hints of the dashboard.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl,
    HelpHintDecl, KeyBindingDecl, Kind, MenuEntryDecl, ScreenDecl, SlotOfferDecl, SpecDecl,
    StatusPlaceDecl, WidgetDecl, WidgetLayout, WidgetPlaceDecl,
};
use paperpaw_sdk::native::ui::HintInput;
use paperpaw_ui_kit::widget::options::{Opt, declare_all, shared_look};

/// Callback tokens: stable within this plugin.
pub const DASHBOARD: u32 = 10;
pub const ADD: u32 = 11;
pub const DRAWER: u32 = 12;
pub const STATUS_ITEMS: u32 = 13;
pub const ALL_TOOLS: u32 = 1;
pub const EDIT_PAGES: u32 = 2;
pub const SETTING_CHANGED: u32 = 3;
pub const TICK: u32 = 4;
/// Widget tokens: the widget's own callbacks (made, dropped, measured, drawn).
pub const DATE: u32 = 20;
pub const CLOCK: u32 = 21;
pub const CARD: u32 = 22;
pub const SHELF: u32 = 23;
pub const STATUS_WIDGET: u32 = 24;

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
    // Its picker is home's own screen, built on the kit's status items editor
    // (`paperpaw_ui_kit::status_items`): the host has no screen for it.
    let mut items = SpecDecl::new(
        "footer.items",
        Kind::List(""),
        GROUP,
        "home.setting.footer-items",
    );
    items.help = "home.setting.footer-items.help";
    d.spec(items)?;
    let mut extra = ExtraDecl::new("footer.items");
    extra.editor = Some(EditorDecl::Screen("status-items"));
    d.extra(extra)?;
    d.submit()
}

/// The dashboard, adding to a page, every tool at once, and the footer items'
/// editor.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("dashboard", "home.title", DASHBOARD))?;
    d.screen(ScreenDecl::new("add-widget", "home.edit.add", ADD))?;
    d.screen(ScreenDecl::new("drawer", "home.drawer", DRAWER))?;
    d.screen(ScreenDecl::new(
        "status-items",
        "home.setting.footer-items",
        STATUS_ITEMS,
    ))?;
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

/// The grid's columns or rows changed (the pages are fitted to it), and the
/// minute tick (the status line, the clock and what else shows the time are
/// drawn anew).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.subscribe_event(EventKind::Tick, TICK)?;
    d.submit()
}

/// A built-in widget of the home screen.
#[derive(Debug, Clone, Copy)]
pub struct Def {
    /// Relative to the plugin: `date` is `home.date`.
    pub name: &'static str,
    pub token: u32,
    pub category: &'static str,
    /// The catalog key of its name (and of its settings group).
    pub title: &'static str,
    /// Where the text sits by default; the widget's look settings (frame,
    /// padding, paper, ink, corners, line, shadow) are the shared ones and
    /// come first, then its own.
    pub align: &'static str,
    /// The frame it has at first.
    pub frame: &'static str,
    /// Whether it shows something new each minute (the time); the books are
    /// asked about at the tick too, but a change of them is not a minute's.
    pub live: bool,
    /// The widget's own settings.
    pub options: &'static [Opt],
}

impl Def {
    /// Every setting of the widget in the order interface 6 registered them:
    /// the shared look, then its own.
    #[allow(
        dead_code,
        reason = "the tests read it; the build script reads the same file and does not"
    )]
    pub fn all(&self) -> impl Iterator<Item = Opt> + '_ {
        shared_look(self.align)
            .into_iter()
            .map(|option| match option {
                Opt::Frame(_) => Opt::Frame(self.frame),
                other => other,
            })
            .chain(self.options.iter().copied())
    }
}

/// The widget categories of the host's catalogue.
const TIME: &str = "core.widgets.time";
const BOOKS: &str = "core.widgets.books";

/// The date in words, the clock with its date, the book being read and the
/// recent ones.
pub const WIDGETS: &[Def] = &[
    Def {
        name: "date",
        token: DATE,
        category: TIME,
        title: "home.widget.date",
        align: "core.align.center",
        frame: "core.frame.none",
        live: true,
        options: &[Opt::Size("core.size.medium", false)],
    },
    Def {
        name: "clock",
        token: CLOCK,
        category: TIME,
        title: "home.widget.clock",
        align: "core.align.center",
        frame: "core.frame.none",
        live: true,
        options: &[
            Opt::Size("core.size.medium", true),
            Opt::Part("date", "home.setting.clock-date", true),
        ],
    },
    Def {
        name: "continue",
        token: CARD,
        category: BOOKS,
        title: "home.widget.continue",
        align: "core.align.left",
        frame: "core.frame.line",
        live: false,
        options: &[
            Opt::Part("cover", "home.setting.card-cover", true),
            Opt::Part("progress", "home.setting.card-progress", true),
        ],
    },
    Def {
        name: "recent",
        token: SHELF,
        category: BOOKS,
        title: "home.widget.recent",
        align: "core.align.center",
        frame: "core.frame.none",
        live: false,
        options: &[
            Opt::Part("heading", "home.setting.recent-heading", true),
            Opt::Part("titles", "home.setting.recent-titles", true),
            Opt::Number(
                "count",
                "home.setting.recent-count",
                1,
                8,
                6,
                "home.setting.recent-count.help",
            ),
        ],
    },
];

/// The status line's parts, each a list of status items chosen on their
/// picker: the line shown instead (none), the time on the left, nothing in the
/// centre, and Wi-Fi and the battery on the right. Name (relative to the
/// widget), default, title, help, and whether the picker is home's items screen.
pub const STATUS_PARTS: &[(&str, &str, &str, &str, bool)] = &[
    (
        "line",
        "",
        "home.status.line",
        "home.status.line.help",
        false,
    ),
    (
        "left",
        "core.status.clock",
        "home.status.left",
        "home.status.part.help",
        true,
    ),
    (
        "center",
        "",
        "home.status.center",
        "home.status.part.help",
        true,
    ),
    (
        "right",
        "lock.touch-status, wifi.status, power.status, core.status.battery",
        "home.status.right",
        "home.status.part.help",
        true,
    ),
];

/// The look of the status line's frame (it has no alignment: its parts sit left,
/// centre and right).
pub const STATUS_LOOK: [Opt; 7] = [
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
];

/// The status line widget, its group of settings (the parts, then the look)
/// and the place that names the three settings that list its items.
fn status<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>) -> Result<(), DeclareError> {
    d.group(GroupDecl::new("status", "home.widget.status"))?;
    let spec = WidgetDecl::new(
        "status",
        "home.widget.status",
        "core.widgets.device",
        STATUS_WIDGET,
    )
    .map_err(|_| DeclareError::TooLong("widget"))?
    .live();
    d.widget(spec)?;
    for &(name, default, title, help, items) in STATUS_PARTS {
        let name = ["status.", name].concat();
        let mut part = SpecDecl::new(&name, Kind::List(default), "home.status", title);
        part.help = help;
        d.spec(part)?;
        if items {
            let mut extra = ExtraDecl::new(&name);
            extra.editor = Some(EditorDecl::Screen("status-items"));
            d.extra(extra)?;
        }
    }
    declare_all(d, "status", "home.status", STATUS_LOOK)?;
    let place = StatusPlaceDecl::new(
        "status-line",
        "home.widget.status",
        "status.left",
        "status.center",
        "status.right",
    )
    .map_err(|_| DeclareError::TooLong("status place"))?;
    d.status_place(place)?;
    d.submit()
}

/// The built-in widgets: each has a group of settings
/// of its own, named as it is.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for widget in WIDGETS {
        d.group(GroupDecl::new(widget.name, widget.title))?;
        let mut spec = WidgetDecl::new(widget.name, widget.title, widget.category, widget.token)
            .map_err(|_| DeclareError::TooLong("widget"))?;
        if widget.live {
            spec = spec.live();
        }
        d.widget(spec)?;
        let group = ["home.", widget.name].concat();
        declare_all(d, widget.name, &group, widget.all())?;
        d.submit()?;
    }
    status(d)
}

/// The first time: how the gestures go, or the buttons (the dashboard's help
/// hints, as interface 6 declared them; zones in thousandths of the screen).
///
/// # Errors
/// A declaration is refused.
pub fn help<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let refused = |_| DeclareError::Empty("help hint");
    for (text, input, zone) in [
        ("home.tips.quick", HintInput::Touch, Some((0, 0, 1000, 120))),
        ("home.tips.back", HintInput::Touch, Some((0, 400, 150, 300))),
        ("home.tips.home", HintInput::Touch, None),
        ("home.tips.tools", HintInput::Touch, None),
        ("home.tips.arrange", HintInput::Touch, None),
        ("home.tips.move", HintInput::Buttons, None),
        ("home.tips.select", HintInput::Buttons, None),
        ("home.tips.menu", HintInput::Buttons, None),
        ("home.tips.panel", HintInput::Buttons, None),
    ] {
        let mut hint = HelpHintDecl::new("home.dashboard", text, input).map_err(refused)?;
        if let Some((x, y, w, h)) = zone {
            hint = hint
                .zone(paperpaw_sdk::Rect::new(x, y, w, h))
                .map_err(refused)?;
        }
        d.help_hint(hint)?;
    }
    d.submit()
}
