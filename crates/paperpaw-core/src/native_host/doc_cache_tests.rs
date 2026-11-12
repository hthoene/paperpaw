extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;

fn id(conversion: u64) -> Identity<'static> {
    Identity {
        book: "00000000000000aa",
        plugin: "epub",
        version: "1.2.0",
        conversion,
        content: "4 aa;-",
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
        complete(storage, &id(conversion), 4).unwrap();
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
    complete(&storage, &id(1), 7).unwrap();
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
    complete(&storage, &id(1), 7).unwrap();
    assert!(!exists(&storage, &folder(1)));
}

#[test]
fn a_cache_without_a_current_stamp_is_removed_before_it_is_used() {
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, false);
    let page = std::format!("{}/pages/1", folder(1));
    assert_eq!(
        prepare(&storage, &id(1), &nothing_pinned).map(|p| p.folder),
        Ok(folder(1))
    );
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
    assert_eq!(
        prepare(&storage, &id(1), &pinned).map(|p| p.folder),
        Ok(folder(1))
    );
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
fn room_is_made_from_unstamped_folders_before_finished_ones() {
    let storage = MemoryStorage::new();
    // The oldest is the reader's finished conversion; the newer one holds
    // only what a `details` open left (no stamp).
    made_cache(&storage, 1, true);
    made_cache(&storage, 2, false);
    prepare(&storage, &id(3), &nothing_pinned).unwrap();
    assert!(exists(&storage, &folder(1)), "the finished one stays");
    assert!(
        !exists(&storage, &folder(2)),
        "the unstamped one goes first"
    );
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
    assert_eq!(
        prepare(&storage, &id(3), &both).map(|p| p.folder),
        Err(ERR_BUSY)
    );
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

#[test]
fn a_stamp_says_what_the_cache_holds_and_a_changed_book_or_css_file_is_another_conversion() {
    let storage = MemoryStorage::new();
    made_cache(&storage, 1, false);
    complete(&storage, &id(1), 12_345).unwrap();
    assert_eq!(current(&storage, &id(1)), Ok(Some(12_345)));
    // Another book at the path (its size or partial MD5), or an edited CSS file:
    // the stamp is not current and the cache is removed before it is used.
    for other in ["5 aa;-", "4 ab;-", "4 aa;1 0 9", "4 aa;-;"] {
        let changed = Identity {
            content: other,
            ..id(1)
        };
        assert_eq!(current(&storage, &changed), Ok(None), "{other}");
    }
    let changed = Identity {
        content: "4 ab;-",
        ..id(1)
    };
    let made = prepare(&storage, &changed, &nothing_pinned).unwrap();
    assert!(made.fresh && made.bytes.is_none());
    assert!(!exists(&storage, &folder(1)));
    // A finished cache found again says its bytes and is not fresh.
    made_cache(&storage, 2, true);
    let found = prepare(&storage, &id(2), &nothing_pinned).unwrap();
    assert_eq!((found.bytes, found.fresh), (Some(4), false));
    // A damaged count is no stamp.
    let stamp = std::format!("{}/{STAMP}", folder(2));
    let good = storage.get(&stamp).unwrap();
    let lines = good.len() - 2;
    let mut no_digit = good[..lines].to_vec();
    no_digit.extend_from_slice(b"x\n");
    let mut no_end = good.clone();
    no_end.pop();
    for bad in [&good[..lines], &no_digit[..], &no_end[..]] {
        storage.write(&stamp, bad).unwrap();
        assert_eq!(current(&storage, &id(2)), Ok(None));
    }
}

#[test]
fn what_the_content_line_tells_changes_with_the_book_and_the_css_file() {
    let storage = MemoryStorage::new();
    storage.write("books/a.epub", b"first edition").unwrap();
    storage.write("style.css", b"p { color: red }").unwrap();
    let first = content(&storage, "books/a.epub", "style.css").unwrap();
    assert_eq!(
        first,
        content(&storage, "books/a.epub", "style.css").unwrap()
    );
    storage.write("books/a.epub", b"second edition").unwrap();
    let book = content(&storage, "books/a.epub", "style.css").unwrap();
    assert_ne!(book, first);
    // The same size, other bytes: the partial MD5 tells.
    storage.write("books/a.epub", b"SECOND EDITION").unwrap();
    assert_ne!(
        content(&storage, "books/a.epub", "style.css").unwrap(),
        book
    );
    storage.write("style.css", b"p { color: red }\n").unwrap();
    assert_ne!(
        content(&storage, "books/a.epub", "style.css").unwrap(),
        book
    );
    // The same size, other bytes in the CSS file.
    let css = content(&storage, "books/a.epub", "style.css").unwrap();
    storage.write("style.css", b"p { color: blue }\n").unwrap();
    assert_ne!(content(&storage, "books/a.epub", "style.css").unwrap(), css);
    // No CSS file and no book are lines of their own.
    assert!(
        content(&storage, "books/none.epub", "")
            .unwrap()
            .ends_with(";-")
    );
}

/// A storage that goes dead (every write, rename, sync and create fails) after
/// `left` of them.
struct Cut<'a> {
    inner: &'a MemoryStorage,
    left: core::cell::Cell<u32>,
}
impl Cut<'_> {
    fn step(&self) -> Result<(), StorageError> {
        match self.left.get() {
            0 => Err(StorageError::Io),
            n => {
                self.left.set(n - 1);
                Ok(())
            }
        }
    }
}
impl Storage for Cut<'_> {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.inner.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.step()?;
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.step()?;
        self.inner.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.inner.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.step()?;
        self.inner.remove(p)
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
        self.step()?;
        self.inner.rename(a, b)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.step()?;
        self.inner.create_dir(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.step()?;
        self.inner.sync()
    }
}

