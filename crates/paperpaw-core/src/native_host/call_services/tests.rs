#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::native_host::host_services::Caller;
use crate::{
    abi7::{
        context::Frame,
        services_wire::{self, ServiceOp},
    },
    native_host::{
        HostResources,
        resources::{FontKey, FontRole},
    },
};
use alloc::vec::Vec;
use core::{pin::pin, ptr};
use paperpaw_native::{
    host_glue::{ERR_BUSY, ERR_FULL, ERR_NOT_FOUND, ERR_STATE, ERR_UTF8},
    metadata::Declared,
};
use paperpaw_platform::Instant;

fn echo(
    _: &dyn Frame,
    _: &Caller<'_>,
    _: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let record = registrations::records_decode_single(input)?;
    registrations::record_encode_one(&record, out)
}
fn failing(
    _: &dyn Frame,
    _: &Caller<'_>,
    _: &ServiceOp,
    _: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    out[0] = 0;
    Err(ERR_BUSY)
}
fn pair(input: &[u8]) -> PpBytes {
    PpBytes {
        ptr: input.as_ptr(),
        len: input.len(),
    }
}
fn output(out: &mut [u8]) -> PpBuffer {
    PpBuffer {
        ptr: out.as_mut_ptr(),
        cap: out.len(),
    }
}
fn declared() -> DeclaredServices {
    DeclaredServices::try_from_iter(core::iter::once(Declared {
        key: services_wire::core_test_echo_v1::KEY,
        major: 1,
        semantic_revision: 0,
    }))
    .unwrap()
}
fn batch(text: &str) -> Vec<u8> {
    let mut out = [0; 512];
    let length = registrations::records_encode(
        &[registrations::Record::ServiceEcho(
            registrations::ServiceEcho { text },
        )],
        &mut out,
    )
    .unwrap();
    out[..length].to_vec()
}

#[test]
fn open_statuses_unique_keys_and_generic_release_preserve_other_families() {
    let pool = HostResources::with_test_service(echo).unwrap();
    let owner = pool.try_owner().unwrap();
    let scope = pin!(EnvironmentScope::with_resources(owner.try_view().unwrap()));
    let declared = declared();
    let key = pair(b"core.test-echo");
    let mut handle = 0x9876;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    // SAFETY: all nonnull fixture pairs are live and disjoint.
    unsafe {
        assert_eq!(
            open(&scope, &owner, &declared, key, 1, 0, &mut handle),
            ERR_STATE
        );
        assert_eq!(
            open(&scope, &owner, &declared, key, 1, 0, ptr::null_mut()),
            ERR_INVALID
        );
        scope
            .as_ref()
            .scope(&env, || {
                for (key, major, minor, status) in [
                    (&b"missing.service"[..], 1, 0, ERR_NOT_FOUND),
                    (&b"bad/name"[..], 1, 0, ERR_INVALID),
                    (&b"\xff"[..], 1, 0, ERR_UTF8),
                    (&b"core.test-echo"[..], 2, 0, ERR_UNSUPPORTED),
                    (&b"core.test-echo"[..], 1, 1, ERR_UNSUPPORTED),
                ] {
                    assert_eq!(
                        open(
                            &scope,
                            &owner,
                            &declared,
                            pair(key),
                            major,
                            minor,
                            &mut handle
                        ),
                        status
                    );
                    assert_eq!(handle, 0x9876);
                }
                assert_eq!(
                    open(
                        &scope,
                        &owner,
                        &DeclaredServices::none(),
                        key,
                        1,
                        0,
                        &mut handle
                    ),
                    ERR_UNSUPPORTED
                );
                assert_eq!(open(&scope, &owner, &declared, key, 1, 0, &mut handle), OK);
                let first = handle;
                assert_eq!(
                    open(&scope, &owner, &declared, key, 1, 0, &mut handle),
                    ERR_FULL
                );
                assert_eq!(handle, first);
                let font = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
                assert_eq!(owner.with_font(handle, |_| Ok(())), Err(ERR_STATE));
                owner.release(handle).unwrap();
                assert_eq!(owner.service(handle).unwrap_err(), ERR_STATE);
                owner.with_font(font, |_| Ok(())).unwrap();
                assert_eq!(open(&scope, &owner, &declared, key, 1, 0, &mut handle), OK);
                assert_ne!(handle, first);
            })
            .unwrap();
    }
}

