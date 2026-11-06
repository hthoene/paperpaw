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

/// Callback-scoped drawing for host-owned plugin contexts.
#[path = "native_host/canvas.rs"]
pub mod canvas;

/// Callback-scoped slot-15 command staging for host-owned plugin contexts.
#[path = "native_host/commands.rs"]
pub mod commands;

/// Registry-free syntactic validation of declaration records.
#[path = "native_host/declarations.rs"]
pub mod declarations;
/// Scoped environment copy-out for host-owned plugin contexts.
#[path = "native_host/environment.rs"]
pub mod environment;

/// Interface-7 event records (tags 32-37): host encode, decode and admission.
#[path = "native_host/events.rs"]
pub mod events;
/// Bounded decoded registration submission during plugin construction.
#[path = "native_host/registrations.rs"]
pub mod registrations;
/// Host-owned copies of declaration records staged during construction.
#[path = "native_host/staged.rs"]
pub mod staged;
/// Interface-7 storage slots 20-26 over the platform storage (not wired).
#[path = "native_host/storage.rs"]
pub mod storage;

use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE};
use registrations::{Record, RecordBatch, Registration};
use staged::Declarations;

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
    /// Host memory for the staged records ran out.
    Memory,
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
    /// `plugin_id` is the host-known id that declaration names are checked
    /// against; it must outlive the transaction.
    ///
    /// # Errors
    /// Returns State outside construction, or Busy for an existing transaction.
    pub fn begin<'a, T>(
        self: Pin<&'a Self>,
        plugin_id: &'a str,
    ) -> Result<Construction<'a, C, M, T>, Error> {
        if !self.inner.is_constructing() {
            return Err(Error::State);
        }
        if self.preparing.replace(true) {
            return Err(Error::Busy);
        }
        Ok(Construction {
            owner: self,
            plugin_id,
            records: core::array::from_fn(|_| None),
            len: 0,
            declarations: Declarations::default(),
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
    plugin_id: &'a str,
    records: [Option<T>; REGISTRATIONS],
    len: usize,
    declarations: Declarations,
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
    /// Returns Invalid after validation refusal or when the staged declarations
    /// fail their cross-record checks ([`Declarations::finish`]), Capacity if staging overflowed
    /// a slot, count or byte limit, Memory if staging ran out of memory, or State if revoked
    /// before commit.
    /// Every Err drops staged records before returning and leaves destruction
    /// unarmed. The loader still owns its validated successful output and must
    /// destroy it exactly once while context and module remain alive. This is
    /// distinct from cleaning partial or invalid output from a failed make.
    pub fn commit(mut self, destroy: fn(Pin<&C>)) -> Result<RegistrationBatch<'a, C, M, T>, Error> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        // Cross-record checks come first: a refusal leaves destruction unarmed.
        self.declarations
            .finish(self.plugin_id)
            .map_err(|_| Error::Invalid)?;
        let lease = CallbackLease(self.owner.inner().activate_leased(destroy)?);
        Ok(RegistrationBatch {
            records: core::mem::replace(&mut self.records, core::array::from_fn(|_| None)),
            len: self.len,
            declarations: core::mem::take(&mut self.declarations),
            lease,
        })
    }

    /// The declarations staged so far.
    #[must_use]
    pub const fn declarations(&self) -> &Declarations {
        &self.declarations
    }

    /// Stage a decoded, mixed batch: its callback registrations (converted by
    /// `convert`) and its declarations, all or nothing for the batch.
    /// Capacity for both is checked before either is changed. A refusal latches
    /// the transaction: Capacity when the eight callback slots, a declaration
    /// count or the byte budget would overflow, Memory when an allocation fails,
    /// Invalid for any other declaration refusal.
    ///
    /// # Errors
    /// The `ERR_*` status of the refusal, or of the earlier latched failure.
    pub fn stage_batch(
        &mut self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
    ) -> Result<(), i32> {
        if let Some(error) = self.failure {
            return Err(registrations::status(error));
        }
        let tokens = batch
            .iter()
            .filter(|record| matches!(record, Record::Registration(_)))
            .count();
        if self.len + tokens > REGISTRATIONS {
            self.failure = Some(Error::Capacity);
            return Err(ERR_NO_SPACE);
        }
        if let Err(code) = self.declarations.stage(self.plugin_id, batch) {
            // Latch the cause, so that a warning can tell a limit or low memory
            // from a broken rule.
            self.failure = Some(match code {
                ERR_NO_SPACE => Error::Capacity,
                ERR_NO_MEMORY => Error::Memory,
                _ => Error::Invalid,
            });
            return Err(code);
        }
        for record in batch.iter() {
            if let (Record::Registration(registration), Some(slot)) =
                (record, self.records.get_mut(self.len))
            {
                *slot = Some(convert(registration));
                self.len += 1;
            }
        }
        Ok(())
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
    declarations: Declarations,
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

    /// Move the staged declarations out, leaving an empty set behind. They are
    /// moved, never copied; the kernel's apply step owns them from here.
    pub fn take_declarations(&mut self) -> Declarations {
        core::mem::take(&mut self.declarations)
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
            .field("declarations", &self.declarations)
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
