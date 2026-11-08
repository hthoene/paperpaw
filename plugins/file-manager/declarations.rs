//! File manager's declarations as borrowed builders, shared by the build script
//! (which writes them into the plugin image) and the tests.
//!
//! The setting for hidden files, the screen of the browser, the action that
//! opens it, its entry in the Tools menu and its icon.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, Instances, Kind, MenuEntryDecl, ScreenDecl, SpecDecl,
};

/// Callback tokens are stable within this plugin image.
pub const OPEN_TOKEN: u32 = 1;
pub const BROWSER_TOKEN: u32 = 10;

/// The group of library settings, where *Show hidden files* is.
pub const GROUP: &str = "core.group.library";

/// The one setting: files and folders starting with a dot show.
pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    let mut hidden = SpecDecl::new(
        "hidden",
        Kind::Bool(false),
        GROUP,
        "file-manager.setting.hidden",
    );
    hidden.help = "file-manager.setting.hidden.help";
    hidden.advanced = true;
    declarations.spec(hidden)?;
    declarations.submit()
}

/// The browser, the action that opens it, its Tools menu entry (order 45) and
/// its folder icon.
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.screen(
        ScreenDecl::new("browser", "file-manager.title", BROWSER_TOKEN)
            .instances(Instances::Single),
    )?;
    declarations.action(ActionDecl::new("open", "file-manager.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "file-manager.open");
    entry.order = 45;
    declarations.menu_entry(entry)?;
    declarations.icon("file-manager.open", "folder")?;
    declarations.submit()
}
