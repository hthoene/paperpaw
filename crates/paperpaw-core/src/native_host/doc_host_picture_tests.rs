//! What the picture stage (`core.picture`) is told of a document's picture.
use super::*;

/// What the stage is told of `document` as `owner`'s.
fn told(w: &World<'_>, owner: u64, document: u32) -> Option<(bool, std::string::String)> {
    let mut told = None;
    crate::native_host::doc_service::DocHost::picture_book(
        &*w.state,
        owner,
        document,
        &mut |whole, path| told = Some((whole, path.into())),
    );
    told
}

#[test]
fn the_picture_stage_is_told_a_documents_book_and_whether_its_pages_are_whole() {
    world(|w| {
        w.install(
            "epub",
            EPUB,
            &[
                format("epub", "epub", 0, 0),
                format("cbz", "cbz", 1 << 3, 0),
            ],
        );
        let book = w.open(1, "books/a.epub").unwrap();
        let comic = w.open(1, "comics/b.cbz").unwrap();
        assert_eq!(told(w, OPENER, book), Some((false, "books/a.epub".into())));
        assert_eq!(told(w, OPENER, comic), Some((true, "comics/b.cbz".into())));
        // Another plugin's document, and one that is gone, tell nothing.
        assert_eq!(told(w, OPENER + 1, book), None);
        assert_eq!(told(w, OPENER, comic + 9), None);
    });
}
