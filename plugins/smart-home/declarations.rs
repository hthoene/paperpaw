//! What the smart home plugin declares: its settings (the server, how often it
//! is fetched while charging, whether servers outside the home network are
//! allowed), its three screens (the dashboard, the entity picker and the
//! connection), the actions that open or fetch, its place in the Tools menu,
//! the four home widgets with their look and settings, and the events it
//! subscribes to. `build.rs` turns it into the image's batches.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Kind,
    MenuEntryDecl, ScreenDecl, SpecDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all, shared_look};

/// The token of the lifecycle events (start, wake, a setting changed, power).
pub const LIFECYCLE_TOKEN: u32 = 1;
/// The token of the end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 2;
/// The token of the work that fetches while charging.
#[allow(
    dead_code,
    reason = "only the plugin schedules work; the build script sees the declarations alone"
)]
pub const WORK_TOKEN: u32 = 3;
/// The actions' tokens.
pub const OPEN_TOKEN: u32 = 5;
pub const REFRESH_TOKEN: u32 = 6;
pub const CHOOSE_TOKEN: u32 = 7;
/// The screens: the dashboard, the entity picker and the connection.
pub const DASH_SCREEN: u32 = 10;
pub const PICK_SCREEN: u32 = 11;
pub const CONNECT_SCREEN: u32 = 12;
/// The widgets: a single value, a group, a switch and a room card.
pub const VALUE_TOKEN: u32 = 20;
pub const GROUP_TOKEN: u32 = 21;
pub const SWITCH_TOKEN: u32 = 22;
pub const ROOM_TOKEN: u32 = 23;

/// The settings group, as the settings name it.
const GROUP: &str = "smart-home.group";

/// The settings by their full ids.
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const SERVER: &str = "smart-home.server";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const REFRESH: &str = "smart-home.refresh";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const ANYWHERE: &str = "smart-home.anywhere";

/// The values of how often the home is fetched while charging.
pub const EVERY: [&str; 5] = [
    "smart-home.every.off",
    "smart-home.every.15",
    "smart-home.every.30",
    "smart-home.every.60",
    "smart-home.every.120",
];

/// The group, the server (edited on the connection screen), how often and
/// whether to allow servers outside the home network.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", GROUP);
    group.order = 91;
    d.group(group)?;
    let mut server = SpecDecl::new("server", Kind::Text(""), GROUP, "smart-home.setting.server");
    server.help = "smart-home.setting.server.help";
    d.spec(server)?;
    let mut on_screen = ExtraDecl::new("server");
    on_screen.editor = Some(EditorDecl::Screen("connect"));
    d.extra(on_screen)?;
    let mut refresh = SpecDecl::new(
        "refresh",
        Kind::Choice {
            default: EVERY[2],
            choices: &EVERY,
        },
        GROUP,
        "smart-home.setting.refresh",
    );
    refresh.help = "smart-home.setting.refresh.help";
    d.spec(refresh)?;
    let mut anywhere = SpecDecl::new(
        "anywhere",
        Kind::Bool(false),
        GROUP,
        "smart-home.setting.anywhere",
    );
    anywhere.help = "smart-home.setting.anywhere.help";
    anywhere.advanced = true;
    d.spec(anywhere)?;
    d.submit()
}

/// The three screens.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "smart-home.title", DASH_SCREEN))?;
    d.screen(ScreenDecl::new(
        "pick",
        "smart-home.pick.title",
        PICK_SCREEN,
    ))?;
    d.screen(ScreenDecl::new(
        "connect",
        "smart-home.connect.title",
        CONNECT_SCREEN,
    ))?;
    d.submit()
}

/// The actions, the Tools menu entry and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "smart-home.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new(
        "refresh",
        "smart-home.action.refresh",
        REFRESH_TOKEN,
    ))?;
    d.action(ActionDecl::new(
        "choose",
        "smart-home.action.choose",
        CHOOSE_TOKEN,
    ))?;
    let mut entry = MenuEntryDecl::new("core.tools", "smart-home.open");
    entry.order = 85;
    d.menu_entry(entry)?;
    d.icon("smart-home.open", "home")?;
    d.icon(GROUP, "home")?;
    d.icon("smart-home.refresh", "refresh")?;
    d.submit()
}

/// The settings of *Smart home value*: which entity (empty: the first value).
pub const VALUE_OPTIONS: &[Opt] = &[
    Opt::Text(
        "entity",
        "smart-home.widget.entity",
        "",
        "smart-home.widget.entity.help",
    ),
    Opt::Part("name", "smart-home.part.name", true),
    Opt::Part("age", "smart-home.part.age", false),
];
/// The settings of *Smart home group*: which room or group (empty: all).
pub const GROUP_OPTIONS: &[Opt] = &[
    Opt::Text(
        "room",
        "smart-home.widget.room",
        "",
        "smart-home.widget.room.help",
    ),
    Opt::Part("title", "smart-home.part.title", true),
    Opt::Part("age", "smart-home.part.age", true),
];
/// The settings of *Smart home switch*: which entity (empty: the first switch).
pub const SWITCH_OPTIONS: &[Opt] = &[
    Opt::Text(
        "entity",
        "smart-home.widget.entity",
        "",
        "smart-home.widget.entity.help",
    ),
    Opt::Part("name", "smart-home.part.name", true),
];
/// The settings of *Smart home room*: which room (empty: the first).
pub const ROOM_OPTIONS: &[Opt] = &[
    Opt::Text(
        "room",
        "smart-home.widget.room",
        "",
        "smart-home.widget.room.help",
    ),
    Opt::Part("switches", "smart-home.part.switches", true),
];

/// The four widgets, their settings groups and their settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, token, cells, frame, options) in [
        (
            "value",
            "smart-home.value",
            VALUE_TOKEN,
            (2, 1),
            "core.frame.none",
            VALUE_OPTIONS,
        ),
        (
            "list",
            "smart-home.list",
            GROUP_TOKEN,
            (2, 2),
            "core.frame.none",
            GROUP_OPTIONS,
        ),
        (
            "switch",
            "smart-home.switch",
            SWITCH_TOKEN,
            (1, 1),
            "core.frame.line",
            SWITCH_OPTIONS,
        ),
        (
            "room",
            "smart-home.room",
            ROOM_TOKEN,
            (2, 2),
            "core.frame.line",
            ROOM_OPTIONS,
        ),
    ] {
        let group = ["smart-home.", name].concat();
        d.group(GroupDecl::new(name, title))?;
        // No `live`: the plugin tells the host when what a widget shows changed.
        let spec = WidgetDecl::new(name, title, "core.widgets.world", token)
            .map_err(|_| DeclareError::TooLong("widget"))?
            .cells(cells.0, cells.1);
        d.widget(spec)?;
        let mut look = shared_look("core.align.center");
        look[1] = Opt::Frame(frame);
        declare_all(
            d,
            name,
            &group,
            look.into_iter().chain(options.iter().copied()),
        )?;
        d.submit()?;
    }
    Ok(())
}

/// The events the plugin watches: the start and the wake, a setting that
/// changed, the charger, the end of a request, the work that fetches while
/// charging.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for kind in [
        EventKind::Started,
        EventKind::Resume,
        EventKind::SettingChanged,
        EventKind::ChargeChanged,
    ] {
        d.subscribe_event(kind, LIFECYCLE_TOKEN)?;
    }
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.submit()
}
