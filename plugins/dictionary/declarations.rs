//! The dictionary's native declarations: the folder setting, the StarDict
//! dictionaries as a lookup provider, and the panel (an overlay screen) with
//! the slot it fills (`core.lookup`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, Kind, LookupProviderDecl,
    ScreenDecl, SlotOfferDecl, SpecDecl,
};

/// The StarDict dictionaries as a lookup provider (a stage subscriber).
pub const PROVIDER_TOKEN: u32 = 20;
/// The panel over the reader's page: the screen that fills `core.lookup`.
pub const POPUP_TOKEN: u32 = 22;
/// The background work that makes the indexes of the dictionaries.
pub const WORK_TOKEN: u32 = 21;
/// Its key.
#[allow(
    dead_code,
    reason = "the build script includes this file and does not use it"
)]
pub const WORK_KEY: &str = "dictionary.index";
/// The key of the timer that ends a panel's wait for providers that answer
/// later (same token as the indexing; told apart by the key).
#[allow(
    dead_code,
    reason = "the build script includes this file and does not use it"
)]
pub const ANSWERS_KEY: &str = "dictionary.answers";
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
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.submit()?;
    Ok(())
}

/// The panel (an overlay: the page shows above it) and the slot it fills.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("popup", "dictionary.title", POPUP_TOKEN).overlay())?;
    d.slot_offer(SlotOfferDecl::new("core.lookup", "dictionary.popup"))?;
    // A provider that answers later tells the panel (todo 98).
    d.subscribe_event(EventKind::LookupReady, POPUP_TOKEN)?;
    d.submit()?;
    Ok(())
}
