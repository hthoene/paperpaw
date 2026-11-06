//! Pinned host ownership and admission shared by every callback lease.
#[path = "../src/abi7/calls.rs"]
mod calls;
#[path = "../src/abi7/lifecycle.rs"]
mod lifecycle;

use calls::{Depth, Instance};
use lifecycle::{Error, Owner};
use std::{
    cell::{Cell, RefCell},
    marker::PhantomPinned,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    rc::Rc,
};

fn ignore_context(_: Pin<&()>) {}

#[test]
fn context_address_stays_pinned_through_activation_dispatch_and_destruction() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    let address = std::ptr::from_ref(owner.as_ref().context().get_ref());
    state.address.set(address);
    owner.as_ref().activate(destroy).unwrap();
    // Moving the owning pointer must not move the pinned context.
    let moved_owner = owner;
    let lease = moved_owner.as_ref().lease().unwrap();
    lease
        .dispatch(&Depth::new(), |context| {
            assert_eq!(std::ptr::from_ref(context.get_ref()), address);
        })
        .unwrap();
    drop(lease);
    moved_owner.as_ref().revoke();
    moved_owner.as_ref().finish().unwrap();
    drop(moved_owner);
    assert_eq!(&*state.events.borrow(), &["destroy", "context", "module"]);
}

#[test]
fn admission_requires_activation_and_stops_at_revocation() {
    let owner = Box::pin(Owner::new((), ()));
    assert!(owner.is_constructing());
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    assert_eq!(owner.as_ref().finish(), Err(Error::State));
    owner.as_ref().activate(ignore_context).unwrap();
    assert!(!owner.is_constructing());
    assert_eq!(owner.as_ref().activate(ignore_context), Err(Error::State));
    let lease = owner.as_ref().lease().unwrap();
    let called = Cell::new(false);
    owner.as_ref().revoke();
    owner.as_ref().revoke();
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    assert_eq!(
        lease.dispatch(&Depth::new(), |_| called.set(true)),
        Err(Error::State)
    );
    assert!(!called.get());
    drop(lease);
    owner.as_ref().finish().unwrap();
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    assert_eq!(owner.as_ref().activate(ignore_context), Err(Error::State));
}

#[test]
fn different_leases_share_one_instance_guard() {
    let owner = Box::pin(Owner::new((), ()));
    owner.as_ref().activate(ignore_context).unwrap();
    let first = owner.as_ref().lease().unwrap();
    let second = owner.as_ref().lease().unwrap();
    let depth = Depth::new();
    let called = Cell::new(false);
    first
        .dispatch(&depth, |_| {
            assert_eq!(
                second.dispatch(&depth, |_| called.set(true)),
                Err(Error::Busy)
            );
        })
        .unwrap();
    assert!(!called.get());
    second.dispatch(&depth, |_| called.set(true)).unwrap();
    assert!(called.get());
}

#[test]
fn distinct_owners_obey_one_shared_depth_limit_and_recover() {
    let owners = [
        Box::pin(Owner::new((), ())),
        Box::pin(Owner::new((), ())),
        Box::pin(Owner::new((), ())),
    ];
    for owner in &owners {
        owner.as_ref().activate(ignore_context).unwrap();
    }
    let first = owners[0].as_ref().lease().unwrap();
    let second = owners[1].as_ref().lease().unwrap();
    let third = owners[2].as_ref().lease().unwrap();
    let depth = Depth::new();
    let called = Cell::new(false);
    first
        .dispatch(&depth, |_| {
            second
                .dispatch(&depth, |_| {
                    assert_eq!(
                        third.dispatch(&depth, |_| called.set(true)),
                        Err(Error::Depth)
                    );
                    assert_eq!(first.dispatch(&depth, |_| ()), Err(Error::Busy));
                })
                .unwrap();
            assert!(!called.get());
            third.dispatch(&depth, |_| called.set(true)).unwrap();
        })
        .unwrap();
    assert!(called.get());
    first.dispatch(&depth, |_| ()).unwrap();
}

#[test]
fn eight_leases_are_allowed_and_releasing_one_restores_capacity() {
    let owner = Box::pin(Owner::new((), ()));
    owner.as_ref().activate(ignore_context).unwrap();
    let mut leases = Vec::new();
    for _ in 0..8 {
        leases.push(owner.as_ref().lease().unwrap());
    }
    assert!(matches!(owner.as_ref().lease(), Err(Error::Capacity)));
    drop(leases.pop().unwrap());
    leases.push(owner.as_ref().lease().unwrap());
    assert!(matches!(owner.as_ref().lease(), Err(Error::Capacity)));
    drop(leases);
    let replacement = owner.as_ref().lease().unwrap();
    replacement.dispatch(&Depth::new(), |_| ()).unwrap();
    drop(replacement);
    owner.as_ref().revoke();
    owner.as_ref().finish().unwrap();
}

