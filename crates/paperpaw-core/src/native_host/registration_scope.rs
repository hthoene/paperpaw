//! Scoped submission of decoded host records during synchronous construction.
//!
//! A pinned owner context can retain this slot, but not the construction it
//! points to. The loader borrows its transaction only around make and validates
//! make's output before committing. This bridge neither activates nor dispatches.

use super::{
    Construction, Error,
    registrations::{self, RecordBatch, Registration},
};
use core::{
    cell::{Cell, RefCell},
    fmt,
    marker::PhantomPinned,
    pin::Pin,
    ptr::NonNull,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_STATE};

trait Target<T> {
    fn admission(&self) -> Result<(), Error>;
    fn submit(&self, record: T) -> Result<(), (Error, T)>;
    fn submit_batch(
        &self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
        wire: Option<&[u8]>,
    ) -> Result<(), i32>;
    fn reject(&self, cause: Error);
}

struct Staging<'scope, 'owner, C, M, T> {
    construction: RefCell<&'scope mut Construction<'owner, C, M, T>>,
    // The first refusal's cause, latched into the construction on drop.
    rejected: Cell<Option<Error>>,
}

impl<'scope, 'owner, C, M, T> Staging<'scope, 'owner, C, M, T> {
    fn new(construction: &'scope mut Construction<'owner, C, M, T>) -> Self {
        Self {
            construction: RefCell::new(construction),
            rejected: Cell::new(None),
        }
    }
}

impl<C, M, T> Drop for Staging<'_, '_, C, M, T> {
    fn drop(&mut self) {
        // Reset is dropped first. All short-lived submission borrows have ended,
        // so propagate refusal without borrowing the RefCell or invoking code.
        if let Some(cause) = self.rejected.get() {
            self.construction.get_mut().reject_with(cause);
        }
    }
}

impl<C, M, T> Target<T> for Staging<'_, '_, C, M, T> {
    fn admission(&self) -> Result<(), Error> {
        let Ok(construction) = self.construction.try_borrow() else {
            self.reject(Error::Invalid);
            return Err(Error::Busy);
        };
        if let Some(error) = construction.failure {
            return Err(error);
        }
        if let Some(cause) = self.rejected.get() {
            return Err(cause);
        }
        Ok(())
    }

    fn submit(&self, record: T) -> Result<(), (Error, T)> {
        if let Err(error) = self.admission() {
            return Err((error, record));
        }
        if let Ok(mut construction) = self.construction.try_borrow_mut() {
            construction.stage(record)
        } else {
            self.reject(Error::Invalid);
            Err((Error::Busy, record))
        }
    }

    fn submit_batch(
        &self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
        wire: Option<&[u8]>,
    ) -> Result<(), i32> {
        if let Err(error) = self.admission() {
            return Err(registrations::status(error));
        }
        if let Ok(mut construction) = self.construction.try_borrow_mut() {
            construction.stage_with_raw(batch, convert, wire)
        } else {
            self.reject(Error::Invalid);
            Err(ERR_BUSY)
        }
    }

    fn reject(&self, cause: Error) {
        // Keep the first cause, as the construction does.
        if self.rejected.get().is_none() {
            self.rejected.set(Some(cause));
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

    /// Whether a synchronous construction scope is currently installed.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.current.get().is_some()
    }

    /// Check admission even when a submitted batch contains no records.
    /// An earlier construction failure takes precedence over decoder rejection.
    ///
    /// # Errors
    /// Returns State outside a scope, Busy during a conflicting borrow, or the
    /// transaction's first failure. Decoder rejection returns Invalid.
    pub fn admission(&self) -> Result<(), Error> {
        let pointer = self.current.get().ok_or(Error::State)?;
        // SAFETY: scope keeps the target alive until its reset guard runs.
        // Admission reads only through a checked, temporary shared borrow.
        unsafe { pointer.as_ref() }.admission()
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
        if self.is_active() {
            return Err(Error::Busy);
        }
        let target = Staging::new(construction);
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

    /// Latch a decoder or validation refusal in the active construction.
    /// This cannot be suppressed by ignoring the host call's error status.
    /// The latch propagates after scope reset on both return and unwind.
    ///
    /// # Errors
    /// Returns State when no construction scope is installed.
    pub fn reject(&self) -> Result<(), Error> {
        self.reject_with(Error::Invalid)
    }

    /// Latch a refusal with its cause, such as Memory when the host could not
    /// allocate to decode a batch; [`reject`](Self::reject) latches Invalid.
    /// The first cause of a transaction stays.
    ///
    /// # Errors
    /// Returns State when no construction scope is installed.
    pub fn reject_with(&self, cause: Error) -> Result<(), Error> {
        let pointer = self.current.get().ok_or(Error::State)?;
        // SAFETY: the same scoped target lifetime as submit. Rejection uses
        // only its independent Cell, including during a submission borrow.
        unsafe { pointer.as_ref() }.reject(cause);
        Ok(())
    }

    /// Stage a decoded mixed batch (callbacks and declarations) all or nothing;
    /// see [`Construction::stage_batch`]. A refusal poisons the construction.
    ///
    /// # Errors
    /// The `ERR_*` status of the refusal; `ERR_STATE` outside a scope.
    pub fn submit_batch(
        &self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
    ) -> Result<(), i32> {
        let Some(pointer) = self.current.get() else {
            return Err(ERR_STATE);
        };
        // SAFETY: same scoped target lifetime as `submit`.
        unsafe { pointer.as_ref() }.submit_batch(batch, convert, None)
    }

    pub(crate) fn submit_wire(
        &self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
        wire: &[u8],
    ) -> Result<(), i32> {
        let Some(pointer) = self.current.get() else {
            return Err(ERR_STATE);
        };
        // SAFETY: the scope keeps the target live; the target copies wire bytes.
        unsafe { pointer.as_ref() }.submit_batch(batch, convert, Some(wire))
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
    use super::{Error, RegistrationScope, Staging, Target};
    use crate::native_host::HostOwner;

    #[test]
    fn overlapping_target_borrow_returns_record_without_panicking() {
        let owner = core::pin::pin!(HostOwner::new((), ()));
        let mut construction = owner.as_ref().begin::<u32>("weather").unwrap();
        {
            let target = Staging::new(&mut construction);
            let held = target.construction.borrow_mut();
            assert_eq!(target.submit(17), Err((Error::Busy, 17)));
            target.reject(Error::Invalid);
            drop(held);
            assert_eq!(target.submit(18), Err((Error::Invalid, 18)));
        }
        assert!(matches!(
            construction.commit(crate::native_host::Teardown {
                report: |_, _, _| {},
                cancel: |_| Ok(()),
                release: |_| Ok(()),
                destroy: |_| ()
            }),
            Err(Error::Invalid)
        ));
        let mut construction = owner.as_ref().begin::<u32>("weather").unwrap();
        let slot = core::pin::pin!(RegistrationScope::new());
        slot.as_ref()
            .scope(&mut construction, || slot.submit(19))
            .unwrap()
            .unwrap();
        assert_eq!(slot.submit(20), Err((Error::State, 20)));
        assert_eq!(
            construction
                .commit(crate::native_host::Teardown {
                    report: |_, _, _| {},
                    cancel: |_| Ok(()),
                    release: |_| Ok(()),
                    destroy: |_| ()
                })
                .unwrap()
                .len(),
            1
        );
    }
}
