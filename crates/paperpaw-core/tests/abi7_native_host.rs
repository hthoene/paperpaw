//! Host-only transaction and ownership checks, not foreign ABI activation proof.
use paperpaw_core::native_host;

use native_host::{DispatchDepth, Error, HostOwner};
use std::{
    cell::{Cell, RefCell},
    marker::PhantomPinned,
    pin::Pin,
    ptr,
    rc::Rc,
};

fn ignore_context(_: Pin<&()>) {}

#[test]
fn ignored_decoder_refusal_rolls_back_before_loader_cleanup() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    let scope = Box::pin(native_host::registration_scope::RegistrationScope::new());
    let mut construction = owner.as_ref().begin("weather").unwrap();
    scope
        .as_ref()
        .scope(&mut construction, || {
            assert!(scope.submit(Record::new(&state, "record")).is_ok());
            assert_eq!(scope.reject(), Ok(()));
        })
        .unwrap();
    assert_eq!(scope.reject(), Err(Error::State));
    assert!(matches!(
        construction.commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy
        }),
        Err(Error::Invalid)
    ));
    assert_eq!(&*state.events.borrow(), &["record"]);
    assert_eq!(state.destroyed.get(), 0);
    destroy(owner.as_ref().context());
    drop(owner);
    assert_eq!(state.destroyed.get(), 1);
    assert_eq!(
        &*state.events.borrow(),
        &["record", "destroy", "context", "module"]
    );
}

#[test]
fn staged_records_cannot_acquire_live_callback_admission() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert!(construction.stage(Record::new(&state, "record")).is_ok());
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    assert_eq!(state.destroyed.get(), 0);
    assert!(state.events.borrow().is_empty());
    construction.abort();
    drop(owner);
    assert_eq!(&*state.events.borrow(), &["record", "context", "module"]);
}

#[test]
fn overlapping_construction_is_busy_and_abort_permits_retry() {
    let owner = Box::pin(HostOwner::new((), ()));
    let first = owner.as_ref().begin::<()>("weather").unwrap();
    assert!(matches!(
        owner.as_ref().begin::<()>("weather"),
        Err(Error::Busy)
    ));
    first.abort();
    let retry = owner.as_ref().begin::<()>("weather").unwrap();
    retry.abort();
    let final_attempt = owner.as_ref().begin::<()>("weather").unwrap();
    let batch = final_attempt
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: ignore_context,
        })
        .unwrap();
    assert!(batch.is_empty());
    assert_eq!(batch.len(), 0);
    assert!(matches!(
        owner.as_ref().begin::<()>("weather"),
        Err(Error::State)
    ));
}

#[test]
fn cancellation_during_make_refuses_commit_and_rolls_back_before_finish() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert!(construction.stage(Record::new(&state, "record")).is_ok());
    owner.as_ref().revoke();
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    assert!(matches!(
        construction.commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy
        }),
        Err(Error::State)
    ));
    assert_eq!(&*state.events.borrow(), &["record"]);
    assert_eq!(state.destroyed.get(), 0);
    assert!(matches!(
        owner.as_ref().begin::<()>("weather"),
        Err(Error::State)
    ));
    owner.as_ref().finish().unwrap();
    drop(owner);
    assert_eq!(&*state.events.borrow(), &["record", "context", "module"]);
}

#[test]
fn ninth_record_is_returned_unchanged_on_capacity_refusal() {
    let owner = Box::pin(HostOwner::new((), ()));
    let mut construction = owner.as_ref().begin("weather").unwrap();
    for index in 0..8 {
        assert!(construction.stage(Box::new(index.to_string())).is_ok());
    }
    let record = Box::new(String::from("retained by caller"));
    let address = ptr::from_ref(record.as_ref());
    let (error, returned) = construction.stage(record).unwrap_err();
    assert_eq!(error, Error::Capacity);
    assert_eq!(ptr::from_ref(returned.as_ref()), address);
    assert_eq!(*returned, "retained by caller");
    assert!(matches!(
        construction.commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: ignore_context
        }),
        Err(Error::Capacity)
    ));
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    let mut retry = owner.as_ref().begin("weather").unwrap();
    for index in 0..8 {
        retry.stage(index.to_string()).unwrap();
    }
    let batch = retry
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: ignore_context,
        })
        .unwrap();
    assert_eq!(batch.len(), 8);
    assert!(!batch.is_empty());
    assert_eq!(
        batch.dispatch(7, &DispatchDepth::new(), |_, item| item.clone()),
        Ok(String::from("7"))
    );
}

