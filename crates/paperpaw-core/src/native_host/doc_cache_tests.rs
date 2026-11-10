extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;

fn id(conversion: u64) -> Identity<'static> {
    Identity {
        book: "00000000000000aa",
        plugin: "epub",
        version: "1.2.0",
        conversion,
    }
}

fn exists(storage: &MemoryStorage, folder: &str) -> bool {
    folder_exists(storage, folder)
}

fn nothing_pinned(_: &str) -> bool {
    false
}

fn folder(conversion: u64) -> std::string::String {
    id(conversion).folder().unwrap()
}

/// A cache of `conversion` with a page in it, finished or not.
fn made_cache(storage: &MemoryStorage, conversion: u64, finished: bool) {
    let page = std::format!("{}/pages/1", folder(conversion));
    storage.write(&page, b"page").unwrap();
    if finished {
        complete(storage, &id(conversion)).unwrap();
    }
}

#[test]
fn the_folder_is_the_book_the_format_plugin_and_the_conversion() {
    assert_eq!(
        folder(0xabc),
        ".paperpaw/cache/00000000000000aa/epub/0000000000000abc"
    );
}

#[test]
fn a_cache_is_finished_only_by_the_stamp_of_exactly_its_conversion() {
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, false);
    assert_eq!(is_complete(&storage, &id(1)), Ok(false));
    complete(&storage, &id(1)).unwrap();
    assert_eq!(is_complete(&storage, &id(1)), Ok(true));
    // A new version of the format, or another conversion, is not finished.
    let newer = Identity {
        version: "1.3.0",
        ..id(1)
    };
    assert_eq!(is_complete(&storage, &newer), Ok(false));
    // The stamp is made through the host's folder; no temp file is left.
    let host = std::format!("{}/.host/.complete", folder(1));
    assert!(storage.get(&host).is_none());
    // A longer stamp is another one.
    let stamp = std::format!("{}/{STAMP}", folder(1));
    let mut longer = storage.get(&stamp).unwrap();
    longer.push(b'x');
    storage.write(&stamp, &longer).unwrap();
    assert_eq!(is_complete(&storage, &id(1)), Ok(false));
}

#[test]
fn a_conversion_that_made_no_cache_is_not_stamped() {
    let storage = MemoryStorage::new();
    complete(&storage, &id(1)).unwrap();
    assert!(!exists(&storage, &folder(1)));
}

#[test]
fn a_cache_without_a_current_stamp_is_removed_before_it_is_used() {
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, false);
    let page = std::format!("{}/pages/1", folder(1));
    assert_eq!(prepare(&storage, &id(1), &nothing_pinned), Ok(folder(1)));
    assert!(storage.get(&page).is_none(), "a half-made cache is removed");
    // A finished one stays.
    made_cache(&storage, 2, true);
    prepare(&storage, &id(2), &nothing_pinned).unwrap();
    assert!(
        storage
            .get(&std::format!("{}/pages/1", folder(2)))
            .is_some()
    );
    // One finished for an older version of the format goes.
    let newer = Identity {
        version: "2.0.0",
        ..id(2)
    };
    prepare(&storage, &newer, &nothing_pinned).unwrap();
    assert!(
        storage
            .get(&std::format!("{}/pages/1", folder(2)))
            .is_none()
    );
}

#[test]
fn a_cache_an_open_document_uses_is_shared_as_it_is() {
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, false);
    let own = folder(1);
    let pinned = |folder: &str| folder == own;
    assert_eq!(prepare(&storage, &id(1), &pinned), Ok(folder(1)));
    assert!(storage.get(&std::format!("{own}/pages/1")).is_some());
}

#[test]
fn a_book_keeps_two_conversions_and_the_older_one_goes() {
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, true);
    made_cache(&storage, 2, true);
    prepare(&storage, &id(3), &nothing_pinned).unwrap();
    // The first listed is the oldest the storage tells of.
    assert!(!exists(&storage, &folder(1)));
    assert!(exists(&storage, &folder(2)));
    // Another book's conversions are not this book's.
    let other = Identity {
        book: "00000000000000bb",
        ..id(9)
    };
    made_cache(&storage, 2, true);
    storage
        .write(&std::format!("{}/pages/1", other.folder().unwrap()), b"x")
        .unwrap();
    prepare(&storage, &id(3), &nothing_pinned).unwrap();
    assert!(exists(&storage, &other.folder().unwrap()));
}

#[test]
fn a_conversion_an_open_document_uses_never_goes_and_the_third_waits() {
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, true);
    made_cache(&storage, 2, true);
    let (one, two) = (folder(1), folder(2));
    // The older is used: the younger goes.
    let pinned = |folder: &str| folder == one;
    prepare(&storage, &id(3), &pinned).unwrap();
    assert!(exists(&storage, &one));
    assert!(!exists(&storage, &two));
    // Both are used: nothing can be removed under an open document.
    made_cache(&storage, 2, true);
    let both = |folder: &str| folder == one || folder == two;
    assert_eq!(prepare(&storage, &id(3), &both), Err(ERR_BUSY));
    assert!(exists(&storage, &one) && exists(&storage, &two));
}

#[test]
fn a_tree_too_big_for_one_call_is_busy_and_the_next_call_goes_on() {
    struct Slow<'a>(&'a MemoryStorage, core::cell::Cell<u32>);
    impl Storage for Slow<'_> {
        fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(p, o, b)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.append(p, d)
        }
        fn list(
            &self,
            d: &str,
            v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            self.0.remove(p)
        }
        fn remove_tree_step(
            &self,
            root: &str,
            keep: bool,
            place: u64,
        ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
            // Each piece takes one entry away and says it is not done.
            self.1.set(self.1.get() + 1);
            let _ = (root, keep);
            Ok(paperpaw_platform::tree::TreeStep::new(1, false, place + 1))
        }
    }
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, false);
    let slow = Slow(&storage, core::cell::Cell::new(0));
    assert_eq!(clear(&slow, &folder(1)), Err(ERR_BUSY));
    assert_eq!(slow.1.get(), CLEAR_STEPS);
    // The card is as it is; a storage that gets done is done.
    assert_eq!(clear(&storage, &folder(1)), Ok(()));
    assert!(!exists(&storage, &folder(1)));
}
