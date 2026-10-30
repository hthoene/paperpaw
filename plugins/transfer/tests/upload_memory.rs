//! What an upload takes of the heap: the kernel and the transfer plugin
//! (what the emulator's memory flows measure, not the radio's buffers) get
//! a large file over the browser transfer as the X4 passes it on, in parts
//! of 16 KiB, and the screen says it came. The heap may hold a part or two
//! and what the answer and the screen take, never the file: the peak above
//! what the open screen holds stays under [`MOST`].
//!
//! Its own test binary: the allocator counts every allocation of the
//! process, so nothing else may run beside it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_core::{Effects, Io, Kernel, ServerCall, ServerEvent};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_transfer::Transfer;
use paperpaw_sdk::{DirEntry, Id, Storage, StorageError};

/// The heap in use, and its most since [`reset_peak`].
static USED: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: passes every call to the system's allocator unchanged; it only
// counts the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as the caller asked.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let used = USED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(used, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        USED.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller asked.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as the caller asked.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            USED.fetch_sub(layout.size(), Ordering::Relaxed);
            let used = USED.fetch_add(new_size, Ordering::Relaxed) + new_size;
            PEAK.fetch_max(used, Ordering::Relaxed);
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn reset_peak() -> usize {
    let used = USED.load(Ordering::Relaxed);
    PEAK.store(used, Ordering::Relaxed);
    used
}

/// The most an upload may take of the heap above what the open screen
/// holds: two parts (it takes one, some 16.6 KB in all, 2026-10-25).
const MOST: usize = 2 * PART;
/// Bytes passed on at a time, as the X4's server does.
const PART: usize = 16 * 1024;
/// Files larger than this are kept as their size alone: their bytes are
/// on the card, not in the heap.
const KEPT: usize = 4 * 1024;

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

/// A file: its bytes while small, else its size alone (reading it gives
/// zeros).
enum File {
    Small(Vec<u8>),
    Large(u64),
}

impl File {
    fn len(&self) -> u64 {
        match self {
            File::Small(bytes) => bytes.len() as u64,
            File::Large(size) => *size,
        }
    }
}

/// A card that keeps large files as their sizes.
#[derive(Clone, Default)]
struct Card(
    Rc<RefCell<BTreeMap<String, File>>>,
    Rc<RefCell<Vec<String>>>,
);

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let files = self.0.borrow();
        let file = files.get(path).ok_or(StorageError::NotFound)?;
        let left = file.len().saturating_sub(offset) as usize;
        let n = left.min(buf.len());
        match file {
            File::Small(bytes) => buf[..n].copy_from_slice(&bytes[offset as usize..][..n]),
            File::Large(_) => buf[..n].fill(0),
        }
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0
            .borrow()
            .get(path)
            .map(File::len)
            .ok_or(StorageError::NotFound)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0
            .borrow_mut()
            .insert(path.to_string(), File::Small(data.to_vec()));
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut files = self.0.borrow_mut();
        let file = files
            .entry(path.to_string())
            .or_insert(File::Small(Vec::new()));
        let len = file.len() + data.len() as u64;
        *file = match std::mem::replace(file, File::Large(0)) {
            File::Small(mut bytes) if len <= KEPT as u64 => {
                bytes.extend_from_slice(data);
                File::Small(bytes)
            }
            _ => File::Large(len),
        };
        Ok(())
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // The card's top folder is "" (or "/").
        let dir = dir.trim_matches('/');
        let prefix = if dir.is_empty() {
            String::new()
        } else if self.1.borrow().iter().any(|d| d == dir) {
            format!("{dir}/")
        } else {
            return Err(StorageError::NotFound);
        };
        for (path, file) in self.0.borrow().iter() {
            if let Some(name) = path.strip_prefix(&prefix).filter(|n| !n.contains('/')) {
                visit(DirEntry::new(name, false, file.len()));
            }
        }
        for path in self.1.borrow().iter() {
            if let Some(name) = path.strip_prefix(&prefix).filter(|n| !n.contains('/')) {
                visit(DirEntry::new(name, true, 0));
            }
        }
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0
            .borrow_mut()
            .remove(path)
            .map(|_| ())
            .ok_or(StorageError::NotFound)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.borrow().contains_key(path) || self.1.borrow().iter().any(|d| d == path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let mut files = self.0.borrow_mut();
        let file = files.remove(from).ok_or(StorageError::NotFound)?;
        files.insert(to.to_string(), file);
        Ok(())
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        let mut dirs = self.1.borrow_mut();
        if !dirs.iter().any(|d| d == path) {
            dirs.push(path.to_string());
        }
        Ok(())
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        let prefix = format!("{path}/");
        self.0.borrow_mut().retain(|p, _| !p.starts_with(&prefix));
        self.1
            .borrow_mut()
            .retain(|d| d != path && !d.starts_with(&prefix));
        Ok(())
    }
}

