//! The library plugin's native declarations: the settings, the screens
//! (the library, a list of books, the shelves, a list of names, book
//! information), the actions that open the shelves and search, their icons
//! and menu entries, and the slots the library fills.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, HelpHintDecl,
    Instances, Kind, MenuEntryDecl, ScreenDecl, SlotOfferDecl, SpecDecl,
};
use paperpaw_sdk::native::ui::HintInput;

/// The library: books of the card, filtered by how far they are read.
pub const BROWSER_TOKEN: u32 = 10;
/// A list of books of one scope (the favourites, an author, a series, a tag, a folder).
pub const SHELF_TOKEN: u32 = 11;
/// The shelves: where else the books are listed.
pub const SHELVES_TOKEN: u32 = 12;
/// A list of the names of one shelf (authors, series, tags, folders).
pub const NAMES_TOKEN: u32 = 13;
/// Book information.
pub const INFO_TOKEN: u32 = 14;
/// Opens the shelves.
pub const SHELVES_OPEN_TOKEN: u32 = 20;
/// Opens the favourites.
pub const FAVOURITES_TOKEN: u32 = 21;
/// Opens the authors, the series, the tags.
pub const BY_AUTHOR_TOKEN: u32 = 22;
pub const BY_SERIES_TOKEN: u32 = 23;
pub const BY_TAG_TOKEN: u32 = 24;
/// Asks for words and lists the books that have them.
pub const SEARCH_TOKEN: u32 = 25;
/// The subscription to the library's changes.
pub const EVENTS_TOKEN: u32 = 3;

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
    // One instance per shelf: a list of authors above a list of series.
    d.screen(ScreenDecl::new("names", "files.title", NAMES_TOKEN))?;
    d.screen(ScreenDecl::new("info", "files.info", INFO_TOKEN))?;
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
    d.action(ActionDecl::new(
        "by-author",
        "files.by-author",
        BY_AUTHOR_TOKEN,
    ))?;
    d.action(ActionDecl::new(
        "by-series",
        "files.by-series",
        BY_SERIES_TOKEN,
    ))?;
    d.action(ActionDecl::new("by-tag", "files.by-tag", BY_TAG_TOKEN))?;
    d.action(ActionDecl::new("search", "files.search", SEARCH_TOKEN))?;
    d.icon("files.shelves", "books")?;
    d.icon("files.favourites", "star")?;
    d.icon("files.by-author", "user")?;
    d.icon("files.by-series", "list-details")?;
    d.icon("files.by-tag", "tag")?;
    d.icon("files.search", "search")?;
    d.icon(SORT, "sort-ascending")?;
    d.icon(VIEW, "layout-grid")?;
    // Gray covers or black and white only where the panel is black and
    // white; a gray one shows them in grays as fast.
    d.menu_need(COVERS, MenuNeed::BlackAndWhite)?;
    let mut favourites = MenuEntryDecl::new("files.shelves", "files.favourites");
    favourites.order = 5;
    d.menu_entry(favourites)?;
    for (order, item) in [
        (10, "files.by-author"),
        (20, "files.by-series"),
        (30, "files.by-tag"),
    ] {
        let mut entry = MenuEntryDecl::new("files.shelves", item);
        entry.order = order;
        d.menu_entry(entry)?;
    }
    // The library's own menu: where interface 6 offered the search.
    // Only what is about the library: night mode and home are the device's.
    for (order, item) in [
        (10, SORT),
        (11, VIEW),
        (21, "files.search"),
        (30, "files.shelves"),
        (80, "core.library-folder"),
        (90, "core.settings"),
    ] {
        let mut entry = MenuEntryDecl::new("files.menu", item);
        entry.order = order;
        d.menu_entry(entry)?;
    }
    d.icon("core.library-folder", "folder")?;
    d.slot_offer(SlotOfferDecl::new("core.library", "files.browser"))?;
    d.slot_offer(SlotOfferDecl::new("core.book-info", "files.info"))?;
    let mut home = SlotOfferDecl::new("core.home", "files.browser");
    home.priority = -100;
    d.slot_offer(home)?;
    d.submit()?;
    Ok(())
}

/// The library changing (books, their status, the walk, a cover that is
/// ready) is read again by the lists on show.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::LibraryChanged, EVENTS_TOKEN)?;
    d.submit()
}

/// What the library tells in the help: how to open a book and what holding
/// one does, by touch and with the buttons.
pub fn help<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (text, input) in [
        ("files.help.open", HintInput::Touch),
        ("files.help.hold", HintInput::Touch),
        ("files.help.pages", HintInput::Touch),
        ("files.help.choose", HintInput::Buttons),
        ("files.help.more", HintInput::Buttons),
    ] {
        let hint = HelpHintDecl::new("files.browser", text, input)
            .map_err(|_| DeclareError::Empty("help hint"))?;
        d.help_hint(hint)?;
    }
    d.submit()
}
