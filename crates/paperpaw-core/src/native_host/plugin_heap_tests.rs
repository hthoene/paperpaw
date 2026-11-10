#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;
use std::vec::Vec;

/// The system's heap, counting what it holds.
struct Sys {
    live: Cell<usize>,
}

impl Sys {
    fn new() -> Self {
        Self { live: Cell::new(0) }
    }
}

impl Services for Sys {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        let layout = Layout::from_size_align(size, align).unwrap();
        // SAFETY: a valid, non-empty layout.
        let block = unsafe { std::alloc::alloc(layout) };
        if !block.is_null() {
            self.live.set(self.live.get() + size);
        }
        block
    }
    unsafe fn dealloc(&self, block: *mut u8, size: usize, align: usize) {
        self.live.set(self.live.get() - size);
        // SAFETY: the trait's contract.
        unsafe { std::alloc::dealloc(block, Layout::from_size_align(size, align).unwrap()) };
    }
    unsafe fn realloc(&self, block: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: the trait's contract.
        let moved = unsafe {
            std::alloc::realloc(block, Layout::from_size_align(old, align).unwrap(), new)
        };
        if !moved.is_null() {
            self.live.set(self.live.get() - old + new);
        }
        moved
    }
    fn micros(&self) -> u64 {
        0
    }
}

struct Fixture {
    sys: Sys,
    state: HeapState,
}

impl Fixture {
    fn new(limit: usize) -> Self {
        let state = HeapState::new();
        state.set_limit(limit, true);
        Self {
            sys: Sys::new(),
            state,
        }
    }

    fn owner(&self) -> u32 {
        self.state.admit().unwrap()
    }

    fn door(&self, owner: u32) -> Charged<'_, Sys> {
        Charged {
            inner: &self.sys,
            state: &self.state,
            owner,
        }
    }
}

#[test]
fn the_budget_is_a_ceiling_over_blocks_headers_and_padding() {
    // A small block (no header) and a large one (header): each gives back
    // exactly what it cost.
    for size in [100, 300] {
        budget_ceiling(size);
    }
}

fn budget_ceiling(size: usize) {
    let cost = block_cost(size, 8);
    let fixture = Fixture::new(3 * cost);
    let a = fixture.owner();
    let door = fixture.door(a);
    let blocks: Vec<_> = (0..3).map(|_| door.alloc(size, 8)).collect();
    assert!(blocks.iter().all(|block| !block.is_null()));
    assert_eq!(fixture.state.used(a), 3 * cost);
    // One past the budget is null, and charges nothing.
    assert!(door.alloc(size, 8).is_null());
    assert_eq!(fixture.state.used(a), 3 * cost);
    // A block gives its whole cost back.
    // SAFETY: a block of this heap with its layout.
    unsafe { door.dealloc(blocks[1], size, 8) };
    assert_eq!(fixture.state.used(a), 2 * cost);
    let again = door.alloc(size, 8);
    assert!(!again.is_null());
    for block in [blocks[0], blocks[2], again] {
        // SAFETY: as above, each once.
        unsafe { door.dealloc(block, size, 8) };
    }
    assert_eq!(fixture.state.used(a), 0);
    assert!(fixture.state.is_empty(a));
    assert_eq!(fixture.sys.live.get(), 0, "the real heap holds nothing");
}

