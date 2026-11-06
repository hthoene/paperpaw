//! Opening a table under a heap limit: no abort, and the retained heap is
//! what the reader says.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{Mem, shipped};
use paperpaw_hyphen::{CheckError, MAX_BLOCKS, OpenError, StoredPatterns, TableError};

struct Limited;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);

// SAFETY: forwards to the system allocator, refusing past the limit.
unsafe impl GlobalAlloc for Limited {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if LIVE.load(Ordering::SeqCst) + layout.size() > LIMIT.load(Ordering::SeqCst) {
            return std::ptr::null_mut();
        }
        LIVE.fetch_add(layout.size(), Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Limited = Limited;

#[test]
fn open_under_a_heap_limit() {
    // One test only: the counters are global.
    let de = shipped().into_iter().find(|s| s.name == "de").unwrap();
    let source = || Mem::new(de.table.clone());
    let (a, b) = (source(), source());

    let before = LIVE.load(Ordering::SeqCst);
    let reader = StoredPatterns::open(a, 0, 8).unwrap();
    let retained = LIVE.load(Ordering::SeqCst) - before;
    // The source holds the table (counted: it was allocated before `before`,
    // so not in `retained`); what is left is the reader's own heap.
    assert_eq!(retained, reader.heap_bytes());
    println!("de retained {retained} bytes (cache 8)");
    drop(reader);

    // Every limit below what the reader needs is a clean error.
    for limit in (0..=retained + 600).step_by(97) {
        // The copy of the table is made before the limit applies.
        let copy = source_clone(&b);
        LIMIT.store(LIVE.load(Ordering::SeqCst) + limit, Ordering::SeqCst);
        let attempt = StoredPatterns::open(copy, 0, 8);
        LIMIT.store(usize::MAX, Ordering::SeqCst);
        match attempt {
            Ok(_) => assert!(limit >= retained, "opened with only {limit} bytes"),
            Err(e) => assert_eq!(e, OpenError::NoMemory, "limit {limit}"),
        }
    }

    // Senior's B1 table: 15,000 one-byte blocks (75 KB on the card, 120 KB
    // of index and fences if it were kept). It is refused for its size with
    // no more heap than the reader's 512-byte scratch buffer.
    let blocks = 15_000u32;
    assert!(blocks > MAX_BLOCKS);
    let entries = 24 + 4 * blocks;
    let mut table = b"PPHP\x01\x02\x02\x05\0\0\0\0".to_vec();
    for word in [blocks, entries + blocks, 0] {
        table.extend_from_slice(&word.to_le_bytes());
    }
    for offset in 0..blocks {
        table.extend_from_slice(&offset.to_le_bytes());
    }
    table.resize(table.len() + blocks as usize, b'a');
    let crafted = Mem::new(table);
    LIMIT.store(LIVE.load(Ordering::SeqCst) + 600, Ordering::SeqCst);
    let attempt = StoredPatterns::open(crafted, 0, 8).map(|_| ());
    LIMIT.store(usize::MAX, Ordering::SeqCst);
    assert_eq!(
        attempt,
        Err(OpenError::Check(CheckError::Table(TableError::TooLarge)))
    );
}

fn source_clone(m: &Mem) -> Mem {
    Mem::new(m.bytes.clone())
}
