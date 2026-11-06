//! Host-only scoped staging checks, not foreign wire or activation proof.
use paperpaw_core::native_host;

use native_host::{DispatchDepth, Error, HostOwner, registration_scope::RegistrationScope};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::{Pin, pin},
    ptr,
    rc::Rc,
};

fn ignore_context<T: 'static>(_: Pin<&RegistrationScope<T>>) {}

#[test]
fn missing_scope_returns_the_same_owned_record() {
    let slot = RegistrationScope::new();
    let record = Box::new(String::from("unchanged"));
    let address = ptr::from_ref(record.as_ref());
    let (error, returned) = slot.submit(record).unwrap_err();
    assert_eq!(error, Error::State);
    assert_eq!(ptr::from_ref(returned.as_ref()), address);
    assert_eq!(*returned, "unchanged");
}

#[test]
fn pinned_owner_stages_without_activation_and_commits_only_after_scope() {
    let owner = pin!(HostOwner::new(RegistrationScope::<String>::new(), ()));
    let slot = owner.as_ref().context();
    let address = ptr::from_ref(slot.get_ref());
    let mut construction = owner.as_ref().begin("weather").unwrap();
    let output = slot
        .scope(&mut construction, || {
            slot.submit(String::from("first")).unwrap();
            slot.submit(String::from("second")).unwrap();
            assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
            assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
            42
        })
        .unwrap();
    assert_eq!(output, 42);
    assert_eq!(
        slot.submit(String::from("late")),
        Err((Error::State, String::from("late")))
    );
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    let batch = construction.commit(ignore_context).unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(
        batch.dispatch(1, &DispatchDepth::new(), |context, record| {
            assert_eq!(ptr::from_ref(context.get_ref()), address);
            record.clone()
        }),
        Ok(String::from("second"))
    );
    drop(batch);
    owner.as_ref().revoke();
    owner.as_ref().finish().unwrap();
}

#[test]
fn nested_scope_is_busy_and_preserves_the_outer_target() {
    let owner = pin!(HostOwner::new(RegistrationScope::<u32>::new(), ()));
    let other = pin!(HostOwner::new((), ()));
    let slot = owner.as_ref().context();
    let mut outer = owner.as_ref().begin("weather").unwrap();
    let mut inner = other.as_ref().begin::<u32>("weather").unwrap();
    let nested_called = Cell::new(false);
    slot.scope(&mut outer, || {
        assert_eq!(
            slot.scope(&mut inner, || nested_called.set(true)),
            Err(Error::Busy)
        );
        slot.submit(9).unwrap();
    })
    .unwrap();
    assert!(!nested_called.get());
    let batch = outer.commit(ignore_context).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch.dispatch(0, &DispatchDepth::new(), |_, record| *record),
        Ok(9)
    );
    assert!(inner.commit(|_| ()).unwrap().is_empty());
}

#[test]
fn a_returned_make_error_clears_the_scope_and_allows_retry() {
    let owner = pin!(HostOwner::new(RegistrationScope::<u32>::default(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert_eq!(
        slot.scope(&mut construction, || {
            slot.submit(1).unwrap();
            Err::<(), _>("make failed")
        }),
        Ok(Err("make failed"))
    );
    assert_eq!(slot.submit(2), Err((Error::State, 2)));
    construction.abort();
    let mut retry = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut retry, || slot.submit(3)).unwrap().unwrap();
    let batch = retry.commit(ignore_context).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch.dispatch(0, &DispatchDepth::new(), |_, record| *record),
        Ok(3)
    );
}

#[test]
fn unwind_clears_scope_before_transaction_rollback_and_retry() {
    let dropped = Rc::new(Cell::new(0));
    let owner = pin!(HostOwner::new(RegistrationScope::<Record>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            slot.scope(&mut construction, || {
                assert!(slot.submit(Record(dropped.clone())).is_ok());
                panic!("make unwound");
            })
            .unwrap();
        }))
        .is_err()
    );
    let Err((error, refused)) = slot.submit(Record(dropped.clone())) else {
        panic!("scope must have been cleared");
    };
    assert_eq!(error, Error::State);
    assert_eq!(dropped.get(), 0);
    drop(refused);
    construction.abort();
    assert_eq!(dropped.get(), 2);
    let mut retry = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut retry, || {
        assert!(slot.submit(Record(dropped.clone())).is_ok());
    })
    .unwrap();
    let batch = retry.commit(ignore_context).unwrap();
    assert_eq!(batch.len(), 1);
    drop(batch);
    assert_eq!(dropped.get(), 3);
}

