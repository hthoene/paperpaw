//! What the reader declares (slice 1): its settings, the screens and the
//! slots they fill, the actions with their menu entries, icons, device needs
//! and key bindings, and the events it hears. Compared with the interface-6
//! registration by `tests/abi7_declarations.rs`. What has no native
//! declaration yet (status places, the sample widget, the
//! book data folder, screen hints, help, the setup step, the reader's stages)
//! is listed in `PORTING-7.md`.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, Instances, KeyBindingDecl, Kind,
    MenuEntryDecl, ReaderDecl, ScreenDecl, SlotOfferDecl, SpecDecl, StageDecl, StatusItemDecl,
    StatusLineDecl, StatusPlaceDecl,
};

/// Callback tokens: stable within this plugin. Screens from 10, actions from
/// 100, events from 3.
pub const SCREEN: u32 = 10;
pub const TOC: u32 = 11;
pub const ANNOTATIONS: u32 = 12;
pub const SKIM: u32 = 13;
pub const FOOTNOTE: u32 = 14;
pub const SEARCH: u32 = 15;
/// The status items and lines of the reader (the reader draws its own itself:
/// the host never calls its caller back for them).
pub const STATUS_ITEM: u32 = 16;
pub const STATUS_LINE: u32 = 17;
pub const STARTED: u32 = 3;
pub const BOOK_MOVED: u32 = 4;
/// The device goes to sleep, stops, or the quiet time after a page turn has
/// gone: the place of the page on show is written (`native_place`).
pub const SUSPEND: u32 = 5;
pub const STOPPING: u32 = 6;
pub const WORK_DUE: u32 = 7;
/// A setting changed: a page laid out with the reading settings is laid out again
/// (`native_pages::affects_layout`), one drawn with the text look drawn again.
pub const SETTING_CHANGED: u32 = 8;
/// A screen opened, closed or came to the top: reading aloud ends when the
/// reading screen is no longer the one on top (`native_view/aloud.rs`).
pub const STACK_CHANGED: u32 = 17;
/// The host's answer to a sentence handed over (`event.spoken`, 89): it goes
/// only to plugins that subscribed (`docs/abi/ui.md` 7), so without this
/// reading aloud stops after one sentence.
pub const SPOKEN: u32 = 16;
/// The token of the search's background scan (`core.work`, key `search`): a step
/// of the book is read each time. Not a subscription: the work's own token comes
/// back in `event.work_due`, which the reader hears as `WORK_DUE`.
#[allow(
    dead_code,
    reason = "the plugin's code uses it; the build script that includes this file does not"
)]
pub const SEARCH_WORK: u32 = 9;
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
    status_settings(d)
}

/// The settings group of the status lines (the kernel's).
const STATUS_GROUP: &str = "core.group.status";
/// The sections of the status settings: the line above the text and the one below.
const SECTION_TOP: &str = "reader.status.section.top";
const SECTION_BOTTOM: &str = "reader.status.section.bottom";

/// The default of each part of the line of items, as interface 6 had them:
/// above nothing; below the chapter on the left, the time in the middle, the
/// page and the device's symbols on the right.
const PARTS: [(&str, &str, &str); 6] = [
    ("status.top-left", "reader.status.top-left", ""),
    ("status.top-center", "reader.status.top-center", ""),
    ("status.top-right", "reader.status.top-right", ""),
    (
        "status.bottom-left",
        "reader.status.bottom-left",
        "reader.status.aloud, reader.status.chapter",
    ),
    (
        "status.bottom-center",
        "reader.status.bottom-center",
        "core.status.clock",
    ),
    (
        "status.bottom-right",
        "reader.status.bottom-right",
        "lock.touch-status, wifi.status, power.status, core.status.battery, reader.status.page",
    ),
];

/// The status lines' settings (interface 6's `status.rs`): the text size and the
/// distance from the edge, the lines to switch between above and below the text,
/// what each part of the line of items shows, and the battery's percent.
pub fn status_settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let number = |name, (default, min, max), title, help, unit, step| {
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, STATUS_GROUP, title);
        spec.help = help;
        (spec, unit, step)
    };
    let (size, unit, step) = number(
        "status.size",
        (6, 5, 16),
        "reader.status.size",
        "reader.status.size.help",
        "core.unit.pt",
        1,
    );
    d.spec(size)?;
    let mut extra = ExtraDecl::new("status.size");
    extra.unit = unit;
    extra.step = step;
    d.extra(extra)?;
    let (mut edge, unit, step) = number(
        "status.edge",
        (15, 0, 150),
        "reader.status.edge",
        "reader.status.edge.help",
        "core.unit.tenth-mm",
        5,
    );
    edge.advanced = true;
    d.spec(edge)?;
    let mut extra = ExtraDecl::new("status.edge");
    extra.unit = unit;
    extra.step = step;
    d.extra(extra)?;
    for (name, title, default, section) in [
        (
            "status.top-lines",
            "reader.status.top-lines",
            "",
            SECTION_TOP,
        ),
        (
            "status.bottom-lines",
            "reader.status.bottom-lines",
            "core.status-line.items, reader.status-line.progress, reader.status-line.empty",
            SECTION_BOTTOM,
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::List(default), STATUS_GROUP, title);
        spec.help = "reader.status.lines.help";
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.section = section;
        d.extra(extra)?;
    }
    for (at, (name, title, default)) in PARTS.into_iter().enumerate() {
        let mut spec = SpecDecl::new(name, Kind::List(default), STATUS_GROUP, title);
        spec.help = "reader.status.part.help";
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.section = if at < 3 { SECTION_TOP } else { SECTION_BOTTOM };
        d.extra(extra)?;
    }
    let mut percent = SpecDecl::new(
        "status.battery-percent",
        Kind::Bool(true),
        STATUS_GROUP,
        "reader.status.battery-percent",
    );
    percent.help = "reader.status.battery-percent.help";
    d.spec(percent)?;
    d.submit()
}

