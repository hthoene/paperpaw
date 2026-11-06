//! The heap word's table reader: missing, short, misaligned and full tables.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
extern crate std;
use super::*;
use core::cell::Cell;
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::mem::{offset_of, size_of};

std::thread_local! {
    static CALLS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}

fn bump(i: usize) {
    CALLS.with(|calls| {
        let mut now = calls.get();
        now[i] += 1;
        calls.set(now);
    });
}

fn calls() -> [usize; 3] {
    CALLS.with(Cell::get)
}

static mut BLOCK: [u64; 8] = [0; 8];

unsafe extern "C" fn fake_alloc(size: usize, align: usize) -> *mut c_void {
    bump(0);
    if size > 64 || align > 8 {
        return ptr::null_mut();
    }
    (&raw mut BLOCK).cast()
}
unsafe extern "C" fn fake_dealloc(_: *mut c_void, _: usize, _: usize) {
    bump(1);
}
unsafe extern "C" fn fake_realloc(
    block: *mut c_void,
    _: usize,
    _: usize,
    new: usize,
) -> *mut c_void {
    bump(2);
    if new > 64 { ptr::null_mut() } else { block }
}
unsafe extern "C" fn fake_abort(_: i32, _: *const u8, _: usize) -> ! {
    std::process::abort()
}

fn full() -> PpHeap {
    PpHeap {
        size: size_of::<PpHeap>() as u32,
        major: 7,
        minor: 1,
        alloc: Some(fake_alloc),
        dealloc: Some(fake_dealloc),
        realloc: Some(fake_realloc),
        abort: Some(fake_abort),
    }
}

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align).unwrap()
}

#[test]
fn a_full_table_allocates_frees_and_grows_through_the_host() {
    let table = full();
    let before = calls();
    // SAFETY: a live, aligned table of its full size.
    unsafe {
        let block = alloc(&table, layout(16, 8));
        assert!(!block.is_null());
        assert!(!realloc(&table, block, layout(16, 8), 32).is_null());
        dealloc(&table, block, layout(32, 8));
    }
    let after = calls();
    assert_eq!(
        [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2]
        ],
        [1, 1, 1]
    );
    // SAFETY: as above.
    assert!(unsafe { abort_entry(&table) }.is_some());
}

#[test]
fn no_word_means_no_heap_and_the_trap_path() {
    // SAFETY: a null table is the documented "no heap" value.
    unsafe {
        assert!(alloc(ptr::null(), layout(8, 8)).is_null());
        assert!(realloc(ptr::null(), ptr::null_mut(), layout(8, 8), 16).is_null());
        dealloc(ptr::null(), ptr::null_mut(), layout(8, 8));
        assert!(abort_entry(ptr::null()).is_none());
    }
}

#[test]
fn a_short_table_has_only_the_entries_its_size_covers() {
    let mut table = full();
    // Up to `alloc` only, as a host of an older minor might give.
    table.size = (offset_of!(PpHeap, alloc) + size_of::<usize>()) as u32;
    let before = calls();
    // SAFETY: a live table; the reader stays within `size`.
    unsafe {
        assert!(!alloc(&table, layout(8, 8)).is_null());
        assert!(realloc(&table, ptr::null_mut(), layout(8, 8), 16).is_null());
        assert!(abort_entry(&table).is_none(), "no abort: the trap path");
    }
    assert_eq!(calls()[2], before[2], "realloc was not called");
    // One byte short of `alloc`: nothing at all.
    table.size -= 1;
    // SAFETY: as above.
    assert!(unsafe { alloc(&table, layout(8, 8)) }.is_null());
}

#[test]
fn a_misaligned_table_and_a_too_large_alignment_are_refused_unread() {
    let mut storage = MaybeUninit::<[PpHeap; 2]>::zeroed();
    let base = storage.as_mut_ptr().cast::<u8>();
    // SAFETY: one byte into storage large enough for a table.
    let shifted = unsafe { base.add(1) }.cast::<PpHeap>();
    // SAFETY: a misaligned pointer is refused before any read.
    assert!(unsafe { alloc(shifted, layout(8, 8)) }.is_null());
    let table = full();
    let before = calls();
    let huge = layout(8, MAX_ALIGN * 2);
    // SAFETY: a live table.
    assert!(unsafe { alloc(&table, huge) }.is_null());
    assert_eq!(calls()[0], before[0], "the host was not asked");
}

#[test]
fn a_panic_message_is_cut_at_a_character_and_classified() {
    let mut message = Message::new();
    let long = "ä".repeat(ABORT_MESSAGE_BYTES);
    assert!(message.write_str(&long).is_err());
    assert_eq!(message.as_str().len(), ABORT_MESSAGE_BYTES);
    assert_eq!(message.code(), ABORT_PANIC);
    let mut oom = Message::new();
    write!(oom, "memory allocation of {} bytes failed", 4096).unwrap();
    assert_eq!(oom.code(), ABORT_ALLOCATION);
}

#[test]
fn the_macro_uses_the_generated_names() {
    let source = include_str!("entry.rs");
    let (symbol, alias, section) = NAMES;
    for needle in [
        alloc::format!(".globl {symbol}"),
        alloc::format!(".hidden {alias}"),
        alloc::format!(".pushsection {section},"),
        alloc::format!("static {alias}:"),
    ] {
        assert!(source.contains(&needle), "{needle}");
    }
}

#[test]
fn a_table_of_another_major_is_no_heap() {
    let mut table = full();
    table.major = 8;
    // SAFETY: a live, aligned table; refused on its major before any entry.
    unsafe {
        assert!(alloc(&table, layout(8, 8)).is_null());
        assert!(abort_entry(&table).is_none());
    }
}