#[test]
fn device_never_offers_the_test_only_service() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    assert_eq!(
        owner
            .service_schema(services_wire::core_test_echo_v1::KEY)
            .unwrap_err(),
        ERR_NOT_FOUND
    );
}

#[test]
fn call_checks_schema_and_owner_before_publishing_a_reply() {
    let pool = HostResources::with_test_service(echo).unwrap();
    let owner = pool.try_owner().unwrap();
    let foreign = pool.try_owner().unwrap();
    let scope = pin!(EnvironmentScope::with_resources(owner.try_view().unwrap()));
    let input = batch("deterministic echo");
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    scope
        .as_ref()
        .scope(&env, || {
            let mut handle = 0;
            // SAFETY: fixture input and output allocations are live and disjoint.
            unsafe {
                assert_eq!(
                    open(
                        &scope,
                        &owner,
                        &declared(),
                        pair(b"core.test-echo"),
                        1,
                        0,
                        &mut handle
                    ),
                    OK
                );
                let mut out = [0xa5; 512];
                for (target, op, bytes, capacity, status) in [
                    (&foreign, 1, input.as_slice(), 512, ERR_STATE),
                    (&owner, 2, input.as_slice(), 512, ERR_UNSUPPORTED),
                    (&owner, 1, input.as_slice(), 511, ERR_NO_SPACE),
                    (&owner, 1, &b"\0"[..], 512, ERR_INVALID),
                ] {
                    assert_eq!(
                        call(
                            &scope,
                            target,
                            "files",
                            handle,
                            op,
                            pair(bytes),
                            PpBuffer {
                                ptr: out.as_mut_ptr(),
                                cap: capacity
                            }
                        ),
                        status
                    );
                    assert_eq!(out, [0xa5; 512]);
                }
                let running = owner.enter_service(owner.service(handle).unwrap()).unwrap();
                assert_eq!(
                    call(
                        &scope,
                        &owner,
                        "files",
                        handle,
                        1,
                        pair(&input),
                        output(&mut out)
                    ),
                    ERR_BUSY
                );
                assert_eq!(out, [0xa5; 512]);
                drop(running);
                assert_eq!(
                    call(
                        &scope,
                        &owner,
                        "files",
                        handle,
                        1,
                        pair(&input),
                        output(&mut out)
                    ),
                    (input.len() - 1) as i32
                );
                assert_eq!(&out[..input.len() - 1], &input[1..]);
                assert!(out[input.len() - 1..].iter().all(|byte| *byte == 0));
            }
        })
        .unwrap();
}

#[test]
fn service_failure_leaves_no_reply_and_does_not_stick_busy() {
    let pool = HostResources::with_test_service(failing).unwrap();
    let owner = pool.try_owner().unwrap();
    let scope = pin!(EnvironmentScope::with_resources(owner.try_view().unwrap()));
    let input = batch("failure");
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    scope
        .as_ref()
        .scope(&env, || {
            let mut handle = 0;
            let mut out = [0xa5; 512];
            // SAFETY: fixture pairs are live, disjoint and bounded.
            unsafe {
                assert_eq!(
                    open(
                        &scope,
                        &owner,
                        &declared(),
                        pair(b"core.test-echo"),
                        1,
                        0,
                        &mut handle
                    ),
                    OK
                );
                for _ in 0..2 {
                    assert_eq!(
                        call(
                            &scope,
                            &owner,
                            "files",
                            handle,
                            1,
                            pair(&input),
                            output(&mut out)
                        ),
                        ERR_BUSY
                    );
                    assert_eq!(out, [0; 512]);
                }
            }
        })
        .unwrap();
}

