//! Native identity of Rules: its two screens, the action that opens the list,
//! its place in the settings menu and the events the rules follow. Callback
//! tokens are stable within this plugin.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, MenuEntryDecl, ScreenDecl,
};

/// Screen tokens, as the plugin's callbacks take them.
pub const LIST: u32 = 20;
pub const PICK: u32 = 21;
/// The action that opens the list.
pub const OPEN: u32 = 1;
/// Event tokens.
pub const STARTED: u32 = 2;
pub const RESUME: u32 = 3;
pub const SUSPEND: u32 = 4;
pub const TICK: u32 = 5;
pub const BOOK_OPENED: u32 = 6;
pub const BOOK_CLOSED: u32 = 7;
pub const BOOK_FINISHED: u32 = 8;
pub const STATUS: u32 = 9;
pub const SETTING: u32 = 10;

/// The list of rules (one at a time) and the choice of an action or a setting.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("list", "rules.title", LIST).instances(Instances::Single))?;
    d.screen(ScreenDecl::new("pick", "rules.title", PICK))?;
    d.submit()?;
    Ok(())
}

/// The action that opens the list, in the settings menu.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "rules.title", OPEN))?;
    let mut entry = MenuEntryDecl::new("core.settings-pages", "rules.open");
    entry.order = 230;
    d.menu_entry(entry)?;
    d.icon("rules.open", "automation")?;
    d.submit()?;
    Ok(())
}

/// What sets rules off: the start, waking and sleeping, the minute, books, the
/// battery and charging, and switches.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::Resume, RESUME)?;
    d.subscribe_event(EventKind::Suspend, SUSPEND)?;
    d.subscribe_event(EventKind::Tick, TICK)?;
    d.subscribe_event(EventKind::BookOpened, BOOK_OPENED)?;
    d.subscribe_event(EventKind::BookClosed, BOOK_CLOSED)?;
    d.subscribe_event(EventKind::BookFinished, BOOK_FINISHED)?;
    d.subscribe_event(EventKind::StatusChanged, STATUS)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING)?;
    d.submit()?;
    Ok(())
}
