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
