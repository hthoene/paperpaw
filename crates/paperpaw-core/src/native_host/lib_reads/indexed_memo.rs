//! Whether each book of a page is indexed, remembered for the library
//! generation it was found in: checking costs the card a look at the book's
//! details file (two where there is none), and every call of a list asks it
//! of every row again. Kept here, in the host, not in a type plugins share.
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_sdk::digest::fnv1a64;

/// The books remembered at once: one word each, 1 KiB in all. A book whose
/// place another took is looked at again.
const SLOTS: usize = 256;

#[derive(Default)]
pub(super) struct IndexedMemo {
    /// The generation the entries were found in; 0 where none are kept
    /// (the catalog is being made, or nothing was asked yet).
    generation: Cell<u32>,
    /// A tag of the book and whether it is indexed, as `tag | indexed`;
    /// 0 for no entry. Made when the first one is kept.
    slots: RefCell<Vec<u32>>,
}

impl IndexedMemo {
    /// The entries found in `generation` stand; those of another are
    /// dropped. `None` keeps nothing (and forgets), as while the catalog is
    /// made: the books are indexed under it with no generation to say so.
    pub(super) fn begin(&self, generation: Option<u32>) {
        let generation = generation.unwrap_or(0);
        if self.generation.replace(generation) != generation {
            self.slots.borrow_mut().fill(0);
        }
    }

    fn place(path: &str, size: Option<u64>) -> (usize, u32) {
        let hash = fnv1a64(path.as_bytes()) ^ size.unwrap_or(0).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        // The high bit marks an entry; the lowest is the answer.
        let tag = ((hash >> 32) as u32 | 1 << 31) & !1;
        (hash as usize % SLOTS, tag)
    }

    /// What was found for the book, where it was in this generation.
    pub(super) fn get(&self, path: &str, size: Option<u64>) -> Option<bool> {
        if self.generation.get() == 0 {
            return None;
        }
        let (at, tag) = Self::place(path, size);
        let entry = *self.slots.borrow().get(at)?;
        (entry & !1 == tag).then_some(entry & 1 == 1)
    }

    /// Keeps `indexed` for the book; nothing where no generation is held or
    /// no room is left.
    pub(super) fn keep(&self, path: &str, size: Option<u64>, indexed: bool) {
        if self.generation.get() == 0 {
            return;
        }
        let mut slots = self.slots.borrow_mut();
        if slots.is_empty() {
            if slots.try_reserve_exact(SLOTS).is_err() {
                return;
            }
            slots.resize(SLOTS, 0);
        }
        let (at, tag) = Self::place(path, size);
        slots[at] = tag | u32::from(indexed);
    }
}
