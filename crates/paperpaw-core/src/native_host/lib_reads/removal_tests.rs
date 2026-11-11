#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The catalog's side of a delete over a card in memory: what is gone after
//! each phase, that a repeat changes nothing, and that other books are left.
extern crate std;
use super::*;
use crate::native_host::lib_reads::tests::card;
use crate::native_host::lib_service::LibQuery;
use paperpaw_platform::StorageError;
use paperpaw_sdk::{annotations, positions};

/// A book with everything the two phases remove.
fn kept(card: &crate::native_host::lib_reads::tests::Card, path: &str) {
    library::set_field(&*card.storage, path, "stars.rating", "4");
    positions::save(
        &*card.storage,
        path,
        &positions::Position::new(paperpaw_sdk::document::Locator::new(2, 3), 12.0),
    )
    .unwrap();
    annotations::add(
        &*card.storage,
        path,
        annotations::Annotation::new(
            annotations::Mark::new(
                annotations::Kind::Highlight,
                paperpaw_sdk::document::Locator::new(1, 0),
                paperpaw_sdk::document::Locator::new(1, 4),
                false,
            ),
            1,
            1,
            1,
            "text",
            "",
        ),
    )
    .unwrap();
    importer::set(&*card.storage, path, "news").unwrap();
}

fn exists(card: &crate::native_host::lib_reads::tests::Card, file: &str) -> bool {
    card.storage.size(file).is_ok()
}

#[test]
fn the_annotation_file_is_the_one_the_sdk_keeps() {
    assert_eq!(
        kept_file(ANNOTATIONS, "books/c.epub").unwrap(),
        annotations::file("books/c.epub")
    );
}

#[test]
fn details_forgets_the_catalog_line_the_details_and_the_fields_but_not_the_marks() {
    let card = card();
    kept(&card, "books/c.epub");
    kept(&card, "books/a.epub");
    assert!(library::book(&*card.storage, "books/c.epub").is_some());
    details(&*card.storage, "books/c.epub").unwrap();
    assert!(library::book(&*card.storage, "books/c.epub").is_none());
    // The marks are the next phase's; the other book is untouched.
    assert!(exists(&card, &positions::file("books/c.epub")));
    assert!(exists(&card, &annotations::file("books/c.epub")));
    assert!(library::book(&*card.storage, "books/a.epub").is_some());
    // The book is out of the catalog: a read says so.
    let reads = card.reads();
    assert_eq!(
        reads.book(
            crate::native_host::lib_service::BookRef::Path("books/c.epub"),
            &mut |_| {}
        ),
        Err(paperpaw_native::host_glue::ERR_NOT_FOUND)
    );
}

#[test]
fn marks_removes_the_position_the_annotations_and_the_importer_record() {
    let card = card();
    kept(&card, "books/c.epub");
    kept(&card, "books/a.epub");
    marks(&*card.storage, "books/c.epub").unwrap();
    assert!(!exists(&card, &positions::file("books/c.epub")));
    assert!(!exists(&card, &annotations::file("books/c.epub")));
    assert_eq!(
        importer::is_by(&*card.storage, "books/c.epub", "news"),
        Ok(false)
    );
    assert!(exists(&card, &positions::file("books/a.epub")));
    assert!(exists(&card, &annotations::file("books/a.epub")));
    assert_eq!(
        importer::is_by(&*card.storage, "books/a.epub", "news"),
        Ok(true)
    );
}

#[test]
fn a_repeat_after_a_cut_finds_it_done_and_a_book_that_kept_nothing_is_no_error() {
    let card = card();
    kept(&card, "books/c.epub");
    for _ in 0..2 {
        details(&*card.storage, "books/c.epub").unwrap();
        marks(&*card.storage, "books/c.epub").unwrap();
    }
    // Book E kept nothing at all.
    details(&*card.storage, "books/e.epub").unwrap();
    marks(&*card.storage, "books/e.epub").unwrap();
    assert!(!exists(&card, &positions::file("books/c.epub")));
}

