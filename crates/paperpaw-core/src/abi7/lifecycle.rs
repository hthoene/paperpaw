//! Host-owned lifetime and admission for one plugin instance.
//!
//! The loader pins this value before `make`, then activates it only after a
//! successful, validated result. Context owns allocator/services dependencies;
//! module keeps executable code mapped. Neither may borrow shorter-lived data.
//! This primitive allocates nothing. The caller must allocate the owner fallibly
//! and charge owner/lease storage against the aggregate resource budget.

use super::calls::{Depth, Instance};
use crate::native_host::Error as HostError;
use core::{cell::Cell, marker::PhantomPinned, pin::Pin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Host lifecycle or teardown-hook refusal. Busy is retryable without effects.
pub(crate) enum Error {
    /// The owner or resource is not in the required state.
    State,
    /// No effects occurred; retry from the next shell tick.
    Busy,
    /// The bounded host capacity is exhausted.
    Capacity,
    /// The callback nesting limit is exhausted.
    Depth,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Constructing,
    Live,
    Revoked,
    Dead,
}

/// Host cleanup stage named in fallback diagnostics. Never crosses the ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeardownStep {
    /// Cancellation of the owner's operations.
    Cancel,
    /// Release of host-owned handles.
    Release,
}

/// Host teardown hooks armed only after validated successful construction.
/// Hooks must not unwind, allocate, reenter teardown or invoke plugin callbacks.
/// Explicit no-op functions are required when a context owns no resources.
pub struct Teardown<C> {
    /// Report a fallback host error once, identifying the owner through context.
    /// This must not allocate, unwind or call plugin code.
    pub report: fn(Pin<&C>, TeardownStep, HostError),
    /// Cancel this owner's open operations without calling the provider.
    pub cancel: fn(Pin<&C>) -> Result<(), HostError>,
    /// Release this owner's handles outside their registry borrow.
    pub release: fn(Pin<&C>) -> Result<(), HostError>,
    /// Destroy the validated plugin state, after every lease has gone.
    pub destroy: fn(Pin<&C>),
}
impl<C> core::fmt::Debug for Teardown<C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Teardown").finish_non_exhaustive()
    }
}
impl<C> Copy for Teardown<C> {}
impl<C> Clone for Teardown<C> {
    fn clone(&self) -> Self {
        *self
    }
}

pub(crate) struct Owner<C, M> {
    // Field order keeps the module alive while the context itself is dropped.
    context: C,
    _module: M,
    state: Cell<State>,
    leases: Cell<u8>,
    instance: Instance,
    teardown: Cell<Option<Teardown<C>>>,
    cancel_done: Cell<bool>,
    release_done: Cell<bool>,
    teardown_error: Cell<Option<HostError>>,
    _pin: PhantomPinned,
}

/// A registration or queued callback retains its owner, context and module.
/// These are host-only borrows, never foreign pointers or Rust wire values.
pub(crate) struct Lease<'a, C, M> {
    owner: Pin<&'a Owner<C, M>>,
}

impl<C, M> Owner<C, M> {
    pub(crate) const fn new(context: C, module: M) -> Self {
        Self {
            context,
            _module: module,
            state: Cell::new(State::Constructing),
            leases: Cell::new(0),
            instance: Instance::new(),
            teardown: Cell::new(None),
            cancel_done: Cell::new(false),
            release_done: Cell::new(false),
            teardown_error: Cell::new(None),
            _pin: PhantomPinned,
        }
    }

    pub(crate) fn is_constructing(&self) -> bool {
        self.state.get() == State::Constructing
    }

    pub(crate) fn context(self: Pin<&Self>) -> Pin<&C> {
        // SAFETY: context is structurally pinned. No method moves it, and Drop
        // leaves it in place. The module outlives its destructor.
        unsafe { self.map_unchecked(|owner| &owner.context) }
    }

    /// Arm exactly-once destruction after successful `make`. The loader owns
    /// cleanup of a failed make result; an unactivated owner never calls it.
    /// The destructor must not unwind or enter ordinary plugin callbacks.
    pub(crate) fn activate(self: Pin<&Self>, teardown: Teardown<C>) -> Result<(), Error> {
        if self.state.get() != State::Constructing {
            return Err(Error::State);
        }
        self.teardown.set(Some(teardown));
        self.state.set(State::Live);
        Ok(())
    }

