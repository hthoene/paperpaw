//! `Storage::entry_identity` on FAT: the first cluster, kept by a rename and
//! a move, and settled before it is answered.

use super::*;

#[test]
fn a_rename_or_a_move_keeps_the_identity_and_others_differ() {
    let s = storage();
    s.write("Books/a.epub", b"a book").unwrap();
    s.write("Books/b.epub", b"another").unwrap();
    s.write("Inbox/c.txt", b"c").unwrap();
    let book = s.entry_identity("Books/a.epub").unwrap().unwrap();
    let folder = s.entry_identity("Inbox").unwrap().unwrap();
    assert_ne!(Some(book), s.entry_identity("Books/b.epub").unwrap());
    assert_ne!(book, folder);
    s.rename("Books/a.epub", "Books/A renamed.epub").unwrap();
    assert_eq!(s.entry_identity("Books/A renamed.epub"), Ok(Some(book)));
    s.rename("Books/A renamed.epub", "Inbox/a.epub").unwrap();
    assert_eq!(s.entry_identity("/Inbox/a.epub/"), Ok(Some(book)));
    assert_eq!(
        s.entry_identity("Books/a.epub"),
        Err(StorageError::NotFound)
    );
    s.rename("Inbox", "Books/Inbox").unwrap();
    assert_eq!(s.entry_identity("Books/Inbox"), Ok(Some(folder)));
    assert_eq!(s.entry_identity("books/inbox/A.EPUB"), Ok(Some(book)));
}

#[test]
fn nothing_an_empty_file_and_the_root_have_none() {
    let s = storage();
    s.write("empty.txt", b"").unwrap();
    assert_eq!(s.entry_identity("empty.txt"), Ok(None));
    assert_eq!(s.entry_identity(""), Ok(None));
    assert_eq!(s.entry_identity("/"), Ok(None));
    assert_eq!(s.entry_identity("gone.txt"), Err(StorageError::NotFound));
    assert_eq!(
        s.entry_identity("gone/deeper.txt"),
        Err(StorageError::NotFound)
    );
    assert!(s.entry_identity("../x").is_err());
}

/// After a cut at any block of a move into another folder, the moved file
/// answers its identity at exactly one of the two places: the half-done move
/// is settled first, so a journal deciding by identity never sees both.
#[test]
fn after_any_cut_of_a_move_one_place_holds_the_identity() {
    let image = prepared(|s| {
        s.write("Books/a.epub", &[7; 3000]).unwrap();
        s.create_dir("Inbox").unwrap();
    });
    let before = mounted(image.clone())
        .entry_identity("Books/a.epub")
        .unwrap()
        .unwrap();
    let cuts = cut_images(image, |s| s.rename("Books/a.epub", "Inbox/a.epub").unwrap());
    let last = cuts.len() - 1;
    for (n, image) in cuts.into_iter().enumerate() {
        let s = mounted(image);
        let at_from = s.entry_identity("Books/a.epub").ok().flatten() == Some(before);
        let at_to = s.entry_identity("Inbox/a.epub").ok().flatten() == Some(before);
        assert!(at_from != at_to, "cut {n}: from {at_from}, to {at_to}");
        if n == last {
            assert!(at_to, "cut {n}");
        }
    }
}