/// The status places (the lines above and below the text, each listing the items
/// of its three parts), the reader's items and its two lines. The reader draws
/// its own items and lines itself; they are declared so that the lists offer
/// them (and other plugins' lines can show the reader's items by their id).
pub fn status<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let refused = |what| move |_| DeclareError::Empty(what);
    for (name, title, parts) in [
        (
            "status.top",
            "reader.status.section.top",
            ["status.top-left", "status.top-center", "status.top-right"],
        ),
        (
            "status.bottom",
            "reader.status.section.bottom",
            [
                "status.bottom-left",
                "status.bottom-center",
                "status.bottom-right",
            ],
        ),
    ] {
        let [left, center, right] = parts.map(|part| ["reader.", part].concat());
        let place = StatusPlaceDecl::new(name, title, &left, &center, &right)
            .map_err(refused("status place"))?;
        d.status_place(place)?;
    }
    // The book's and the chapter's titles give way first where a line has no
    // room, the page last.
    for (name, title, priority) in [
        ("status.title", "reader.status.item.title", 64),
        ("status.chapter", "reader.status.item.chapter", 64),
        ("status.page", "reader.status.item.page", 224),
        ("status.progress", "reader.status.item.progress", 128),
    ] {
        let item = StatusItemDecl::new(name, title, STATUS_ITEM)
            .map_err(refused("status item"))?
            .priority(priority);
        d.status_item(item)?;
    }
    for (name, title) in [
        ("status-line.progress", "reader.status.line.progress"),
        ("status-line.empty", "reader.status.line.empty"),
    ] {
        // A line is as high as 2.1 times the text size.
        let line =
            StatusLineDecl::new(name, title, 2100, STATUS_LINE).map_err(refused("status line"))?;
        d.status_line(line)?;
    }
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
        let decl = ScreenDecl::new(name, title, token).instances(Instances::PerParams);
        // The note and the skim bar are laid over the page, which shows above them.
        d.screen(if matches!(token, FOOTNOTE | SKIM) {
            decl.overlay()
        } else {
            decl
        })?;
    }
    // The reading app `reader.app` (interface 6 `ReaderSpec`): its screen shows
    // books of every format. The host counts a book's reading session only for
    // a reader it knows: without this, no progress, "Continue reading" or book
    // event.
    d.reader(ReaderDecl::new("app", "reader.screen"))?;
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

/// The start (reopen the last book), a book moved (the last book follows), and
/// what saves the place: sleep, stop, and the quiet time (`core.work`).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::BookMoved, BOOK_MOVED)?;
    d.subscribe_event(EventKind::Suspend, SUSPEND)?;
    d.subscribe_event(EventKind::Stopping, STOPPING)?;
    d.subscribe_event(EventKind::WorkDue, WORK_DUE)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.subscribe_event(EventKind::StackChanged, STACK_CHANGED)?;
    d.subscribe_event(EventKind::Spoken, SPOKEN)?;
    d.submit()
}

/// The bounds of `reader.notes-exported`, as `stages/reader.notes-exported@1.def`
/// says (a test holds them to the generated codec).
pub const NOTES_SCHEMA: u32 = 1;
pub const NOTES_MAX_INPUT: u32 = 800;
pub const NOTES_MAX_REPLY: u32 = 16;
pub const NOTES_MAX_RECORDS: u32 = 8;

/// The bounds of `reader.word-looked-up`, as `stages/reader.word-looked-up@1.def` says.
pub const WORDS_SCHEMA: u32 = 1;
pub const WORDS_MAX_INPUT: u32 = 2560;
pub const WORDS_MAX_REPLY: u32 = 16;
pub const WORDS_MAX_RECORDS: u32 = 8;

/// The bounds of `reader.filter`, as `stages/reader.filter@1.def` says.
pub const FILTER_SCHEMA: u32 = 1;
pub const FILTER_MAX_INPUT: u32 = 160;
pub const FILTER_MAX_REPLY: u32 = 3840;
pub const FILTER_MAX_RECORDS: u32 = 4;

/// The stage the reader runs after it wrote a book's notes as Markdown
/// (`core.notes-exported` of interface 6): a notes app or a sync subscribes
/// (`registration.stage`) and answers each run with one `notes.done` record
/// the reader ignores.
pub fn stages<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.stage(StageDecl::new(
        "notes-exported",
        NOTES_SCHEMA,
        NOTES_MAX_INPUT,
        NOTES_MAX_REPLY,
        NOTES_MAX_RECORDS,
    ))?;
    // A word looked up (`core.word-looked-up` of interface 6): a vocabulary
    // list subscribes and keeps it with its place in the book.
    d.stage(StageDecl::new(
        "word-looked-up",
        WORDS_SCHEMA,
        WORDS_MAX_INPUT,
        WORDS_MAX_REPLY,
        WORDS_MAX_RECORDS,
    ))?;
    // The text filters (aids and the like): the reader asks each subscriber
    // for its rules once per book open or change and applies them to whole
    // blocks before layout.
    d.stage(StageDecl::new(
        "filter",
        FILTER_SCHEMA,
        FILTER_MAX_INPUT,
        FILTER_MAX_REPLY,
        FILTER_MAX_RECORDS,
    ))?;
    d.submit()
}
