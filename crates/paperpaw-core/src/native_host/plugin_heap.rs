//! The plugin heap's accounting (`abi/heap-word.md` section 4): who owns each
//! block a plugin allocates, and how much of its budget a plugin has used.
//!
//! Both doors of the host heap (the host table's slots 0–2 and the `PpHeap`
//! table) allocate through [`Charged`]. Two kinds of block:
//!
//! - A **large** block (`LARGE_BLOCK` bytes or more) gets a [`Header`] in front
//!   of it: the owner (an index into the device's owner table) and the links of
//!   that owner's block list. `dealloc` and `realloc` follow the block's header,
//!   never the frame, so a block freed in another plugin's frame leaves both
//!   budgets exact, and the list lets the teardown sweep find every large
//!   block a plugin still owns without walking the heap. Unlinking is O(1).
//! - A **small** block has no header: a plugin holds hundreds of them (strings,
//!   list items), and 12 bytes each on the X4 would be kilobytes of heap
//!   Beta 3 did not spend. It is charged by its size, rounded up to
//!   `SMALL_GRAIN`, to the owner of the frame that allocates it and credited
//!   to the owner of the frame that frees it. A plugin frees its own blocks, in
//!   its own frames (the destructor runs in the owner's frame); a small block
//!   freed in another plugin's frame leaves that budget short by its size
//!   (clamped at zero) and the allocator's own heap exact. A small block a
//!   plugin leaks is counted in its use and never found by the sweep.
//!
//! The size decides the kind, at `alloc`, `dealloc` and `realloc`: a plugin
//! passes the size it allocated with (`abi/heap-word.md` section 3).
//!
//! The budget is a ceiling, not a reservation: the header and the padding an
//! alignment needs count against it, and a full real heap still gives null.

use alloc::vec::Vec;
use core::alloc::Layout;
use core::cell::{Cell, RefCell};
use core::ptr;
use paperpaw_native::host_glue::Services;
use paperpaw_platform::MemoryClass;

/// The per-plugin heap budget of a `Small` device (the X4), in bytes. A
/// device measurement may revise it.
pub const PLUGIN_HEAP_SMALL: usize = 48 * 1024;
/// The per-plugin heap budget of a `Large` device, in bytes.
pub const PLUGIN_HEAP_LARGE: usize = 4 * 1024 * 1024;

/// The default per-plugin heap budget of a memory class. A class added later
/// gets the small budget until someone decides otherwise (fail closed, as
/// the other limits by class do).
#[must_use]
pub const fn default_plugin_heap(class: MemoryClass) -> usize {
    match class {
        MemoryClass::Large => PLUGIN_HEAP_LARGE,
        _ => PLUGIN_HEAP_SMALL,
    }
}

/// What is kept in front of every plugin block.
#[repr(C)]
struct Header {
    prev: *mut Header,
    next: *mut Header,
    /// The owner's place in the device's owner table.
    owner: u32,
}

/// Blocks of at least this many bytes get a header; smaller ones are charged
/// by size (the header would be more than 5 % of the block, and a plugin holds
/// hundreds of small ones).
pub const LARGE_BLOCK: usize = 256;

/// The unit a small block is charged in.
const SMALL_GRAIN: usize = 8;

const fn is_large(size: usize) -> bool {
    size >= LARGE_BLOCK
}

const HEADER: usize = core::mem::size_of::<Header>();
const HEADER_ALIGN: usize = core::mem::align_of::<Header>();

// The header costs 12 bytes on the 32-bit targets (the X4) and 24 on 64-bit
// hosts (`abi/heap-word.md` section 4); a change is a decision.
#[cfg(target_pointer_width = "32")]
const _: () = assert!(HEADER == 12);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(HEADER == 24);

/// Where a block of alignment `align` starts after its header: the header's
/// size rounded up to the alignment, so the block is aligned and the header
/// ends right before it.
const fn offset(align: usize) -> usize {
    HEADER.next_multiple_of(align)
}

/// What a block of `size` bytes aligned to `align` costs its owner: a large
/// block its header, the padding the alignment needs, and the block; a small
/// one its size rounded up to `SMALL_GRAIN`.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "read by the budget tests; the teardown sweep (PR 3) reads it"
    )
)]
#[must_use]
pub(crate) const fn block_cost(size: usize, align: usize) -> usize {
    if is_large(size) {
        offset(align) + size
    } else {
        size.next_multiple_of(SMALL_GRAIN)
    }
}

/// One owner's use of the heap.
struct Owner {
    /// Bytes charged: headers, padding and blocks.
    used: usize,
    /// The newest block, or null.
    head: *mut Header,
}