#[test]
fn a_cut_after_any_step_of_the_stamp_leaves_the_old_stamp_or_none_never_a_wrong_one() {
    // The steps of `complete`: sync, create `.host`, write, sync, rename, sync.
    for steps in 0..=6 {
        // First stamp, no earlier one: a cut leaves none, or the whole stamp.
        let storage = MemoryStorage::new();
        made_cache(&storage, 1, false);
        let cut = Cut {
            inner: &storage,
            left: core::cell::Cell::new(steps),
        };
        let done = complete(&cut, &id(1), 9);
        assert_eq!(done.is_ok(), steps >= 6, "{steps}");
        let found = current(&storage, &id(1)).unwrap();
        assert!(found.is_none() || found == Some(9), "{steps}: {found:?}");
        // Without the whole stamp, the next open removes the cache.
        if found.is_none() {
            let made = prepare(&storage, &id(1), &nothing_pinned).unwrap();
            assert!(made.fresh, "{steps}");
            let page = std::format!("{}/pages/1", folder(1));
            assert!(storage.get(&page).is_none(), "{steps}");
        }
        // Another stamp over a finished cache: the old one stays or the new
        // one is whole (never a mixture).
        let storage = MemoryStorage::new();
        made_cache(&storage, 1, true);
        let cut = Cut {
            inner: &storage,
            left: core::cell::Cell::new(steps),
        };
        let _ = complete(&cut, &id(1), 9);
        let found = current(&storage, &id(1)).unwrap();
        assert!(found == Some(4) || found == Some(9), "{steps}: {found:?}");
    }
}

#[test]
fn a_folder_book_is_told_by_its_top_level_and_not_by_the_time_of_the_folder() {
    let storage = MemoryStorage::new();
    storage.write("books/folder/a.txt", b"one").unwrap();
    let first = content(&storage, "books/folder", "").unwrap();
    assert!(first.starts_with('d'), "{first}");
    assert_eq!(first, content(&storage, "books/folder", "").unwrap());
    storage.write("books/folder/a.txt", b"other").unwrap();
    let edited = content(&storage, "books/folder", "").unwrap();
    assert_ne!(edited, first);
    storage.write("books/folder/b.txt", b"x").unwrap();
    assert_ne!(content(&storage, "books/folder", "").unwrap(), edited);
}