    /// Transfer a validated instance and reserve its first lease together.
    /// Every refusal precedes arming destruction; no failure follows transfer.
    pub(crate) fn activate_leased(
        self: Pin<&Self>,
        teardown: Teardown<C>,
    ) -> Result<Lease<'_, C, M>, Error> {
        if self.leases.get() >= 8 {
            return Err(Error::Capacity);
        }
        self.activate(teardown)?;
        self.leases.set(self.leases.get() + 1);
        Ok(Lease { owner: self })
    }

    /// Reserve before publishing a registration or queued callback. Eight is
    /// a conservative local lease bound, not a device-wide memory guarantee.
    pub(crate) fn lease(self: Pin<&Self>) -> Result<Lease<'_, C, M>, Error> {
        if self.state.get() != State::Live {
            return Err(Error::State);
        }
        if self.leases.get() >= 8 {
            return Err(Error::Capacity);
        }
        self.leases.set(self.leases.get() + 1);
        Ok(Lease { owner: self })
    }

    /// Refuse new admission before removing registrations and cancelling work.
    pub(crate) fn revoke(self: Pin<&Self>) {
        if self.state.get() != State::Dead {
            self.state.set(State::Revoked);
        }
    }

    /// A callback can request stop reentrantly. Revoke immediately, but defer
    /// resource cleanup until that callback has returned.
    pub(crate) fn is_active(&self) -> bool {
        self.instance.is_active()
    }

    pub(crate) fn prepare_teardown(self: Pin<&Self>) -> Result<(), Error> {
        self.revoke();
        if self.is_active() {
            return Err(Error::Busy);
        }
        self.release_resources()
    }

    fn release_resources(self: Pin<&Self>) -> Result<(), Error> {
        let Some(teardown) = self.teardown.get() else {
            return Ok(());
        };
        self.run_hook(&self.cancel_done, teardown.cancel)?;
        self.run_hook(&self.release_done, teardown.release)
    }

    fn run_hook(
        self: Pin<&Self>,
        done: &Cell<bool>,
        hook: fn(Pin<&C>) -> Result<(), HostError>,
    ) -> Result<(), Error> {
        if done.get() {
            return Ok(());
        }
        match hook(self.context()) {
            Ok(()) => done.set(true),
            Err(HostError::Busy) => return Err(Error::Busy),
            Err(error) => {
                // The shell logs this programming error; do not retry forever.
                if self.teardown_error.get().is_none() {
                    self.teardown_error.set(Some(error));
                }
                done.set(true);
            }
        }
        Ok(())
    }

    pub(crate) fn take_teardown_error(&self) -> Option<HostError> {
        self.teardown_error.take()
    }

    /// The caller releases registrations and queued work before finishing.
    /// A running callback holds a lease too, so destruction cannot overlap it.
    pub(crate) fn finish(self: Pin<&Self>) -> Result<(), Error> {
        if self.state.get() == State::Dead {
            return Ok(());
        }
        if self.state.get() != State::Revoked {
            return Err(Error::State);
        }
        if self.is_active() {
            return Err(Error::Busy);
        }
        self.release_resources()?;
        if self.leases.get() != 0 {
            return Err(Error::Busy);
        }
        self.state.set(State::Dead);
        if let Some(teardown) = self.teardown.take() {
            (teardown.destroy)(self.context());
        }
        Ok(())
    }
}

impl<C, M> Lease<'_, C, M> {
    /// Host dispatch supplies the registered callback only after admission.
    /// The shared instance guard covers every token for this owner. There is no
    /// wire table or registration parser in this increment.
    pub(crate) fn dispatch<R>(
        &self,
        depth: &Depth,
        callback: impl FnOnce(Pin<&C>) -> R,
    ) -> Result<R, Error> {
        if self.owner.state.get() != State::Live {
            return Err(Error::State);
        }
        let _call = self
            .owner
            .instance
            .enter(depth)
            .map_err(|error| match error {
                super::calls::Error::Busy => Error::Busy,
                super::calls::Error::Depth => Error::Depth,
            })?;
        Ok(callback(self.owner.context()))
    }
}

impl<C, M> Drop for Lease<'_, C, M> {
    fn drop(&mut self) {
        self.owner.leases.set(self.owner.leases.get() - 1);
    }
}

// Destroy before the owner fields and module. Hooks must not unwind; this
// guard still preserves destruction order if a host bug violates that contract.
struct DestroyOnDrop<'a, C> {
    context: Pin<&'a C>,
    destroy: fn(Pin<&C>),
}
impl<C> Drop for DestroyOnDrop<'_, C> {
    fn drop(&mut self) {
        (self.destroy)(self.context);
    }
}

impl<C, M> Drop for Owner<C, M> {
    fn drop(&mut self) {
        self.state.set(State::Dead);
        let Some(teardown) = self.teardown.take() else {
            return;
        };
        // SAFETY: activation pins context through cleanup and module release.
        let context = unsafe { Pin::new_unchecked(&self.context) };
        let _destroy = DestroyOnDrop {
            context,
            destroy: teardown.destroy,
        };
        for (step, done, hook) in [
            (TeardownStep::Cancel, &self.cancel_done, teardown.cancel),
            (TeardownStep::Release, &self.release_done, teardown.release),
        ] {
            if done.get() {
                continue;
            }
            match hook(context) {
                Ok(()) => done.set(true),
                Err(error) => {
                    // Drop cannot retry or panic. Skipped entries are bounded,
                    // host-only and keyed by a dead, never-reused owner id.
                    // They must not reference plugin code, state or its image.
                    // This also avoids a second panic during an existing unwind.
                    (teardown.report)(context, step, error);
                }
            }
        }
    }
}
