//! A refused allocation while a note of a vault is opened and read is an
//! `OutOfMemory` answer: never an abort or a panic, never output that differs
//! from a clean run, never a cache a later try cannot repair. The n-th
//! allocation of the Markdown conversion (front matter, wikilinks, embeds
//! found by a walk of the vault, tables, callouts) is refused, for each n.
//!
//! The only test in its binary: the allocator below is the process's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_plugin_notes_vault::MarkdownDocument;
use paperpaw_sdk::document::{Block, Document, DocumentError, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

std::thread_local! {
    /// Allocations counted since the sweep began (not those inside storage).
    static COUNT: Cell<u64> = const { Cell::new(0) };
    /// The allocation to refuse, 1-based; 0 none.
    static REFUSE: Cell<u64> = const { Cell::new(0) };
    /// Above 0: the storage (the host's) is allocating, not the plugin.
    static PAUSED: Cell<u32> = const { Cell::new(0) };
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

struct Refusing;

fn refused() -> bool {
    ARMED
        .try_with(|armed| {
            if !armed.get() || PAUSED.with(Cell::get) > 0 {
                return false;
            }
            let count = COUNT.with(|count| {
                count.set(count.get() + 1);
                count.get()
            });
            count == REFUSE.with(Cell::get)
        })
        .unwrap_or(false)
}

// SAFETY: allocations and frees are the system allocator's; a refusal is a
// null return, which `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refused() {
            trace();
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's layout contract passes through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refused() {
            trace();
            return core::ptr::null_mut();
        }
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refused() {
            trace();
            return core::ptr::null_mut();
        }
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

/// Where the refusal happened, when `ALLOC_TRACE` is set (the process aborts
/// where the code cannot take it).
fn trace() {
    PAUSED.with(|paused| paused.set(paused.get() + 1));
    if std::env::var_os("ALLOC_TRACE").is_some() {
        eprintln!(
            "refused allocation {}:\n{}",
            REFUSE.with(Cell::get),
            std::backtrace::Backtrace::force_capture()
        );
    }
    PAUSED.with(|paused| paused.set(paused.get() - 1));
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// A storage whose own allocations are the host's, not counted or refused.
struct Host(MemoryStorage);

fn host<T>(call: impl FnOnce() -> T) -> T {
    PAUSED.with(|paused| paused.set(paused.get() + 1));
    let result = call();
    PAUSED.with(|paused| paused.set(paused.get() - 1));
    result
}

impl Storage for Host {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        host(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        host(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        host(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        host(|| self.0.append(path, data))
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        host(|| self.0.write_at(path, offset, data))
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        host(|| self.0.truncate(path, len))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // The entries are the storage's; what the visitor does with them is
        // the plugin's heap, and is counted and refused.
        let mut entries: Vec<(String, bool, u64, Option<i64>)> = Vec::new();
        host(|| {
            self.0.list(dir, &mut |e| {
                entries.push((e.name.to_string(), e.is_dir, e.size, e.modified));
            })
        })?;
        for (name, is_dir, size, modified) in &entries {
            visit(DirEntry::new(name, *is_dir, *size).with_modified(*modified));
        }
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        host(|| self.0.remove(path))
    }
    fn modified(&self, path: &str) -> Option<i64> {
        host(|| self.0.modified(path))
    }
    fn exists(&self, path: &str) -> bool {
        host(|| self.0.exists(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        host(|| self.0.rename(from, to))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        host(|| self.0.create_dir(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        host(|| self.0.remove_dir_all(path))
    }
}

/// What a reader sees: the title and every block (section, text, end).
type Seen = (Option<String>, Vec<(u32, String, u32)>);

fn read(storage: &Host) -> Result<Seen, DocumentError> {
    let doc = MarkdownDocument::open(storage, "v/Notes/n.md", true)?;
    let mut blocks = Vec::new();
    for section in 0..doc.sections() {
        let mut at = Locator::new(section, 0);
        while let Some(block) = doc.block_at(storage, at)? {
            let Block { text, end, .. } = block;
            assert!(end > at.offset, "no progress at {at:?}");
            at = Locator::new(section, end);
            host(|| blocks.push((section, text, end)));
        }
    }
    for index in 0..doc.toc_len(storage) {
        let _ = doc.toc_entry(storage, index);
    }
    let _ = doc.link_target(storage, "#big-part");
    Ok((host(|| doc.metadata().title.clone()), blocks))
}

const NOTE: &str = "---\ntitle: x\ntags: [a, \"b c\", nested/t]\n---\n# A title\n\n\
A paragraph with *emphasis*, **strong**, `code`, [a link](https://example.org/x), \
[[Other|a label]] and [[Other#Big Part]] ^ab12c\n\n\
![[Cat.png]] ![[Cat.png|200]] ![[gone.png]] ![[Other]] ![alt](pic.png)\n\n\
> [!warning] Careful now\n> The body.\n\n\
| a | *b* |\n|---|:-:|\n| 1 | [[n|x]] |\n\n\
- one\n- two\n\n1. first\n2. second\n\n```\ncode block\n```\n\n---\n\n## Big Part\n\nMore text.\n";

fn fresh() -> Host {
    Host(
        MemoryStorage::new()
            .with("v/.obsidian/app.json", b"{}".to_vec())
            .with("v/Notes/n.md", NOTE.as_bytes().to_vec())
            .with("v/Notes/Other.md", b"# Other\n".to_vec())
            // The pictures are not on the storage: a picture that is there and
            // cannot be decoded is the format plugin's (it words the decoder's
            // error without asking for room), and out of this sweep.
            .with("v/Files/deep/Dog.txt", b"x".to_vec()),
    )
}

#[test]
fn a_refused_allocation_in_a_vault_note_is_out_of_memory() {
    // A clean run, counting what the plugin allocates.
    ARMED.with(|armed| armed.set(true));
    REFUSE.with(|refuse| refuse.set(0));
    COUNT.with(|count| count.set(0));
    let clean = read(&fresh()).expect("a clean run reads");
    let total = COUNT.with(Cell::get);
    ARMED.with(|armed| armed.set(false));
    assert!(!clean.1.is_empty(), "the note has blocks");
    assert!(total > 100, "{total} allocations is not representative");
    let mut refusals = 0;
    for nth in 1..=total {
        let storage = fresh();
        COUNT.with(|count| count.set(0));
        REFUSE.with(|refuse| refuse.set(nth));
        ARMED.with(|armed| armed.set(true));
        let result = read(&storage);
        ARMED.with(|armed| armed.set(false));
        match result {
            Err(DocumentError::OutOfMemory) => refusals += 1,
            Err(other) => panic!("allocation {nth} refused: {other:?}"),
            Ok(seen) => assert_eq!(seen, clean, "allocation {nth} refused"),
        }
        // Nothing half-made stays: the same storage, nothing refused, reads
        // as a clean run does.
        assert_eq!(
            read(&storage).expect("a retry reads"),
            clean,
            "retry after allocation {nth}"
        );
    }
    assert!(refusals > 0, "no refusal answered OutOfMemory");
}
