//! The frame owner and the `PpHeap` table (`abi/heap-word.md` sections 3, 4):
//! every entry into plugin code runs it as the frame owner and gives the
//! previous owner back, and the table's door reaches the same heap as the
//! host table's.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::tests::{HEAP_LIVE, Heap, NORMAL, fake, finish, frame, reset};
use super::*;
use crate::native_host::frame_owner;
use crate::native_host::plugin_heap::block_cost as cost;
use core::any::TypeId;
use paperpaw_native::host_glue::{
    ERR_INVALID, OK, read_heap_abort, read_heap_alloc, read_heap_dealloc, read_heap_realloc,
};

std::thread_local! {
    /// The kinds of entry (bit 1 make, 2 event, 4 draw, 8 destroy) that ran
    /// the plugin as the frame owner, and that did not (no allocation here:
    /// the failing-allocation tests count them).
    static FRAME_OURS: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    static FRAME_NOT: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

pub(super) fn clear() {
    FRAME_OURS.with(|bits| bits.set(0));
    FRAME_NOT.with(|bits| bits.set(0));
}

/// Notes whether the code of the plugin with context `ctx` runs as the frame owner.
pub(super) fn in_frame(kind: u32, ctx: *mut c_void) {
    // SAFETY: the admitted fixture's own pinned context.
    let context = unsafe { &*ctx.cast::<Context<Heap>>() };
    let seen = if frame_owner::owner() == Some(context.owner_key()) {
        &FRAME_OURS
    } else {
        &FRAME_NOT
    };
    seen.with(|bits| bits.set(bits.get() | kind));
}

fn used(context: Pin<&Context<Heap>>) -> usize {
    let (state, owner) = context.resources.heap();
    state.used(owner)
}

fn bits(seen: &'static std::thread::LocalKey<core::cell::Cell<u32>>) -> u32 {
    seen.with(core::cell::Cell::get)
}

#[test]
fn every_entry_of_a_registered_plugin_runs_it_as_the_frame_owner() {
    reset(NORMAL);
    let hosted = fake();
    assert_eq!(frame_owner::owner(), None);
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(frame_owner::owner(), None, "back after make");
    let depth = DispatchDepth::new();
    assert_eq!(
        dispatch_registration(&batch, 0, &depth, Invocation::Event(&[1, 2, 3])),
        Ok(OK)
    );
    assert_eq!(
        dispatch_registration(&batch, 1, &depth, Invocation::Draw(&frame())),
        Ok(OK)
    );
    // An error return gives the owner back too: both callbacks refuse here.
    assert_eq!(
        dispatch_registration(&batch, 0, &depth, Invocation::Event(&[1])),
        Ok(ERR_INVALID)
    );
    assert_eq!(
        dispatch_registration(&batch, 1, &depth, Invocation::Draw(&[0])),
        Ok(ERR_INVALID)
    );
    assert_eq!(frame_owner::owner(), None, "back after an error return");
    // Nested: another plugin's frame is the previous owner, and is back.
    frame_owner::within(99, ptr::null(), TypeId::of::<()>(), || {
        assert_eq!(
            dispatch_registration(&batch, 0, &depth, Invocation::Event(&[1, 2, 3])),
            Ok(OK)
        );
        assert_eq!(frame_owner::owner(), Some(99), "back after a nested entry");
    });
    drop(batch);
    finish(&hosted);
    assert_eq!(frame_owner::owner(), None, "back after the destructor");
    // Make, event, draw and destroy each ran the plugin as the frame owner,
    // and no entry ran it as another.
    assert_eq!(bits(&FRAME_OURS), 1 | 2 | 4 | 8);
    assert_eq!(bits(&FRAME_NOT), 0);
}

#[test]
fn the_table_has_the_host_heap_entries_and_no_abort_yet() {
    let table = handed_heap::<Heap>();
    assert_eq!(table.major, paperpaw_native::host_glue::MAJOR);
    assert_eq!(table.minor, paperpaw_native::host_glue::MINOR);
    let raw = ptr::from_ref(table);
    // SAFETY: a live, aligned table of this crate.
    unsafe {
        assert!(read_heap_alloc(raw).is_some());
        assert!(read_heap_dealloc(raw).is_some());
        assert!(read_heap_realloc(raw).is_some());
        assert!(
            read_heap_abort(raw).is_none(),
            "the size stops before abort"
        );
    }
    // The same static every time.
    assert!(ptr::eq(table, handed_heap::<Heap>()));
}

#[test]
fn outside_a_frame_the_table_allocates_nothing_and_ignores_what_it_cannot_free() {
    let table = handed_heap::<Heap>();
    let alloc = table.alloc.unwrap();
    let realloc = table.realloc.unwrap();
    let dealloc = table.dealloc.unwrap();
    // SAFETY: no block is involved: nothing is allocated, a null is freed.
    unsafe {
        assert!(alloc(64, 8).is_null());
        assert!(realloc(ptr::NonNull::<u8>::dangling().as_ptr().cast(), 8, 8, 16).is_null());
        dealloc(ptr::null_mut(), 8, 8);
    }
}

#[test]
fn the_table_and_the_host_table_are_two_doors_to_one_heap() {
    reset(NORMAL);
    let hosted = fake();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let slots = handed_table::<Heap>();
    let table = handed_heap::<Heap>();
    let (alloc, dealloc, realloc) = (
        table.alloc.unwrap(),
        table.dealloc.unwrap(),
        table.realloc.unwrap(),
    );
    let before = HEAP_LIVE.with(core::cell::Cell::get);
    context.run_plugin(|| {
        // SAFETY: blocks of this heap with the layouts they were made with,
        // each freed once, through either door.
        unsafe {
            // The table's block, freed by the slot.
            let block = alloc(64, 16);
            assert!(!block.is_null() && block.addr().is_multiple_of(16));
            assert_eq!(HEAP_LIVE.with(core::cell::Cell::get), before + cost(64, 16));
            assert_eq!(used(context), cost(64, 16), "charged to the frame owner");
            assert_eq!((slots.heap_dealloc)(ctx, block, 64, 16), OK);
            assert_eq!(HEAP_LIVE.with(core::cell::Cell::get), before);
            // The slot's block, grown and freed by the table.
            let mut slot_block = ptr::null_mut();
            assert_eq!((slots.heap_alloc)(ctx, 32, 8, &mut slot_block), OK);
            let grown = realloc(slot_block, 32, 8, 128);
            assert!(!grown.is_null());
            assert_eq!(HEAP_LIVE.with(core::cell::Cell::get), before + cost(128, 8));
            assert_eq!(
                used(context),
                cost(128, 8),
                "the slot's block, grown by the table"
            );
            dealloc(grown, 128, 8);
            assert_eq!(HEAP_LIVE.with(core::cell::Cell::get), before);
            // Refusals are null, as the slot's are statuses: a zero size, a
            // bad alignment, one past the largest.
            assert!(alloc(0, 8).is_null());
            assert!(alloc(8, 3).is_null());
            assert!(alloc(8, 8192).is_null());
            assert!(alloc(usize::MAX, 8).is_null());
            // A null block is ignored; a failed realloc leaves the block.
            dealloc(ptr::null_mut(), 8, 8);
            let block = alloc(16, 8);
            assert!(realloc(block, 16, 8, usize::MAX).is_null());
            dealloc(block, 16, 8);
        }
    });
    assert_eq!(HEAP_LIVE.with(core::cell::Cell::get), before);
    assert_eq!(used(context), 0, "every block gave its cost back");
    drop(hosted);
}

/// Another services type: its table's entries read no context of a frame
/// that was pushed for `Heap`, and none of a frame pushed for another table.
struct Other;

impl Services for Other {
    fn alloc(&self, _: usize, _: usize) -> *mut u8 {
        ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: usize, _: usize) {}
    unsafe fn realloc(&self, _: *mut u8, _: usize, _: usize, _: usize) -> *mut u8 {
        ptr::null_mut()
    }
    fn micros(&self) -> u64 {
        0
    }
}

#[test]
fn a_table_reads_only_the_context_of_a_frame_pushed_for_it() {
    reset(NORMAL);
    let hosted = fake();
    let context = hosted.owner().context();
    let (heap, other) = (handed_heap::<Heap>(), handed_heap::<Other>());
    context.run_plugin(|| {
        // SAFETY: nothing is allocated through the wrong table.
        unsafe {
            let block = (heap.alloc.unwrap())(16, 8);
            assert!(!block.is_null());
            (heap.dealloc.unwrap())(block, 16, 8);
            assert!((other.alloc.unwrap())(16, 8).is_null());
            assert!(
                (other.realloc.unwrap())(ptr::NonNull::<u8>::dangling().as_ptr().cast(), 8, 8, 16)
                    .is_null()
            );
        }
    });
    // A frame of no table (a bug) gives the entries nothing either.
    frame_owner::within(1, ptr::null(), TypeId::of::<()>(), || {
        // SAFETY: as above.
        assert!(unsafe { (heap.alloc.unwrap())(16, 8) }.is_null());
    });
    drop(hosted);
}
