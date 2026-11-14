//! Books made to do harm, or damaged: nothing panics, and what is
//! allocated stays small whatever the headers claim.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::{Builder, png};
use paperpaw_plugin_format_mobi::MobiDocument;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::{DirEntry, MemoryStorage, OpenOptions, Storage, StorageError};
use proptest::prelude::*;

/// Counts what the running thread allocates, and the most at one time
/// since the last [`reset_peak`]. Per thread, so tests that run side by
/// side do not count each other.
struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn grow(by: usize) {
    LIVE.with(|live| {
        live.set(live.get() + by as isize);
        PEAK.with(|peak| peak.set(peak.get().max(live.get())));
    });
}

fn shrink(by: usize) {
    LIVE.with(|live| live.set(live.get() - by as isize));
}

// SAFETY: forwards every call to the system allocator, which does the work.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrink(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if new > layout.size() {
            grow(new - layout.size());
        } else {
            shrink(layout.size() - new);
        }
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Starts measuring: what is live now is the base.
fn reset_peak() -> usize {
    let live = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(live));
    live.max(0) as usize
}

fn peak_over(base: usize) -> usize {
    (PEAK.with(Cell::get).max(0) as usize).saturating_sub(base)
}

/// Reads the book from memory and throws what is written away, so only
/// what the plugin itself holds is counted.
struct Discarding {
    inner: MemoryStorage,
    cache: String,
}

impl Storage for Discarding {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.starts_with(&self.cache) {
            Ok(())
        } else {
            self.inner.write(path, data)
        }
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.starts_with(&self.cache) {
            Ok(())
        } else {
            self.inner.append(path, data)
        }
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.inner.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }
}

const PATH: &str = "books/x.mobi";
const CACHE: &str = ".paperpaw/plugins/format-mobi/book";

fn options() -> OpenOptions {
    OpenOptions::default().image_fit(Fit::screen(480, 800))
}

/// Opens, and reads every block.
fn read_all(storage: &dyn Storage) {
    let Ok(document) = MobiDocument::open_converted(storage, PATH, CACHE, &options()) else {
        return;
    };
    let _ = document.cover(storage);
    let mut at = Locator::new(0, 0);
    for _ in 0..50_000 {
        match document.block_at(storage, at) {
            Ok(Some(block)) => {
                if let Some(image) = &block.image {
                    let _ = document.prepare_image(storage, image);
                }
                let _ = document.links(storage, 0, block.start, block.end);
                at = Locator::new(0, block.end.max(at.offset + 1));
            }
            _ => break,
        }
    }
    for i in 0..document.toc_len(storage).min(100) {
        let _ = document.toc_entry(storage, i);
    }
}

fn valid() -> Vec<u8> {
    let html = format!(
        "{}<p><img recindex=1 /></p>",
        "<h1>T</h1><p>The <b>wind</b> <a filepos=100>came</a> over &amp; the hills.<br/>More.</p>"
            .repeat(300)
    );
    let mut builder = Builder::new(&html);
    builder.pictures = vec![png(30, 20)];
    builder.cover = Some(0);
    builder.build()
}

fn memory(bytes: Vec<u8>) -> MemoryStorage {
    MemoryStorage::new().with(PATH, bytes)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn random_bytes_are_no_book(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        read_all(&memory(bytes));
    }

    #[test]
    fn a_book_with_damage_does_no_harm(
        hits in proptest::collection::vec((0usize..40_000, any::<u8>()), 1..24),
        cut in proptest::option::of(0usize..40_000),
    ) {
        let mut bytes = valid();
        for (at, value) in hits {
            let at = at % bytes.len();
            bytes[at] = value;
        }
        if let Some(cut) = cut {
            bytes.truncate(cut % bytes.len());
        }
        read_all(&memory(bytes));
    }

    #[test]
    fn a_damaged_header_does_no_harm(
        hits in proptest::collection::vec((0usize..600, any::<u8>()), 1..12),
    ) {
        let mut bytes = valid();
        for (at, value) in hits {
            bytes[at % 600] = value;
        }
        read_all(&memory(bytes));
    }
}

/// A header that claims less text than the text records hold: the last
/// record's share is nothing, not a negative length.
#[test]
fn a_text_length_shorter_than_the_records_does_no_harm() {
    let mut bytes = valid();
    // The first record, from the first entry of the record list; the text
    // length is the second word of the PalmDOC header there.
    let first = u32::from_be_bytes(bytes[78..82].try_into().unwrap()) as usize;
    for length in [0u32, 1, 100, 5000] {
        bytes[first + 4..first + 8].copy_from_slice(&length.to_be_bytes());
        read_all(&memory(bytes.clone()));
    }
}

#[test]
fn a_long_book_is_converted_in_little_memory() {
    let html = "<h2>Chapter</h2><p>The <b>wind</b> came <i>over</i> the &amp; hills and over the sea, <a filepos=77>again</a>.</p>"
        .repeat(8000);
    assert!(html.len() > 800_000);
    let storage = Discarding {
        inner: memory(Builder::new(&html).build()),
        cache: String::from(".paperpaw/plugins/format-mobi"),
    };
    let base = reset_peak();
    let document = MobiDocument::open_converted(&storage, PATH, CACHE, &options()).unwrap();
    let used = peak_over(base);
    assert!(used < 96 * 1024, "converting took {used} bytes");
    drop(document);
}

#[test]
fn claims_larger_than_the_file_cost_nothing() {
    let mut bytes = valid();
    // The number of records, the text length, the records' size.
    bytes[76..78].copy_from_slice(&0xffffu16.to_be_bytes());
    let base = reset_peak();
    read_all(&memory(bytes.clone()));
    assert!(peak_over(base) < 64 * 1024, "{}", peak_over(base));
    let mut bytes = valid();
    let first = u32::from_be_bytes(bytes[78..82].try_into().unwrap()) as usize;
    bytes[first + 4..first + 8].copy_from_slice(&u32::MAX.to_be_bytes());
    bytes[first + 8..first + 10].copy_from_slice(&0xffffu16.to_be_bytes());
    bytes[first + 10..first + 12].copy_from_slice(&0xffffu16.to_be_bytes());
    let base = reset_peak();
    read_all(&memory(bytes));
    assert!(peak_over(base) < 64 * 1024, "{}", peak_over(base));
}

#[test]
fn a_book_of_nothing_but_tags_and_entities() {
    for html in [
        "<".repeat(100_000),
        "&".repeat(100_000),
        "<p>".repeat(30_000),
        "&#".repeat(30_000),
        "<a filepos=1>".repeat(30_000),
        format!("<p {}", "a=\"".repeat(20_000)),
        "<img recindex=1 />".repeat(10_000),
        "<b>".repeat(50_000),
    ] {
        let base = reset_peak();
        read_all(&memory(Builder::new(&html).build()));
        assert!(peak_over(base) < 6 * 1024 * 1024, "{}", peak_over(base));
    }
}
