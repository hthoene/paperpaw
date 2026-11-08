//! What the reader declares (slice 1): its settings, the screens and the
//! slots they fill, the actions with their menu entries, icons, device needs
//! and key bindings, and the events it hears. Compared with the interface-6
//! registration by `tests/abi7_declarations.rs`. What has no native
//! declaration yet (status places, the sample widget, the reading app, the
//! book data folder, screen hints, help, the setup step, the reader's stages)
//! is listed in `PORTING-7.md`.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, Instances, KeyBindingDecl, Kind,
    MenuEntryDecl, ScreenDecl, SlotOfferDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin. Screens from 10, actions from
/// 100, events from 3.
pub const SCREEN: u32 = 10;
pub const TOC: u32 = 11;
pub const ANNOTATIONS: u32 = 12;
pub const SKIM: u32 = 13;
pub const FOOTNOTE: u32 = 14;
pub const SEARCH: u32 = 15;
pub const STARTED: u32 = 3;
pub const BOOK_MOVED: u32 = 4;
pub const FIRST_ACTION: u32 = 100;

const BEHAVIOUR: &str = "core.group.reading-behaviour";
const CONTROLS: &str = "core.group.controls";
const ALOUD: &str = "core.group.reading-aloud";

/// Choices of the reading-aloud timer: minutes, off for none.
pub const TIMER_CHOICES: [&str; 5] = [
    "reader.aloud-timer.off",
    "reader.aloud-timer.15",
    "reader.aloud-timer.30",
    "reader.aloud-timer.60",
    "reader.aloud-timer.90",
];

/// The reader's own settings (the type of pages is the kernel's).
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, default, group, advanced) in [
        ("open-last", true, BEHAVIOUR, true),
        ("corner-bookmark", true, CONTROLS, true),
        ("hold-chapters", false, CONTROLS, true),
    ] {
        let title = ["reader.setting.", name].concat();
        let help = [title.as_str(), ".help"].concat();
        let mut spec = SpecDecl::new(name, Kind::Bool(default), group, &title);
        spec.help = &help;
        spec.advanced = advanced;
        d.spec(spec)?;
    }
    let mut turn = SpecDecl::new(
        "auto-turn-seconds",
        Kind::Int {
            default: 30,
            min: 5,
            max: 600,
        },
        BEHAVIOUR,
        "reader.setting.auto-turn-seconds",
    );
    turn.help = "reader.setting.auto-turn-seconds.help";
    turn.advanced = true;
    d.spec(turn)?;
    let mut extra = ExtraDecl::new("auto-turn-seconds");
    extra.unit = "core.unit.seconds";
    extra.step = 5;
    d.extra(extra)?;
    let mut speed = SpecDecl::new(
        "aloud-speed",
        Kind::Int {
            default: 100,
            min: 50,
            max: 300,
        },
        ALOUD,
        "reader.setting.aloud-speed",
    );
    speed.help = "reader.setting.aloud-speed.help";
    d.spec(speed)?;
    let mut extra = ExtraDecl::new("aloud-speed");
    extra.unit = "core.unit.percent";
    extra.step = 10;
    extra.slider = true;
    d.extra(extra)?;
    for (name, title, help) in [
        (
            "aloud-highlight",
            "reader.setting.aloud-highlight",
            "reader.setting.aloud-highlight.help",
        ),
        (
            "aloud-turn-pages",
            "reader.setting.aloud-turn-pages",
            "reader.setting.aloud-turn-pages.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Bool(true), ALOUD, title);
        spec.help = help;
        d.spec(spec)?;
    }
    let mut timer = SpecDecl::new(
        "aloud-timer",
        Kind::Choice {
            default: TIMER_CHOICES[0],
            choices: &TIMER_CHOICES,
        },
        ALOUD,
        "reader.setting.aloud-timer",
    );
    timer.help = "reader.setting.aloud-timer.help";
    d.spec(timer)?;
    d.submit()
}

