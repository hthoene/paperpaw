//! Private single-threaded shared ownership with checked allocation and counts.
//!
//! Production values are a dispatch budget or a boxed FlatInstance; plugin
//! contexts stay inside PinnedOwner, never a bare HostOwner in Shared. The
//! instance holds no Shared back-pointer. Subscription closures hold handles
//! outside the instance, so teardown removes subscriptions to release them.
//! Self-referential handle cycles would leak and must not be introduced.

use alloc::{alloc::alloc, boxed::Box, rc::Rc};
use core::{alloc::Layout, cell::Cell, fmt, marker::PhantomData, ops::Deref, ptr::NonNull};

#[derive(Debug)]
pub(crate) struct Allocation;

struct Inner<T> {
    count: Cell<u32>,
    value: T,
}

pub(crate) struct Shared<T> {
    pointer: NonNull<Inner<T>>,
    // Explicitly single-threaded even if T itself is Send and Sync.
    _local: PhantomData<Rc<()>>,
    _owned: PhantomData<Inner<T>>,
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
            value,
        })
        .ok_or(Allocation)?;
        Ok(Self {
            pointer: NonNull::from(Box::leak(inner)),
            _local: PhantomData,
            _owned: PhantomData,
        })
    }

    pub(crate) fn try_clone(&self) -> Option<Self> {
        // SAFETY: this shared owner keeps the initialized allocation live.
        let inner = unsafe { self.pointer.as_ref() };
        inner.count.set(inner.count.get().checked_add(1)?);
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
        &unsafe { self.pointer.as_ref() }.value
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        // SAFETY: the allocation remains live until its last counted owner drops.
        let inner = unsafe { self.pointer.as_ref() };
        let remaining = inner.count.get() - 1;
        inner.count.set(remaining);
        if remaining != 0 {
            return;
        }
        // SAFETY: this is the last owner. Reconstruct the original Box so its
        // normal drop glue also frees the allocation if T's destructor unwinds.
        unsafe { drop(Box::from_raw(self.pointer.as_ptr())) };
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
}