#[test]
fn abort_drops_records_with_dependencies_alive_and_allows_a_new_transaction() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert!(construction.stage(Record::new(&state, "first")).is_ok());
    assert!(construction.stage(Record::new(&state, "second")).is_ok());
    construction.abort();
    assert_eq!(&*state.events.borrow(), &["first", "second"]);
    assert_eq!(state.destroyed.get(), 0);
    let mut retry = owner.as_ref().begin("weather").unwrap();
    assert!(retry.stage(Record::new(&state, "retry")).is_ok());
    let batch = retry
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy,
        })
        .unwrap();
    assert_eq!(
        batch.dispatch(0, &DispatchDepth::new(), |_, record| record.event),
        Ok("retry")
    );
    drop(batch);
    owner.as_ref().revoke();
    owner.as_ref().finish().unwrap();
    drop(owner);
    assert_eq!(
        &*state.events.borrow(),
        &["first", "second", "retry", "destroy", "context", "module"]
    );
}

#[test]
fn commit_admits_records_with_a_stable_pinned_context() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    let address = state.address.get();
    let moved_owner = owner;
    let mut construction = moved_owner.as_ref().begin("weather").unwrap();
    assert!(construction.stage(Record::new(&state, "record")).is_ok());
    let batch = construction
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy,
        })
        .unwrap();
    assert_eq!(batch.len(), 1);
    let depth = DispatchDepth::default();
    batch
        .dispatch(0, &depth, |context, record| {
            assert_eq!(ptr::from_ref(context.get_ref()), address);
            assert_eq!(record.event, "record");
        })
        .unwrap();
    let called = Cell::new(false);
    assert_eq!(
        batch.dispatch(1, &depth, |_, _| called.set(true)),
        Err(Error::State)
    );
    assert!(!called.get());
    drop(batch);
    moved_owner.as_ref().revoke();
    moved_owner.as_ref().finish().unwrap();
    drop(moved_owner);
    assert_eq!(
        &*state.events.borrow(),
        &["record", "destroy", "context", "module"]
    );
}

#[test]
fn batch_and_queued_callback_share_instance_reentry_admission() {
    let owner = Box::pin(HostOwner::new((), ()));
    let mut construction = owner.as_ref().begin("weather").unwrap();
    construction.stage(17).unwrap();
    let batch = construction
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: ignore_context,
        })
        .unwrap();
    let queued = owner.as_ref().lease().unwrap();
    let depth = DispatchDepth::new();
    let called = Cell::new(false);
    batch
        .dispatch(0, &depth, |_, _| {
            assert_eq!(
                queued.dispatch(&depth, |_| called.set(true)),
                Err(Error::Busy)
            );
            assert_eq!(batch.dispatch(0, &depth, |_, _| ()), Err(Error::Busy));
        })
        .unwrap();
    assert!(!called.get());
    queued
        .dispatch(&depth, |_| {
            assert_eq!(batch.dispatch(0, &depth, |_, _| ()), Err(Error::Busy));
        })
        .unwrap();
    assert_eq!(batch.dispatch(0, &depth, |_, record| *record), Ok(17));
}

#[test]
fn revoked_batch_and_queue_refuse_callbacks_and_hold_finish_busy() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert!(construction.stage(Record::new(&state, "record")).is_ok());
    let batch = construction
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy,
        })
        .unwrap();
    let queued = owner.as_ref().lease().unwrap();
    owner.as_ref().revoke();
    let called = Cell::new(false);
    let depth = DispatchDepth::new();
    assert_eq!(
        batch.dispatch(0, &depth, |_, _| called.set(true)),
        Err(Error::State)
    );
    assert_eq!(
        queued.dispatch(&depth, |_| called.set(true)),
        Err(Error::State)
    );
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    assert!(!called.get());
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    drop(batch);
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    assert_eq!(state.destroyed.get(), 0);
    drop(queued);
    owner.as_ref().finish().unwrap();
    owner.as_ref().finish().unwrap();
    assert_eq!(state.destroyed.get(), 1);
    drop(owner);
    assert_eq!(
        &*state.events.borrow(),
        &["record", "destroy", "context", "module"]
    );
}

