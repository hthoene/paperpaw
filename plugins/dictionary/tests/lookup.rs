//! The dictionary with the real kernel: a damaged dictionary is not read
//! again with every word, the caches of dictionaries removed go, and the
//! good ones are still indexed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// The damaged dictionary's index.
const DAMAGED: &str = "dictionaries/bad/bad.idx";
/// The cache of a dictionary no longer there.
const OLD_CACHE: &str = ".paperpaw/plugins/dictionary/0123456789abcdef/idx.qbi";

/// A card that counts the reads of the damaged dictionary's index.
struct Counted {
    inner: MemoryStorage,
    reads: Rc<Cell<usize>>,
}

impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path == DAMAGED {
            self.reads.set(self.reads.get() + 1);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }
}

/// A StarDict `.ifo` with `words` words.
fn ifo(name: &str, words: usize) -> String {
    format!("StarDict's dict ifo file\nversion=2.4.2\nbookname={name}\nwordcount={words}\n")
}

/// An index entry: the word, then where its definition is and how long.
fn idx(word: &str, offset: u32, size: u32) -> Vec<u8> {
    let mut entry = word.as_bytes().to_vec();
    entry.push(0);
    entry.extend_from_slice(&offset.to_be_bytes());
    entry.extend_from_slice(&size.to_be_bytes());
    entry
}

/// A good dictionary and a damaged one (its index ends after one of the
/// five words it says it has), and the cache of one removed.
fn card() -> MemoryStorage {
    MemoryStorage::new()
        .with("dictionaries/good/good.ifo", ifo("Good", 1))
        .with("dictionaries/good/good.idx", idx("quire", 0, 18))
        .with("dictionaries/good/good.dict", "twenty-four sheets")
        .with("dictionaries/bad/bad.ifo", ifo("Bad", 5))
        .with(DAMAGED, idx("quire", 0, 5))
        .with("dictionaries/bad/bad.dict", "sheet")
        .with(OLD_CACHE, "old")
}

fn settle(kernel: &mut Kernel, mut effects: Effects) {
    let mut now = 0;
    for _ in 0..500 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 10_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    kernel.render(&mut |_, _| {});
}

/// How many dictionaries have a cache: an index made, or a note that it
/// could not be.
fn caches(kernel: &Kernel) -> usize {
    let storage = kernel.storage();
    let dir = ".paperpaw/plugins/dictionary";
    let mut n = 0;
    storage
        .list(dir, &mut |entry| {
            let cache = format!("{dir}/{}", entry.name);
            let made = storage.exists(&format!("{cache}/idx.qbi"))
                || storage.exists(&format!("{cache}/failed"));
            n += usize::from(made);
        })
        .unwrap();
    n
}

#[test]
fn a_damaged_dictionary_is_read_once() {
    let reads = Rc::new(Cell::new(0));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Counted {
            inner: card(),
            reads: Rc::clone(&reads),
        })
        .user_config("[core]\nlanguage = en\n")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    settle(&mut kernel, effects);
    let word = Params::new().with(params::WORD, "quire");
    let effects = kernel.open_screen(Instant(0), paperpaw_plugin_dictionary::POPUP, word.clone());
    settle(&mut kernel, effects);
    assert!(reads.get() > 0);
    // The good one indexed, ready for lookups.
    assert_eq!(
        caches(&kernel),
        2,
        "a cache each, the damaged one saying so"
    );
    assert!(!kernel.storage().exists(OLD_CACHE), "the old cache stayed");

    let effects = kernel.run_action(Instant(0), actions::BACK);
    settle(&mut kernel, effects);
    reads.set(0);
    let effects = kernel.open_screen(Instant(0), paperpaw_plugin_dictionary::POPUP, word);
    settle(&mut kernel, effects);
    assert_eq!(reads.get(), 0, "the damaged index was read again");
}
