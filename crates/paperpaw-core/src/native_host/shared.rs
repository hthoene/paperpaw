//! Private single-threaded shared ownership with checked allocation and counts.
//!
//! Production values are a dispatch budget, a boxed FlatInstance or the one
//! `FlatHost` all native plugins' dispatches share (`Kernel::shared_flat_host`,
//! made with the first plugin; the kernel and each dispatch hold a handle); plugin
//! contexts stay inside PinnedOwner, never a bare HostOwner in Shared. The
//! instance holds no Shared back-pointer. Subscription closures hold handles
//! outside the instance, so teardown removes subscriptions to release them.
//! Self-referential handle cycles would leak and must not be introduced.

use alloc::{
    alloc::{alloc, dealloc},
    boxed::Box,
    rc::Rc,
};
use core::{
    alloc::Layout, cell::Cell, fmt, marker::PhantomData, mem::ManuallyDrop, ops::Deref,
    ptr::NonNull,
};

#[derive(Debug)]
pub(crate) struct Allocation;

struct Inner<T> {
    count: Cell<u32>,
    weak: Cell<u32>,
    value: ManuallyDrop<T>,
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        if self.count.get() != 0 {
            // SAFETY: only failed header allocation drops an Inner with a
            // positive strong count. Its value has not been destroyed yet.
            unsafe { ManuallyDrop::drop(&mut self.value) };
        }
    }
}

pub(crate) struct Shared<T> {
    pointer: NonNull<Inner<T>>,
    // Explicitly single-threaded even if T itself is Send and Sync.
    _local: PhantomData<Rc<()>>,
    _owned: PhantomData<Inner<T>>,
}

pub(crate) struct Weak<T> {
    pointer: NonNull<Inner<T>>,
    _local: PhantomData<Rc<()>>,
}
impl<T> fmt::Debug for Weak<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Weak").finish_non_exhaustive()
    }
}
impl<T> Weak<T> {
    pub(crate) fn try_clone(&self) -> Option<Self> {
        // SAFETY: this weak handle retains the header allocation.
        let weak = unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).weak) };
        weak.set(weak.get().checked_add(1)?);
        Some(Self {
            pointer: self.pointer,
            _local: PhantomData,
        })
    }
    pub(crate) fn upgrade(&self) -> Result<Option<Shared<T>>, Allocation> {
        // SAFETY: this weak handle retains the header, even after T drops.
        let count = unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).count) };
        if count.get() == 0 {
            return Ok(None);
        }
        count.set(count.get().checked_add(1).ok_or(Allocation)?);
        Ok(Some(Shared {
            pointer: self.pointer,
            _local: PhantomData,
            _owned: PhantomData,
        }))
    }
}
impl<T> Drop for Weak<T> {
    fn drop(&mut self) {
        // SAFETY: every weak handle owns one header reference. The last one
        // frees only a header whose value was already destroyed.
        let weak = unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).weak) };
        let remaining = weak.get() - 1;
        weak.set(remaining);
        if remaining == 0 {
            // SAFETY: the implicit weak count ends after value destruction.
            // Deallocate directly: constructing a Box would reference dead T.
            unsafe { dealloc(self.pointer.as_ptr().cast(), Layout::new::<Inner<T>>()) };
        }
    }
}

impl<T> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared").finish_non_exhaustive()
    }
}

impl<T> Shared<T> {
    pub(crate) fn try_new(value: T) -> Result<Self, Allocation> {
        let inner = try_box(Inner {
            count: Cell::new(1),
            weak: Cell::new(1),
            value: ManuallyDrop::new(value),
        })
        .ok_or(Allocation)?;
        Ok(Self {
            pointer: NonNull::from(Box::leak(inner)),
            _local: PhantomData,
            _owned: PhantomData,
        })
    }