/// A card that cannot remove the details file.
struct Stuck(alloc::rc::Rc<paperpaw_sdk::MemoryStorage>);
impl Storage for Stuck {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        if path.ends_with(".ini") {
            return Err(paperpaw_platform::StorageError::Io);
        }
        self.0.remove(path)
    }
}

#[test]
fn a_card_that_cannot_remove_the_details_is_the_steps_error_not_an_ok() {
    let card = card();
    kept(&card, "books/c.epub");
    let stuck = Stuck(card.storage.clone());
    assert_eq!(
        details(&stuck, "books/c.epub"),
        Err(paperpaw_native::host_glue::ERR_IO)
    );
    // The fields are still there: a later book at this path must not inherit
    // them, so the step is repeated; with a working card it is done.
    assert!(library::book(&*card.storage, "books/c.epub").is_some());
    details(&*card.storage, "books/c.epub").unwrap();
    assert!(library::book(&*card.storage, "books/c.epub").is_none());
}

#[test]
fn an_unreadable_list_makes_the_delete_step_fail_after_the_rest_is_forgotten() {
    let card = card();
    kept(&card, "books/c.epub");
    let newer = b"# format 9\nbooks/c.epub\n";
    card.storage
        .write(".paperpaw/library/favourites.txt", newer)
        .unwrap();
    assert_eq!(
        details(&*card.storage, "books/c.epub"),
        Err(paperpaw_native::host_glue::ERR_INVALID)
    );
    // The details and the fields are gone; the list is as it was, so a repeat
    // gives the same answer and the delete does not finish; the code is a
    // permanent one, not the retried `ERR_IO`.
    assert!(library::book(&*card.storage, "books/c.epub").is_none());
    let mut read = [0_u8; 64];
    let n = card
        .storage
        .read_at(".paperpaw/library/favourites.txt", 0, &mut read)
        .unwrap();
    assert_eq!(&read[..n], newer);
    assert_eq!(
        details(&*card.storage, "books/c.epub"),
        Err(paperpaw_native::host_glue::ERR_INVALID)
    );
}

/// A card that refuses the catalog's log (an append) and/or the removal of the
/// details file, with the given error.
struct Refusing {
    inner: alloc::rc::Rc<paperpaw_sdk::MemoryStorage>,
    append: Option<paperpaw_platform::StorageError>,
    remove: Option<paperpaw_platform::StorageError>,
}
impl Storage for Refusing {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(p, o, b)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        match self.append {
            Some(error) if path.ends_with("status.log") => Err(error),
            _ => self.inner.append(path, data),
        }
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        match self.remove {
            Some(error) if path.ends_with(".ini") => Err(error),
            _ => self.inner.remove(path),
        }
    }
}

#[test]
fn a_full_card_is_err_full_so_the_step_repeats_and_too_large_is_the_permanent_code() {
    use paperpaw_native::host_glue::{ERR_FULL, ERR_NO_SPACE};
    let card = card();
    kept(&card, "books/c.epub");
    let refusing = |remove| Refusing {
        inner: card.storage.clone(),
        append: None,
        remove: Some(remove),
    };
    let full = details(&refusing(StorageError::Full), "books/c.epub");
    assert_eq!(full, Err(ERR_FULL));
    // The journal repeats ERR_FULL (room may be made), not ERR_NO_SPACE ...
    assert_eq!(
        details(&refusing(StorageError::TooLarge), "books/c.epub"),
        Err(ERR_NO_SPACE)
    );
    // ... and with room again the same step is done.
    details(&*card.storage, "books/c.epub").unwrap();
}

#[test]
fn a_catalog_that_cannot_be_written_still_lets_the_details_go_and_is_the_error() {
    use paperpaw_native::host_glue::ERR_INVALID;
    let card = card();
    kept(&card, "books/c.epub");
    let damaged = Refusing {
        inner: card.storage.clone(),
        append: Some(StorageError::InvalidData),
        remove: None,
    };
    // The catalog line cannot be noted (damaged), but the rest is removed, and
    // the first error is the answer.
    assert_eq!(details(&damaged, "books/c.epub"), Err(ERR_INVALID));
    assert!(library::book(&*card.storage, "books/c.epub").is_none());
}