#[test]
fn revocation_during_dispatch_defers_destruction_until_every_lease_is_released() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    state
        .address
        .set(std::ptr::from_ref(owner.as_ref().context().get_ref()));
    owner.as_ref().activate(destroy).unwrap();
    let running = owner.as_ref().lease().unwrap();
    let queued = owner.as_ref().lease().unwrap();
    let depth = Depth::new();
    running
        .dispatch(&depth, |_| {
            owner.as_ref().revoke();
            assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
            assert_eq!(queued.dispatch(&depth, |_| ()), Err(Error::State));
            assert_eq!(state.destroyed.get(), 0);
        })
        .unwrap();
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    drop(running);
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    drop(queued);
    owner.as_ref().finish().unwrap();
    assert_eq!(state.destroyed.get(), 1);
}

#[test]
fn finish_and_owner_drop_destroy_exactly_once_before_dependencies_drop() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    state
        .address
        .set(std::ptr::from_ref(owner.as_ref().context().get_ref()));
    owner.as_ref().activate(destroy).unwrap();
    owner.as_ref().revoke();
    owner.as_ref().finish().unwrap();
    owner.as_ref().finish().unwrap();
    assert_eq!(state.destroyed.get(), 1);
    assert!(state.context_live.get());
    assert!(state.module_live.get());
    drop(owner);
    assert_eq!(state.destroyed.get(), 1);
    assert_eq!(&*state.events.borrow(), &["destroy", "context", "module"]);
}

#[test]
fn owner_drop_also_destroys_an_activated_instance_exactly_once() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    state
        .address
        .set(std::ptr::from_ref(owner.as_ref().context().get_ref()));
    owner.as_ref().activate(destroy).unwrap();
    drop(owner);
    assert_eq!(state.destroyed.get(), 1);
    assert_eq!(&*state.events.borrow(), &["destroy", "context", "module"]);
}

#[test]
fn unactivated_owner_only_drops_its_host_dependencies() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    owner.as_ref().revoke();
    assert_eq!(owner.as_ref().finish(), Ok(()));
    drop(owner);
    assert_eq!(state.destroyed.get(), 0);
    assert_eq!(&*state.events.borrow(), &["context", "module"]);
}

#[test]
fn callback_error_and_unwind_release_instance_and_depth_admission() {
    let owner = Box::pin(Owner::new((), ()));
    owner.as_ref().activate(ignore_context).unwrap();
    let lease = owner.as_ref().lease().unwrap();
    let depth = Depth::new();
    assert_eq!(lease.dispatch(&depth, |_| Err::<(), _>(7)), Ok(Err(7)));
    assert_full_admission(&lease, &depth);
    let result = catch_unwind(AssertUnwindSafe(|| {
        lease
            .dispatch(&depth, |_| panic!("test callback unwind"))
            .unwrap();
    }));
    assert!(result.is_err());
    assert_full_admission(&lease, &depth);
}

fn assert_full_admission(lease: &lifecycle::Lease<'_, (), ()>, depth: &Depth) {
    let other = Instance::new();
    assert_eq!(
        lease.dispatch(depth, |_| {
            let second = other.enter(depth);
            assert!(second.is_ok());
            let third = Instance::new();
            assert!(matches!(third.enter(depth), Err(calls::Error::Depth)));
        }),
        Ok(())
    );
}

#[derive(Default)]
struct Observations {
    context_live: Cell<bool>,
    module_live: Cell<bool>,
    destroyed: Cell<usize>,
    address: Cell<*const Context>,
    events: RefCell<Vec<&'static str>>,
}

struct Context {
    state: Rc<Observations>,
    _pin: PhantomPinned,
}

struct Module(Rc<Observations>);

fn tracked_owner(state: &Rc<Observations>) -> Pin<Box<Owner<Context, Module>>> {
    state.context_live.set(true);
    state.module_live.set(true);
    Box::pin(Owner::new(
        Context {
            state: state.clone(),
            _pin: PhantomPinned,
        },
        Module(state.clone()),
    ))
}

fn destroy(context: Pin<&Context>) {
    let state = &context.state;
    assert!(state.context_live.get());
    assert!(state.module_live.get());
    assert_eq!(std::ptr::from_ref(context.get_ref()), state.address.get());
    assert_eq!(state.destroyed.replace(state.destroyed.get() + 1), 0);
    state.events.borrow_mut().push("destroy");
}

impl Drop for Context {
    fn drop(&mut self) {
        assert!(self.state.module_live.get());
        assert!(self.state.context_live.replace(false));
        self.state.events.borrow_mut().push("context");
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        assert!(!self.0.context_live.get());
        assert!(self.0.module_live.replace(false));
        self.0.events.borrow_mut().push("module");
    }
}

#[test]
fn activation_with_first_lease_transfers_only_on_success() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    state
        .address
        .set(std::ptr::from_ref(owner.as_ref().context().get_ref()));
    let lease = owner.as_ref().activate_leased(destroy).unwrap();
    assert!(matches!(
        owner.as_ref().activate_leased(destroy),
        Err(Error::State)
    ));
    owner.as_ref().revoke();
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    drop(lease);
    owner.as_ref().finish().unwrap();
    drop(owner);
    assert_eq!(state.destroyed.get(), 1);
}