#[test]
fn capacity_refusal_preserves_record_and_poisoned_transaction() {
    let owner = pin!(HostOwner::new(RegistrationScope::<Box<u32>>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut construction, || {
        for number in 0..8 {
            slot.submit(Box::new(number)).unwrap();
        }
        let record = Box::new(8);
        let address = ptr::from_ref(record.as_ref());
        let (error, returned) = slot.submit(record).unwrap_err();
        assert_eq!(error, Error::Capacity);
        assert_eq!(ptr::from_ref(returned.as_ref()), address);
        assert_eq!(*returned, 8);
        assert_eq!(
            slot.submit(Box::new(9)),
            Err((Error::Capacity, Box::new(9)))
        );
    })
    .unwrap();
    assert!(matches!(
        construction.commit(ignore_context),
        Err(Error::Capacity)
    ));
    assert_eq!(slot.submit(Box::new(10)), Err((Error::State, Box::new(10))));
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
}

struct Record(Rc<Cell<usize>>);

impl Drop for Record {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn destroy_untransferred_output<T: 'static>(_: Pin<&RegistrationScope<T>>) {
    panic!("rejected commit must leave output destruction to the loader");
}

#[test]
fn ignored_rejection_blocks_successful_make_and_drops_all_staged_records() {
    let staged_drops = Rc::new(Cell::new(0));
    let output_drops = Rc::new(Cell::new(0));
    let owner = pin!(HostOwner::new(RegistrationScope::<Record>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    let output = slot
        .scope(&mut construction, || {
            for _ in 0..2 {
                assert!(slot.submit(Record(staged_drops.clone())).is_ok());
            }
            assert_eq!(slot.reject(), Ok(()));
            Ok::<_, ()>(Record(output_drops.clone()))
        })
        .unwrap()
        .unwrap();
    assert_eq!(staged_drops.get(), 0);
    assert!(matches!(
        construction.commit(destroy_untransferred_output),
        Err(Error::Invalid)
    ));
    assert_eq!(staged_drops.get(), 2);
    assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    owner.as_ref().revoke();
    owner.as_ref().finish().unwrap();
    assert_eq!(output_drops.get(), 0);
    drop(output);
    assert_eq!(output_drops.get(), 1);
}

#[test]
fn rejection_returns_further_records_unchanged_and_survives_scope_exit() {
    let owner = pin!(HostOwner::new(RegistrationScope::<Box<String>>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    let record = Box::new(String::from("decoder-owned"));
    let address = ptr::from_ref(record.as_ref());
    let returned = slot
        .scope(&mut construction, || {
            assert_eq!(slot.reject(), Ok(()));
            let (error, returned) = slot.submit(record).unwrap_err();
            assert_eq!(error, Error::Invalid);
            assert_eq!(ptr::from_ref(returned.as_ref()), address);
            assert_eq!(*returned, "decoder-owned");
            returned
        })
        .unwrap();
    slot.scope(&mut construction, || {
        let (error, returned) = slot.submit(returned).unwrap_err();
        assert_eq!(error, Error::Invalid);
        assert_eq!(ptr::from_ref(returned.as_ref()), address);
        assert_eq!(*returned, "decoder-owned");
    })
    .unwrap();
    assert!(matches!(
        construction.commit(ignore_context),
        Err(Error::Invalid)
    ));
}

#[test]
fn rejection_outside_scope_returns_state_without_poisoning_construction() {
    let owner = pin!(HostOwner::new(RegistrationScope::<u32>::new(), ()));
    let slot = owner.as_ref().context();
    assert_eq!(slot.reject(), Err(Error::State));
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert_eq!(slot.reject(), Err(Error::State));
    slot.scope(&mut construction, || slot.submit(7))
        .unwrap()
        .unwrap();
    assert_eq!(slot.reject(), Err(Error::State));
    let batch = construction.commit(ignore_context).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch.dispatch(0, &DispatchDepth::new(), |_, record| *record),
        Ok(7)
    );
}

#[test]
fn rejection_survives_caught_unwind_after_scope_reset() {
    let dropped = Rc::new(Cell::new(0));
    let owner = pin!(HostOwner::new(RegistrationScope::<Record>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            slot.scope(&mut construction, || {
                assert!(slot.submit(Record(dropped.clone())).is_ok());
                assert_eq!(slot.reject(), Ok(()));
                panic!("make unwound after decoder rejection");
            })
            .unwrap();
        }))
        .is_err()
    );
    assert_eq!(slot.reject(), Err(Error::State));
    assert_eq!(dropped.get(), 0);
    assert!(matches!(
        construction.commit(ignore_context),
        Err(Error::Invalid)
    ));
    assert_eq!(dropped.get(), 1);
}

#[test]
fn aborting_rejected_transaction_leaves_a_clean_retry() {
    let dropped = Rc::new(Cell::new(0));
    let owner = pin!(HostOwner::new(RegistrationScope::<Record>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut construction, || {
        assert!(slot.submit(Record(dropped.clone())).is_ok());
        assert_eq!(slot.reject(), Ok(()));
    })
    .unwrap();
    construction.abort();
    assert_eq!(dropped.get(), 1);
    let mut retry = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut retry, || {
        assert!(slot.submit(Record(dropped.clone())).is_ok());
    })
    .unwrap();
    let batch = retry.commit(ignore_context).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(dropped.get(), 1);
    drop(batch);
    assert_eq!(dropped.get(), 2);
}

#[test]
fn repeated_rejection_is_idempotent() {
    let owner = pin!(HostOwner::new(RegistrationScope::<u32>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut construction, || {
        for _ in 0..3 {
            assert_eq!(slot.reject(), Ok(()));
        }
        assert_eq!(slot.submit(4), Err((Error::Invalid, 4)));
    })
    .unwrap();
    assert!(matches!(
        construction.commit(ignore_context),
        Err(Error::Invalid)
    ));
    assert_eq!(slot.reject(), Err(Error::State));
}

#[test]
fn capacity_failure_precedes_later_decoder_rejection() {
    let dropped = Rc::new(Cell::new(0));
    let owner = pin!(HostOwner::new(RegistrationScope::<Record>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut construction, || {
        for _ in 0..8 {
            assert!(slot.submit(Record(dropped.clone())).is_ok());
        }
        let (error, refused) = slot.submit(Record(dropped.clone())).unwrap_err();
        assert_eq!(error, Error::Capacity);
        assert_eq!(slot.reject(), Ok(()));
        drop(refused);
    })
    .unwrap();
    assert_eq!(dropped.get(), 1);
    assert!(matches!(
        construction.commit(ignore_context),
        Err(Error::Capacity)
    ));
    assert_eq!(dropped.get(), 9);
}