/// The device-wide table of owners' heap use, and the budget each may reach.
pub(crate) struct HeapState {
    limit: Cell<usize>,
    /// A shell named the limit itself: a class's default no longer applies.
    named: Cell<bool>,
    owners: RefCell<Vec<Owner>>,
}

impl core::fmt::Debug for HeapState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeapState")
            .field("limit", &self.limit.get())
            .finish_non_exhaustive()
    }
}

impl HeapState {
    pub(crate) const fn new() -> Self {
        Self {
            limit: Cell::new(PLUGIN_HEAP_SMALL),
            named: Cell::new(false),
            owners: RefCell::new(Vec::new()),
        }
    }

    /// Sets the budget: `named` by the shell outright (it wins over every
    /// default), otherwise the default of a memory class (kept while the shell
    /// named none). Read at each charge, so a limit below a plugin's use
    /// reclaims nothing: its later allocations are null until its use drops
    /// below the limit.
    pub(crate) fn set_limit(&self, bytes: usize, named: bool) {
        if named {
            self.named.set(true);
        } else if self.named.get() {
            return;
        }
        self.limit.set(bytes);
    }

    #[cfg(test)]
    pub(crate) fn limit(&self) -> usize {
        self.limit.get()
    }

    /// The owners admitted, and the room the table holds.
    #[cfg(test)]
    pub(crate) fn owners_room(&self) -> (usize, usize) {
        let owners = self.owners.borrow();
        (owners.len(), owners.capacity())
    }

    /// A place for one more owner, reserved without an abort. Reserved
    /// exactly (#248): the table only grows, one owner at each admission (not
    /// a hot path), and a doubling one held room for up to as many owners
    /// again for the whole run.
    ///
    /// # Errors
    /// `Err(())` when there is no room for it.
    pub(crate) fn admit(&self) -> Result<u32, ()> {
        let mut owners = self.owners.try_borrow_mut().map_err(|_| ())?;
        owners.try_reserve_exact(1).map_err(|_| ())?;
        let index = u32::try_from(owners.len()).map_err(|_| ())?;
        owners.push(Owner {
            used: 0,
            head: ptr::null_mut(),
        });
        Ok(index)
    }

    /// Whether owner place `index` was handed out.
    pub(crate) fn has(&self, index: u32) -> bool {
        self.owners
            .try_borrow()
            .is_ok_and(|owners| (index as usize) < owners.len())
    }

    /// The owner places handed out (tests).
    #[cfg(test)]
    pub(crate) fn rows(&self) -> usize {
        self.owners.try_borrow().map_or(0, |owners| owners.len())
    }

    /// The bytes charged to owner `index` now.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "read by the budget tests; the teardown sweep (PR 3) reads it"
        )
    )]
    pub(crate) fn used(&self, index: u32) -> usize {
        self.owners
            .try_borrow()
            .ok()
            .and_then(|owners| owners.get(index as usize).map(|owner| owner.used))
            .unwrap_or(0)
    }

    /// Whether owner `index` has no block left.
    #[cfg(test)]
    pub(crate) fn is_empty(&self, index: u32) -> bool {
        self.owners
            .try_borrow()
            .ok()
            .and_then(|owners| owners.get(index as usize).map(|owner| owner.head.is_null()))
            .unwrap_or(true)
    }
}

/// The shell's allocator with owners and a budget: what both doors of the
/// plugin heap allocate through. Charges `owner` on `alloc`; frees and grows
/// follow the block's own header.
pub(crate) struct Charged<'a, S> {
    pub(crate) inner: &'a S,
    pub(crate) state: &'a HeapState,
    /// The owner an allocation is charged to.
    pub(crate) owner: u32,
}

