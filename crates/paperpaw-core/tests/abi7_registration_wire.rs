//! Real wire decoding and host admission; shared-library loading is tested by
//! the desktop loader, not simulated by these synchronous host tests.
use paperpaw_core::native_host::{
    DispatchDepth, Error, HostOwner,
    registration_scope::RegistrationScope,
    registrations::{self, EVENT_TICK_TAG, Registration, registrations_encode},
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK};
use std::{cell::Cell, pin::Pin, ptr, rc::Rc};

type Context = (RegistrationScope<Registration>, Rc<Cell<usize>>);

fn destroy(context: Pin<&Context>) {
    context.1.set(context.1.get() + 1);
}

fn scope(context: Pin<&Context>) -> Pin<&RegistrationScope<Registration>> {
    // SAFETY: projecting the pinned context's first field without moving it.
    unsafe { context.map_unchecked(|context| &context.0) }
}

fn encode(records: &[Registration]) -> Result<Vec<u8>, i32> {
    let mut out = vec![0; registrations::REGISTRATION_MAX_BYTES];
    let length = registrations_encode(records, &mut out)?;
    out.truncate(length);
    Ok(out)
}

fn submit(scope: &RegistrationScope<Registration>, bytes: &[u8]) -> i32 {
    // SAFETY: this immutable slice remains readable for the entire call.
    unsafe { registrations::submit(scope, bytes.as_ptr(), bytes.len()) }
}

#[test]
fn decoded_records_are_admitted_only_after_successful_commit() {
    let dropped = Rc::new(Cell::new(0));
    let owner = Box::pin(HostOwner::new(
        (RegistrationScope::new(), dropped.clone()),
        (),
    ));
    let slot = scope(owner.as_ref().context());
    let records = [
        Registration::Event {
            event_tag: EVENT_TICK_TAG,
            token: 41,
        },
        Registration::Draw { token: 42 },
    ];
    let bytes = encode(&records).unwrap();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut construction, || {
        assert_eq!(submit(&slot, &bytes), OK);
        assert!(matches!(owner.as_ref().lease(), Err(Error::State)));
    })
    .unwrap();
    let batch = construction
        .commit(paperpaw_core::native_host::Teardown {
            cancel: |_| {},
            release: |_| {},
            destroy,
        })
        .unwrap();
    for (index, expected) in records.iter().enumerate() {
        assert_eq!(
            batch.dispatch(index, &DispatchDepth::new(), |_, record| *record),
            Ok(*expected)
        );
    }
    drop(batch);
    owner.as_ref().revoke();
    owner.as_ref().finish().unwrap();
    assert_eq!(dropped.get(), 1);
}

#[test]
fn ignored_malformed_or_unsupported_batch_refuses_all_publication() {
    for bytes in [
        vec![0x80],
        encode(&[Registration::Event {
            event_tag: u32::MAX,
            token: 1,
        }])
        .unwrap(),
    ] {
        let dropped = Rc::new(Cell::new(0));
        let owner = Box::pin(HostOwner::new(
            (RegistrationScope::new(), dropped.clone()),
            (),
        ));
        let slot = scope(owner.as_ref().context());
        let mut construction = owner.as_ref().begin("weather").unwrap();
        slot.scope(&mut construction, || {
            assert_eq!(
                submit(&slot, &encode(&[Registration::Draw { token: 3 }]).unwrap()),
                OK
            );
            assert!(matches!(
                submit(&slot, &bytes),
                ERR_INVALID | ERR_UNSUPPORTED
            ));
            assert_eq!(submit(&slot, &encode(&[]).unwrap()), ERR_INVALID);
        })
        .unwrap();
        assert!(matches!(
            construction.commit(paperpaw_core::native_host::Teardown {
                cancel: |_| {},
                release: |_| {},
                destroy
            }),
            Err(Error::Invalid)
        ));
        assert_eq!(dropped.get(), 0);
        // Successful foreign output would still belong to the loader here.
        destroy(owner.as_ref().context());
        drop(owner);
        assert_eq!(dropped.get(), 1);
    }
}