#[test]
fn record_cleanup_precedes_owner_drop_and_plugin_dependency_cleanup() {
    let state = Rc::new(Observations::default());
    let owner = tracked_owner(&state);
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert!(construction.stage(Record::new(&state, "first")).is_ok());
    assert!(construction.stage(Record::new(&state, "second")).is_ok());
    let batch = construction
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy,
        })
        .unwrap();
    drop(batch);
    assert_eq!(&*state.events.borrow(), &["first", "second"]);
    drop(owner);
    assert_eq!(state.destroyed.get(), 1);
    assert_eq!(
        &*state.events.borrow(),
        &["first", "second", "destroy", "context", "module"]
    );
}

#[test]
fn owned_host_callback_result_survives_owner_teardown() {
    let owner = Box::pin(HostOwner::new(String::from("owned host context"), ()));
    let mut construction = owner.as_ref().begin("weather").unwrap();
    construction.stage(String::from("record")).unwrap();
    let batch = construction
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: |_| (),
        })
        .unwrap();
    // This copies host data; it does not validate a foreign callback result.
    let result = batch
        .dispatch(0, &DispatchDepth::new(), |context, record| {
            format!("{}: {record}", context.get_ref())
        })
        .unwrap();
    drop(batch);
    owner.as_ref().revoke();
    owner.as_ref().finish().unwrap();
    drop(owner);
    assert_eq!(result, "owned host context: record");
}

#[test]
fn distinct_owners_share_depth_limit_and_recover_after_refusal() {
    let owners = [
        Box::pin(HostOwner::new((), ())),
        Box::pin(HostOwner::new((), ())),
        Box::pin(HostOwner::new((), ())),
    ];
    let batches: Vec<_> = owners
        .iter()
        .map(|owner| {
            owner
                .as_ref()
                .begin::<()>("weather")
                .unwrap()
                .commit(paperpaw_core::native_host::Teardown {
                    report: |_, _, _| {},
                    cancel: |_| Ok(()),
                    release: |_| Ok(()),
                    destroy: ignore_context,
                })
                .unwrap()
        })
        .collect();
    let leases: Vec<_> = owners
        .iter()
        .map(|owner| owner.as_ref().lease().unwrap())
        .collect();
    let depth = DispatchDepth::new();
    let called = Cell::new(false);
    leases[0]
        .dispatch(&depth, |_| {
            leases[1]
                .dispatch(&depth, |_| {
                    assert_eq!(
                        leases[2].dispatch(&depth, |_| called.set(true)),
                        Err(Error::Depth)
                    );
                })
                .unwrap();
            assert!(!called.get());
            leases[2].dispatch(&depth, |_| called.set(true)).unwrap();
        })
        .unwrap();
    assert!(called.get());
    drop(leases);
    drop(batches);
}

#[test]
fn rejected_commit_releases_records_before_explicit_foreign_cleanup_and_unload() {
    for overflow in [false, true] {
        let state = Rc::new(Observations::default());
        let owner = tracked_owner(&state);
        let mut construction = owner.as_ref().begin("weather").unwrap();
        for _ in 0..8 {
            assert!(construction.stage(Record::new(&state, "staged")).is_ok());
        }
        let expected = if overflow {
            let refused = construction.stage(Record::new(&state, "refused"));
            let Err((error, record)) = refused else {
                panic!("capacity must refuse")
            };
            assert_eq!(error, Error::Capacity);
            drop(record);
            Error::Capacity
        } else {
            owner.as_ref().revoke();
            Error::State
        };
        assert!(
            matches!(construction.commit(paperpaw_core::native_host::Teardown { report: |_, _, _| {}, cancel: |_| Ok(()), release: |_| Ok(()), destroy }), Err(error) if error == expected)
        );
        assert_eq!(state.records_dropped.get(), if overflow { 9 } else { 8 });
        assert_eq!(state.destroyed.get(), 0);
        assert_eq!(state.module_dropped.get(), 0);
        assert!(state.context_live.get() && state.module_live.get());
        // The loader explicitly cleans its failed make output after rollback.
        assert_eq!(state.foreign_cleanup.replace(1), 0);
        state.events.borrow_mut().push("foreign cleanup");
        drop(owner);
        assert_eq!(state.foreign_cleanup.get(), 1);
        assert_eq!(state.module_dropped.get(), 1);
        let events = state.events.borrow();
        assert_eq!(
            &events[events.len() - 3..],
            &["foreign cleanup", "context", "module"]
        );
    }
}

