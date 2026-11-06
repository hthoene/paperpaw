//! Deferred cleanup with the production lifetime primitive and fake resources.
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
fn cancel(context: Pin<&Context>) {
    handles::cancel_owner(&context.registry, 1).unwrap();
}
fn release(context: Pin<&Context>) {
    handles::release_owner(&context.registry, 1).unwrap();
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
