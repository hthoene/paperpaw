//! Deferred cleanup with the production lifetime primitive and fake resources.
#![allow(clippy::unwrap_used, reason = "fixture refusals must fail the test")]
#[path = "../src/abi7/handles.rs"]
mod handles;
use handles::{Handles, Kind, Release};
use paperpaw_core::native_host::{DispatchDepth, Error, HostOwner, Teardown};
use std::{
    cell::{Cell, RefCell},
    pin::Pin,
    rc::Rc,
};

type Log = Rc<RefCell<Vec<&'static str>>>;
struct Resource {
    log: Log,
    cancelled: Rc<Cell<bool>>,
}
impl Release for Resource {
    fn cancel(&mut self) {
        self.cancelled.set(true);
        self.log.borrow_mut().push("cancel");
    }
    fn release(self) {
        self.log.borrow_mut().push("release");
    }
}
struct Context {
    log: Log,
    registry: Rc<RefCell<Handles<Resource>>>,
}
struct Module(Log);
impl Drop for Module {
    fn drop(&mut self) {
        self.0.borrow_mut().push("module");
    }
}
struct Subscription(Log);
impl Drop for Subscription {
    fn drop(&mut self) {
        self.0.borrow_mut().push("subscription");
    }
}
fn cancel(context: Pin<&Context>) -> Result<(), paperpaw_core::native_host::TeardownError> {
    handles::cancel_owner(&context.registry, 1)
        .map_err(|_| paperpaw_core::native_host::TeardownError::Busy)
}
fn release(context: Pin<&Context>) -> Result<(), paperpaw_core::native_host::TeardownError> {
    handles::release_owner(&context.registry, 1)
        .map_err(|_| paperpaw_core::native_host::TeardownError::Busy)
}
fn destroy(context: Pin<&Context>) {
    context.log.borrow_mut().push("destroy");
}

#[test]
fn reentrant_stop_only_revokes_then_next_tick_cancels_releases_and_destroys_once() {
    let log = Log::default();
    let registry = Rc::new(RefCell::new(Handles::new()));
    let cancelled = Rc::new(Cell::new(false));
    let handle = registry
        .borrow_mut()
        .insert(
            1,
            Kind::Blob,
            Resource {
                log: Rc::clone(&log),
                cancelled: Rc::clone(&cancelled),
            },
        )
        .ok()
        .unwrap();
    let owner = Box::pin(HostOwner::new(
        Context {
            log: Rc::clone(&log),
            registry: Rc::clone(&registry),
        },
        Module(Rc::clone(&log)),
    ));
    let mut construction = owner.as_ref().begin("fixture").unwrap();
    assert!(construction.stage(Subscription(Rc::clone(&log))).is_ok());
    let batch = construction
        .commit(Teardown {
            cancel,
            release,
            destroy,
        })
        .unwrap();
    let depth = DispatchDepth::new();
    batch
        .dispatch(0, &depth, |_, _| {
            assert!(owner.is_active());
            assert_eq!(owner.as_ref().prepare_teardown(), Err(Error::Busy));
            assert!(log.borrow().is_empty());
            assert!(!cancelled.get());
        })
        .unwrap();
    assert!(!owner.is_active());
    owner.as_ref().prepare_teardown().unwrap();
    assert!(cancelled.get());
    assert_eq!(&*log.borrow(), &["cancel", "release"]);
    let decoded = Cell::new(false);
    if registry.borrow().get(1, Kind::Blob, handle).is_ok() {
        decoded.set(true);
    }
    assert!(
        !decoded.get(),
        "late completion must be refused before decoding"
    );
    assert_eq!(
        handles::release(&registry, 1, Kind::Blob, handle),
        Err(handles::Error::State)
    );
    assert_eq!(
        batch.dispatch(0, &depth, |_, _| panic!("revoked callback")),
        Err(Error::State)
    );
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    assert!(!log.borrow().contains(&"destroy"));
    drop(batch);
    owner.as_ref().finish().unwrap();
    owner.as_ref().prepare_teardown().unwrap();
    owner.as_ref().finish().unwrap();
    drop(owner);
    assert_eq!(
        &*log.borrow(),
        &["cancel", "release", "subscription", "destroy", "module"]
    );
}

#[test]
fn running_handle_release_cancels_once_and_other_owners_survive_teardown() {
    let log = Log::default();
    let registry = RefCell::new(Handles::new());
    let make = || Resource {
        log: Rc::clone(&log),
        cancelled: Rc::new(Cell::new(false)),
    };
    let first = registry
        .borrow_mut()
        .insert(1, Kind::Blob, make())
        .ok()
        .unwrap();
    for _ in 0..3 {
        assert!(registry.borrow_mut().insert(1, Kind::Blob, make()).is_ok());
    }
    let (error, _) = registry
        .borrow_mut()
        .insert(1, Kind::Blob, make())
        .err()
        .unwrap();
    assert_eq!(error.status(), paperpaw_native::host_glue::ERR_FULL);
    let other = registry
        .borrow_mut()
        .insert(2, Kind::Font, make())
        .ok()
        .unwrap();
    handles::release(&registry, 1, Kind::Blob, first).unwrap();
    assert_eq!(&*log.borrow(), &["cancel", "release"]);
    assert_eq!(
        handles::release(&registry, 1, Kind::Blob, first)
            .unwrap_err()
            .status(),
        paperpaw_native::host_glue::ERR_STATE
    );
    handles::cancel_owner(&registry, 1).unwrap();
    handles::release_owner(&registry, 1).unwrap();
    assert!(registry.borrow().get(2, Kind::Font, other).is_ok());
    assert!(registry.borrow_mut().insert(1, Kind::Blob, make()).is_ok());
    assert_eq!(log.borrow().iter().filter(|&&v| v == "release").count(), 4);
}

