//! The author's check of a signed plugin file with no memory for itself
//! (#250): a memory problem, not a failed signature, so a good plugin is
//! not taken for a badly signed one, and the marks of its install wait for
//! the next start.
#![allow(clippy::unwrap_used, reason = "test setup and checks fail loudly")]

use paperpaw_sdk::install::marks::{self, From, Marks};
use paperpaw_sdk::native::{self, Chip, Header, Problem, signature};
use paperpaw_sdk::{MemoryStorage, Storage};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Refusing;

thread_local! {
    /// Allocations of this size are refused (0: none).
    static REFUSE: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: forwards to `System`, refusing (null) the size asked for.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let refuse = REFUSE.try_with(Cell::get).unwrap_or(0);
        if refuse != 0 && layout.size() == refuse {
            core::ptr::null_mut()
        } else {
            // SAFETY: as `GlobalAlloc::alloc`.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: as `GlobalAlloc::dealloc`.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// `operation` with no memory for the author's check.
fn without_room<T>(operation: impl FnOnce() -> T) -> T {
    /// Puts the heap back even if `operation` panics.
    struct Back;
    impl Drop for Back {
        fn drop(&mut self) {
            REFUSE.with(|refuse| refuse.set(0));
        }
    }
    let size = core::mem::size_of::<signature::SignatureCheck>();
    REFUSE.with(|refuse| refuse.set(size));
    let _back = Back;
    operation()
}

const SEED: [u8; 32] = [2; 32];

/// A plugin file signed with [`SEED`], and its SHA-256.
fn signed() -> (Vec<u8>, [u8; 32]) {
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: 1,
        base: paperpaw_native::LINK_BASE,
        image_len: 128,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: native::INTERFACE,
        sha256: [0; 32],
    };
    let image = [9u8; 128];
    let mut sha = header.digest();
    sha.update(&image);
    header.sha256 = sha.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    let author = signature::sign(&file, &SEED);
    file.extend_from_slice(&author.to_bytes());
    (file, header.sha256)
}

#[test]
fn a_check_without_memory_is_a_memory_problem_not_a_bad_signature() {
    let storage = MemoryStorage::new();
    storage.write("a.ppn", &signed().0).unwrap();
    let refused = without_room(|| native::check_file(&storage, "a.ppn").map(|_| ()));
    assert_eq!(refused, Err(Problem::Other(1)));
    assert!(native::check_file(&storage, "a.ppn").unwrap().1.is_some());
}

#[test]
fn a_start_without_memory_for_the_check_keeps_the_marks_for_the_next() {
    let storage = MemoryStorage::new();
    let (file, sha256) = signed();
    let key = signature::sign(&file[..file.len() - signature::SIGNATURE_LEN], &SEED).key;
    storage.write("plugins/dials/code.ppn", &file).unwrap();
    let marks = Marks::new(From::Local).code(sha256).author(key);
    marks::write(&storage, "plugins/dials", &marks).unwrap();
    // Not judged: not named installed, not refused, the marks kept.
    assert!(without_room(|| marks::settle(
        &storage,
        "dials",
        "plugins/dials"
    )));
    assert_eq!(native::installed_code(&storage, "dials"), None);
    assert!(storage.exists(&format!("plugins/dials/{}", marks::FILE)));
    // The next start, with the memory, names it.
    assert!(marks::settle(&storage, "dials", "plugins/dials"));
    assert_eq!(native::installed_code(&storage, "dials"), Some(sha256));
    assert!(!storage.exists(&format!("plugins/dials/{}", marks::FILE)));
}