#[test]
fn generic_schema_validation_accepts_multiplicity_but_echo_requires_one_record() {
    let bytes = batch("one");
    assert_eq!(
        validate_batch(&bytes, &[registrations::SERVICE_ECHO_TAG], 2),
        Ok(())
    );
    let mut two = bytes.clone();
    two[0] = 2;
    two.extend_from_slice(&bytes[1..]);
    assert_eq!(
        validate_batch(&two, &[registrations::SERVICE_ECHO_TAG], 2),
        Ok(())
    );
    assert_eq!(
        validate_batch(&[0], &[registrations::SERVICE_ECHO_TAG], 2),
        Ok(())
    );
    assert_eq!(validate_batch(&bytes, &[], 2), Err(ERR_UNSUPPORTED));
    let key = services_wire::service_schema("core.test-echo", 1).unwrap();
    let op = key.op(1).unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let mut out = [0xa5; 512];
    let hosts = crate::native_host::host_services::HostServices::new();
    let who = Caller {
        owner: 1,
        plugin: "files",
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
        hosts: &hosts,
    };
    assert_eq!(echo(&env, &who, op, &two, &mut out), Err(ERR_INVALID));
    assert_eq!(echo(&env, &who, op, &[0], &mut out), Err(ERR_INVALID));
    assert_eq!(out, [0xa5; 512]);
}

std::thread_local! {
    static BROKEN_REPLY: core::cell::Cell<u8> = const { core::cell::Cell::new(0) };
}
fn broken_then_echo(
    frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    match BROKEN_REPLY.with(core::cell::Cell::get) {
        1 => {
            out[0] = 0;
            Ok(out.len() + 1)
        }
        2 => {
            out[0] = 1;
            Ok(1)
        } // One record promised, no frame.
        3 => registrations::records_encode(
            &[registrations::Record::EventStarted(
                registrations::EventStarted { now_ms: 0 },
            )],
            out,
        ),
        _ => echo(frame, who, op, input, out),
    }
}

#[test]
fn broken_backend_replies_preserve_output_and_clear_busy_before_recovery() {
    let pool = HostResources::with_test_service(broken_then_echo).unwrap();
    let owner = pool.try_owner().unwrap();
    let scope = pin!(EnvironmentScope::with_resources(owner.try_view().unwrap()));
    let input = batch("after broken reply");
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    scope
        .as_ref()
        .scope(&env, || {
            let mut handle = 0;
            // SAFETY: all fixture pairs are live, bounded and disjoint.
            unsafe {
                assert_eq!(
                    open(
                        &scope,
                        &owner,
                        &declared(),
                        pair(b"core.test-echo"),
                        1,
                        0,
                        &mut handle
                    ),
                    OK
                );
                for (mode, status) in [(1, ERR_INVALID), (2, ERR_INVALID), (3, ERR_INVALID)] {
                    let mut out = [0xa5; 512];
                    BROKEN_REPLY.with(|value| value.set(mode));
                    assert_eq!(
                        call(
                            &scope,
                            &owner,
                            "files",
                            handle,
                            1,
                            pair(&input),
                            output(&mut out)
                        ),
                        status
                    );
                    assert_eq!(out, [0; 512]);
                    BROKEN_REPLY.with(|value| value.set(0));
                    assert_eq!(
                        call(
                            &scope,
                            &owner,
                            "files",
                            handle,
                            1,
                            pair(&input),
                            output(&mut out)
                        ),
                        (input.len() - 1) as i32
                    );
                    assert_eq!(&out[..input.len() - 1], &input[1..]);
                    assert!(out[input.len() - 1..].iter().all(|byte| *byte == 0));
                }
            }
        })
        .unwrap();
}

#[path = "row_tests.rs"]
mod row_tests;