#[test]
fn repeated_batches_cannot_exceed_the_transaction_capacity() {
    let dropped = Rc::new(Cell::new(0));
    let owner = Box::pin(HostOwner::new((RegistrationScope::new(), dropped), ()));
    let slot = scope(owner.as_ref().context());
    let mut construction = owner.as_ref().begin("weather").unwrap();
    let bytes = encode(&[Registration::Draw { token: 1 }; 8]).unwrap();
    slot.scope(&mut construction, || {
        assert_eq!(submit(&slot, &bytes), OK);
        assert_eq!(
            submit(&slot, &encode(&[Registration::Draw { token: 2 }]).unwrap()),
            ERR_NO_SPACE
        );
        assert_eq!(submit(&slot, &encode(&[]).unwrap()), ERR_NO_SPACE);
        assert_eq!(slot.admission(), Err(Error::Capacity));
    })
    .unwrap();
    assert!(matches!(
        construction.commit(paperpaw_core::native_host::Teardown {
            cancel: |_| {},
            release: |_| {},
            destroy
        }),
        Err(Error::Capacity)
    ));
}

#[test]
fn empty_batch_checks_decoder_latch_even_without_staged_records() {
    let dropped = Rc::new(Cell::new(0));
    let owner = Box::pin(HostOwner::new((RegistrationScope::new(), dropped), ()));
    let slot = scope(owner.as_ref().context());
    let mut construction = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut construction, || {
        assert_eq!(submit(&slot, &encode(&[]).unwrap()), OK);
        slot.reject().unwrap();
        assert_eq!(submit(&slot, &encode(&[]).unwrap()), ERR_INVALID);
    })
    .unwrap();
    assert!(matches!(
        construction.commit(paperpaw_core::native_host::Teardown {
            cancel: |_| {},
            release: |_| {},
            destroy
        }),
        Err(Error::Invalid)
    ));
}

#[test]
fn null_oversized_and_empty_inputs_obey_scope_and_refusal_rules() {
    let dropped = Rc::new(Cell::new(0));
    let owner = Box::pin(HostOwner::new((RegistrationScope::new(), dropped), ()));
    let slot = scope(owner.as_ref().context());
    assert_eq!(submit(&slot, &encode(&[]).unwrap()), ERR_STATE);
    for length in [0, 1, registrations::REGISTRATION_MAX_BYTES + 1] {
        let mut construction = owner.as_ref().begin("weather").unwrap();
        slot.scope(&mut construction, || {
            // SAFETY: null is rejected or treated as an empty slice before reads.
            let status = unsafe { registrations::submit(&slot, ptr::null(), length) };
            assert_eq!(
                status,
                if length > registrations::REGISTRATION_MAX_BYTES {
                    ERR_NO_SPACE
                } else {
                    ERR_INVALID
                }
            );
        })
        .unwrap();
        assert!(matches!(
            construction.commit(paperpaw_core::native_host::Teardown {
                cancel: |_| {},
                release: |_| {},
                destroy
            }),
            Err(Error::Invalid)
        ));
    }
}

#[test]
fn revoked_queued_and_registered_work_is_refused_before_the_body_runs() {
    let dropped = Rc::new(Cell::new(0));
    let owner = Box::pin(HostOwner::new(
        (RegistrationScope::new(), dropped.clone()),
        (),
    ));
    let slot = scope(owner.as_ref().context());
    let mut construction = owner.as_ref().begin("weather").unwrap();
    slot.scope(&mut construction, || {
        assert_eq!(
            submit(&slot, &encode(&[Registration::Draw { token: 8 }]).unwrap()),
            OK
        )
    })
    .unwrap();
    let batch = construction
        .commit(paperpaw_core::native_host::Teardown {
            cancel: |_| {},
            release: |_| {},
            destroy,
        })
        .unwrap();
    let queued = owner.as_ref().lease().unwrap();
    let calls = Cell::new(0);
    owner.as_ref().revoke();
    assert_eq!(
        batch.dispatch(0, &DispatchDepth::new(), |_, _| calls.set(calls.get() + 1)),
        Err(Error::State)
    );
    assert_eq!(
        queued.dispatch(&DispatchDepth::new(), |_| calls.set(calls.get() + 1)),
        Err(Error::State)
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    drop(batch);
    assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
    drop(queued);
    owner.as_ref().finish().unwrap();
    assert_eq!(dropped.get(), 1);
}