    /// [`Self::try_new`] for a value that is big: the block is had first,
    /// then `make(arg)` writes the value straight into it (not built on this
    /// frame and copied, as [`try_box_with`]). Where the heap has no room
    /// nothing is built and `arg` is given back.
    #[inline(always)]
    pub(crate) fn try_new_from<A>(arg: A, make: impl FnOnce(A) -> T) -> Result<Self, A> {
        let layout = Layout::new::<Inner<T>>();
        // SAFETY: `Inner` holds two counts: its layout is never zero-sized.
        let pointer = unsafe { alloc(layout).cast::<Inner<T>>() };
        let Some(pointer) = NonNull::new(pointer) else {
            return Err(arg);
        };
        // SAFETY: owned, aligned, uninitialized storage for one `Inner<T>`:
        // each field is written once through its own place, the value last
        // (`ManuallyDrop<T>` is laid out as `T`).
        unsafe {
            let inner = pointer.as_ptr();
            core::ptr::addr_of_mut!((*inner).count).write(Cell::new(1));
            core::ptr::addr_of_mut!((*inner).weak).write(Cell::new(1));
            core::ptr::addr_of_mut!((*inner).value)
                .cast::<T>()
                .write(make(arg));
        }
        Ok(Self {
            pointer,
            _local: PhantomData,
            _owned: PhantomData,
        })
    }

    /// Whether this is the one strong handle and no weak one is out.
    pub(crate) fn is_unique(&self) -> bool {
        // SAFETY: this handle retains the initialized allocation.
        let inner = unsafe { &*self.pointer.as_ptr() };
        inner.count.get() == 1 && inner.weak.get() == 1
    }

    /// Whether `a` and `b` share one value.
    #[cfg(test)]
    pub(crate) fn ptr_eq(a: &Self, b: &Self) -> bool {
        a.pointer == b.pointer
    }

    pub(crate) fn downgrade(&self) -> Option<Weak<T>> {
        // SAFETY: this strong handle retains both the value and header.
        let weak = unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).weak) };
        weak.set(weak.get().checked_add(1)?);
        Some(Weak {
            pointer: self.pointer,
            _local: PhantomData,
        })
    }

    #[cfg(test)]
    pub(crate) fn strong_count(&self) -> u32 {
        // SAFETY: this handle retains the initialized allocation.
        unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).count) }.get()
    }

    pub(crate) fn try_clone(&self) -> Option<Self> {
        // SAFETY: this shared owner keeps the initialized allocation live.
        let count = unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).count) };
        count.set(count.get().checked_add(1)?);
        Some(Self {
            pointer: self.pointer,
            _local: PhantomData,
            _owned: PhantomData,
        })
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: a live owner retains value; no API exposes a mutable reference.
        unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).value).cast::<T>() }
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        // SAFETY: the allocation remains live until its last counted owner drops.
        let count = unsafe { &*core::ptr::addr_of!((*self.pointer.as_ptr()).count) };
        let remaining = count.get() - 1;
        count.set(remaining);
        if remaining != 0 {
            return;
        }
        // The implicit weak reference keeps the header alive while T drops,
        // including if T drops its own weak back-pointer or unwinds.
        let _release = Weak {
            pointer: self.pointer,
            _local: PhantomData,
        };
        // SAFETY: strong count is zero, so no shared handle can access T.
        // Weak::upgrade refuses zero. The value is destroyed exactly once.
        unsafe { ManuallyDrop::drop(&mut (*self.pointer.as_ptr()).value) };
    }
}

/// Allocate the actual value, not a temporary probe. Null drops value normally.
pub(crate) fn try_box<T>(value: T) -> Option<Box<T>> {
    let layout = Layout::new::<T>();
    let pointer = if layout.size() == 0 {
        NonNull::<T>::dangling().as_ptr()
    } else {
        // SAFETY: exact layout; a null allocation is handled without writing.
        unsafe { alloc(layout).cast::<T>() }
    };
    if pointer.is_null() {
        return None;
    }
    // SAFETY: owned, suitably aligned storage for T (or a ZST dangling pointer),
    // initialized before its Box is made. Box uses this same global allocator.
    unsafe {
        pointer.write(value);
        Some(Box::from_raw(pointer))
    }
}