impl<S: Services> Charged<'_, S> {
    /// The header of the large block at `block`.
    fn header(block: *mut u8) -> *mut Header {
        block.wrapping_sub(HEADER).cast()
    }

    /// The start of the whole allocation of the large block at `block`.
    fn base(block: *mut u8, align: usize) -> *mut u8 {
        block.wrapping_sub(offset(align))
    }

    fn layout(total: usize, align: usize) -> Option<Layout> {
        Layout::from_size_align(total, align.max(HEADER_ALIGN)).ok()
    }

    /// A block charged to `self.owner`, with `credit` bytes of its use about
    /// to be given back (a realloc that moves).
    fn take(&self, size: usize, align: usize, credit: usize) -> *mut u8 {
        if Layout::from_size_align(size, align).is_err() {
            return ptr::null_mut();
        }
        let large = is_large(size);
        let total = if large {
            let Some(total) = offset(align).checked_add(size) else {
                return ptr::null_mut();
            };
            total
        } else {
            block_cost(size, align)
        };
        let Ok(mut owners) = self.state.owners.try_borrow_mut() else {
            return ptr::null_mut();
        };
        let Some(owner) = owners.get_mut(self.owner as usize) else {
            return ptr::null_mut();
        };
        match owner.used.saturating_sub(credit).checked_add(total) {
            Some(used) if used <= self.state.limit.get() => {}
            _ => return ptr::null_mut(),
        }
        if !large {
            let block = self.inner.alloc(size, align);
            if !block.is_null() {
                owner.used += total;
            }
            return block;
        }
        let Some(layout) = Self::layout(total, align) else {
            return ptr::null_mut();
        };
        let base = self.inner.alloc(layout.size(), layout.align());
        if base.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: `base` is `total` bytes aligned to `HEADER_ALIGN` at least
        // and `offset(align)` is a multiple of it (see `offset`), so the header
        // ends right before the block, inside the allocation and aligned.
        let block = unsafe { base.add(offset(align)) };
        let header = Self::header(block);
        // SAFETY: as above; the old head, if any, is a live header of this owner.
        unsafe {
            header.write(Header {
                prev: ptr::null_mut(),
                next: owner.head,
                owner: self.owner,
            });
            if let Some(next) = owner.head.as_mut() {
                next.prev = header;
            }
        }
        owner.head = header;
        owner.used += total;
        block
    }

    /// A small block of the owner of this door, grown or shrunk in place of
    /// the allocator's own `realloc` (a small block stays small).
    ///
    /// # Safety
    /// The contract of `Services::realloc` for a block `alloc` made small.
    unsafe fn realloc_small(
        &self,
        block: *mut u8,
        old_size: usize,
        align: usize,
        new_size: usize,
    ) -> *mut u8 {
        if Layout::from_size_align(new_size, align).is_err() {
            return ptr::null_mut();
        }
        let (old_cost, new_cost) = (block_cost(old_size, align), block_cost(new_size, align));
        let Ok(mut owners) = self.state.owners.try_borrow_mut() else {
            return ptr::null_mut();
        };
        let Some(owner) = owners.get_mut(self.owner as usize) else {
            return ptr::null_mut();
        };
        let rest = owner.used.saturating_sub(old_cost);
        if new_cost > old_cost && rest.saturating_add(new_cost) > self.state.limit.get() {
            return ptr::null_mut();
        }
        // SAFETY: the caller's contract.
        let moved = unsafe { self.inner.realloc(block, old_size, align, new_size) };
        if !moved.is_null() {
            owner.used = rest + new_cost;
        }
        moved
    }

    /// A block that changes kind (small to large or back): a new block, the
    /// bytes that fit, the old one freed. The old block's cost is credited
    /// before the new one is checked, so a shrink is never refused.
    ///
    /// # Safety
    /// The contract of `Services::realloc` for a block of this heap.
    unsafe fn realloc_across(
        &self,
        block: *mut u8,
        old_size: usize,
        align: usize,
        new_size: usize,
    ) -> *mut u8 {
        let credit = block_cost(old_size, align);
        let moved = self.take(new_size, align, credit);
        if moved.is_null() {
            return moved;
        }
        // SAFETY: both blocks are live and at least `min` bytes, and distinct.
        unsafe {
            ptr::copy_nonoverlapping(block, moved, old_size.min(new_size));
            self.dealloc(block, old_size, align);
        }
        moved
    }
}

