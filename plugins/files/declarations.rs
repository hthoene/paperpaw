//! The library plugin's native declarations (slice 1): the settings, the
//! screens (the library, a list of books, the shelves), the actions that
//! open the shelves, their icons and menu entries, and the slots the
//! library fills.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, ExtraDecl, Instances, Kind, MenuEntryDecl,
    ScreenDecl, SlotOfferDecl, SpecDecl,
};

/// The library: books of the card, filtered by how far they are read.
pub const BROWSER_TOKEN: u32 = 10;
/// A list of books of one scope (the favourites, an author, a series, a tag, a folder).
pub const SHELF_TOKEN: u32 = 11;
/// The shelves: where else the books are listed.
pub const SHELVES_TOKEN: u32 = 12;
/// Opens the shelves.
pub const SHELVES_OPEN_TOKEN: u32 = 20;
/// Opens the favourites.
pub const FAVOURITES_TOKEN: u32 = 21;

/// The library's setting values, each its name's key.
pub const SORT_RECENT: &str = "files.sort.recent";
pub const SORT_NAME: &str = "files.sort.name";
pub const VIEW_LIST: &str = "files.view.list";
pub const VIEW_COVERS: &str = "files.view.covers";
pub const COVERS_MONO: &str = "files.covers.mono";
pub const COVERS_GRAY: &str = "files.covers.gray";
/// The order of books (a setting).
pub const SORT: &str = "files.sort";
/// How books are shown (a setting).
pub const VIEW: &str = "files.view";
/// How covers show (a setting).
pub const COVERS: &str = "files.covers";
/// The host slot that chooses a folder.
pub const CHOOSE_FOLDER: &str = "core.choose-folder";

/// Where the archive is and how the library shows its books (the library
/// folder is the kernel's).
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::LIBRARY;
    let mut archive = SpecDecl::new(
        "archive",
        Kind::Text("books/Archive"),
        group.as_str(),
        "files.setting.archive",
    );
    archive.help = "files.setting.archive.help";
    archive.kept_on_reset = true;
    archive.advanced = true;
    d.spec(archive)?;
    let mut folder = ExtraDecl::new("archive");
    folder.editor = Some(EditorDecl::Slot(CHOOSE_FOLDER));
    d.extra(folder)?;
    let mut view = SpecDecl::new(
        "view",
        Kind::Choice {
            default: VIEW_LIST,
            choices: &[VIEW_LIST, VIEW_COVERS],
        },
        group.as_str(),
        "files.setting.view",
    );
    view.help = "files.setting.view.help";
    d.spec(view)?;
    let mut covers = SpecDecl::new(
        "covers",
        Kind::Choice {
            default: COVERS_MONO,
            choices: &[COVERS_MONO, COVERS_GRAY],
        },
        group.as_str(),
        "files.setting.covers",
    );
    covers.help = "files.setting.covers.help";
    d.spec(covers)?;
    let mut sort = SpecDecl::new(
        "sort",
        Kind::Choice {
            default: SORT_RECENT,
            choices: &[SORT_RECENT, SORT_NAME],
        },
        group.as_str(),
        "files.setting.sort",
    );
    sort.help = "files.setting.sort.help";
    d.spec(sort)?;
    d.submit()?;
    Ok(())
}

pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("browser", "files.title", BROWSER_TOKEN).instances(Instances::Single),
    )?;
    // One instance per scope: a series, an author, the favourites.
    d.screen(ScreenDecl::new("shelf", "files.title", SHELF_TOKEN))?;
    d.screen(
        ScreenDecl::new("shelves", "files.shelves", SHELVES_TOKEN).instances(Instances::Single),
    )?;
    d.submit()?;
    Ok(())
}

/// The actions, their icons, the menu of shelves, where covers in grays
/// are offered, and the slots: the library (and the home screen, where
/// no home plugin is installed).
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new(
        "shelves",
        "files.shelves",
        SHELVES_OPEN_TOKEN,
    ))?;
    d.action(ActionDecl::new(
        "favourites",
        "files.favourites",
        FAVOURITES_TOKEN,
    ))?;
    d.icon("files.shelves", "books")?;
    d.icon("files.favourites", "star")?;
    d.icon(SORT, "sort-ascending")?;
    d.icon(VIEW, "layout-grid")?;
    // Gray covers or black and white only where the panel is black and
    // white; a gray one shows them in grays as fast.
    d.menu_need(COVERS, MenuNeed::BlackAndWhite)?;
    let mut favourites = MenuEntryDecl::new("files.shelves", "files.favourites");
    favourites.order = 5;
    d.menu_entry(favourites)?;
    d.slot_offer(SlotOfferDecl::new("core.library", "files.browser"))?;
    let mut home = SlotOfferDecl::new("core.home", "files.browser");
    home.priority = -100;
    d.slot_offer(home)?;
    d.submit()?;
    Ok(())
}
