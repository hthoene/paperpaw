//! What the sleep screen declares, compared with the interface-6 registration
//! by the declaration tests.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Instances, Kind,
    ScreenDecl, SlotOfferDecl, SpecDecl, WidgetLayout, WidgetPlaceDecl,
};

/// Callback tokens: stable within this plugin.
pub const SCREEN: u32 = 11;
/// The event subscription's token.
pub const EVENTS: u32 = 12;
/// The start and the plugins changed (event 70): a stored foreign mode binds the
/// slot where the reader has not bound it (`native_plugin`).
pub const STARTED: u32 = 13;
pub const PLUGINS_CHANGED: u32 = 15;
/// A setting changed: a change of `sleep.show` binds or unbinds the slot
/// (`native_plugin`); a start binds nothing.
pub const SETTING_CHANGED: u32 = 14;
/// A picture the host decoded for the screen is ready (`native_screen`).
pub const IMAGE_READY: u32 = 16;
/// The picker of the picture or the widget over the page (`native_picker`).
pub const PICK: u32 = 17;

const GROUP: &str = "sleep.group";
const SHOW: [&str; 7] = [
    "sleep.show.cat",
    "sleep.show.cover",
    "sleep.show.picture",
    "sleep.show.overlay",
    "sleep.show.widgets",
    "sleep.show.message",
    "sleep.show.page",
];
const POSITIONS: [&str; 7] = [
    "sleep.position.center",
    "sleep.position.top",
    "sleep.position.bottom",
    "sleep.position.top-left",
    "sleep.position.top-right",
    "sleep.position.bottom-left",
    "sleep.position.bottom-right",
];
const WHEN: [&str; 3] = [
    "sleep.when.always",
    "sleep.when.reading",
    "sleep.when.elsewhere",
];

/// The settings that need a device that sleeps (the group too).
const NEEDS: [&str; 10] = [
    "sleep.group",
    "sleep.show",
    "sleep.folder",
    "sleep.widgets",
    "sleep.overlay-picture",
    "sleep.overlay-widget",
    "sleep.overlay-position",
    "sleep.overlay-size",
    "sleep.overlay-see-through",
    "sleep.overlay-when",
];

/// What the sleep screen shows, the folder of its pictures, its widgets and
/// its picture over the page.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "sleep.title");
    group.order = 62;
    d.group(group)?;
    let mut show = SpecDecl::new(
        "show",
        Kind::Choice {
            default: SHOW[0],
            choices: &SHOW,
        },
        GROUP,
        "sleep.setting.show",
    );
    show.help = "sleep.setting.show.help";
    d.spec(show)?;
    let mut folder = SpecDecl::new("folder", Kind::Text("sleep"), GROUP, "sleep.setting.folder");
    folder.help = "sleep.setting.folder.help";
    d.spec(folder)?;
    let mut extra = ExtraDecl::new("folder");
    extra.editor = Some(EditorDecl::Slot("core.choose-folder"));
    d.extra(extra)?;
    let mut widgets = SpecDecl::new(
        "widgets",
        Kind::List("home.date, home.continue, stats.widget.today, home.recent"),
        GROUP,
        "sleep.setting.widgets",
    );
    widgets.help = "sleep.setting.widgets.help";
    d.spec(widgets)?;
    let mut extra = ExtraDecl::new("widgets");
    extra.editor = Some(EditorDecl::Slot("core.edit-widgets"));
    d.extra(extra)?;
    for (name, title, help) in [
        (
            "overlay-picture",
            "sleep.setting.overlay-picture",
            "sleep.setting.overlay-picture.help",
        ),
        (
            "overlay-widget",
            "sleep.setting.overlay-widget",
            "sleep.setting.overlay-widget.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Text(""), GROUP, title);
        spec.help = help;
        d.spec(spec)?;
        // Both picked on the picker (the folder, a preset, a widget).
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen("pick-overlay"));
        d.extra(extra)?;
    }
    let mut position = SpecDecl::new(
        "overlay-position",
        Kind::Choice {
            default: POSITIONS[6],
            choices: &POSITIONS,
        },
        GROUP,
        "sleep.setting.overlay-position",
    );
    position.help = "sleep.setting.overlay-position.help";
    d.spec(position)?;
    for (name, min, default, title, help) in [
        (
            "overlay-size",
            10,
            35,
            "sleep.setting.overlay-size",
            "sleep.setting.overlay-size.help",
        ),
        (
            "overlay-see-through",
            0,
            0,
            "sleep.setting.overlay-see-through",
            "sleep.setting.overlay-see-through.help",
        ),
    ] {
        let max = if name == "overlay-size" { 100 } else { 90 };
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
        spec.help = help;
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.unit = "core.unit.percent";
        extra.step = 5;
        extra.slider = true;
        d.extra(extra)?;
    }
    let mut when = SpecDecl::new(
        "overlay-when",
        Kind::Choice {
            default: WHEN[0],
            choices: &WHEN,
        },
        GROUP,
        "sleep.setting.overlay-when",
    );
    when.help = "sleep.setting.overlay-when.help";
    d.spec(when)?;
    d.submit()
}

/// The picker of what goes over the page and the sleep screen.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new(
        "pick-overlay",
        "sleep.setting.overlay-picture",
        PICK,
    ))?;
    // An overlay: the host draws the page below first, which the modes that
    // leave it (`page`, `overlay`) need; the screen never draws it itself. A
    // locked device never shows the page below it (`ui.md` section 1.2).
    // No row of button hints, as Beta 3's sleep screen had none.
    d.screen(
        ScreenDecl::new("screen", "sleep.title", SCREEN)
            .instances(Instances::Single)
            .overlay()
            .hints_hidden(),
    )?;
    d.submit()
}

/// The device waking closes the screen (`event.resume`); a change of
/// `sleep.show` (or a reset of every setting) binds or unbinds the slot
/// `core.sleep` for the mode chosen, and the start and a change of the plugins
/// bind a stored foreign mode where the reader has not bound the slot.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Resume, EVENTS)?;
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::PluginsChanged, PLUGINS_CHANGED)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.subscribe_event(EventKind::ImageReady, IMAGE_READY)?;
    d.submit()
}

/// The slot, the icon, the needs and the places the widgets go.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.icon("sleep.group", "moon")?;
    for item in NEEDS {
        d.menu_need(item, MenuNeed::Sleep)?;
    }
    d.slot_offer(SlotOfferDecl::new("core.sleep", "sleep.screen"))?;
    for (name, key, value) in [
        ("widgets", "sleep.widgets", "sleep.show.widgets"),
        ("overlay", "sleep.overlay-widget", "sleep.show.overlay"),
    ] {
        let place = WidgetPlaceDecl::new(name, "sleep.title", key, WidgetLayout::Stack)
            .and_then(|place| place.shown_if("sleep.show", value))
            .map_err(|_| DeclareError::Empty("widget place"))?;
        d.widget_place(place)?;
    }
    d.submit()
}