#[test]
fn the_padding_of_a_large_alignment_counts() {
    // A page-aligned large block costs a whole page more than its size.
    assert!(block_cost(LARGE_BLOCK, 4096) > 4096 + LARGE_BLOCK - 1);
    let fixture = Fixture::new(4096);
    let a = fixture.owner();
    let door = fixture.door(a);
    assert!(
        door.alloc(LARGE_BLOCK, 4096).is_null(),
        "page + 256 is over the budget"
    );
    let block = door.alloc(1, 8);
    assert!(!block.is_null());
    assert_eq!(block.addr() % 8, 0);
    // SAFETY: a block of this heap with its layout.
    unsafe { door.dealloc(block, 1, 8) };
    // Every alignment up to the largest gives an aligned block.
    let big = Fixture::new(1 << 20);
    let a = big.owner();
    for align in [1, 2, 4, 8, 16, 64, 256, 4096] {
        for size in [24, 300] {
            let door = big.door(a);
            let block = door.alloc(size, align);
            assert!(!block.is_null());
            assert_eq!(block.addr() % align, 0, "align {align} size {size}");
            // SAFETY: a block of this heap with its layout.
            unsafe { door.dealloc(block, size, align) };
        }
    }
    assert_eq!(big.state.used(a), 0);
}

#[test]
fn a_block_freed_in_another_owners_frame_leaves_both_budgets_exact() {
    let fixture = Fixture::new(1 << 16);
    let (a, b) = (fixture.owner(), fixture.owner());
    let block_a = fixture.door(a).alloc(400, 8);
    let block_b = fixture.door(b).alloc(500, 8);
    assert_eq!(fixture.state.used(a), block_cost(400, 8));
    assert_eq!(fixture.state.used(b), block_cost(500, 8));
    // B's door frees A's block: the block's own owner gets it back.
    // SAFETY: a block of this heap with its layout, freed once.
    unsafe { fixture.door(b).dealloc(block_a, 400, 8) };
    assert_eq!(fixture.state.used(a), 0);
    assert_eq!(fixture.state.used(b), block_cost(500, 8));
    assert!(fixture.state.is_empty(a));
    // SAFETY: as above.
    unsafe { fixture.door(a).dealloc(block_b, 500, 8) };
    assert_eq!(fixture.state.used(b), 0);
    assert_eq!(fixture.sys.live.get(), 0);
}

#[test]
fn a_small_block_is_charged_to_the_frame_that_allocates_and_the_frame_that_frees() {
    let fixture = Fixture::new(1 << 16);
    let (a, b) = (fixture.owner(), fixture.owner());
    let block = fixture.door(a).alloc(50, 8);
    assert_eq!(
        fixture.state.used(a),
        56,
        "rounded up to the grain, no header"
    );
    // Freed in B's frame: B's use is clamped at zero, A's stays charged, and
    // the real heap is exact.
    // SAFETY: a block of this heap with its layout, freed once.
    unsafe { fixture.door(b).dealloc(block, 50, 8) };
    assert_eq!(fixture.state.used(b), 0);
    assert_eq!(fixture.state.used(a), 56);
    assert_eq!(fixture.sys.live.get(), 0);
}

#[test]
fn small_blocks_cost_their_size_and_no_header() {
    assert_eq!(block_cost(1, 1), 8);
    assert_eq!(block_cost(8, 8), 8);
    assert_eq!(block_cost(9, 4), 16);
    assert_eq!(block_cost(LARGE_BLOCK - 1, 1), LARGE_BLOCK);
    assert_eq!(block_cost(LARGE_BLOCK, 1), LARGE_BLOCK + HEADER);
    // Growing within the small kind charges the difference and keeps the bytes.
    let fixture = Fixture::new(1 << 12);
    let a = fixture.owner();
    let door = fixture.door(a);
    let block = door.alloc(10, 8);
    // SAFETY: a block of this heap with its layout.
    unsafe { block.write_bytes(7, 10) };
    // SAFETY: as above.
    let grown = unsafe { door.realloc(block, 10, 8, 100) };
    assert!(!grown.is_null());
    assert_eq!(fixture.state.used(a), block_cost(100, 8));
    // SAFETY: the first ten bytes are the old block's.
    assert!(
        unsafe { core::slice::from_raw_parts(grown, 10) }
            .iter()
            .all(|b| *b == 7)
    );
    // SAFETY: as above.
    unsafe { door.dealloc(grown, 100, 8) };
    assert_eq!(fixture.state.used(a), 0);
    assert_eq!(fixture.sys.live.get(), 0);
}

