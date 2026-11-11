//! Giving back the room a list kept while it grew by doubling, without the
//! abort `Vec::shrink_to_fit` has where the allocator refuses: on the X4 a
//! shrink may allocate, copy and free, and the heap is at its fullest right
//! after the plugins are hosted.

use alloc::vec::Vec;

/// Makes `list` exactly as large as it is long, if a list of that size can be
/// made; the old room stays where it cannot (nothing is lost, nothing aborts).
pub(crate) fn shrink<T>(list: &mut Vec<T>) {
    if list.capacity() == list.len() {
        return;
    }
    let mut exact = Vec::new();
    if exact.try_reserve_exact(list.len()).is_err() {
        return;
    }
    exact.append(list);
    *list = exact;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_gives_back_its_reserve_and_keeps_its_items() {
        let mut list: Vec<u32> = Vec::new();
        for n in 0..13 {
            list.push(n);
        }
        assert!(list.capacity() >= 16);
        shrink(&mut list);
        assert_eq!((list.len(), list.capacity()), (13, 13));
        assert!(list.iter().copied().eq(0..13));
        // Already exact, or empty: nothing to do.
        shrink(&mut list);
        let mut none: Vec<u32> = Vec::with_capacity(4);
        shrink(&mut none);
        assert_eq!((none.len(), none.capacity()), (0, 0));
    }
}