/// The screens, and the slots they fill (any other plugin can fill them).
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, token) in [
        ("screen", "reader.title", SCREEN),
        ("toc", "reader.toc", TOC),
        ("annotations", "reader.annotations", ANNOTATIONS),
        ("skim-screen", "reader.skim", SKIM),
        ("footnote", "reader.title", FOOTNOTE),
        ("search-results", "reader.search", SEARCH),
    ] {
        // One instance for each set of parameters (the book, the section), as in
        // interface 6.
        d.screen(ScreenDecl::new(name, title, token).instances(Instances::PerParams))?;
    }
    d.submit()
}

/// The actions: name, title, and whether the reader handles it in a callback
/// of its own (all do; the token is `FIRST_ACTION` + the position).
pub const ACTIONS: [(&str, &str); 29] = [
    ("show-toc", "reader.toc"),
    ("search", "reader.search"),
    ("go-to-page", "reader.go-to-page"),
    ("go-to-percent", "reader.go-to-percent"),
    ("read-aloud", "reader.read-aloud"),
    ("select-text", "reader.select"),
    ("links", "reader.links"),
    ("next-status-line", "reader.next-status-line"),
    ("next-top-status-line", "reader.next-top-status-line"),
    ("toggle-bookmark", "reader.toggle-bookmark"),
    ("show-annotations", "reader.annotations"),
    ("selection.look-up", "reader.select.look-up"),
    ("selection.highlight", "reader.select.highlight"),
    ("selection.copy", "reader.select.copy"),
    ("selection.translate", "reader.select.translate"),
    ("selection.read-aloud", "reader.select.read-aloud"),
    ("selection.note", "reader.select.note"),
    ("book-info", "reader.book-info"),
    ("auto-turn", "reader.auto-turn"),
    ("skim-book", "reader.skim"),
    ("next-chapter", "reader.next-chapter"),
    ("prev-chapter", "reader.prev-chapter"),
    ("selection.extend", "reader.select.extend"),
    ("selection.cancel", "reader.select.cancel"),
    ("selection.more", "reader.select.more"),
    ("font-larger", "reader.action.font-larger"),
    ("font-smaller", "reader.action.font-smaller"),
    ("toggle-justify", "reader.action.toggle-justify"),
    ("quick-settings", "reader.layout"),
];

/// The menus the reader puts its entries in.
const SELECTION: &str = "core.selection-menu";
const MENU: &str = "core.reader-menu";
const MORE: &str = "core.reader-more";
const PANEL: &str = "core.reader-panel";

/// Menu, item, order.
const ENTRIES: [(&str, &str, i16); 34] = [
    (SELECTION, "reader.selection.look-up", 10),
    (SELECTION, "reader.selection.highlight", 20),
    (SELECTION, "reader.selection.note", 22),
    (SELECTION, "reader.selection.read-aloud", 25),
    (SELECTION, "reader.selection.copy", 30),
    (SELECTION, "reader.selection.translate", 35),
    (SELECTION, "reader.selection.extend", 80),
    (SELECTION, "reader.selection.cancel", 90),
    (MENU, "reader.quick-settings", 5),
    (MENU, "reader.show-toc", 10),
    (MENU, "reader.search", 20),
    (MENU, "reader.read-aloud", 30),
    (MENU, "reader.show-annotations", 40),
    (MENU, "core.night-mode", 60),
    (MENU, "reader.toggle-bookmark", 65),
    (MENU, MORE, 70),
    (MENU, "core.home", 80),
    (PANEL, "core.scope-only", 1),
    (PANEL, "core.reading-font", 5),
    (PANEL, "core.font-size", 10),
    (PANEL, "core.font-weight", 15),
    (PANEL, "core.line-spacing", 20),
    (PANEL, "core.margin", 30),
    (PANEL, "core.justify", 40),
    (PANEL, "core.hyphenate", 50),
    (PANEL, "core.publisher-styles", 60),
    (MORE, "reader.skim-book", 15),
    (MORE, "reader.go-to-page", 16),
    (MORE, "reader.go-to-percent", 17),
    (MORE, "reader.auto-turn", 17),
    (MORE, "reader.select-text", 20),
    (MORE, "reader.links", 25),
    (MORE, "reader.next-status-line", 30),
    (MORE, "core.settings", 50),
];

