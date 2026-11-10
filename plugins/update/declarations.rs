//! What the update declares (pinned by the declaration tests): its group of
//! settings (Settings, Device, Updates) with the three settings, the page, the
//! two actions with their places and icons, the device needs and the first-run
//! step.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, GroupDecl, Instances, Kind, MenuEntryDecl,
    ScreenDecl, SetupPage, SetupStepDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const CHECK_TOKEN: u32 = 2;
pub const PAGE: u32 = 10;
/// The events it hears: the start (how an install went, said after the
/// restart), the end of an install, and its progress.
pub const STARTED: u32 = 3;
pub const INSTALL_DONE: u32 = 4;
pub const FIRMWARE_CHANGED: u32 = 5;
/// The online flow: a request of `core.net` ended or made progress, declared
/// work came due, and a setting changed (looking by itself).
pub const HTTP_DONE: u32 = 6;
pub const HTTP_PROGRESS: u32 = 7;
pub const WORK_DUE: u32 = 8;
pub const SETTING_CHANGED: u32 = 9;

const GROUP: &str = "update.group";
/// The server PaperPaw asks unless the setting says otherwise.
pub const DEFAULT_SERVER: &str = "https://paperpaw.org";

/// The group and its settings: the address of the update server (for
/// developers, in their group), looking by itself and betas.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "update.group");
    group.order = 94;
    d.group(group)?;
    let mut server = SpecDecl::new(
        "server",
        Kind::Text(DEFAULT_SERVER),
        "core.group.developer",
        "update.setting.server",
    );
    server.help = "update.setting.server.help";
    d.spec(server)?;
    for (name, title, help) in [
        ("look", "update.setting.look", "update.setting.look.help"),
        ("betas", "update.setting.betas", "update.setting.betas.help"),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Bool(false), GROUP, title);
        spec.help = help;
        d.spec(spec)?;
    }
    d.submit()?;
    Ok(())
}

/// The page, one at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("page", "update.title", PAGE).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The actions, the page's row first in the group, the icons, the needs of a
/// device whose firmware PaperPaw is, and the first-run step.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.icon(GROUP, "cloud-download")?;
    for item in [
        GROUP,
        "update.server",
        "update.look",
        "update.check",
        "update.open",
    ] {
        d.menu_need(item, MenuNeed::Firmware)?;
    }
    d.action(ActionDecl::new("open", "update.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("check", "update.check", CHECK_TOKEN))?;
    d.icon("update.check", "cloud-download")?;
    d.icon("update.open", "circle-arrow-up")?;
    d.menu_entry(MenuEntryDecl::new(GROUP, "update.open"))?;
    // The look setting asked as the setting it is (its title and its help,
    // which the interface-6 step left empty to take), where PaperPaw updates
    // the firmware.
    let mut step = SetupStepDecl::new(
        "setup",
        "update.setting.look",
        SetupPage::Setting("update.look"),
    );
    step.help = "update.setting.look.help";
    step.order = 57;
    step.need = Some(MenuNeed::Firmware);
    d.setup_step(step)?;
    d.submit()?;
    Ok(())
}

/// The events it hears: the start (how an install went, said after the
/// restart), the end of an install, its progress, the answers and downloads of
/// the online flow, the work that looks for updates and the setting that turns
/// that on or off.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::InstallDone, INSTALL_DONE)?;
    d.subscribe_event(EventKind::FirmwareChanged, FIRMWARE_CHANGED)?;
    d.subscribe_event(EventKind::HttpDone, HTTP_DONE)?;
    d.subscribe_event(EventKind::HttpProgress, HTTP_PROGRESS)?;
    d.subscribe_event(EventKind::WorkDue, WORK_DUE)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.submit()?;
    Ok(())
}
