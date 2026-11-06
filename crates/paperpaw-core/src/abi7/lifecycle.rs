//! Host-owned lifetime and admission for one plugin instance.
//!
//! The loader pins this value before `make`, then activates it only after a
//! successful, validated result. Context owns allocator/services dependencies;
//! module keeps executable code mapped. Neither may borrow shorter-lived data.
//! This primitive allocates nothing. The caller must allocate the owner fallibly
//! and charge owner/lease storage against the aggregate resource budget.

use super::calls::{Depth, Instance};
use core::{cell::Cell, marker::PhantomPinned, pin::Pin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    State,
    Busy,
    Capacity,
    Depth,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Constructing,
    Live,
    Revoked,
    Dead,
}

type Destroy<C> = fn(Pin<&C>);

pub(crate) struct Owner<C, M> {
    // Field order keeps the module alive while the context itself is dropped.
    context: C,
    _module: M,
    state: Cell<State>,
    leases: Cell<u8>,
    instance: Instance,
    destroy: Cell<Option<Destroy<C>>>,
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
            destroy: Cell::new(None),
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
    pub(crate) fn activate(self: Pin<&Self>, destroy: Destroy<C>) -> Result<(), Error> {
        if self.state.get() != State::Constructing {
            return Err(Error::State);
        }
        self.destroy.set(Some(destroy));
        self.state.set(State::Live);
        Ok(())
    }

    /// Transfer a validated instance and reserve its first lease together.
    /// Every refusal precedes arming destruction; no failure follows transfer.
    pub(crate) fn activate_leased(
        self: Pin<&Self>,
        destroy: Destroy<C>,
    ) -> Result<Lease<'_, C, M>, Error> {
        if self.leases.get() >= 8 {
            return Err(Error::Capacity);
        }
        self.activate(destroy)?;
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

    /// The caller releases registrations and queued work before finishing.
    /// A running callback holds a lease too, so destruction cannot overlap it.
    pub(crate) fn finish(self: Pin<&Self>) -> Result<(), Error> {
        if self.state.get() == State::Dead {
            return Ok(());
        }
        if self.state.get() != State::Revoked {
            return Err(Error::State);
        }
        if self.leases.get() != 0 {
            return Err(Error::Busy);
        }
        self.state.set(State::Dead);
        if let Some(destroy) = self.destroy.take() {
            destroy(self.context());
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

impl<C, M> Drop for Owner<C, M> {
    fn drop(&mut self) {
        self.state.set(State::Dead);
        if let Some(destroy) = self.destroy.take() {
            // SAFETY: only activate on a pinned owner arms destruction. Context
            // has not moved since then and remains in place through field drop.
            destroy(unsafe { Pin::new_unchecked(&self.context) });
        }
    }
}
