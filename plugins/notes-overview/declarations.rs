//! What All notes declares: its two screens, the action that opens the list,
//! its place in the Tools menu (order 37) and its icon.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, MenuEntryDecl, ScreenDecl,
};

/// The action's token.
pub const OPEN_TOKEN: u32 = 1;
/// The list of books with notes.
pub const LIST_TOKEN: u32 = 10;
/// The notes of one book (parameters `key` and `generation`: the book's catalog handle).
pub const BOOK_TOKEN: u32 = 11;

/// The screens, the action, the Tools menu entry and the icon.
pub fn ui<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>) -> Result<(), DeclareError> {
    // Both reuse the instance with equal parameters, as the interface-6
    // registration did (its default).
    d.screen(ScreenDecl::new(
        "screen",
        "notes-overview.title",
        LIST_TOKEN,
    ))?;
    d.screen(ScreenDecl::new("book", "notes-overview.title", BOOK_TOKEN))?;
    d.action(ActionDecl::new("open", "notes-overview.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "notes-overview.open");
    entry.order = 37;
    d.menu_entry(entry)?;
    d.icon("notes-overview.open", "bookmarks")?;
    d.submit()
}
