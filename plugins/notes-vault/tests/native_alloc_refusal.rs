//! Every allocation of the model — the lines read, the excerpts, the tags, the
//! windows of a list, the entries a source offers, the walk of the vault and
//! the tagging, the state — may be refused: each ends in `NoMemory` or
//! succeeds, never aborts. The storages are not the plugin's heap: their own
//! allocations are out of the count (`fixture`).
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_plugin_notes_vault::native_model as model;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

std::thread_local! {
    /// The allocation to refuse, counted from 1; 0 turns counting off.
    static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

struct Failing;

fn refuse() -> bool {
    let at = REFUSE_AT.try_with(Cell::get).unwrap_or(0);
    if at == 0 {
        return false;
    }
    COUNT
        .try_with(|count| {
            count.set(count.get() + 1);
            count.get() == at
        })
        .unwrap_or(false)
}

// SAFETY: every call forwards to `System` unchanged, or returns null to refuse.
unsafe impl GlobalAlloc for Failing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout contract passes through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOC: Failing = Failing;

/// Builds test input or does a storage's work while nothing is refused or
/// counted.
fn fixture<T>(build: impl FnOnce() -> T) -> T {
    let at = REFUSE_AT.with(|a| a.replace(0));
    let value = build();
    REFUSE_AT.with(|a| a.set(at));
    value
}

/// Runs `run` once to count its allocations, then once more with each of them
/// refused in turn: a refused run ends in `Err(NoMemory)` or succeeds anyway.
fn sweep(name: &str, mut run: impl FnMut() -> Result<(), Error>) {
    REFUSE_AT.with(|a| a.set(usize::MAX));
    COUNT.with(|c| c.set(0));
    let first = run();
    let total = COUNT.with(Cell::get);
    REFUSE_AT.with(|a| a.set(0));
    assert_eq!(first, Ok(()), "{name}: the unrefused run must succeed");
    assert!(
        total > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
    for n in 1..=total {
        COUNT.with(|c| c.set(0));
        REFUSE_AT.with(|a| a.set(n));
        let result = run();
        REFUSE_AT.with(|a| a.set(0));
        if let Err(error) = result {
            assert_eq!(error, Error::NoMemory, "{name}: allocation {n} of {total}");
        }
    }
    assert_eq!(run(), Ok(()), "{name}: the run recovers after refusals");
}

/// A memory storage whose own allocations are not the plugin's.
struct Quiet(MemoryStorage);

impl Quiet {
    fn new(build: impl FnOnce() -> MemoryStorage) -> Self {
        Self(fixture(build))
    }
}

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        fixture(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        fixture(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // The entries are collected on the storage's side; what the visitor
        // does with them is the plugin's heap, and is counted and refused.
        let mut entries: Vec<(String, bool, u64, Option<i64>)> = Vec::new();
        fixture(|| {
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
        fixture(|| self.0.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        fixture(|| self.0.rename(from, to))
    }
    fn exists(&self, path: &str) -> bool {
        fixture(|| self.0.exists(path))
    }
}

fn vault() -> MemoryStorage {
    let mut storage = MemoryStorage::new();
    for n in 0..6 {
        let dir = ["", "A/", "A/B/"][n % 3];
        storage = storage.with(
            &format!("v/{dir}Note {n}.md"),
            format!("---\ntags: [t{n}, shared]\n---\n# Title\n\nSome **words** and [[Link|a label]] #inline{n}\n"),
        );
    }
    storage.with("v/A/other.txt", "not a note")
}

#[test]
fn reading_lines_excerpts_and_tags_survive_refused_allocations() {
    let vault = Quiet::new(vault);
    sweep("lines", || {
        model::lines::each(&vault, "v/Note 0.md", &mut |_| true)
    });
    sweep("excerpt", || {
        model::note::excerpt("---\ntags: [a]\n---\n# H\n\n> **Bold** and [[Other|a link]] with [x](https://y.org) `code`.\n")
            .map(|_| ())
    });
    let long = "word ".repeat(100);
    sweep("a long excerpt", || model::note::excerpt(&long).map(|_| ()));
    sweep("excerpt of a file", || {
        model::note::excerpt_of_file(&vault, "v/Note 0.md").map(|_| ())
    });
    sweep("tags of text", || {
        model::tags::of_text("---\ntags: [a, \"b c\"]\n---\n#x #y/z `#no`\n").map(|_| ())
    });
    sweep("tags of a file", || {
        model::tags::of_file(&vault, "v/Note 1.md").map(|_| ())
    });
}

#[test]
fn windows_and_sources_survive_refused_allocations() {
    let vault = Quiet::new(vault);
    let data = Quiet::new(|| {
        MemoryStorage::new()
            .with(
                "index.txt",
                "5\tv/Note 0.md\n7\tv/A/Note 1.md\n1\tv/A/B/Note 2.md\n",
            )
            .with(
                "tags.txt",
                "v/Note 0.md\tt0 shared\nv/A/Note 1.md\tt1 shared\n",
            )
    });
    let sources = [
        model::source::Source::Folder("v".to_string()),
        model::source::Source::Notes {
            folder: "v/A".to_string(),
            query: "note".to_string(),
        },
        model::source::Source::Tag("shared".to_string()),
    ];
    for source in &sources {
        sweep(&format!("{source:?}"), || {
            let mut feed = |emit: &mut model::page::Emit<'_>| source.feed(&vault, &data, emit);
            let window = model::page::window(model::page::Order::Recent, None, &mut feed)?;
            assert!(!window.rows.is_empty());
            let last = window.rows.last();
            let mut feed = |emit: &mut model::page::Emit<'_>| source.feed(&vault, &data, emit);
            model::page::window(model::page::Order::Name, last, &mut feed).map(|_| ())
        });
    }
}

#[test]
fn the_walk_the_tagging_and_the_state_survive_refused_allocations() {
    let vault = Quiet::new(vault);
    sweep("the walk, the tags and what is read of them", || {
        let data = Quiet::new(MemoryStorage::new);
        let mut walk = model::index::Indexer::new("v/")?;
        let state = loop {
            match walk.step(&vault, &data)? {
                model::index::Step::More => {}
                model::index::Step::Done(state) => break state,
                model::index::Step::Failed => panic!("the walk failed"),
            }
        };
        assert_eq!(state.notes, 6);
        let mut tagger = model::index::Tagger::new();
        while tagger.step(&vault, &data)? != Some(true) {}
        let (counts, _) = model::index::tag_counts(&data)?;
        assert!(counts.iter().any(|(tag, n)| tag == "shared" && *n == 6));
        model::index::notes_with_tag(&data, "t1", &mut |_| true)?;
        model::index::notes(&data, &mut |_, _| true)?;
        state.write(&data)?;
        assert!(model::index::kept(&data, "v")?.is_some());
        Ok(())
    });
}
