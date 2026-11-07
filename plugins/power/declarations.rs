//! Power's declarations as borrowed builders, shared by the build script
//! (which writes them into the plugin image) and the tests.
use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, KeyBindingDecl, Kind,
    MenuEntryDecl, SpecDecl, StatusItemDecl, SubscribeDecl,
};

/// Callback tokens are stable within this plugin image.
pub const OPEN_TOKEN: u32 = 10;
pub const MENU_TOKEN: u32 = 11;
pub const STATUS_TOKEN: u32 = 12;
pub const EVENTS_TOKEN: u32 = 1;

pub const GROUP: &str = "power.group";
/// The choices of `power.mode`; translation keys too.
pub const MODES: [&str; 4] = [
    "power.mode.auto",
    "power.mode.normal",
    "power.mode.saving",
    "power.mode.offline",
];

pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    let mut group = GroupDecl::new("group", "power.title");
    group.order = 86;
    declarations.group(group)?;
    let mut mode = SpecDecl::new(
        "mode",
        Kind::Choice {
            default: MODES[0],
            choices: &MODES,
        },
        GROUP,
        "power.setting.mode",
    );
    mode.help = "power.setting.mode.help";
    declarations.spec(mode)?;
    for (name, default, min, max) in [("saving-below", 20, 5, 50), ("offline-below", 10, 0, 30)] {
        let (title, help) = if name == "saving-below" {
            (
                "power.setting.saving-below",
                "power.setting.saving-below.help",
            )
        } else {
            (
                "power.setting.offline-below",
                "power.setting.offline-below.help",
            )
        };
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
        spec.help = help;
        declarations.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.unit = "core.unit.percent";
        declarations.extra(extra)?;
    }
    let mut charging = SpecDecl::new(
        "normal-while-charging",
        Kind::Bool(true),
        GROUP,
        "power.setting.normal-while-charging",
    );
    charging.help = "power.setting.normal-while-charging.help";
    declarations.spec(charging)?;
    declarations.submit()
}

/// Actions, the quick-panel entry, icons, the held Power key and the status item.
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.action(ActionDecl::new("open", "power.title", OPEN_TOKEN))?;
    declarations.action(ActionDecl::new("menu", "power.menu.title", MENU_TOKEN))?;
    // The mode in the quick panel, as segments.
    let quick = menus::QUICK;
    let mut entry = MenuEntryDecl::new(quick.as_str(), "power.mode");
    entry.order = 30;
    declarations.menu_entry(entry)?;
    declarations.icon(GROUP, "leaf")?;
    declarations.icon("power.open", "leaf")?;
    // The power button held opens the power menu.
    declarations.key_binding(KeyBindingDecl::new("global", "long.power", "power.menu"))?;
    // Tapped, the power settings.
    declarations.status_item(
        StatusItemDecl::new("status", "power.status.item", STATUS_TOKEN)
            .and_then(|item| item.tap("open"))
            .map_err(|_| DeclareError::Empty("status item"))?,
    )?;
    declarations.submit()
}

/// Events that make the plugin look at the battery again and the status
/// stage. The dialog's answer is not subscribed: the host delivers it to the
/// asker on its own.
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    for kind in [
        EventKind::Started,
        EventKind::Resume,
        EventKind::BatteryMeasured,
        EventKind::ChargeChanged,
        EventKind::SettingChanged,
    ] {
        declarations.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    declarations.subscribe(SubscribeDecl::new("core.status", STATUS_TOKEN))?;
    declarations.submit()
}