#[test]
fn a_block_that_crosses_the_size_class_changes_kind_and_keeps_its_bytes() {
    let fixture = Fixture::new(1 << 16);
    let a = fixture.owner();
    let door = fixture.door(a);
    let small = door.alloc(100, 8);
    // SAFETY: a block of this heap with its layout.
    unsafe { small.write_bytes(9, 100) };
    // SAFETY: as above.
    let large = unsafe { door.realloc(small, 100, 8, 1000) };
    assert!(!large.is_null());
    assert_eq!(fixture.state.used(a), block_cost(1000, 8));
    assert!(!fixture.state.is_empty(a), "a large block is in the list");
    // SAFETY: the first 100 bytes were copied.
    assert!(
        unsafe { core::slice::from_raw_parts(large, 100) }
            .iter()
            .all(|b| *b == 9)
    );
    // Back down: small again, off the list.
    // SAFETY: as above.
    let back = unsafe { door.realloc(large, 1000, 8, 40) };
    assert!(!back.is_null());
    assert_eq!(fixture.state.used(a), block_cost(40, 8));
    assert!(fixture.state.is_empty(a));
    // SAFETY: as above.
    unsafe { door.dealloc(back, 40, 8) };
    assert_eq!(fixture.state.used(a), 0);
    assert_eq!(fixture.sys.live.get(), 0);
}

#[test]
fn shrinking_across_the_class_is_never_refused_by_a_full_budget() {
    let cost = block_cost(1000, 8);
    let fixture = Fixture::new(cost);
    let a = fixture.owner();
    let door = fixture.door(a);
    let block = door.alloc(1000, 8);
    assert!(!block.is_null());
    // SAFETY: a block of this heap with its layout.
    let small = unsafe { door.realloc(block, 1000, 8, 16) };
    assert!(!small.is_null());
    // SAFETY: as above.
    unsafe { door.dealloc(small, 16, 8) };
    assert_eq!(fixture.sys.live.get(), 0);
}

#[test]
fn a_grown_block_keeps_its_owner_and_the_owner_pays() {
    let fixture = Fixture::new(block_cost(300, 8) + block_cost(1000, 8));
    let (a, b) = (fixture.owner(), fixture.owner());
    let first = fixture.door(a).alloc(300, 8);
    // B's door grows A's block; A pays, B is not charged.
    // SAFETY: a block of this heap with its layout.
    let grown = unsafe { fixture.door(b).realloc(first, 300, 8, 500) };
    assert!(!grown.is_null());
    assert_eq!(fixture.state.used(a), block_cost(500, 8));
    assert_eq!(fixture.state.used(b), 0);
    // Past A's budget: null, the block untouched and still A's.
    // SAFETY: as above.
    let refused = unsafe { fixture.door(b).realloc(grown, 500, 8, 100_000) };
    assert!(refused.is_null());
    assert_eq!(fixture.state.used(a), block_cost(500, 8));
    // Shrinking always fits and gives bytes back.
    // SAFETY: as above.
    let shrunk = unsafe { fixture.door(a).realloc(grown, 500, 8, 260) };
    assert!(!shrunk.is_null());
    assert_eq!(fixture.state.used(a), block_cost(260, 8));
    // SAFETY: as above.
    unsafe { fixture.door(a).dealloc(shrunk, 260, 8) };
    assert_eq!(fixture.state.used(a), 0);
    assert_eq!(fixture.sys.live.get(), 0);
}

