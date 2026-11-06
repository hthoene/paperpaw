//! Host-only ownership for flat plugins, not installed in production dispatch.
//!
//! Shells pin an owner before calling make. The context owns the host table and
//! allocator/service dependencies; the module guard keeps code mapped. Queues
//! and registrations live outside the owner and retain its borrowed leases.
//! No type here is a wire record or a foreign callback signature.

use crate::abi7::{calls, lifecycle};
use core::{cell::Cell, fmt, pin::Pin};

#[path = "native_host/registration_scope.rs"]
pub mod registration_scope;

const REGISTRATIONS: usize = 8;

/// Scoped environment copy-out for host-owned plugin contexts.
#[path = "native_host/environment.rs"]
pub mod environment;
/// Bounded decoded registration submission during plugin construction.
#[path = "native_host/registrations.rs"]
pub mod registrations;

/// Host admission or construction failure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Registration decoding or validation refused the construction transaction.
    Invalid,
    /// The owner is not in the required lifetime phase.
    State,
    /// Construction, callback execution or retained leases prevent this action.
    Busy,
    /// The bounded registration or lease capacity is exhausted.
    Capacity,
    /// The device-wide callback nesting limit is exhausted.
    Depth,
}

impl From<lifecycle::Error> for Error {
    fn from(error: lifecycle::Error) -> Self {
        match error {
            lifecycle::Error::State => Self::State,
            lifecycle::Error::Busy => Self::Busy,
            lifecycle::Error::Capacity => Self::Capacity,
            lifecycle::Error::Depth => Self::Depth,
        }
    }
}

/// One depth budget shared across all owners on the dispatch thread.
#[non_exhaustive]
pub struct DispatchDepth(calls::Depth);

impl DispatchDepth {
    /// Construct an idle budget. Owners must share this value during nesting.
    #[must_use]
    pub const fn new() -> Self {
        Self(calls::Depth::new())
    }
}

impl Default for DispatchDepth {
    fn default() -> Self {
        Self::new()
    }
}

/// Pinned host context and opaque executable-module guard for one plugin.
/// Allocate fallibly and account for C, M and registration storage externally.
#[non_exhaustive]
pub struct HostOwner<C, M> {
    inner: lifecycle::Owner<C, M>,
    preparing: Cell<bool>,
}

impl<C, M> HostOwner<C, M> {
    /// Own the context and module without calling plugin code or allocating.
    #[must_use]
    pub const fn new(context: C, module: M) -> Self {
        Self {
            inner: lifecycle::Owner::new(context, module),
            preparing: Cell::new(false),
        }
    }

    fn inner(self: Pin<&Self>) -> Pin<&lifecycle::Owner<C, M>> {
        // SAFETY: inner is structurally pinned and is never moved by this API.
        unsafe { self.map_unchecked(|owner| &owner.inner) }
    }

    /// Stable context view for make and host-service forwarding.
    #[must_use]
    pub fn context(self: Pin<&Self>) -> Pin<&C> {
        self.inner().context()
    }

    /// Begin a bounded host-side transaction before invoking make.
    /// No staged record can dispatch until commit after result validation.
    ///
    /// # Errors
    /// Returns State outside construction, or Busy for an existing transaction.
    pub fn begin<T>(self: Pin<&Self>) -> Result<Construction<'_, C, M, T>, Error> {
        if !self.inner.is_constructing() {
            return Err(Error::State);
        }
        if self.preparing.replace(true) {
            return Err(Error::Busy);
        }
        Ok(Construction {
            owner: self,
            records: core::array::from_fn(|_| None),
            len: 0,
            failure: None,
        })
    }

    /// Retain a live owner for a queued callback outside its registrations.
    ///
    /// # Errors
    /// Returns State unless live, or Capacity when all lease slots are held.
    pub fn lease(self: Pin<&Self>) -> Result<CallbackLease<'_, C, M>, Error> {
        Ok(CallbackLease(self.inner().lease()?))
    }

    /// Stop callback admission before removing registrations and queued work.
    pub fn revoke(self: Pin<&Self>) {
        self.inner().revoke();
    }

    /// Destroy the plugin once all leases have been released.
    /// Host context, allocator and module remain alive throughout destruction.
    ///
    /// # Errors
    /// Returns Busy while construction or leases remain, or State before revoke.
    pub fn finish(self: Pin<&Self>) -> Result<(), Error> {
        if self.preparing.get() {
            return Err(Error::Busy);
        }
        self.inner().finish().map_err(Into::into)
    }
}

/// Staged host registration records retained under the constructing owner.
/// Drop rolls back records before releasing the construction borrow. The
/// loader remains responsible for partial foreign output from failed make.
#[non_exhaustive]
pub struct Construction<'a, C, M, T> {
    owner: Pin<&'a HostOwner<C, M>>,
    records: [Option<T>; REGISTRATIONS],
    len: usize,
    failure: Option<Error>,
}

