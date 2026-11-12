//! Test-only: the card a test kernel is given, kept over restarts of the
//! kernel (each handle is the same storage), with two books.
#![allow(
    dead_code,
    reason = "shared by several test files; each uses part of it"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the test"
)]

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;
use std::rc::Rc;

pub const ALICE: &str = "books/alice-in-wonderland.txt";
pub const GREGOR: &str = "books/die-verwandlung.txt";
pub const SETUP_DONE: &str = ".paperpaw/plugins/setup/done";

thread_local! {
    static CHANGING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether this thread is inside a call that changes the card's files: the
/// memory it takes or frees is the card's own (a test that counts the
/// kernel's heap leaves it out, `sleep_cycles`).
pub fn changing_card() -> bool {
    CHANGING.with(std::cell::Cell::get)
}

fn changing<T>(change: impl FnOnce() -> T) -> T {
    let was = CHANGING.with(|changing| changing.replace(true));
    let result = change();
    CHANGING.with(|changing| changing.set(was));
    result
}

/// The card, kept over restarts of the kernel.
#[derive(Clone)]
pub struct Card(pub Rc<MemoryStorage>);

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Card")
    }
}

impl Card {
    /// A card with two books; past the first-run setup if `set_up`.
    pub fn new(set_up: bool) -> Self {
        let mut storage = MemoryStorage::new()
            .with(
                ALICE,
                include_str!("../../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(
                GREGOR,
                include_str!("../../../../samples/library/books/die-verwandlung.txt"),
            );
        if set_up {
            storage = storage.with(SETUP_DONE, "");
        }
        Self(Rc::new(storage))
    }

    pub fn text(&self, path: &str) -> Option<String> {
        self.0
            .get(path)
            .map(|data| String::from_utf8(data).expect("UTF-8"))
    }
}

/// Every method is the memory card's own, the defaults too: the kernel and the
/// library see what the card does, not a wrapper's "unsupported".
impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        changing(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        changing(|| self.0.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        changing(|| self.0.remove(path))
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        changing(|| self.0.rename(from, to))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        changing(|| self.0.create_dir(path))
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        changing(|| self.0.write_at(path, offset, data))
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        changing(|| self.0.truncate(path, len))
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        changing(|| self.0.remove_dir_all(path))
    }
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.0.entry_identity(path)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
    fn modified(&self, path: &str) -> Option<i64> {
        self.0.modified(path)
    }
    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.0.same_entry(a, b)
    }
    fn free_space(&self) -> Option<u64> {
        self.0.free_space()
    }
    fn total_space(&self) -> Option<u64> {
        self.0.total_space()
    }
    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.0.local_path(path, with);
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.0.list_from(dir, from, visit)
    }
    fn list_bounded(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.0.list_bounded(dir, from, visit)
    }
    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.0.remove_listed(dir, place)
    }
    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        self.0.remove_tree_step(root, keep_root, place)
    }
}
