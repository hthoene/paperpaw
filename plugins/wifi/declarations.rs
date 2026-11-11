//! What the Wi-Fi plugin declares, built into its image by `build.rs` and
//! compared with the interface-6 registration by the declaration tests.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, GroupDecl, Instances, Kind, ScreenDecl,
    SetupPage, SetupStepDecl, SpecDecl, StatusItemDecl,
};

/// Callback tokens: stable within this plugin.
pub const PAGE: u32 = 10;
pub const NETWORKS: u32 = 11;
pub const PHONE: u32 = 12;
pub const OPEN: u32 = 1;
pub const STATUS: u32 = 2;
pub const STARTED: u32 = 3;
pub const STATUS_CHANGED: u32 = 4;
pub const WIFI_CHANGED: u32 = 6;
pub const POWER_CHANGED: u32 = 7;
pub const WIFI_SCANNED: u32 = 8;
pub const WIFI_DONE: u32 = 9;
pub const NETWORK_WANTED: u32 = 13;
pub const NETWORK_IDLE: u32 = 14;
/// The timers (idle off, retry) come due as work.
pub const WORK: u32 = 15;
/// The events of the page the setup with a phone serves.
pub const SERVER_EVENT: u32 = 16;
pub const SERVED_REQUEST: u32 = 17;

/// The idle choices, in the order the page lists them.
pub const IDLE_OFF: [&str; 5] = [
    "wifi.idle-off.30s",
    "wifi.idle-off.1m",
    "wifi.idle-off.2m",
    "wifi.idle-off.5m",
    "wifi.idle-off.10m",
];

/// The group, shown as the Wi-Fi page, and when the radio is on.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "wifi.title");
    group.order = 85;
    group.page = "page";
    d.group(group)?;
    let mut on_demand = SpecDecl::new(
        "on-demand",
        Kind::Bool(true),
        "wifi.group",
        "wifi.setting.on-demand",
    );
    on_demand.help = "wifi.setting.on-demand.help";
    d.spec(on_demand)?;
    let mut idle_off = SpecDecl::new(
        "idle-off",
        Kind::Choice {
            default: "wifi.idle-off.1m",
            choices: &IDLE_OFF,
        },
        "wifi.group",
        "wifi.setting.idle-off",
    );
    idle_off.help = "wifi.setting.idle-off.help";
    d.spec(idle_off)?;
    let mut charging = SpecDecl::new(
        "while-charging",
        Kind::Bool(false),
        "wifi.group",
        "wifi.setting.while-charging",
    );
    charging.help = "wifi.setting.while-charging.help";
    d.spec(charging)?;
    let mut first = SpecDecl::new(
        "join-first",
        Kind::Choice {
            default: "wifi.join-first.picked",
            choices: &["wifi.join-first.picked", "wifi.join-first.strongest"],
        },
        "wifi.group",
        "wifi.setting.join-first",
    );
    first.help = "wifi.setting.join-first.help";
    d.spec(first)?;
    d.submit()
}

/// The Wi-Fi page, the networks in reach, and the setup with a phone.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("page", "wifi.title", PAGE).instances(Instances::Single))?;
    d.screen(
        ScreenDecl::new("networks", "wifi.networks.title", NETWORKS).instances(Instances::Single),
    )?;
    d.screen(ScreenDecl::new("phone", "wifi.phone.title", PHONE).instances(Instances::Single))?;
    d.submit()
}

/// The action and the status item that open the page, the icons, the
/// settings and the first-run step that only apply where PaperPaw runs the
/// radio.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "wifi.title", OPEN))?;
    d.icon("wifi.group", "wifi")?;
    d.icon("wifi.open", "wifi")?;
    // The names are constants the tests pin, so these cannot be refused.
    let status = StatusItemDecl::new("status", "wifi.status.item", STATUS)
        .and_then(|item| item.tap("open"))
        .map_err(|_| DeclareError::Empty("status item"))?;
    d.status_item(status)?;
    for setting in [
        "wifi.on-demand",
        "wifi.idle-off",
        "wifi.while-charging",
        "wifi.join-first",
    ] {
        d.menu_need(setting, MenuNeed::Wifi)?;
    }
    let mut step = SetupStepDecl::new("setup", "wifi.title", SetupPage::Screen("wifi.page"));
    step.help = "wifi.setup.help";
    step.order = 50;
    step.need = Some(MenuNeed::Wifi);
    d.setup_step(step)?;
    d.submit()
}

/// The events the plugin handles: the radio's own (scanned, done, network
/// wanted and idle) and the device's.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::StatusChanged, STATUS_CHANGED)?;
    d.subscribe_event(EventKind::WifiChanged, WIFI_CHANGED)?;
    d.subscribe_event(EventKind::PowerChanged, POWER_CHANGED)?;
    d.subscribe_event(EventKind::WifiScanned, WIFI_SCANNED)?;
    d.subscribe_event(EventKind::WifiDone, WIFI_DONE)?;
    d.subscribe_event(EventKind::NetworkWanted, NETWORK_WANTED)?;
    d.subscribe_event(EventKind::NetworkIdle, NETWORK_IDLE)?;
    d.subscribe_event(EventKind::WorkDue, WORK)?;
    d.subscribe_event(EventKind::Server, SERVER_EVENT)?;
    d.subscribe_event(EventKind::ServedRequest, SERVED_REQUEST)?;
    d.submit()
}
