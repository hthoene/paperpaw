//! The progress sync's native declarations (slice 1): its settings group and
//! the three settings (the account's user name, whether books sync by
//! themselves, the server). The account editor, the confirmation overlay, the
//! menu entry *Sync progress* and the icons follow with their screens
//! (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, GroupDecl, Instances, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const ACCOUNT: u32 = 10;
pub const SYNC_NOW: u32 = 1;
pub const STARTED: u32 = 2;
pub const SETTING_CHANGED: u32 = 3;
pub const BOOK_OPENED: u32 = 4;
pub const BOOK_CLOSED: u32 = 5;
pub const PAGE_TURNED: u32 = 6;
pub const SUSPEND: u32 = 7;
pub const RESUME: u32 = 8;
pub const WORK_DUE: u32 = 9;
pub const WORK_ALLOWED: u32 = 11;
pub const HTTP_DONE: u32 = 12;
pub const SCREEN_SHOWN: u32 = 13;
/// The work that sends the progress when the radio is on anyway.
pub const PUSH_WORK: &str = "push";

/// The server KOReader's own sync service runs on.
pub const DEFAULT_SERVER: &str = "https://sync.koreader.rocks";

/// The settings group and its settings.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "kosync.group");
    group.order = 28;
    d.group(group)?;
    let mut user = SpecDecl::new(
        "user",
        Kind::Text(""),
        "kosync.group",
        "kosync.setting.user",
    );
    user.help = "kosync.setting.user.help";
    user.kept_on_reset = true;
    d.spec(user)?;
    let mut auto = SpecDecl::new(
        "auto",
        Kind::Bool(true),
        "kosync.group",
        "kosync.setting.auto",
    );
    auto.help = "kosync.setting.auto.help";
    d.spec(auto)?;
    let mut server = SpecDecl::new(
        "server",
        Kind::Text(DEFAULT_SERVER),
        "kosync.group",
        "kosync.setting.server",
    );
    server.help = "kosync.setting.server.help";
    server.kept_on_reset = true;
    server.advanced = true;
    d.spec(server)?;
    d.submit()?;
    Ok(())
}

/// The account screen.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("account", "kosync.group", ACCOUNT).instances(Instances::Single))?;
    d.submit()
}

/// *Sync progress* in the reading menu's *More*, and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("sync-now", "kosync.sync-now", SYNC_NOW))?;
    let mut entry = MenuEntryDecl::new("core.reader-more", "kosync.sync-now");
    entry.order = 25;
    d.menu_entry(entry)?;
    d.icon("kosync.group", "devices")?;
    d.icon("kosync.sync-now", "refresh")?;
    d.submit()
}

/// The events the sync follows.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.subscribe_event(EventKind::BookOpened, BOOK_OPENED)?;
    d.subscribe_event(EventKind::BookClosed, BOOK_CLOSED)?;
    d.subscribe_event(EventKind::PageTurned, PAGE_TURNED)?;
    d.subscribe_event(EventKind::Suspend, SUSPEND)?;
    d.subscribe_event(EventKind::Resume, RESUME)?;
    d.subscribe_event(EventKind::WorkDue, WORK_DUE)?;
    d.subscribe_event(EventKind::WorkAllowed, WORK_ALLOWED)?;
    d.subscribe_event(EventKind::HttpDone, HTTP_DONE)?;
    d.subscribe_event(EventKind::ScreenShown, SCREEN_SHOWN)?;
    d.submit()
}