#[test]
fn a_fault_inside_a_borrowed_slot_only_revokes_until_the_call_unwinds() {
    let log = Log::default();
    let registry = Rc::new(RefCell::new(Handles::new()));
    let cancelled = Rc::new(Cell::new(false));
    assert!(
        registry
            .borrow_mut()
            .insert(
                1,
                Kind::Service,
                Resource {
                    log: Rc::clone(&log),
                    cancelled: Rc::clone(&cancelled),
                }
            )
            .is_ok()
    );
    let owner = Box::pin(HostOwner::new(
        Context {
            log: Rc::clone(&log),
            registry: Rc::clone(&registry),
        },
        Module(Rc::clone(&log)),
    ));
    let mut construction = owner.as_ref().begin("fixture").unwrap();
    assert!(construction.stage(Subscription(Rc::clone(&log))).is_ok());
    let batch = construction
        .commit(Teardown {
            cancel,
            release,
            destroy,
        })
        .unwrap();
    batch
        .dispatch(0, &DispatchDepth::new(), |context, _| {
            // A slot discovers a fault while it still holds the device registry.
            let _slot_borrow = context.registry.borrow_mut();
            owner.as_ref().revoke();
            assert_eq!(owner.as_ref().prepare_teardown(), Err(Error::Busy));
            assert!(!cancelled.get());
            assert!(log.borrow().is_empty());
        })
        .unwrap();
    owner.as_ref().prepare_teardown().unwrap();
    assert!(cancelled.get());
    drop(batch);
    owner.as_ref().finish().unwrap();
    drop(owner);
    assert_eq!(
        &*log.borrow(),
        &["cancel", "release", "subscription", "destroy", "module"]
    );
}

#[test]
fn a_borrowed_registry_defers_release_without_repeating_cancel() {
    use paperpaw_core::native_host::TeardownError;
    #[derive(Default)]
    struct Retry {
        registry: RefCell<()>,
        cancel: Cell<u8>,
        release: Cell<u8>,
        destroy: Cell<u8>,
    }
    let owner = Box::pin(HostOwner::new(Retry::default(), ()));
    let context = owner.as_ref().context();
    let batch = owner
        .as_ref()
        .begin::<()>("fixture")
        .unwrap()
        .commit(Teardown {
            cancel: |context| {
                context.cancel.set(context.cancel.get() + 1);
                Ok(())
            },
            release: |context| {
                let _registry = context
                    .registry
                    .try_borrow_mut()
                    .map_err(|_| TeardownError::Busy)?;
                context.release.set(context.release.get() + 1);
                Ok(())
            },
            destroy: |context| context.destroy.set(context.destroy.get() + 1),
        })
        .unwrap();
    let registry_borrow = context.registry.borrow_mut();
    assert_eq!(owner.as_ref().prepare_teardown(), Err(Error::Busy));
    assert_eq!(owner.as_ref().prepare_teardown(), Err(Error::Busy));
    assert_eq!(
        (
            context.cancel.get(),
            context.release.get(),
            context.destroy.get()
        ),
        (1, 0, 0)
    );
    drop(registry_borrow);
    owner.as_ref().prepare_teardown().unwrap();
    assert_eq!(context.release.get(), 1);
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    assert_eq!(context.destroy.get(), 0);
    drop(batch);
    owner.as_ref().finish().unwrap();
    owner.as_ref().prepare_teardown().unwrap();
    owner.as_ref().finish().unwrap();
    assert_eq!(
        (
            context.cancel.get(),
            context.release.get(),
            context.destroy.get()
        ),
        (1, 1, 1)
    );
}

#[test]
fn a_non_retryable_hook_error_is_reported_once_and_does_not_retry_forever() {
    use paperpaw_core::native_host::TeardownError;
    let owner = Box::pin(HostOwner::new(Cell::new(0), ()));
    let batch = owner
        .as_ref()
        .begin::<()>("fixture")
        .unwrap()
        .commit(Teardown {
            cancel: |context| {
                context.set(context.get() + 1);
                Err(TeardownError::State)
            },
            release: |_| Ok(()),
            destroy: |_| {},
        })
        .unwrap();
    owner.as_ref().prepare_teardown().unwrap();
    assert_eq!(owner.take_teardown_error(), Some(TeardownError::State));
    owner.as_ref().prepare_teardown().unwrap();
    assert_eq!(owner.take_teardown_error(), None);
    assert_eq!(owner.as_ref().context().get(), 1);
    drop(batch);
    owner.as_ref().finish().unwrap();
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "busy teardown hook during owner Drop")]
fn busy_in_drop_asserts_after_arming_exactly_once_destruction() {
    struct Verify(Rc<Cell<u8>>);
    impl Drop for Verify {
        fn drop(&mut self) {
            assert_eq!(self.0.get(), 1);
        }
    }
    let destroyed = Rc::new(Cell::new(0));
    let _verify = Verify(Rc::clone(&destroyed));
    let owner = Box::pin(HostOwner::new(Rc::clone(&destroyed), ()));
    let batch = owner
        .as_ref()
        .begin::<()>("fixture")
        .unwrap()
        .commit(Teardown {
            cancel: |_| Err(paperpaw_core::native_host::TeardownError::Busy),
            release: |_| panic!("release must wait for cancel"),
            destroy: |context| context.set(context.get() + 1),
        })
        .unwrap();
    drop(batch);
    drop(owner);
}
