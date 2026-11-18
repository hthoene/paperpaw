//! What the File sync plugin declares: the settings (when it syncs, and for
//! each of three pairs the card's folder, the server, the user and the mode),
//! the screens (the home and a pair), the actions, the places in the tools,
//! the three widgets (*Last sync*, *Pending changes*, *Sync now*) and the
//! events it reads. A pair's password is a secret, never a setting.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Instances,
    Kind, MenuEntryDecl, ScreenDecl, SpecDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const SYNC_TOKEN: u32 = 2;
/// The end of a request.
pub const HTTP_TOKEN: u32 = 3;
/// The plugin stops: a sync under way is cancelled.
pub const STOP_TOKEN: u32 = 4;
/// Events that change what is synced or when.
pub const EVENTS_TOKEN: u32 = 5;
/// The minute tick.
pub const TICK_TOKEN: u32 = 6;
/// The widgets' drawing and taps: *Last sync*, *Pending changes*, *Sync now*.
pub const LAST_TOKEN: u32 = 20;
pub const PENDING_TOKEN: u32 = 21;
pub const BUTTON_TOKEN: u32 = 22;
/// The screens.
pub const HOME: u32 = 10;
pub const PAIR: u32 = 11;

/// The pairs of folders.
pub const PAIRS: usize = 3;

/// When the folders are synced.
pub const WHEN: [&str; 3] = [
    "file-sync.when.manual",
    "file-sync.when.charging",
    "file-sync.when.online",
];
/// How a pair is synced.
pub const MODE: [&str; 2] = ["file-sync.mode.two-way", "file-sync.mode.download"];

/// The widgets, relative to the plugin: their ids are `file-sync.last` and so on.
pub const WIDGETS: [(&str, &str, u32); 3] = [
    ("last", "file-sync.widget.last", LAST_TOKEN),
    ("pending", "file-sync.widget.pending", PENDING_TOKEN),
    ("button", "file-sync.widget.button", BUTTON_TOKEN),
];
/// The widgets' settings: the shared look.
pub const WIDGET_OPTIONS: [Opt; 7] = [
    Opt::Frame("core.frame.line"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
];

/// The titles of the settings groups of the pairs.
const PAIR_GROUPS: [&str; PAIRS] = [
    "file-sync.pair-1.title",
    "file-sync.pair-2.title",
    "file-sync.pair-3.title",
];

/// The settings: when to sync and, for each pair, the card's folder, the
/// mode, the server's address and the user (the address and the user are
/// edited with the password on the pair's screen).
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "file-sync.title");
    group.order = 64;
    d.group(group)?;
    let mut when = SpecDecl::new(
        "when",
        Kind::Choice {
            default: WHEN[0],
            choices: &WHEN,
        },
        "file-sync.group",
        "file-sync.setting.when",
    );
    when.help = "file-sync.setting.when.help";
    d.spec(when)?;
    d.submit()?;
    for (index, title) in PAIR_GROUPS.iter().enumerate() {
        pair_settings(d, index + 1, title)?;
    }
    Ok(())
}

fn pair_settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
    n: usize,
    title: &'static str,
) -> Result<(), DeclareError> {
    let digit = ["", "1", "2", "3"].get(n).copied().unwrap_or("1");
    let group_name = ["pair-", digit].concat();
    let group_id = ["file-sync.", &group_name].concat();
    let mut group = GroupDecl::new(&group_name, title);
    group.order = 64 + n as i32;
    d.group(group)?;
    for (stem, kind, title, help) in [
        (
            "folder",
            Kind::Text(""),
            "file-sync.setting.folder",
            "file-sync.setting.folder.help",
        ),
        (
            "mode",
            Kind::Choice {
                default: MODE[0],
                choices: &MODE,
            },
            "file-sync.setting.mode",
            "file-sync.setting.mode.help",
        ),
        (
            "server",
            Kind::Text(""),
            "file-sync.setting.server",
            "file-sync.setting.server.help",
        ),
        (
            "user",
            Kind::Text(""),
            "file-sync.setting.user",
            "file-sync.setting.user.help",
        ),
    ] {
        let name = [stem, "-", digit].concat();
        let mut spec = SpecDecl::new(&name, kind, &group_id, title);
        spec.help = help;
        spec.kept_on_reset = stem == "server" || stem == "user" || stem == "folder";
        d.spec(spec)?;
        if stem == "folder" {
            let mut extra = ExtraDecl::new(&name);
            extra.editor = Some(EditorDecl::Slot("core.choose-folder"));
            d.extra(extra)?;
        }
    }
    d.submit()?;
    Ok(())
}

/// The screens: the home (the pairs, sync now, what the last sync did) and a
/// pair (its folder, server, user, password and mode).
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("home", "file-sync.title", HOME).instances(Instances::Single))?;
    d.screen(ScreenDecl::new("pair", "file-sync.pair", PAIR).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The actions and the places in the tools: the home, and *Sync now* in one step.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "file-sync.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("sync", "file-sync.sync", SYNC_TOKEN))?;
    let mut open = MenuEntryDecl::new("core.tools", "file-sync.open");
    open.order = 50;
    d.menu_entry(open)?;
    let mut sync = MenuEntryDecl::new("core.tools", "file-sync.sync");
    sync.order = 51;
    d.menu_entry(sync)?;
    d.icon("file-sync.open", "refresh")?;
    d.icon("file-sync.sync", "refresh")?;
    d.submit()?;
    Ok(())
}

/// The three widgets: their groups, the widgets (the plugin says when what
/// they show changed) and their settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, token) in WIDGETS {
        let group = ["file-sync.", name].concat();
        d.group(GroupDecl::new(name, title))?;
        let widget = WidgetDecl::new(name, title, "core.widgets.personal", token)
            .map_err(|_| DeclareError::TooLong("widget"))?
            .cells(if name == "button" { 1 } else { 2 }, 1);
        d.widget(widget)?;
        declare_all(d, name, &group, WIDGET_OPTIONS)?;
        d.submit()?;
    }
    Ok(())
}

/// The events: the end of a request (a request is refused without the
/// subscription), the plugin's stopping, the start, the settings, the
/// charger, the Wi-Fi and the minute.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()?;
    d.subscribe_event(EventKind::Stopping, STOP_TOKEN)?;
    d.submit()?;
    for kind in [
        EventKind::Started,
        EventKind::SettingChanged,
        EventKind::ChargeChanged,
        EventKind::WifiChanged,
    ] {
        d.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    d.submit()?;
    d.subscribe_event(EventKind::Tick, TICK_TOKEN)?;
    d.submit()?;
    Ok(())
}