const ICONS: [(&str, &str); 13] = [
    ("reader.show-toc", "list"),
    ("reader.links", "link"),
    ("reader.search", "search"),
    ("reader.go-to-page", "hash"),
    ("reader.go-to-percent", "arrow-bar-to-right"),
    ("reader.show-annotations", "bookmarks"),
    ("reader.quick-settings", "typography"),
    ("reader.toggle-bookmark", "bookmark"),
    ("reader.select-text", "cursor-text"),
    ("reader.read-aloud", "volume"),
    ("reader.auto-turn", "clock-play"),
    ("reader.skim-book", "arrows-horizontal"),
    ("reader.next-status-line", "layout-bottombar"),
];

/// Key bindings on the reader's slot layer: a tap at the bottom opens font
/// and layout, swipes in from the sides turn pages, a pinch resizes the text,
/// holding Select marks the page. Input, action.
pub const KEYS: [(&str, &str); 6] = [
    ("tap.bottom", "reader.quick-settings"),
    ("edge.left", "core.prev-page"),
    ("edge.right", "core.next-page"),
    ("pinch.out", "reader.font-larger"),
    ("pinch.in", "reader.font-smaller"),
    ("long.confirm", "reader.toggle-bookmark"),
];

/// Holding a page button goes a chapter on or back, while the switch is on.
pub const HOLD_KEYS: [(&str, &str); 2] = [
    ("long.page-forward", "reader.next-chapter"),
    ("long.page-back", "reader.prev-chapter"),
];

/// The switch that turns [`HOLD_KEYS`] on.
pub const HOLD_SWITCH: &str = "reader.hold-chapters";

/// Items for which the device needs reading aloud (speech), no touch screen,
/// or a touch screen.
const SPEECH: [&str; 6] = [
    "reader.read-aloud",
    "reader.selection.read-aloud",
    "reader.aloud-speed",
    "reader.aloud-highlight",
    "reader.aloud-turn-pages",
    "reader.aloud-timer",
];
const NO_TOUCH: [&str; 3] = [
    "reader.select-text",
    "reader.links",
    "reader.next-status-line",
];

/// The actions, their menu entries, icons, needs, key bindings and the slots.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (slot, screen) in [
        ("core.reader", "reader.screen"),
        ("core.toc", "reader.toc"),
        ("core.annotations", "reader.annotations"),
        ("core.footnote", "reader.footnote"),
        ("core.search", "reader.search-results"),
        ("core.skim", "reader.skim-screen"),
    ] {
        d.slot_offer(SlotOfferDecl::new(slot, screen))?;
    }
    for (token, (name, title)) in (FIRST_ACTION..).zip(ACTIONS) {
        d.action(ActionDecl::new(name, title, token))?;
    }
    for (menu, item, order) in ENTRIES {
        let mut entry = MenuEntryDecl::new(menu, item);
        entry.order = order;
        d.menu_entry(entry)?;
    }
    for (item, icon) in ICONS {
        d.icon(item, icon)?;
    }
    for item in SPEECH {
        d.menu_need(item, MenuNeed::Speech)?;
    }
    for item in NO_TOUCH {
        d.menu_need(item, MenuNeed::NoTouch)?;
    }
    d.menu_need("reader.corner-bookmark", MenuNeed::Touch)?;
    d.submit()
}

/// The key bindings.
pub fn keys<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (input, action) in KEYS {
        d.key_binding(KeyBindingDecl::new("core.reader", input, action))?;
    }
    d.submit()
}

/// The key bindings that hold while [`HOLD_SWITCH`] is on.
pub fn hold_keys<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (input, action) in HOLD_KEYS {
        let mut binding = KeyBindingDecl::new("core.reader", input, action);
        binding.when_key = HOLD_SWITCH;
        binding.when_value = "true";
        d.key_binding(binding)?;
    }
    d.submit()
}

/// The start (reopen the last book) and a book moved (the last book follows).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::BookMoved, BOOK_MOVED)?;
    d.submit()
}
