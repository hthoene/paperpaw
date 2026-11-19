//! What the picture stage (`core.picture`, `docs/abi/document.md` 5) is told
//! of a document's picture: the document's format and its book.
use super::DocState;

/// `format.spec.flags` bit 3: the format reads whole pages.
const WHOLE_PAGES: u32 = 1 << 3;

impl DocState {
    /// Visits whether the format of `owner`'s `document` reads whole pages
    /// and the book's path on root 1 (empty for a book of another root).
    /// Nothing is visited for a document that is gone or while the tables are
    /// in use.
    pub(super) fn picture_book_of(
        &self,
        owner: u64,
        document: u32,
        visit: &mut dyn FnMut(bool, &str),
    ) {
        let Ok(row) = self.table.lease(owner, document) else {
            return;
        };
        let whole = self
            .index()
            .is_ok_and(|index| index.has_flag(row.format, WHOLE_PAGES));
        let Ok(books) = self.books.try_borrow() else {
            return;
        };
        let path = books
            .iter()
            .find(|book| book.document == document && book.root == 1)
            .map_or("", |book| book.path.as_str());
        visit(whole, path);
    }
}
