//! Host-owned lifetime and admission for one plugin instance.
//!
//! The loader pins this value before `make`, then activates it only after a
//! successful, validated result. Context owns allocator/services dependencies;
//! module keeps executable code mapped. Neither may borrow shorter-lived data.
//! This primitive allocates nothing. The caller must allocate the owner fallibly
//! and charge owner/lease storage against the aggregate resource budget.

use super::calls::{Depth, Instance};
use crate::native_host::Error as HostError;
use core::{
    cell::Cell,
    marker::{PhantomData, PhantomPinned},
    mem::ManuallyDrop,
    pin::Pin,
    ptr::NonNull,
};

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
    /// Fallback Drop calls this even if cancellation failed. It must safely
    /// release an uncancelled operation without delivering into plugin memory.
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
    // Drop explicitly keeps the module alive through context destruction.
    context: ManuallyDrop<C>,
    module: ManuallyDrop<M>,
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
    owner: NonNull<Owner<C, M>>,
    _borrow: PhantomData<Pin<&'a Owner<C, M>>>,
}

impl<C, M> Owner<C, M> {
    pub(crate) const fn new(context: C, module: M) -> Self {
        Self {
            context: ManuallyDrop::new(context),
            module: ManuallyDrop::new(module),
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

    /// Fault injection for active-owner Drop without violating a live borrow.
    #[cfg(test)]
    pub(crate) fn force_active(&self) {
        let depth = Depth::new();
        // Forget only the test guard: its active flag then survives the borrow.
        core::mem::forget(self.instance.enter(&depth).unwrap());
    }

    /// The allocation owner must retain the whole Box when this returns true.
    pub(crate) fn abandon_if_active(self: Pin<&Self>) -> bool {
        if !self.instance.is_active() {
            return false;
        }
        self.state.set(State::Dead);
        if let Some(teardown) = self.teardown.take() {
            (teardown.report)(self.context(), TeardownStep::Release, HostError::Busy);
        }
        true
    }

    pub(crate) fn is_constructing(&self) -> bool {
        self.state.get() == State::Constructing
    }

    pub(crate) fn context(self: Pin<&Self>) -> Pin<&C> {
        // SAFETY: context is structurally pinned. No method moves it, and Drop
        // leaves it in place. The module outlives its destructor.
        unsafe { self.map_unchecked(|owner| &*owner.context) }
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
        Ok(Lease {
            owner: NonNull::from(self.get_ref()),
            _borrow: PhantomData,
        })
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
        Ok(Lease {
            owner: NonNull::from(self.get_ref()),
            _borrow: PhantomData,
        })
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
    /// The caller must retain the pinned owner until this lease is dropped.
    #[allow(
        dead_code,
        reason = "standalone lifecycle tests omit the retained host adapter"
    )]
    pub(crate) unsafe fn into_retained(self) -> Lease<'static, C, M> {
        let owner = self.owner;
        core::mem::forget(self);
        Lease {
            owner,
            _borrow: PhantomData,
        }
    }

    /// Host dispatch supplies the registered callback only after admission.
    /// The shared instance guard covers every token for this owner. There is no
    /// wire table or registration parser in this increment.
    pub(crate) fn dispatch<R>(
        &self,
        depth: &Depth,
        callback: impl FnOnce(Pin<&C>) -> R,
    ) -> Result<R, Error> {
        // SAFETY: the lease's borrow or its retained holder keeps this owner
        // pinned and alive. This reference lasts only through this dispatch.
        let owner = unsafe { Pin::new_unchecked(self.owner.as_ref()) };
        if owner.state.get() != State::Live {
            return Err(Error::State);
        }
        let _call = owner.instance.enter(depth).map_err(|error| match error {
            super::calls::Error::Busy => Error::Busy,
            super::calls::Error::Depth => Error::Depth,
        })?;
        Ok(callback(owner.context()))
    }
}

