//! Scoped submission of decoded host records during synchronous construction.
//!
//! A pinned owner context can retain this slot, but not the construction it
//! points to. The loader borrows its transaction only around make and validates
//! make's output before committing. This bridge neither activates nor dispatches.

use super::{Construction, Error};
use core::{
    cell::{Cell, RefCell},
    fmt,
    marker::PhantomPinned,
    pin::Pin,
    ptr::NonNull,
};

trait Target<T> {
    fn submit(&self, record: T) -> Result<(), (Error, T)>;
}

impl<C, M, T> Target<T> for RefCell<&mut Construction<'_, C, M, T>> {
    fn submit(&self, record: T) -> Result<(), (Error, T)> {
        match self.try_borrow_mut() {
            Ok(mut construction) => construction.stage(record),
            Err(_) => Err((Error::Busy, record)),
        }
    }
}

/// Host-only construction slot in a pinned context, with no allocation.
/// Records must already be decoded and validated, including retained-byte limits.
/// The slot keeps no registry or construction reference outside `scope`.
#[non_exhaustive]
pub struct RegistrationScope<T: 'static> {
    current: Cell<Option<NonNull<dyn Target<T>>>>,
    _pin: PhantomPinned,
}

struct Reset<'a, T: 'static>(&'a RegistrationScope<T>);

impl<T: 'static> Drop for Reset<'_, T> {
    fn drop(&mut self) {
        self.0.current.set(None);
    }
}

impl<T: 'static> RegistrationScope<T> {
    /// Create an empty slot before pinning its containing owner context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: Cell::new(None),
            _pin: PhantomPinned,
        }
    }

    /// Admit submissions only while the synchronous make closure runs.
    /// Clears the slot on return or unwind. The closure receives no construction
    /// reference, so it cannot commit or retain a borrow through this bridge.
    /// The caller validates make's result and commits or aborts after this ends.
    ///
    /// # Errors
    /// Returns Busy without running make if this slot already has a scope.
    pub fn scope<C, M, R>(
        self: Pin<&Self>,
        construction: &mut Construction<'_, C, M, T>,
        make: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        if self.current.get().is_some() {
            return Err(Error::Busy);
        }
        let target = RefCell::new(construction);
        let borrowed = NonNull::from(&target as &dyn Target<T>);
        // SAFETY: target and its exclusive construction borrow outlive make.
        // Reset clears the erased pointer before either borrow ends, including
        // unwinding. Nested installation is refused. Cell prevents sharing
        // between threads, and submit exposes no pointer or borrowed reference.
        let erased = unsafe {
            core::mem::transmute::<NonNull<dyn Target<T> + '_>, NonNull<dyn Target<T> + 'static>>(
                borrowed,
            )
        };
        self.current.set(Some(erased));
        let _reset = Reset(self.get_ref());
        Ok(make())
    }

    /// Stage one validated host-owned record through the active scope.
    /// No mutable construction view remains borrowed after this call.
    ///
    /// # Errors
    /// Returns State outside a scope, Busy for overlapping submission, or the
    /// transaction's staging error. Every refusal returns the unchanged record.
    pub fn submit(&self, record: T) -> Result<(), (Error, T)> {
        let Some(pointer) = self.current.get() else {
            return Err((Error::State, record));
        };
        // SAFETY: only scope installs a pointer. Its reset guard keeps the
        // target alive throughout this call. Target's RefCell refuses aliasing
        // mutable borrows; neither the target nor its construction can escape.
        unsafe { pointer.as_ref() }.submit(record)
    }
}

impl<T: 'static> Default for RegistrationScope<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> fmt::Debug for RegistrationScope<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationScope")
            .field("active", &self.current.get().is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, RefCell, RegistrationScope, Target};
    use crate::native_host::HostOwner;

    #[test]
    fn overlapping_target_borrow_returns_record_without_panicking() {
        let owner = core::pin::pin!(HostOwner::new((), ()));
        let mut construction = owner.as_ref().begin::<u32>().unwrap();
        {
            let target = RefCell::new(&mut construction);
            let held = target.borrow_mut();
            assert_eq!(target.submit(17), Err((Error::Busy, 17)));
            drop(held);
            assert_eq!(target.submit(18), Ok(()));
        }
        let slot = core::pin::pin!(RegistrationScope::new());
        slot.as_ref()
            .scope(&mut construction, || slot.submit(19))
            .unwrap()
            .unwrap();
        assert_eq!(slot.submit(20), Err((Error::State, 20)));
        assert_eq!(construction.commit(|_| ()).unwrap().len(), 2);
    }
}
