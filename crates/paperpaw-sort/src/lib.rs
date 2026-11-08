//! One shared sorter (#228, flash gate lever E).
//!
//! `slice::sort_by` and friends compile a full copy of the sort for every
//! element type and comparison closure: on the X4 that is about 170 KB of
//! flash in more than fifty copies. The sorts here compile **one** sort,
//! driven through a type-erased `less(i, j)` / `swap(i, j)` pair, and the thin
//! generic wrappers ([`sort`], [`sort_by`], [`sort_by_key`],
//! [`sort_by_cached_key`]) hold no sort of their own.
//!
//! **Order.** Every function is **stable**: equal items keep their order, so
//! a result equals that of the standard library's `sort`, `sort_by` and
//! `sort_by_key` (and, for a total order, that of the unstable sorts).
//!
//! **How.** Up to 2,048 items, the positions `0..n` (as `u32`) are sorted by the items they
//! stand for (ties by position, which makes the order stable), with one
//! compiled `sort_unstable_by`, and the items are then moved into place by
//! the cycles of that permutation: `4 * n` bytes, and about 1.0 to 1.6 times
//! the time of the standard library's sort on lists of 2,000 to 10,000
//! entries (the comparison is one indirect call).
//!
//! **Long lists.** From `POSITIONS_MAX` (2,048) items on, the sort goes
//! straight to the in-place path: a list of 10,000 would hold 40 KB for the
//! positions (120 KB with `u64` keys cached), too much of the X4's heap at
//! its peak, for a sort that is about 2.5 to 3 times slower in place.
//! (Measured on x86: 10,000 entries 0.9 ms through positions, 2.8 ms in place.)
//!
//! **Short lists.** Up to 12 items (`BLOCK`) are insertion-sorted in place,
//! with no allocation.
//!
//! **No memory.** When the heap refuses the `4 * n` bytes (or `n` does not fit
//! a `u32`), the sort is done in place: insertion-sorted blocks merged by
//! `SymMerge` rotations, `O(n log n)` comparisons and `O(n log² n)` swaps, with
//! no allocation. [`sort_by_cached_key`] also keeps its `n` keys and, when
//! the heap refuses those, compares by calling the key function instead.
//! Nothing here aborts or panics on a full heap.
//!
//! **A comparison that is no total order** (it contradicts itself: `a < b`,
//! `b < c` and `c < a`, or it differs from one call to the next) gives an
//! order that is not said, and never loses or repeats an item. Lists of up to
//! 12 items, lists from 2,048 items on and lists sorted without memory run
//! on loops that never panic on it. A list of 13 to 2,048 items is ordered
//! through its positions by the standard library's `sort_unstable_by`, which
//! **may panic** ("user-provided comparison function does not correctly
//! implement a total order", Rust 1.81 and later), as `slice::sort_by` itself
//! may; the X4 aborts on a panic. So write the comparison as a total order:
//! compare the same fields in the same direction, a float by `total_cmp`,
//! never by anything that changes while sorting.
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::cmp::Ordering;

/// The two operations the sort needs on a sequence of `len` items.
trait Ops {
    /// How the item at `a` compares to the item at `b`.
    fn compare(&mut self, a: usize, b: usize) -> Ordering;
    /// Whether the item at `a` sorts strictly before the item at `b`.
    fn less(&mut self, a: usize, b: usize) -> bool;
    /// Exchanges the items at `a` and `b`.
    fn swap(&mut self, a: usize, b: usize);
}

/// Items compared by a function of two items.
struct ByCompare<'a, T, F> {
    items: &'a mut [T],
    compare: F,
}

impl<T, F: FnMut(&T, &T) -> Ordering> Ops for ByCompare<'_, T, F> {
    fn compare(&mut self, a: usize, b: usize) -> Ordering {
        (self.compare)(&self.items[a], &self.items[b])
    }
    fn less(&mut self, a: usize, b: usize) -> bool {
        self.compare(a, b) == Ordering::Less
    }
    fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }
}

/// Items with their keys, which move together.
struct Cached<'a, T, K> {
    items: &'a mut [T],
    keys: Vec<K>,
}

impl<T, K: Ord> Ops for Cached<'_, T, K> {
    fn compare(&mut self, a: usize, b: usize) -> Ordering {
        self.keys[a].cmp(&self.keys[b])
    }
    fn less(&mut self, a: usize, b: usize) -> bool {
        self.keys[a] < self.keys[b]
    }
    fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
        self.keys.swap(a, b);
    }
}

/// Sorts `items` by `compare`, stably. `compare` must be a total order
/// (see the crate documentation): where it is not, the order is not said and,
/// for 13 to 2,048 items, the sort may panic.
pub fn sort_by<T>(items: &mut [T], compare: impl FnMut(&T, &T) -> Ordering) {
    let len = items.len();
    stable(len, &mut ByCompare { items, compare });
}

