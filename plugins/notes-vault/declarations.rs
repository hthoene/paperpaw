//! The notes vault's native declarations (slice 1): its settings group and the
//! vault's folder. The tools entry, its icon, the three screens, the Markdown
//! format and the home widgets follow with their host pieces
//! (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, ExtraDecl, GroupDecl, Kind, SpecDecl,
};

/// The settings group (`notes-vault.group`).
pub const GROUP: &str = "notes-vault.group";

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