#[test]
fn the_reply_room_keeps_a_checked_reply_and_wipes_everything_else() {
    let secret = b"private reply";
    let mut bytes = [0xa5; 64];
    {
        let room = ReplyOnFailure(&mut bytes);
        room.0.fill(0x73);
        room.0[..secret.len()].copy_from_slice(secret);
        room.keep(secret.len());
    }
    assert_eq!(&bytes[..secret.len()], secret);
    assert!(
        bytes[secret.len()..].iter().all(|byte| *byte == 0),
        "what the service left past the reply is wiped"
    );

    let fail = |bytes: &mut [u8]| -> Result<(), i32> {
        let room = ReplyOnFailure(bytes);
        room.0[..secret.len()].copy_from_slice(secret);
        Err(ERR_BUSY)
    };
    assert_eq!(fail(&mut bytes), Err(ERR_BUSY));
    assert_eq!(bytes, [0; 64]);
}

#[cfg(panic = "unwind")]
#[test]
fn the_reply_room_is_wiped_when_backend_unwinds() {
    let mut bytes = [0xa5; 64];
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let room = ReplyOnFailure(&mut bytes);
        room.0.fill(0x73);
        panic!("fixture backend unwound");
    }));
    assert!(result.is_err());
    assert_eq!(bytes, [0; 64]);
}

std::thread_local! {
    static SCRATCH_MODE: core::cell::Cell<u8> = const { core::cell::Cell::new(0) };
}

fn observed_reply(
    frame: &dyn Frame,
    caller: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    out.fill(0x73);
    match SCRATCH_MODE.with(core::cell::Cell::get) {
        1 => Err(ERR_BUSY),
        2 => {
            out[0] = 0xff;
            Ok(1)
        }
        _ => echo(frame, caller, op, input, out),
    }
}

#[test]
fn a_failed_reply_is_wiped_and_a_kept_one_has_no_tail_left() {
    let pool = HostResources::with_test_service(observed_reply).unwrap();
    let owner = pool.try_owner().unwrap();
    let scope = pin!(EnvironmentScope::with_resources(owner.try_view().unwrap()));
    let input = batch("private service reply");
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    scope
        .as_ref()
        .scope(&env, || {
            let mut handle = 0;
            // SAFETY: all fixture pairs remain live, initialized and disjoint.
            unsafe {
                assert_eq!(
                    open(
                        &scope,
                        &owner,
                        &declared(),
                        pair(b"core.test-echo"),
                        1,
                        0,
                        &mut handle
                    ),
                    OK
                );
                for mode in 0..3 {
                    SCRATCH_MODE.with(|value| value.set(mode));
                    let mut out = [0xa5; 512];
                    let status = call(
                        &scope,
                        &owner,
                        "probe",
                        handle,
                        1,
                        pair(&input),
                        output(&mut out),
                    );
                    if mode == 0 {
                        assert_eq!(status, (input.len() - 1) as i32);
                        assert_eq!(&out[..input.len() - 1], &input[1..]);
                        assert!(out[input.len() - 1..].iter().all(|byte| *byte == 0));
                    } else {
                        assert_eq!(status, if mode == 1 { ERR_BUSY } else { ERR_INVALID });
                        assert_eq!(out, [0; 512]);
                    }
                }
            }
        })
        .unwrap();
}

#[test]
fn input_and_output_sharing_a_byte_are_apart_only_when_they_do_not() {
    let mut bytes = [0_u8; 32];
    let base = bytes.as_mut_ptr();
    let input = |at: usize, len: usize| PpBytes {
        // SAFETY: inside `bytes` (at most its end).
        ptr: unsafe { base.add(at) }.cast_const(),
        len,
    };
    let out = |at: usize, cap: usize| PpBuffer {
        // SAFETY: inside `bytes` (at most its end).
        ptr: unsafe { base.add(at) },
        cap,
    };
    assert!(apart(input(0, 8), out(8, 8)));
    assert!(apart(input(16, 8), out(8, 8)));
    assert!(!apart(input(0, 9), out(8, 8)));
    assert!(!apart(input(15, 4), out(8, 8)));
    assert!(!apart(input(10, 2), out(8, 8)));
    assert!(!apart(input(0, 32), out(8, 8)));
    // An empty side shares nothing.
    assert!(apart(input(8, 0), out(8, 8)));
    assert!(apart(input(8, 8), out(8, 0)));
}
