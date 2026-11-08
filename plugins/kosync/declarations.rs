//! The progress sync's native declarations (slice 1): its settings group and
//! the three settings (the account's user name, whether books sync by
//! themselves, the server). The account editor, the confirmation overlay, the
//! menu entry *Sync progress* and the icons follow with their screens
//! (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, GroupDecl, Kind, SpecDecl};

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
