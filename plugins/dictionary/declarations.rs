//! The dictionary's native declarations (slice 1): the folder setting and the
//! StarDict dictionaries as a lookup provider. The panel's screen and the
//! slot it fills (`core.lookup`) follow with the panel (`PORTING-7.md`): a
//! screen that cannot draw is not declared.
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, ExtraDecl, Kind, LookupProviderDecl, SpecDecl,
};

/// The StarDict dictionaries as a lookup provider (a stage subscriber).
pub const PROVIDER_TOKEN: u32 = 20;
/// The host slot that chooses a folder.
pub const CHOOSE_FOLDER: &str = "core.choose-folder";
/// Where dictionaries are looked for unless the user chose a folder.
pub const DEFAULT_FOLDER: &str = "dictionaries";

/// The folder of the dictionaries, chosen on the folder chooser.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::READING;
    let mut folder = SpecDecl::new(
        "folder",
        Kind::Text(DEFAULT_FOLDER),
        group.as_str(),
        "dictionary.setting.folder",
    );
    folder.help = "dictionary.setting.folder.help";
    folder.advanced = true;
    d.spec(folder)?;
    let mut editor = ExtraDecl::new("folder");
    editor.editor = Some(EditorDecl::Slot(CHOOSE_FOLDER));
    d.extra(editor)?;
    d.submit()?;
    Ok(())
}

/// The provider: offline, every language.
pub fn providers<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.lookup_provider(LookupProviderDecl::new(
        "stardict",
        "dictionary.title",
        PROVIDER_TOKEN,
    ))?;
    d.submit()?;
    Ok(())
}