/// Sorts `items` by `key`, stably. `key` is called again for every
/// comparison, like `slice::sort_by_key`; use [`sort_by_cached_key`] for a
/// key that costs.
pub fn sort_by_key<T, K: Ord>(items: &mut [T], mut key: impl FnMut(&T) -> K) {
    sort_by(items, |a, b| key(a).cmp(&key(b)));
}

/// Sorts `items` by `key`, stably, calling `key` once per item (like
/// `slice::sort_by_cached_key`). Up to 12 items, from `POSITIONS_MAX` on, or
/// without room for the keys, it calls `key` for every comparison instead and
/// allocates nothing.
pub fn sort_by_cached_key<T, K: Ord>(items: &mut [T], mut key: impl FnMut(&T) -> K) {
    let len = items.len();
    if len <= BLOCK || len > POSITIONS_MAX {
        // Too few for the keys to be worth a block of memory, or too many to
        // hold them (and the positions) at once.
        return sort_by_key(items, key);
    }
    let mut keys = Vec::new();
    if keys.try_reserve_exact(len).is_err() {
        return sort_by_key(items, key);
    }
    keys.extend(items.iter().map(&mut key));
    stable(len, &mut Cached { items, keys });
}

/// Sorts `items`, stably.
pub fn sort<T: Ord>(items: &mut [T]) {
    sort_by(items, T::cmp);
}

/// The sort through positions: `u32` positions ordered by the (unmoved) items,
/// ties by position, then the items moved by the cycles of the permutation.
fn by_positions(len: usize, ops: &mut dyn Ops) -> bool {
    let Ok(last) = u32::try_from(len) else {
        return false;
    };
    let mut order: Vec<u32> = Vec::new();
    if order.try_reserve_exact(len).is_err() {
        return false;
    }
    order.extend(0..last);
    order.sort_unstable_by(|&a, &b| ops.compare(a as usize, b as usize).then(a.cmp(&b)));
    for start in 0..len {
        let mut at = start;
        loop {
            let from = order[at] as usize;
            order[at] = u32::try_from(at).unwrap_or(u32::MAX);
            if from == start {
                break;
            }
            ops.swap(at, from);
            at = from;
        }
    }
    true
}

/// Items per insertion-sorted block.
const BLOCK: usize = 12;

/// The longest list sorted through positions: its `4 * n` bytes (8 KiB) are
/// the most the sorts ask for. A longer one is sorted in place.
const POSITIONS_MAX: usize = 2_048;

/// The one sort: through positions, or in place without memory.
fn stable(len: usize, ops: &mut dyn Ops) {
    if len <= BLOCK {
        // Menus, settings and most lists: in place, no allocation at all.
        insertion(ops, 0, len);
    } else if len > POSITIONS_MAX || !by_positions(len, ops) {
        in_place(len, ops);
    }
}

/// The sort without memory: insertion-sorted blocks merged by doubling widths.
fn in_place(len: usize, ops: &mut dyn Ops) {
    let mut start = 0;
    while start < len {
        insertion(ops, start, (start + BLOCK).min(len));
        start += BLOCK;
    }
    let mut width = BLOCK;
    while width < len {
        let mut a = 0;
        while a + width < len {
            let end = (a + 2 * width).min(len);
            sym_merge(ops, a, a + width, end);
            a = end;
        }
        width *= 2;
    }
}

/// Stable insertion sort of `[a, b)`.
fn insertion(ops: &mut dyn Ops, a: usize, b: usize) {
    for end in a + 1..b {
        let mut at = end;
        while at > a && ops.less(at, at - 1) {
            ops.swap(at, at - 1);
            at -= 1;
        }
    }
}

/// Merges the sorted `[a, m)` and `[m, b)` in place, stably (`SymMerge`,
/// Kim and Kutzner 2004): binary search for the split that makes both halves
/// independent, rotate the middle, recurse on the two sides.
fn sym_merge(ops: &mut dyn Ops, a: usize, m: usize, b: usize) {
    if m - a == 1 {
        // One item on the left: insert it into the right by binary search.
        let (mut lo, mut hi) = (m, b);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if ops.less(mid, a) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        for k in a..lo - 1 {
            ops.swap(k, k + 1);
        }
        return;
    }
    if b - m == 1 {
        // One item on the right: insert it into the left.
        let (mut lo, mut hi) = (a, m);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if ops.less(m, mid) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        for k in (lo + 1..=m).rev() {
            ops.swap(k, k - 1);
        }
        return;
    }
    let mid = a + (b - a) / 2;
    let n = mid + m;
    let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
    let p = n - 1;
    while start < r {
        let c = start + (r - start) / 2;
        if ops.less(p - c, c) {
            r = c;
        } else {
            start = c + 1;
        }
    }
    let end = n - start;
    if start < m && m < end {
        rotate(ops, start, m, end);
    }
    if a < start && start < mid {
        sym_merge(ops, a, start, mid);
    }
    if mid < end && end < b {
        sym_merge(ops, mid, end, b);
    }
}