impl<C, M> Drop for Lease<'_, C, M> {
    fn drop(&mut self) {
        // SAFETY: the lease drops before its borrowed or retained pinned owner.
        let owner = unsafe { self.owner.as_ref() };
        owner.leases.set(owner.leases.get() - 1);
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

// Local guards keep context-before-module field cleanup even if a host hook
// violates its no-unwind contract. The active-call guard creates neither.
struct DropField<T>(*mut ManuallyDrop<T>);
impl<T> Drop for DropField<T> {
    fn drop(&mut self) {
        // SAFETY: the owner outlives its local guards. Each guard drops one
        // field exactly once, after destroy has returned or unwound.
        unsafe { ManuallyDrop::drop(&mut *self.0) };
    }
}

impl<C, M> Drop for Owner<C, M> {
    fn drop(&mut self) {
        self.state.set(State::Dead);
        let teardown = self.teardown.take();
        if self.instance.is_active() {
            if let Some(teardown) = teardown {
                // SAFETY: the context remains pinned and deliberately retained.
                let context = unsafe { Pin::new_unchecked(&*self.context) };
                (teardown.report)(context, TeardownStep::Release, HostError::Busy);
            }
            // Plugin code may still be running. Retain both fields, including
            // executable code, rather than destroying or unmapping either.
            return;
        }
        // Reverse local drop order preserves context before module, including
        // an unactivated owner and a panic in a host cleanup hook.
        let _module = DropField(&raw mut self.module);
        let _context_field = DropField(&raw mut self.context);
        let Some(teardown) = teardown else {
            return;
        };
        // SAFETY: the field guard drops this pinned context only after destroy.
        let context = unsafe { Pin::new_unchecked(&*self.context) };
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

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use alloc::{boxed::Box, rc::Rc, vec::Vec};
    use core::cell::RefCell;

    struct Probe {
        log: Rc<RefCell<Vec<&'static str>>>,
        name: &'static str,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            self.log.borrow_mut().push(self.name);
        }
    }

    fn owner(log: &Rc<RefCell<Vec<&'static str>>>) -> Pin<Box<Owner<Probe, Probe>>> {
        Box::pin(Owner::new(
            Probe {
                log: Rc::clone(log),
                name: "context",
            },
            Probe {
                log: Rc::clone(log),
                name: "module",
            },
        ))
    }

    fn hooks() -> Teardown<Probe> {
        Teardown {
            report: |context, step, error| {
                assert_eq!(step, TeardownStep::Release);
                assert_eq!(error, HostError::Busy);
                context.log.borrow_mut().push("report");
            },
            cancel: |context| {
                context.log.borrow_mut().push("cancel");
                Ok(())
            },
            release: |context| {
                context.log.borrow_mut().push("release");
                Ok(())
            },
            destroy: |context| context.log.borrow_mut().push("destroy"),
        }
    }

    #[test]
    fn active_owner_requires_retaining_the_whole_allocation() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let owner = owner(&log);
        owner.as_ref().activate(hooks()).unwrap();
        owner.force_active();
        assert!(owner.as_ref().abandon_if_active());
        assert!(owner.as_ref().abandon_if_active());
        core::mem::forget(owner);
        assert_eq!(&*log.borrow(), &["report"]);
    }

    #[test]
    fn hook_unwind_still_destroys_then_drops_context_then_module() {
        extern crate std as test_std;
        let log = Rc::new(RefCell::new(Vec::new()));
        let owner = owner(&log);
        let mut hooks = hooks();
        hooks.cancel = |_| panic!("host hook contract violation");
        owner.as_ref().activate(hooks).unwrap();
        let result =
            test_std::panic::catch_unwind(test_std::panic::AssertUnwindSafe(|| drop(owner)));
        assert!(result.is_err());
        assert_eq!(&*log.borrow(), &["destroy", "context", "module"]);
    }

    #[test]
    fn inactive_owner_drop_destroys_then_drops_context_then_module() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let owner = owner(&log);
        owner.as_ref().activate(hooks()).unwrap();
        drop(owner);
        assert_eq!(
            &*log.borrow(),
            &["cancel", "release", "destroy", "context", "module"]
        );
    }

    #[test]
    fn unactivated_owner_drops_context_before_module_without_hooks() {
        let log = Rc::new(RefCell::new(Vec::new()));
        drop(owner(&log));
        assert_eq!(&*log.borrow(), &["context", "module"]);
    }
}