impl<'a, C, M, T> Construction<'a, C, M, T> {
    /// Poison this transaction after any registration decoding or validation
    /// refusal, even if foreign code ignores the returned status. Preserve an
    /// earlier staging failure. Only a new transaction can clear this latch.
    pub fn reject(&mut self) {
        if self.failure.is_none() {
            self.failure = Some(Error::Invalid);
        }
    }

    /// Stage a validated host record, returning it untouched on refusal.
    /// Generic T is not a wire schema; decoding and retained-byte limits apply
    /// before calling this method. Eight records is an experimental count cap.
    ///
    /// # Errors
    /// Returns Capacity with the unchanged record if all eight slots are filled,
    /// or the already latched failure if this transaction was rejected.
    pub fn stage(&mut self, record: T) -> Result<(), (Error, T)> {
        if let Some(error) = self.failure {
            return Err((error, record));
        }
        if self.len == REGISTRATIONS {
            self.failure = Some(Error::Capacity);
            return Err((Error::Capacity, record));
        }
        self.records[self.len] = Some(record);
        self.len += 1;
        Ok(())
    }

    /// Transfer a validated successful instance and publish its records.
    /// Store the successful make output in C first. Only Ok transfers its
    /// destruction to this owner. The destructor must not unwind or reenter.
    ///
    /// # Errors
    /// Returns Invalid after validation refusal, Capacity if staging overflowed,
    /// or State if revoked before commit.
    /// Every Err drops staged records before returning and leaves destruction
    /// unarmed. The loader still owns its validated successful output and must
    /// destroy it exactly once while context and module remain alive. This is
    /// distinct from cleaning partial or invalid output from a failed make.
    pub fn commit(mut self, destroy: fn(Pin<&C>)) -> Result<RegistrationBatch<'a, C, M, T>, Error> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let lease = CallbackLease(self.owner.inner().activate_leased(destroy)?);
        Ok(RegistrationBatch {
            records: core::mem::replace(&mut self.records, core::array::from_fn(|_| None)),
            len: self.len,
            lease,
        })
    }

    /// Discard staged records while context and code remain alive.
    pub fn abort(self) {
        drop(self);
    }

    fn clear(&mut self) {
        for record in &mut self.records {
            drop(record.take());
        }
        self.len = 0;
    }
}

impl<C, M, T> Drop for Construction<'_, C, M, T> {
    fn drop(&mut self) {
        // Keep construction excluded during arbitrary host-record destruction.
        self.clear();
        self.owner.preparing.set(false);
    }
}

/// Published records share a lease which keeps their context and code alive.
/// Record destructors run before that lease is released. External queues need
/// their own leases and must be cancelled before owner teardown.
#[non_exhaustive]
pub struct RegistrationBatch<'a, C, M, T> {
    records: [Option<T>; REGISTRATIONS],
    len: usize,
    lease: CallbackLease<'a, C, M>,
}

impl<C, M, T> RegistrationBatch<'_, C, M, T> {
    /// Number of published host records.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether this batch contains no records.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Dispatch one registered record after owner and shared-state admission.
    /// The supplied host wrapper performs scoped frame setup and foreign ABI
    /// validation. This method alone does not execute a foreign function table.
    ///
    /// # Errors
    /// Returns State for revoked admission or an invalid record index, Busy for
    /// same-instance reentry, or Depth when nesting capacity is exhausted.
    pub fn dispatch<R>(
        &self,
        index: usize,
        depth: &DispatchDepth,
        callback: impl FnOnce(Pin<&C>, &T) -> R,
    ) -> Result<R, Error> {
        let record = self
            .records
            .get(index)
            .and_then(Option::as_ref)
            .ok_or(Error::State)?;
        self.lease
            .dispatch(depth, |context| callback(context, record))
    }
}

/// A queued or in-flight callback's borrowed context/module lifetime.
#[non_exhaustive]
pub struct CallbackLease<'a, C, M>(lifecycle::Lease<'a, C, M>);

impl<C, M> CallbackLease<'_, C, M> {
    /// Run host dispatch only after checking revocation, reentry and depth.
    ///
    /// # Errors
    /// Returns State after revocation, Busy on same-instance reentry, or Depth
    /// when the shared nesting capacity is exhausted.
    pub fn dispatch<R>(
        &self,
        depth: &DispatchDepth,
        callback: impl FnOnce(Pin<&C>) -> R,
    ) -> Result<R, Error> {
        self.0.dispatch(&depth.0, callback).map_err(Into::into)
    }
}

impl fmt::Debug for DispatchDepth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DispatchDepth").finish_non_exhaustive()
    }
}

impl<C, M> fmt::Debug for HostOwner<C, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostOwner")
            .field("preparing", &self.preparing.get())
            .finish_non_exhaustive()
    }
}

impl<C, M, T> fmt::Debug for Construction<'_, C, M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Construction")
            .field("len", &self.len)
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}

impl<C, M, T> fmt::Debug for RegistrationBatch<'_, C, M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationBatch")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl<C, M> fmt::Debug for CallbackLease<'_, C, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackLease").finish_non_exhaustive()
    }
}
