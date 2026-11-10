//! A large library on the X4's storage, measured on the host (#79): the
//! books of a folder (`make-library.py`) copied onto a FAT card in memory
//! with PaperPaw's own FAT code, then the kernel's library walked, read
//! and searched as the library's views do it. For each operation it says
//! the host time, the card's blocks read and written (the basis of an X4
//! estimate: an SD card over SPI reads a block in a few hundred µs) and
//! the heap's peak above what was live before (host sizes: 64-bit).
//!
//! usage: `paperpaw-library-bench [--walks] <library-dir> [<more-books-dir>]`
//!
//! `<library-dir>/books` goes onto the card; the books of
//! `<more-books-dir>/books` are added after the first walks, as books
//! copied onto the card later. `--walks` measures the catalog's walks
//! only.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use paperpaw_core::Kernel;
use paperpaw_fat::fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, Write};
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk};
use paperpaw_platform::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation, Storage,
    StorageError,
};
use paperpaw_sdk::builtin::{events, settings};
use paperpaw_sdk::{Env, Id, Manifest, Plugin, RegisterError, Registrar, Version};

/// The heap now and at its peak, as the program allocates.
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// Set while the card keeps a block: the card's memory is not the
/// device's heap, so it is not counted.
static IN_CARD: AtomicBool = AtomicBool::new(false);
/// Blocks of the card read and written.
static READ: AtomicU64 = AtomicU64::new(0);
static WRITTEN: AtomicU64 = AtomicU64::new(0);

/// The system's allocator, counting.
struct Counting;

// SAFETY: every call goes to the system allocator as it came; only the
// counts are added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !IN_CARD.load(Relaxed) {
            grew(layout.size());
        }
        // SAFETY: as the caller promised for `layout`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !IN_CARD.load(Relaxed) {
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        // SAFETY: as the caller promised for `ptr` and `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if !IN_CARD.load(Relaxed) {
            LIVE.fetch_sub(layout.size(), Relaxed);
            grew(new_size);
        }
        // SAFETY: as the caller promised for `ptr`, `layout` and `new_size`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn grew(size: usize) {
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The blocks of a card of `blocks` blocks, those written alone kept (the
/// others read as zeros), counting the blocks read and written.
#[derive(Clone)]
struct Bytes {
    blocks: u32,
    kept: Rc<RefCell<HashMap<u32, [u8; BLOCK]>>>,
}

impl Blocks for Bytes {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        READ.fetch_add(1, Relaxed);
        if index >= self.blocks {
            return Err(DiskError);
        }
        *block = self
            .kept
            .borrow()
            .get(&index)
            .copied()
            .unwrap_or([0; BLOCK]);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        WRITTEN.fetch_add(1, Relaxed);
        if index >= self.blocks {
            return Err(DiskError);
        }
        IN_CARD.store(true, Relaxed);
        self.kept.borrow_mut().insert(index, *block);
        IN_CARD.store(false, Relaxed);
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok(self.blocks)
    }
}

type Fat = FatStorage<SharedDisk<Bytes>>;

