//! Borrowed settings shared by the build-time emitter and parity fixtures.
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{Declarations, DeclareError, Kind, SpecDecl};

pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    let mut enabled = SpecDecl::new(
        "enabled",
        Kind::Bool(true),
        "power.group",
        "battery.setting.enabled",
    );
    enabled.help = "battery.setting.enabled.help";
    declarations.spec(enabled)?;
    let developer_group = groups::DEVELOPER;
    let mut by_plugin = SpecDecl::new(
        "by-plugin",
        Kind::Bool(true),
        developer_group.as_str(),
        "battery.setting.by-plugin",
    );
    by_plugin.help = "battery.setting.by-plugin.help";
    declarations.spec(by_plugin)?;
    declarations.submit()
}

/// Callback tokens are stable within this plugin image.
pub const SCREEN_TOKEN: u32 = 10;
pub const OPEN_TOKEN: u32 = 11;

pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    use paperpaw_sdk::native::declare::{ActionDecl, ScreenDecl};
    let mut declarations = Declarations::new(sink);
    declarations.screen(ScreenDecl::new("screen", "battery.title", SCREEN_TOKEN))?;
    declarations.action(ActionDecl::new("open", "battery.title", OPEN_TOKEN))?;
    declarations.submit()
}

pub const RECORDER_TOKEN: u32 = 1;

pub fn lifecycle(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    use paperpaw_sdk::native::declare::EventKind;
    let mut declarations = Declarations::new(sink);
    for kind in [
        EventKind::Started,
        EventKind::Resume,
        EventKind::Suspend,
        EventKind::Stopping,
        EventKind::BatteryMeasured,
        EventKind::PageTurned,
    ] {
        declarations.subscribe_event(kind, RECORDER_TOKEN)?;
    }
    declarations.submit()
}