/// [`try_box`] for a value that is big: `make` runs only once the room is
/// there, and its value is written straight into it (not built on this
/// frame and copied). Nothing is built where the heap has no room.
#[inline(always)]
pub(crate) fn try_box_with<T>(make: impl FnOnce() -> T) -> Option<Box<T>> {
    let layout = Layout::new::<T>();
    let pointer = if layout.size() == 0 {
        NonNull::<T>::dangling().as_ptr()
    } else {
        // SAFETY: exact layout; a null allocation is handled without writing.
        unsafe { alloc(layout).cast::<T>() }
    };
    if pointer.is_null() {
        return None;
    }
    // SAFETY: owned, suitably aligned storage for T (or a ZST dangling pointer),
    // initialized before its Box is made. Box uses this same global allocator.
    unsafe {
        pointer.write(make());
        Some(Box::from_raw(pointer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;

    struct Probe(Rc<Cell<u32>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn copies_keep_value_alive_and_last_drop_destroys_once() {
        let dropped = Rc::new(Cell::new(0));
        let first = Shared::try_new(Probe(Rc::clone(&dropped))).unwrap();
        let second = first.try_clone().unwrap();
        drop(first);
        assert_eq!(dropped.get(), 0);
        drop(second);
        assert_eq!(dropped.get(), 1);
    }

    #[test]
    fn count_overflow_refuses_without_effects_or_panic() {
        let value = Shared::try_new(17).unwrap();
        // SAFETY: test-only count fault injection; restored before drop.
        let inner = unsafe { value.pointer.as_ref() };
        inner.count.set(u32::MAX);
        assert!(value.try_clone().is_none());
        assert_eq!(inner.count.get(), u32::MAX);
        assert_eq!(*value, 17);
        inner.count.set(1);
    }
    #[test]
    fn weak_upgrade_retains_value_and_expiry_never_revives_it() {
        let dropped = Rc::new(Cell::new(0));
        let value = Shared::try_new(Probe(Rc::clone(&dropped))).unwrap();
        let weak = value.downgrade().unwrap();
        let copy = weak.try_clone().unwrap();
        let retained = weak.upgrade().unwrap().unwrap();
        drop(value);
        assert_eq!(dropped.get(), 0);
        drop(retained);
        assert_eq!(dropped.get(), 1);
        assert!(weak.upgrade().unwrap().is_none());
        assert!(copy.upgrade().unwrap().is_none());
        drop(weak);
        drop(copy);
        assert_eq!(dropped.get(), 1);
    }

    #[test]
    fn weak_count_and_upgrade_exhaustion_have_no_side_effects() {
        let value = Shared::try_new(17).unwrap();
        let weak = value.downgrade().unwrap();
        // SAFETY: test-only counters are restored before any handle drops.
        let inner = unsafe { value.pointer.as_ref() };
        inner.weak.set(u32::MAX);
        assert!(value.downgrade().is_none());
        assert!(weak.try_clone().is_none());
        assert_eq!(inner.weak.get(), u32::MAX);
        inner.weak.set(2);
        inner.count.set(u32::MAX);
        assert!(weak.upgrade().is_err());
        assert_eq!(inner.count.get(), u32::MAX);
        inner.count.set(1);
    }

    #[test]
    fn weak_back_pointer_does_not_keep_its_owner_alive() {
        struct Back {
            weak: core::cell::RefCell<Option<Weak<Back>>>,
            dropped: Rc<Cell<bool>>,
        }
        impl Drop for Back {
            fn drop(&mut self) {
                assert!(
                    self.weak
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .upgrade()
                        .unwrap()
                        .is_none()
                );
                let weak = self.weak.get_mut().take().unwrap();
                let copy = weak.try_clone().unwrap();
                assert!(copy.upgrade().unwrap().is_none());
                drop(weak);
                drop(copy);
                self.dropped.set(true);
            }
        }
        let dropped = Rc::new(Cell::new(false));
        let value = Shared::try_new(Back {
            weak: core::cell::RefCell::new(None),
            dropped: Rc::clone(&dropped),
        })
        .unwrap();
        *value.weak.borrow_mut() = value.downgrade();
        drop(value);
        assert!(dropped.get());
    }
}
