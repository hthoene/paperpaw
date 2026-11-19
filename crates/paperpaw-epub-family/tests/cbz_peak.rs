//! The heap the comic extras ask for on a page of the X4's size (480 x 800):
//! finding the panels, drawing a panel, and reading a large `ComicInfo.xml`.
//! This binary's allocator counts the live bytes of the allocations made
//! outside the in-memory storage (its files are the card, not the heap) and
//! notes the peak since reset (the only test in it, so no other test counts).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_epub_family::{CbzDocument, PANELS};
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::image::PngWriter;
use paperpaw_sdk::{DirEntry, MemoryStorage, OpenOptions, Storage, StorageError};
use paperpaw_zip::testing::ZipBuilder;

use std::cell::Cell;

thread_local! {
    /// Inside a call of the storage: what it allocates is the card's.
    static IN_STORAGE: Cell<bool> = const { Cell::new(false) };
}

fn counted(size: usize) -> bool {
    size < usize::MAX && !IN_STORAGE.get()
}

/// The in-memory storage as a card: its calls are not counted.
struct Card(MemoryStorage);

impl Card {
    fn quiet<T>(&self, f: impl FnOnce(&MemoryStorage) -> T) -> T {
        IN_STORAGE.set(true);
        let value = f(&self.0);
        IN_STORAGE.set(false);
        value
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.quiet(|s| s.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.quiet(|s| s.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.quiet(|s| s.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.quiet(|s| s.append(path, data))
    }
    fn list(&self, path: &str, each: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.quiet(|s| s.list(path, each))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.quiet(|s| s.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.quiet(|s| s.rename(from, to))
    }
}

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

fn grow(by: usize) {
    let live = LIVE.fetch_add(by, Ordering::Relaxed) + by;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn shrink(by: usize) {
    LIVE.fetch_sub(by.min(LIVE.load(Ordering::Relaxed)), Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if counted(layout.size()) {
            grow(layout.size());
        }
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if counted(layout.size()) {
            grow(layout.size());
        }
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if counted(layout.size()) {
            shrink(layout.size());
        }
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if counted(layout.size()) {
            shrink(layout.size());
        }
        if counted(new_size) {
            grow(new_size);
        }
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The peak of `f` above what is live when it starts.
fn peak<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let start = LIVE.load(Ordering::Relaxed);
    PEAK.store(start, Ordering::Relaxed);
    let value = f();
    (value, PEAK.load(Ordering::Relaxed).saturating_sub(start))
}

#[test]
fn comic_extras_stay_within_a_few_kib_on_a_page_of_the_x4() {
    let scratch = MemoryStorage::new();
    let mut png = PngWriter::new(&scratch, "p.png", 480, 800).unwrap();
    for y in 0..800u32 {
        let mut row = vec![255u8; 480];
        if (20..380).contains(&y) {
            row[20..230].fill(30);
            row[250..460].fill(90);
        }
        if (400..780).contains(&y) {
            row[20..460].fill(60);
        }
        png.row(&scratch, "p.png", &row).unwrap();
    }
    let page = paperpaw_sdk::read_file(&scratch, "p.png", 4 << 20).unwrap();
    let info = format!(
        "<ComicInfo><Title>T</Title><Summary>{}</Summary><Pages>{}</Pages></ComicInfo>",
        "word ".repeat(20_000),
        "<Page Image=\"1\" ImageSize=\"123\"/>".repeat(3_000)
    );
    let zip = ZipBuilder::new()
        .stored("1.png", page)
        .stored("ComicInfo.xml", info.as_bytes().to_vec())
        .build();
    let small = ZipBuilder::new()
        .stored(
            "1.png",
            paperpaw_sdk::read_file(&scratch, "p.png", 4 << 20).unwrap(),
        )
        .stored(
            "ComicInfo.xml",
            b"<ComicInfo><Title>T</Title></ComicInfo>".to_vec(),
        )
        .build();
    let storage = Card(MemoryStorage::new().with("c.cbz", zip).with("s.cbz", small));
    let on = OpenOptions::default().settings(vec![(PANELS.into(), "true".into())]);
    // The book's pages and their lists are made first, so what is measured
    // is the extras alone.
    let plain = CbzDocument::open(&storage, "c.cbz").unwrap();
    plain.block_at(&storage, Locator::new(0, 0)).unwrap();
    CbzDocument::open(&storage, "s.cbz").unwrap();
    let (_, baseline) = peak(|| CbzDocument::open_options(&storage, "s.cbz", &on).unwrap());
    let (doc, with_info) = peak(|| CbzDocument::open_options(&storage, "c.cbz", &on).unwrap());
    let opened = with_info.saturating_sub(baseline);
    let (block, found) = peak(|| doc.block_at(&storage, Locator::new(0, 1)).unwrap().unwrap());
    let image = block.image.unwrap();
    // A panel is a part of the page's picture: showing it makes nothing.
    let (made, drawn) = peak(|| doc.prepare_image(&storage, &image));
    assert!(made.is_none());
    let (part, asked) = peak(|| {
        doc.extended(
            &storage,
            paperpaw_epub_family::IMAGE_PART,
            &[0, 0, 0, 0, 1, 0, 0, 0],
        )
    });
    assert_eq!(part.map(|part| part.len()), Some(16));
    let drawn = drawn.max(asked);
    println!(
        "open: {baseline} B with a small ComicInfo.xml (a stored one: a deflated one adds the archive reader's 32 KiB window, as before), {opened} B more with a 160 KiB one, find panels: {found} B, a panel's part: {drawn} B"
    );
    assert!(opened < 12 * 1024, "{opened}");
    assert!(found < 12 * 1024, "{found}");
    assert!(drawn < 14 * 1024, "{drawn}");
}
