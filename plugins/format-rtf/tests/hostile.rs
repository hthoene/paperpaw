//! Documents made to do harm, or damaged: nothing panics, and what is
//! allocated stays small whatever the file claims.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::{picture, rtf};
use paperpaw_plugin_format_rtf::RtfDocument;
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

/// Reads the document from memory and throws what is written away, so only
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

const PATH: &str = "books/x.rtf";
const CACHE: &str = ".paperpaw/plugins/format-rtf/book";

fn options() -> OpenOptions {
    OpenOptions::default().image_fit(Fit::screen(480, 800))
}

/// Opens, and reads every block, picture and contents entry.
fn read_all(storage: &dyn Storage) {
    let Ok(document) = RtfDocument::open_converted(storage, PATH, CACHE, &options()) else {
        return;
    };
    let mut at = Locator::new(0, 0);
    for _ in 0..50_000 {
        match document.block_at(storage, at) {
            Ok(Some(block)) => {
                if let Some(image) = &block.image {
                    let _ = document.prepare_image(storage, image);
                }
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
    let body = format!(
        "{{\\stylesheet{{\\s1 heading 1;}}}}{}\\pard {}\\par",
        "\\pard\\s1 T\\par\\pard The \\b wind\\b0  came over \\u8364? the \\'e9 hills.\\line More.\\par\r\n"
            .repeat(300),
        picture(30, 20),
    );
    rtf(&body)
}

fn memory(bytes: Vec<u8>) -> MemoryStorage {
    MemoryStorage::new().with(PATH, bytes)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn random_bytes_are_no_document(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        read_all(&memory(bytes));
    }

    #[test]
    fn random_bytes_after_the_head_do_no_harm(bytes in proptest::collection::vec(any::<u8>(), 0..8192)) {
        let mut document = b"{\\rtf1\\ansi ".to_vec();
        document.extend(bytes);
        read_all(&memory(document));
    }

    #[test]
    fn random_control_words_and_braces_do_no_harm(
        parts in proptest::collection::vec(
            prop_oneof![
                Just("{".to_string()),
                Just("}".to_string()),
                Just("\\*".to_string()),
                Just("\\par ".to_string()),
                Just("\\u-5000?".to_string()),
                Just("\\'f".to_string()),
                Just("\\bin9 ".to_string()),
                Just("\\pict\\pngblip ".to_string()),
                Just("\\cell ".to_string()),
                Just("\\row ".to_string()),
                "[a-z0-9\\\\ -]{0,12}",
            ],
            0..400,
        ),
    ) {
        let mut document = b"{\\rtf1 ".to_vec();
        document.extend(parts.concat().into_bytes());
        read_all(&memory(document));
    }

    #[test]
    fn a_document_with_damage_does_no_harm(
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
}

#[test]
fn a_long_document_is_converted_in_little_memory() {
    let body = "\\pard\\s1 Chapter\\par\\pard The {\\b wind} came \\i over\\i0  the \\'e9 hills \\u8364? and over the sea.\\par\r\n"
        .repeat(8000);
    let document = rtf(&body);
    assert!(document.len() > 800_000);
    let storage = Discarding {
        inner: memory(document),
        cache: String::from(".paperpaw/plugins/format-rtf"),
    };
    let base = reset_peak();
    let document = RtfDocument::open_converted(&storage, PATH, CACHE, &options()).unwrap();
    let used = peak_over(base);
    println!("converting took {used} bytes");
    assert!(used < 64 * 1024, "converting took {used} bytes");
    drop(document);
}

#[test]
fn a_block_is_read_in_little_memory() {
    let storage = memory(valid());
    let document = RtfDocument::open_converted(&storage, PATH, CACHE, &options()).unwrap();
    let base = reset_peak();
    let block = document
        .block_at(&storage, Locator::new(0, 0))
        .unwrap()
        .unwrap();
    let used = peak_over(base);
    println!("a block took {used} bytes");
    assert!(used < 8 * 1024, "reading a block took {used} bytes");
    drop(block);
}

#[test]
fn a_picture_costs_its_header_not_its_size() {
    // 2 MB of hexadecimal digits that are no picture, and one that is.
    let junk = format!("{{\\pict\\pngblip {}}}", "ab".repeat(1_000_000));
    let storage = Discarding {
        inner: memory(rtf(&format!("\\pard A{junk}B{}C\\par", picture(30, 20)))),
        cache: String::from(".paperpaw/plugins/format-rtf"),
    };
    let base = reset_peak();
    read_all(&storage);
    let used = peak_over(base);
    assert!(used < 64 * 1024, "{used}");
}

#[test]
fn the_size_a_file_claims_costs_nothing() {
    for body in [
        "\\bin2000000000 ".to_string(),
        "\\bin99999999999999999999 x".to_string(),
        "\\u99999999999999999999?".to_string(),
        "\\uc99999999999 \\u65?AAAA".to_string(),
        format!("\\{}1 x", "a".repeat(100_000)),
        format!("\\par{}", "9".repeat(100_000)),
    ] {
        let storage = memory(rtf(&body));
        let base = reset_peak();
        read_all(&storage);
        assert!(peak_over(base) < 64 * 1024, "{}", peak_over(base));
    }
}

#[test]
fn a_document_of_nothing_but_braces_and_words() {
    for body in [
        "{".repeat(200_000),
        "}".repeat(200_000),
        "{\\b ".repeat(50_000),
        "{\\*\\x ".repeat(50_000),
        "\\par".repeat(50_000),
        "\\cell".repeat(50_000),
        "\\row\\trowd".repeat(30_000),
        "\\'".repeat(50_000),
        "\\u-1?".repeat(50_000),
        "{\\pict\\pngblip 00}".repeat(5_000),
        "{\\pict\\pngblip ".repeat(5_000),
        "{\\stylesheet{\\s1 heading 1;}".repeat(5_000),
        "\\s1 \\outlinelevel0 x\\par".repeat(20_000),
    ] {
        let storage = memory(rtf(&body));
        let base = reset_peak();
        read_all(&storage);
        assert!(peak_over(base) < 6 * 1024 * 1024, "{}", peak_over(base));
    }
}

#[test]
fn a_file_larger_than_the_limit_is_refused_without_reading_it() {
    struct Huge;
    impl Storage for Huge {
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
            panic!("read")
        }
        fn size(&self, _: &str) -> Result<u64, StorageError> {
            Ok(u64::MAX)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }
    assert!(RtfDocument::open_converted(&Huge, PATH, CACHE, &options()).is_err());
}
