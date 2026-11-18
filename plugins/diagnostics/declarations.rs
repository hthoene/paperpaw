//! Diagnostics' declarations as borrowed builders, shared by the build script
//! (which writes them into the plugin image) and the tests.
use paperpaw_sdk::builtin::{groups, menus};
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, KeyBindingDecl, Kind,
    MenuEntryDecl, ScreenDecl, SpecDecl,
};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;
pub const SCREEN_TOKEN: u32 = 10;
pub const CHECK_TOKEN: u32 = 11;
pub const RESET_TOKEN: u32 = 12;
pub const OPEN_CHECK_TOKEN: u32 = 13;

/// How many pages the test turns.
pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    let developer = groups::DEVELOPER;
    let mut pages = SpecDecl::new(
        "pages",
        Kind::Int {
            default: 20,
            min: 1,
            max: 10_000,
        },
        developer.as_str(),
        "diagnostics.setting.pages",
    );
    pages.help = "diagnostics.setting.pages.help";
    declarations.spec(pages)?;
    declarations.submit()
}

/// Screens, actions, the Tools entry, its icon, and the held Select on the test screen.
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.screen(ScreenDecl::new(
        "check",
        "diagnostics.check.title",
        CHECK_TOKEN,
    ))?;
    declarations.screen(
        ScreenDecl::new("screen", "diagnostics.title", SCREEN_TOKEN).instances(Instances::Single),
    )?;
    let mut reset = ActionDecl::new("reset", "diagnostics.action.reset", RESET_TOKEN);
    reset.needs_screen = true;
    declarations.action(reset)?;
    declarations.action(ActionDecl::new(
        "open-check",
        "diagnostics.check.title",
        OPEN_CHECK_TOKEN,
    ))?;
    // Under Tools; apps with this plugin built in (Android) keep their Tools for readers.
    let tools = menus::TOOLS;
    let mut entry = MenuEntryDecl::new(tools.as_str(), "diagnostics.open-check");
    entry.order = 99;
    declarations.menu_entry(entry)?;
    declarations.menu_need("diagnostics.open-check", paperpaw_sdk::MenuNeed::Firmware)?;
    declarations.icon("diagnostics.open-check", "list-check")?;
    declarations.key_binding(KeyBindingDecl::new(
        "diagnostics.screen",
        "long.confirm",
        "diagnostics.reset",
    ))?;
    declarations.submit()
}

/// The time, the battery, the radio and sleeping, for the device check.
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    for kind in [
        EventKind::Tick,
        EventKind::Suspend,
        EventKind::Resume,
        EventKind::BatteryMeasured,
        EventKind::ChargeChanged,
        EventKind::WifiChanged,
    ] {
        declarations.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    declarations.submit()
}
