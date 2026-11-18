//! The account's key leaves no copy in the heap: what the writers and readers
//! of the sync copy of it is zeroed before it is freed, also where an
//! allocation is refused half way. A global allocator looks into every block
//! it is given back while a check runs, for the start of the key.
#![allow(
    clippy::unwrap_used,
    unsafe_code,
    reason = "fixture; the allocator forwards to System"
)]
use paperpaw_plugin_kosync::{native_flow, native_model as model, native_store as store};
use paperpaw_ui_kit::mem::wipe_text;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// What a key starts with: no other text in a test does.
const NEEDLE: &[u8] = b"KEYMARK-9f3a";

std::thread_local! {
    static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
    static WATCH: Cell<bool> = const { Cell::new(false) };
    static LEAKED: Cell<usize> = const { Cell::new(0) };
}

struct Watching;

fn refuse() -> bool {
    let at = REFUSE_AT.with(Cell::get);
    if at == 0 {
        return false;
    }
    let n = COUNT.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    n == at
}

/// Counts a block given back with the needle in it.
fn look(ptr: *mut u8, len: usize) {
    if WATCH.with(Cell::get) && !ptr.is_null() && len >= NEEDLE.len() {
        // SAFETY: the block is live until it is forwarded to the system below.
        let block = unsafe { std::slice::from_raw_parts(ptr, len) };
        if block.windows(NEEDLE.len()).any(|w| w == NEEDLE) {
            LEAKED.with(|l| l.set(l.get() + 1));
        }
    }
}

// SAFETY: every call forwards to `System` unchanged, or refuses with null.
unsafe impl GlobalAlloc for Watching {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's contract passes through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        look(ptr, layout.size());
        // SAFETY: as above.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // The old block is given back at a move.
        look(ptr, layout.size());
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static ALLOC: Watching = Watching;

/// Runs `check` with the nth allocation refused (0: none), looking at the
/// blocks it frees; the number of blocks freed with the needle in them.
fn run(refuse_at: usize, check: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    LEAKED.with(|l| l.set(0));
    REFUSE_AT.with(|r| r.set(refuse_at));
    WATCH.with(|w| w.set(true));
    check();
    WATCH.with(|w| w.set(false));
    REFUSE_AT.with(|r| r.set(0));
    LEAKED.with(Cell::get)
}

/// The allocations `check` makes, counted without refusing any.
fn allocations(check: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    REFUSE_AT.with(|r| r.set(usize::MAX));
    check();
    REFUSE_AT.with(|r| r.set(0));
    COUNT.with(Cell::get)
}

/// Every allocation of `check` refused in turn: no block with the key is freed
/// unzeroed. `check` wipes what it returns.
fn sweep(check: impl Fn()) {
    let total = allocations(&check);
    assert!(total > 0);
    for at in 0..=total {
        assert_eq!(run(at, &check), 0, "allocation {at} of {total}");
    }
}

/// A key with escapes in it and room to grow if it were let: made before a
/// check starts, so it is not part of what is swept (it is the caller's).
fn key() -> String {
    format!("{}{}\\\n\t", "KEYMARK-9f3a", "\"".repeat(20))
}

#[test]
fn the_watch_sees_a_key_dropped_unwiped() {
    // The check itself: a key that is dropped as it is is found.
    let held = key();
    assert!(run(0, || drop(held)) >= 1);
}

#[test]
fn a_json_string_of_the_key_leaves_no_copy() {
    let key = key();
    sweep(|| {
        if let Ok(text) = model::json_string(&key) {
            wipe_text(text);
        }
    });
}

#[test]
fn the_create_body_leaves_no_copy() {
    let key = key();
    sweep(|| {
        if let Ok(body) = native_flow::create_body("anna", &key) {
            wipe_text(body);
        }
    });
}

#[test]
fn a_key_read_from_a_file_leaves_no_copy() {
    // An escaped value is a copy of its own while it is read.
    let file = "[account]\nkey = \"KEYMARK-9f3a-0123456789abcdef\\\"x\"\n".to_string();
    sweep(|| {
        if let Ok(Some(value)) = store::value_of(&file, "key") {
            wipe_text(value);
        }
    });
}