#[test]
fn many_blocks_free_in_any_order_and_the_list_stays_whole() {
    let fixture = Fixture::new(1 << 24);
    let a = fixture.owner();
    let door = fixture.door(a);
    let mut blocks: Vec<(*mut u8, usize)> = (0..500)
        .map(|n| {
            let size = 1 + n % 97;
            let block = door.alloc(size, 8);
            assert!(!block.is_null());
            (block, size)
        })
        .collect();
    // Grow every fifth block, which may move it and so its neighbours' links.
    for (block, size) in blocks.iter_mut().step_by(5) {
        // SAFETY: a block of this heap with its layout.
        let moved = unsafe { door.realloc(*block, *size, 8, *size + 4000) };
        assert!(!moved.is_null());
        *block = moved;
        *size += 4000;
    }
    // A fixed shuffle: stride through the blocks.
    let order: Vec<usize> = (0..500).map(|n| (n * 137) % 500).collect();
    for at in order {
        let (block, size) = blocks[at];
        // SAFETY: each block freed once with its layout.
        unsafe { door.dealloc(block, size, 8) };
    }
    assert_eq!(fixture.state.used(a), 0);
    assert!(fixture.state.is_empty(a));
    assert_eq!(fixture.sys.live.get(), 0);
}

#[test]
fn a_limit_set_below_the_use_reclaims_nothing_and_blocks_later_allocations() {
    let fixture = Fixture::new(1 << 16);
    let a = fixture.owner();
    let door = fixture.door(a);
    let block = door.alloc(1000, 8);
    let used = fixture.state.used(a);
    fixture.state.set_limit(used / 2, true);
    assert_eq!(fixture.state.used(a), used, "nothing reclaimed");
    assert!(door.alloc(8, 8).is_null());
    // SAFETY: a block of this heap with its layout.
    unsafe { door.dealloc(block, 1000, 8) };
    assert!(!door.alloc(8, 8).is_null(), "under the limit again");
    // Zero: every allocation is null.
    fixture.state.set_limit(0, true);
    assert!(door.alloc(1, 1).is_null());
}

#[test]
fn a_foreign_block_header_is_left_alone() {
    let fixture = Fixture::new(1 << 16);
    let a = fixture.owner();
    let door = fixture.door(a);
    let block = door.alloc(400, 8);
    // A wrong size larger than the use: refused, nothing changes.
    // SAFETY: the block is this heap's; the size is a plugin fault the heap tolerates.
    unsafe { door.dealloc(block, 1 << 20, 8) };
    assert_eq!(fixture.state.used(a), block_cost(400, 8));
    // SAFETY: the right call.
    unsafe { door.dealloc(block, 400, 8) };
    assert_eq!(fixture.state.used(a), 0);
}

#[test]
fn the_defaults_are_pinned_and_a_class_set_later_does_not_override_a_named_limit() {
    assert_eq!(PLUGIN_HEAP_SMALL, 48 * 1024);
    assert_eq!(PLUGIN_HEAP_LARGE, 4 * 1024 * 1024);
    assert_eq!(default_plugin_heap(MemoryClass::Small), 48 * 1024);
    assert_eq!(default_plugin_heap(MemoryClass::Large), 4 * 1024 * 1024);
    let state = HeapState::new();
    assert_eq!(state.limit(), PLUGIN_HEAP_SMALL);
    state.set_limit(PLUGIN_HEAP_LARGE, false);
    assert_eq!(state.limit(), PLUGIN_HEAP_LARGE);
    state.set_limit(1000, true);
    state.set_limit(PLUGIN_HEAP_LARGE, false);
    assert_eq!(state.limit(), 1000, "the shell's own limit wins");
}

/// The header is pinned per target (the X4 pays 12 bytes a block).
#[test]
fn the_header_size_is_pinned() {
    assert_eq!(
        HEADER,
        if cfg!(target_pointer_width = "32") {
            12
        } else {
            24
        }
    );
    assert_eq!(block_cost(LARGE_BLOCK, 1), LARGE_BLOCK + HEADER);
    assert_eq!(block_cost(LARGE_BLOCK, 4096), 4096 + LARGE_BLOCK);
    assert_eq!(LARGE_BLOCK, 256);
}