impl<S: Services> Services for Charged<'_, S> {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        self.take(size, align, 0)
    }

    unsafe fn dealloc(&self, block: *mut u8, size: usize, align: usize) {
        let Ok(mut owners) = self.state.owners.try_borrow_mut() else {
            return;
        };
        if !is_large(size) {
            // No header: the frame's owner gets it back (clamped at zero).
            if let Some(owner) = owners.get_mut(self.owner as usize) {
                owner.used = owner.used.saturating_sub(block_cost(size, align));
            }
            // SAFETY: the contract: a block of this heap with this size and
            // alignment, freed once.
            unsafe { self.inner.dealloc(block, size, align) };
            return;
        }
        let Some(total) = offset(align).checked_add(size) else {
            return;
        };
        let header = Self::header(block);
        // SAFETY: the contract: a block of this heap with this size and
        // alignment, so its header is in front of it.
        let owner_index = unsafe { (*header).owner } as usize;
        let Some(owner) = owners.get_mut(owner_index) else {
            // Not a block of this heap (a plugin fault): left alone.
            return;
        };
        if owner.used < total {
            return;
        }
        // SAFETY: the header's links are live headers of the same owner.
        unsafe { unlink(owner, header) };
        owner.used -= total;
        let Some(layout) = Self::layout(total, align) else {
            return;
        };
        // SAFETY: the allocation `alloc` made, with its layout, freed once.
        unsafe {
            self.inner
                .dealloc(Self::base(block, align), layout.size(), layout.align());
        }
    }

    unsafe fn realloc(
        &self,
        block: *mut u8,
        old_size: usize,
        align: usize,
        new_size: usize,
    ) -> *mut u8 {
        match (is_large(old_size), is_large(new_size)) {
            // SAFETY: the contract of `Services::realloc`, passed on.
            (false, false) => unsafe { self.realloc_small(block, old_size, align, new_size) },
            // SAFETY: as above.
            (true, false) | (false, true) => unsafe {
                self.realloc_across(block, old_size, align, new_size)
            },
            (true, true) => {
                // SAFETY: as above.
                unsafe { self.realloc_large(block, old_size, align, new_size) }
            }
        }
    }

    fn micros(&self) -> u64 {
        self.inner.micros()
    }
}

impl<S: Services> Charged<'_, S> {
    /// A large block grown or shrunk: its header follows it.
    ///
    /// # Safety
    /// The contract of `Services::realloc` for a large block of this heap.
    unsafe fn realloc_large(
        &self,
        block: *mut u8,
        old_size: usize,
        align: usize,
        new_size: usize,
    ) -> *mut u8 {
        let (Some(old_total), Some(new_total)) = (
            offset(align).checked_add(old_size),
            offset(align).checked_add(new_size),
        ) else {
            return ptr::null_mut();
        };
        let (Some(old_layout), Some(new_layout)) = (
            Self::layout(old_total, align),
            Self::layout(new_total, align),
        ) else {
            return ptr::null_mut();
        };
        let header = Self::header(block);
        // SAFETY: as for `dealloc`.
        let owner_index = unsafe { (*header).owner } as usize;
        let Ok(mut owners) = self.state.owners.try_borrow_mut() else {
            return ptr::null_mut();
        };
        let Some(owner) = owners.get_mut(owner_index) else {
            return ptr::null_mut();
        };
        if owner.used < old_total {
            return ptr::null_mut();
        }
        // The owner of the block pays for its growth, whoever calls.
        let rest = owner.used - old_total;
        let Some(used) = rest.checked_add(new_total) else {
            return ptr::null_mut();
        };
        if new_total > old_total && used > self.state.limit.get() {
            return ptr::null_mut();
        }
        // SAFETY: the allocation `alloc` made, with its layout.
        let base = unsafe {
            self.inner.realloc(
                Self::base(block, align),
                old_layout.size(),
                old_layout.align(),
                new_layout.size(),
            )
        };
        if base.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: the move copied the header with the rest of the block, and
        // the new block is `new_total` bytes with its header right before it.
        let moved = unsafe { base.add(offset(align)) };
        let new_header = Self::header(moved);
        if new_header != header {
            // SAFETY: the neighbours are live headers of this owner.
            unsafe { relink(owner, new_header) };
        }
        owner.used = used;
        moved
    }
}

/// Takes `header` out of `owner`'s block list.
///
/// # Safety
///
/// `header` is a live header in `owner`'s list.
unsafe fn unlink(owner: &mut Owner, header: *mut Header) {
    // SAFETY: the caller's contract; the neighbours are live headers.
    unsafe {
        let (prev, next) = ((*header).prev, (*header).next);
        match prev.as_mut() {
            Some(prev) => prev.next = next,
            None => owner.head = next,
        }
        if let Some(next) = next.as_mut() {
            next.prev = prev;
        }
    }
}

/// Points the neighbours of a header that moved at `header` (its own links were
/// copied with it).
///
/// # Safety
///
/// `header` holds the links the block had before it moved, whose targets are
/// live headers of `owner`.
unsafe fn relink(owner: &mut Owner, header: *mut Header) {
    // SAFETY: the caller's contract.
    unsafe {
        let (prev, next) = ((*header).prev, (*header).next);
        match prev.as_mut() {
            Some(prev) => prev.next = header,
            None => owner.head = header,
        }
        if let Some(next) = next.as_mut() {
            next.prev = header;
        }
    }
}

#[cfg(test)]
#[path = "plugin_heap_tests.rs"]
mod tests;
