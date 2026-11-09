//! The notes vault's native declarations (slice 1): its settings group and the
//! vault's folder. The tools entry, its icon, the three screens, the Markdown
//! format and the home widgets follow with their host pieces
//! (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Instances,
    Kind, MenuEntryDecl, ScreenDecl, SpecDecl,
};

/// The settings group (`notes-vault.group`).
pub const GROUP: &str = "notes-vault.group";

/// Callback tokens are stable within this plugin image.
pub const OPEN: u32 = 1;
/// The vault's page.
pub const HOME: u32 = 10;
/// A list of notes: folder, latest changes, tag, search results.
pub const LIST: u32 = 11;
/// The list of tags.
pub const TAGS: u32 = 12;
/// The index's work comes due.
pub const WORK_DUE: u32 = 20;

/// The settings group and the vault's folder.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.group(GroupDecl::new("group", "notes-vault.title"))?;
    let mut folder = SpecDecl::new(
        "folder",
        Kind::Text(""),
        GROUP,
        "notes-vault.setting.folder",
    );
    folder.help = "notes-vault.setting.folder.help";
    d.spec(folder)?;
    let mut extra = ExtraDecl::new("folder");
    extra.editor = Some(EditorDecl::Slot("core.choose-folder"));
    d.extra(extra)?;
    d.submit()
}

/// The vault's page, the lists and the tags, the action that opens the page,
/// its Tools entry (order 38), its icons and the subscription to the work that
/// makes the index.
pub fn ui<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "notes-vault.title", HOME).instances(Instances::Single))?;
    // A folder opens in a list of its own: each open is an instance.
    d.screen(ScreenDecl::new("list", "notes-vault.title", LIST).instances(Instances::Many))?;
    d.screen(ScreenDecl::new("tags", "notes-vault.tags", TAGS).instances(Instances::Single))?;
    d.action(ActionDecl::new("open", "notes-vault.title", OPEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "notes-vault.open");
    entry.order = 38;
    d.menu_entry(entry)?;
    d.icon("notes-vault.open", "note")?;
    d.icon(GROUP, "note")?;
    d.subscribe_event(EventKind::WorkDue, WORK_DUE)?;
    d.submit()
}