/// A random source for the key: the same each run.
fn fill(buf: &mut [u8]) -> bool {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte = (i as u8).wrapping_mul(97).wrapping_add(13);
    }
    true
}

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn event(&mut self, event: ServerEvent) {
        self.now += 10;
        let effects = self.kernel.server_event(Instant(self.now), event);
        self.settle(effects);
    }

    fn calls(&mut self) -> Vec<ServerCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Server(call) => Some(call),
                _ => None,
            })
            .collect()
    }
}

/// A 6 MB upload in parts of 16 KiB, the screen drawn as each part comes
/// and after: the heap holds no more than [`MOST`] above the open screen.
#[test]
fn an_upload_takes_no_more_than_a_few_parts_of_the_heap() {
    let card = Card::default();
    card.create_dir("Books").unwrap();
    let kernel = Kernel::builder(DEVICE)
        .storage(card.clone())
        .user_config("[core]\nlanguage = en\n")
        .network(true)
        .random(fill)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Transfer)
        .build()
        .unwrap();
    let mut device = Device { kernel, now: 0 };
    let effects = device.kernel.start(Instant(0));
    device.settle(effects);
    device.now += 100;
    let effects = device
        .kernel
        .run_action(Instant(device.now), Id::new_static("transfer.open"));
    device.settle(effects);
    let calls = device.calls();
    let Some(ServerCall::Listen { server, .. }) = calls.first() else {
        panic!("no server: {calls:?}");
    };
    let server = *server;
    device.event(ServerEvent::Started {
        server,
        address: "192.168.1.20:8080".into(),
    });
    device.kernel.render(&mut |_, _| {});
    let key = device
        .kernel
        .describe()
        .into_iter()
        .find_map(|p| p.label.rsplit_once("k=").map(|(_, k)| k.to_string()))
        .expect("the key on the screen");
    // A first upload, so that what stays once (the library's list, the
    // screen's message) is there before measuring.
    let upload = |device: &mut Device, request: u32, name: &str, size: usize| {
        device.event(ServerEvent::Request {
            server,
            request,
            method: "PUT".into(),
            target: format!("/api/file?k={key}&path=Books/{name}"),
            body: true,
        });
        let mut left = size;
        while left > 0 {
            let n = left.min(PART);
            left -= n;
            device.event(ServerEvent::Body {
                request,
                data: vec![7; n],
                last: left == 0,
            });
            device.kernel.render(&mut |_, _| {});
        }
        let answered = device.calls().into_iter().find_map(|call| match call {
            ServerCall::Respond {
                request: r,
                status,
                body,
                ..
            } if r == request => {
                if status != 200 {
                    println!("{status}: {}", String::from_utf8_lossy(&body));
                }
                Some(status)
            }
            _ => None,
        });
        assert_eq!(answered, Some(200), "{name}");
        device.kernel.render(&mut |_, _| {});
    };
    upload(&mut device, 1, "first.bin", 64 * 1024);
    let before = reset_peak();
    upload(&mut device, 2, "large.bin", 6 * 1024 * 1024 + 123);
    let peak = PEAK.load(Ordering::Relaxed);
    let after = USED.load(Ordering::Relaxed);
    println!(
        "heap: {before} before, {after} after, peak {peak} ({} above before)",
        peak - before
    );
    assert_eq!(card.size("Books/large.bin").unwrap(), 6 * 1024 * 1024 + 123);
    assert!(
        peak - before <= MOST,
        "an upload took {} bytes of the heap above the open screen's",
        peak - before
    );
    // Nothing stays of it but the little the screen and library keep.
    assert!(after <= before + 4 * 1024, "{after} after, {before} before");
}
