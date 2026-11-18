//! The sorts when the heap refuses memory: the same order as the standard
//! library's, never an abort.
use paperpaw_sort::{
    sort, sort_by, sort_by_cached_key, sort_by_key, sort_unstable, sort_unstable_by,
    sort_unstable_by_key,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Refuses the allocations of `FAIL` (the n-th big one from now, or all).
struct Refusing;

thread_local! {
    /// 0: no refusal. n > 0: refuse the n-th allocation of 64 bytes or more
    /// from now (once). `usize::MAX`: refuse every one.
    static FAIL: Cell<usize> = const { Cell::new(0) };
    /// Every allocation of this thread, of any size.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: forwards to `System`, refusing (null) only where a refusal is armed.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        if layout.size() >= 64 {
            let refuse = FAIL.with(|fail| match fail.get() {
                0 => false,
                usize::MAX => true,
                1 => {
                    fail.set(0);
                    true
                }
                n => {
                    fail.set(n - 1);
                    false
                }
            });
            if refuse {
                return core::ptr::null_mut();
            }
        }
        // SAFETY: the caller's layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by `System` above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

fn list(len: usize) -> Vec<(u8, u16)> {
    let mut seed = 7_u32;
    (0..len)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (
                u8::try_from(seed >> 29).unwrap_or(0),
                u16::try_from(seed >> 8 & 0xFFFF).unwrap_or(0),
            )
        })
        .collect()
}

/// Runs `sorted` with the refusal armed as `fail` says.
fn with_refusal<R>(fail: usize, sorted: impl FnOnce() -> R) -> R {
    FAIL.with(|cell| cell.set(fail));
    let result = sorted();
    FAIL.with(|cell| cell.set(0));
    result
}

#[test]
fn a_refused_permutation_is_sorted_in_place() {
    for len in [0, 1, 2, 13, 100, 1000] {
        let items = list(len);
        let mut expected = items.clone();
        expected.sort_by_key(|item| item.0);
        let mut by_key = items.clone();
        with_refusal(usize::MAX, || sort_by_key(&mut by_key, |item| item.0));
        assert_eq!(by_key, expected, "sort_by_key, {len} items");
        let mut by = items.clone();
        with_refusal(usize::MAX, || sort_by(&mut by, |a, b| a.0.cmp(&b.0)));
        assert_eq!(by, expected, "sort_by, {len} items");
        let mut plain = items.clone();
        let mut expected_plain = items;
        expected_plain.sort();
        with_refusal(usize::MAX, || sort(&mut plain));
        assert_eq!(plain, expected_plain, "sort, {len} items");
    }
}

#[test]
fn a_cached_key_sort_survives_either_allocation_being_refused() {
    for len in [2, 13, 100, 1000] {
        let items = list(len);
        let mut expected = items.clone();
        expected.sort_by_key(|item| item.0);
        // 1: the keys are refused (the key function is called per comparison);
        // 2: the keys fit, the permutation is refused (in place, keys move);
        // all: nothing fits.
        for fail in [1, 2, usize::MAX] {
            let mut cached = items.clone();
            with_refusal(fail, || sort_by_cached_key(&mut cached, |item| item.0));
            assert_eq!(cached, expected, "refusal {fail}, {len} items");
        }
    }
}

/// How many allocations `sorted` makes.
fn allocations(sorted: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    sorted();
    ALLOCATIONS.with(Cell::get) - before
}

#[test]
fn up_to_twelve_items_are_sorted_in_place_without_any_allocation() {
    for len in 0..=12 {
        let items = list(len);
        let mut expected = items.clone();
        expected.sort_by_key(|item| item.0);
        let mut expected_plain = items.clone();
        expected_plain.sort();

        let mut by_key = items.clone();
        assert_eq!(
            allocations(|| sort_by_key(&mut by_key, |item| item.0)),
            0,
            "sort_by_key, {len} items"
        );
        assert_eq!(by_key, expected);
        let mut by = items.clone();
        assert_eq!(
            allocations(|| sort_by(&mut by, |a, b| a.0.cmp(&b.0))),
            0,
            "sort_by, {len} items"
        );
        assert_eq!(by, expected);
        let mut cached = items.clone();
        assert_eq!(
            allocations(|| sort_by_cached_key(&mut cached, |item| item.0)),
            0,
            "sort_by_cached_key, {len} items"
        );
        assert_eq!(cached, expected);
        let mut plain = items.clone();
        assert_eq!(allocations(|| sort(&mut plain)), 0, "sort, {len} items");
        assert_eq!(plain, expected_plain);
    }
    // The counter sees allocations: a longer list needs its positions.
    let mut longer = list(13);
    assert!(allocations(|| sort(&mut longer)) > 0);
}

#[test]
fn the_unstable_sorts_allocate_nothing_at_any_length() {
    for len in [0, 1, 12, 13, 100, 2_049, 6_000] {
        let items = list(len);
        let mut expected = items.clone();
        expected.sort();

        let mut plain = items.clone();
        assert_eq!(allocations(|| sort_unstable(&mut plain)), 0, "{len} items");
        assert_eq!(plain, expected);
        let mut by = items.clone();
        assert_eq!(
            allocations(|| sort_unstable_by(&mut by, |a, b| a.cmp(b))),
            0,
            "sort_unstable_by, {len} items"
        );
        assert_eq!(by, expected);
        let mut by_key = items.clone();
        assert_eq!(
            allocations(|| sort_unstable_by_key(&mut by_key, |item| (item.0, item.1))),
            0,
            "sort_unstable_by_key, {len} items"
        );
        assert_eq!(by_key, expected);
    }
}