/// The card's storage, as the X4 has it: FAT32, clusters of 32 KiB.
#[derive(Clone, Copy)]
struct Card(&'static Fat);

impl Card {
    fn new(size: u64) -> Result<Card, String> {
        let bytes = Bytes {
            blocks: u32::try_from(size / BLOCK as u64).map_err(|_| "card too large")?,
            kept: Rc::default(),
        };
        let mut disk = BlockDisk::new(bytes.clone()).map_err(|e| format!("disk: {e:?}"))?;
        let options = FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .bytes_per_cluster(32 * 1024);
        paperpaw_fat::fatfs::format_volume(&mut disk, options).map_err(|e| format!("{e:?}"))?;
        disk.flush().map_err(|e| format!("disk: {e:?}"))?;
        let disk = BlockDisk::new(bytes).map_err(|e| format!("disk: {e:?}"))?;
        let shared = SharedDisk::new(disk);
        let fs = FileSystem::new(shared.clone(), FsOptions::new()).map_err(|e| format!("{e:?}"))?;
        let storage = FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared);
        Ok(Card(Box::leak(Box::new(storage))))
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        tally(path, || self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        tally(path, || self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        tally(path, || self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        tally(path, || self.0.append(path, data))
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        tally(path, || self.0.write_at(path, offset, data))
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        tally(path, || self.0.truncate(path, len))
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        tally(dir, || self.0.list(dir, visit))
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        tally(dir, || self.0.list_from(dir, from, visit))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        tally(path, || self.0.remove(path))
    }
    fn modified(&self, path: &str) -> Option<i64> {
        tally(path, || self.0.modified(path))
    }
    fn exists(&self, path: &str) -> bool {
        tally(path, || self.0.exists(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        tally(from, || self.0.rename(from, to))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        tally(path, || self.0.create_dir(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        tally(path, || self.0.remove_dir_all(path))
    }
    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
    fn free_space(&self) -> Option<u64> {
        self.0.free_space()
    }
    fn total_space(&self) -> Option<u64> {
        self.0.total_space()
    }
}

thread_local! {
    /// Blocks read and written by the calls on paths of each kind, since
    /// [`tallied`] was asked last.
    static TALLY: RefCell<Vec<(&'static str, u64, u64, u64)>> = const { RefCell::new(Vec::new()) };
}

/// What a path is: the books' details, their covers, the catalog and the
/// rest of the library's own files, the books, others.
fn kind(path: &str) -> &'static str {
    if path.starts_with(".paperpaw/library/details") || path.starts_with(".paperpaw/library/books")
    {
        "details"
    } else if path.starts_with(".paperpaw/library/thumbs")
        || path.starts_with(".paperpaw/library/covers")
    {
        "covers"
    } else if path.starts_with(".paperpaw/library") {
        "library files"
    } else if path.starts_with("books") {
        "books"
    } else {
        other(path)
    }
}

/// Any other path's kind: its first two parts, each named once.
fn other(path: &str) -> &'static str {
    thread_local! {
        static NAMES: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }
    let mut parts = path.trim_start_matches('/').splitn(3, '/');
    let name = match (parts.next(), parts.next()) {
        (Some(first), Some(second)) => format!("{first}/{second}"),
        (Some(first), None) => first.to_owned(),
        _ => String::new(),
    };
    NAMES.with_borrow_mut(|names| match names.iter().find(|n| **n == name) {
        Some(known) => *known,
        None => {
            let leaked: &'static str = Box::leak(name.into_boxed_str());
            names.push(leaked);
            leaked
        }
    })
}

/// `f` on `path`, its blocks counted to the path's kind.
fn tally<T>(path: &str, f: impl FnOnce() -> T) -> T {
    let (read, written) = (READ.load(Relaxed), WRITTEN.load(Relaxed));
    let value = f();
    let (read, written) = (READ.load(Relaxed) - read, WRITTEN.load(Relaxed) - written);
    let kind = kind(path);
    TALLY.with_borrow_mut(|tally| match tally.iter_mut().find(|t| t.0 == kind) {
        Some(t) => {
            t.1 += 1;
            t.2 += read;
            t.3 += written;
        }
        None => tally.push((kind, 1, read, written)),
    });
    value
}

/// The calls, blocks read and written of each kind since the last time.
fn tallied() -> String {
    TALLY.with_borrow_mut(|tally| {
        let text: Vec<String> = tally
            .iter()
            .map(|(kind, calls, read, written)| {
                format!("{kind}: {calls} calls {read} r {written} w")
            })
            .collect();
        tally.clear();
        text.join("; ")
    })
}

/// Copies the files under `from` onto `card` at `to`; how many files.
fn copy(card: &dyn Storage, from: &Path, to: &str) -> Result<usize, String> {
    let _ = card.create_dir(to);
    let mut files = 0;
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{to}/{name}");
        if entry.path().is_dir() {
            files += copy(card, &entry.path(), &path)?;
        } else {
            let data = std::fs::read(entry.path()).map_err(|e| format!("{path}: {e}"))?;
            card.write(&path, &data)
                .map_err(|e| format!("{path}: {e:?}"))?;
            files += 1;
        }
    }
    Ok(files)
}

/// What one operation took.
struct Took {
    millis: f64,
    read: u64,
    written: u64,
    peak: usize,
}

/// Runs `f`, measured.
fn measure<T>(f: impl FnOnce() -> T) -> (T, Took) {
    let (read, written) = (READ.load(Relaxed), WRITTEN.load(Relaxed));
    let live = LIVE.load(Relaxed);
    PEAK.store(live, Relaxed);
    let start = Instant::now();
    let value = f();
    let took = Took {
        millis: start.elapsed().as_secs_f64() * 1000.0,
        read: READ.load(Relaxed) - read,
        written: WRITTEN.load(Relaxed) - written,
        peak: PEAK.load(Relaxed).saturating_sub(live),
    };
    (value, took)
}

/// One line of the report: tab-separated, for the note's table.
fn report(what: &str, took: &Took, note: &str) {
    println!(
        "{what}\t{:.1} ms\t{} blocks read\t{} written\t{:.1} KiB peak\t{note}\t{}",
        took.millis,
        took.read,
        took.written,
        took.peak as f64 / 1024.0,
        tallied()
    );
}

mod run;
#[cfg(test)]
mod tests;

/// The device: the X4's screen and memory class (its index holds fewer
/// books).
const DEVICE: DeviceInfo = DeviceInfo::new(
    "bench",
    "X4 as benchmarked",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray2, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// Hears the start and runs the operations ([`run::all`]).
struct Bench {
    more: Option<String>,
    walks: bool,
}

impl Plugin for Bench {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("bench"), "Bench", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (more, walks) = (self.more.clone(), self.walks);
        r.on(events::STARTED, move |env: &Env<'_>, _: &(), _| {
            run::all(env, more.as_deref(), walks);
        })
    }
}

fn main() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let walks = args.iter().any(|a| a == "--walks");
    args.retain(|a| a != "--walks");
    let mut args = args.into_iter();
    let library = args
        .next()
        .ok_or("usage: paperpaw-library-bench [--walks] <library-dir> [<more>]")?;
    let more = args.next();
    // FAT32 with clusters of 32 KiB, as large as a card of the X4 often
    // is: 20,000 books, their details and covers in clusters of their own
    // fill 4 GiB (its memory is taken only where written).
    let card = Card::new(32 << 30)?;
    let (files, took) = measure(|| copy(&card, &Path::new(&library).join("books"), "books"));
    report("copy onto the card", &took, &format!("{} files", files?));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .setting_default(&settings::LIBRARY_FOLDER, "books".into())
        .plugin(paperpaw_epub_family::EpubFormat)
        .plugin(Bench { more, walks })
        .build()
        .map_err(|e| format!("{e:?}"))?;
    kernel.start(paperpaw_platform::Instant(0));
    Ok(())
}