/// Rotates `[a, b)` so that `[m, b)` comes first (block swaps).
fn rotate(ops: &mut dyn Ops, a: usize, m: usize, b: usize) {
    let (mut i, mut j) = (m - a, b - m);
    while i != j {
        if i > j {
            swap_range(ops, m - i, m, j);
            i -= j;
        } else {
            swap_range(ops, m - i, m + j - i, i);
            j -= i;
        }
    }
    swap_range(ops, m - i, m, i);
}

fn swap_range(ops: &mut dyn Ops, a: usize, b: usize, n: usize) {
    for i in 0..n {
        ops.swap(a + i, b + i);
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn in_place_by<T>(items: &mut [T], compare: impl FnMut(&T, &T) -> Ordering) {
        let len = items.len();
        in_place(len, &mut ByCompare { items, compare });
    }

    #[test]
    fn small_inputs() {
        let mut none: [u8; 0] = [];
        sort(&mut none);
        let mut one = [3];
        sort(&mut one);
        assert_eq!(one, [3]);
        let mut few = [3, 1, 2, 1];
        sort(&mut few);
        assert_eq!(few, [1, 1, 2, 3]);
    }

    #[test]
    fn a_cached_key_is_computed_once_per_item() {
        let mut calls = 0;
        let mut items: [u32; 20] = core::array::from_fn(|at| (at as u32 * 7) % 20);
        sort_by_cached_key(&mut items, |item| {
            calls += 1;
            *item
        });
        assert_eq!(calls, 20);
        assert!(items.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    /// A comparison that is no total order gives some order of the same items,
    /// without a panic, where the module says so: up to 12 items, from 2,048
    /// items on, and in place.
    #[test]
    fn a_comparison_that_is_no_total_order_loses_no_item_where_it_does_not_panic() {
        let cycle = |a: &u32, b: &u32| match (a + 3 - b % 3) % 3 {
            0 => Ordering::Equal,
            1 => Ordering::Less,
            _ => Ordering::Greater,
        };
        for len in [0, 1, 5, 12, 13, 40, 300, 2_049, 3_000] {
            let items: Vec<u32> = (0..len as u32)
                .map(|at| at.wrapping_mul(2_654_435_761))
                .collect();
            let mut all = items.clone();
            all.sort_unstable();
            let same_items = |sorted: &[u32]| {
                let mut left = sorted.to_vec();
                left.sort_unstable();
                left == all
            };
            let mut place = items.clone();
            in_place_by(&mut place, cycle);
            assert!(same_items(&place), "in place, len {len}");
            if len <= BLOCK || len > POSITIONS_MAX {
                let mut sorted = items.clone();
                sort_by(&mut sorted, cycle);
                assert!(same_items(&sorted), "len {len}");
                // Always less, always greater, and a coin that flips.
                let mut flips = 0_u32;
                for compare in [0_u8, 1, 2] {
                    let mut again = items.clone();
                    sort_by(&mut again, |_, _| match compare {
                        0 => Ordering::Less,
                        1 => Ordering::Greater,
                        _ => {
                            flips = flips.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                            if (flips >> 16) & 1 == 0 {
                                Ordering::Less
                            } else {
                                Ordering::Greater
                            }
                        }
                    });
                    assert!(same_items(&again), "len {len}, compare {compare}");
                }
            }
        }
    }

    proptest! {
        /// Many equal keys, so that stability decides the result.
        #[test]
        fn equals_the_standard_sorts(
            items in proptest::collection::vec((0_u8..6, any::<u16>()), 0..300)
        ) {
            let mut by_key = items.clone();
            by_key.sort_by_key(|item| item.0);
            let mut mine = items.clone();
            sort_by_key(&mut mine, |item| item.0);
            prop_assert_eq!(&mine, &by_key);
            let mut cached = items.clone();
            sort_by_cached_key(&mut cached, |item| item.0);
            prop_assert_eq!(&cached, &by_key);
            let mut by = items.clone();
            by.sort_by_key(|item| core::cmp::Reverse(item.0));
            let mut mine_by = items.clone();
            sort_by(&mut mine_by, |a, b| b.0.cmp(&a.0));
            prop_assert_eq!(&mine_by, &by);
            let mut plain = items.clone();
            plain.sort();
            let mut mine_plain = items.clone();
            sort(&mut mine_plain);
            prop_assert_eq!(&mine_plain, &plain);
            // The memoryless path gives the same order.
            let mut place = items.clone();
            in_place_by(&mut place, |a, b| a.0.cmp(&b.0));
            prop_assert_eq!(&place, &by_key);
        }

        #[test]
        fn the_in_place_sort_is_stable_for_long_lists(
            items in proptest::collection::vec((0_u8..3, any::<u8>()), 200..900)
        ) {
            let mut expected = items.clone();
            expected.sort_by_key(|item| item.0);
            let mut place = items;
            in_place_by(&mut place, |a, b| a.0.cmp(&b.0));
            prop_assert_eq!(place, expected);
        }
    }
}