#[test]
fn successful_make_output_stays_loader_owned_on_every_commit_rejection() {
    for overflow in [false, true] {
        let state = Rc::new(Observations::default());
        let owner = tracked_owner(&state);
        let mut construction = owner.as_ref().begin("weather").unwrap();
        for _ in 0..8 {
            assert!(construction.stage(Record::new(&state, "staged")).is_ok());
        }
        let expected = if overflow {
            let Err((error, returned)) = construction.stage(Record::new(&state, "refused")) else {
                panic!("capacity must refuse");
            };
            drop(returned);
            error
        } else {
            owner.as_ref().revoke();
            Error::State
        };
        // The fixture's make output is successful and its destructor validated.
        assert!(
            matches!(construction.commit(paperpaw_core::native_host::Teardown { report: |_, _, _| {}, cancel: |_| Ok(()), release: |_| Ok(()), destroy }), Err(error) if error == expected)
        );
        assert_eq!(state.records_dropped.get(), if overflow { 9 } else { 8 });
        assert_eq!(state.destroyed.get(), 0);
        assert_eq!(state.module_dropped.get(), 0);
        // Err retains loader ownership: call the supplied destructor ourselves.
        destroy(owner.as_ref().context());
        assert_eq!(state.destroyed.get(), 1);
        owner.as_ref().revoke();
        owner.as_ref().finish().unwrap();
        drop(owner);
        assert_eq!(state.destroyed.get(), 1);
        assert_eq!(state.module_dropped.get(), 1);
        let events = state.events.borrow();
        assert_eq!(
            &events[events.len() - 3..],
            &["destroy", "context", "module"]
        );
    }
}

#[derive(Default)]
struct Observations {
    context_live: Cell<bool>,
    module_live: Cell<bool>,
    destroyed: Cell<usize>,
    records_dropped: Cell<usize>,
    foreign_cleanup: Cell<usize>,
    module_dropped: Cell<usize>,
    address: Cell<*const Context>,
    events: RefCell<Vec<&'static str>>,
}

struct Context {
    state: Rc<Observations>,
    _pin: PhantomPinned,
}

struct Module(Rc<Observations>);

struct Record {
    state: Rc<Observations>,
    event: &'static str,
}

impl Record {
    fn new(state: &Rc<Observations>, event: &'static str) -> Self {
        Self {
            state: state.clone(),
            event,
        }
    }
}

fn tracked_owner(state: &Rc<Observations>) -> Pin<Box<HostOwner<Context, Module>>> {
    state.context_live.set(true);
    state.module_live.set(true);
    let owner = Box::pin(HostOwner::new(
        Context {
            state: state.clone(),
            _pin: PhantomPinned,
        },
        Module(state.clone()),
    ));
    state
        .address
        .set(ptr::from_ref(owner.as_ref().context().get_ref()));
    owner
}

fn destroy(context: Pin<&Context>) {
    let state = &context.state;
    assert!(state.context_live.get());
    assert!(state.module_live.get());
    assert_eq!(ptr::from_ref(context.get_ref()), state.address.get());
    assert_eq!(state.destroyed.replace(state.destroyed.get() + 1), 0);
    state.events.borrow_mut().push("destroy");
}

impl Drop for Record {
    fn drop(&mut self) {
        assert!(self.state.context_live.get());
        assert!(self.state.module_live.get());
        assert_eq!(self.state.destroyed.get(), 0);
        assert_eq!(self.state.foreign_cleanup.get(), 0);
        self.state
            .records_dropped
            .set(self.state.records_dropped.get() + 1);
        self.state.events.borrow_mut().push(self.event);
    }
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
        assert_eq!(self.0.module_dropped.replace(1), 0);
        self.0.events.borrow_mut().push("module");
    }
}
