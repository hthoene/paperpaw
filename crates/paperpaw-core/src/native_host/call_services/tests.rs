#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
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
use core::{pin::pin, ptr};
use paperpaw_native::{
    host_glue::{ERR_BUSY, ERR_FULL, ERR_NOT_FOUND, ERR_STATE, ERR_UTF8},
    metadata::Declared,
};
use paperpaw_platform::Instant;

fn echo(_: &dyn Frame, _: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let records = registrations::records_decode(input)?;
    if records.len() != 1 {
        return Err(ERR_INVALID);
    }
    out[..input.len()].copy_from_slice(input);
    Ok(input.len())
}
fn failing(_: &dyn Frame, _: &ServiceOp, _: &[u8], out: &mut [u8]) -> Result<usize, i32> {
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
                    call(&scope, &owner, handle, 1, pair(&input), output(&mut out)),
                    ERR_BUSY
                );
                assert_eq!(out, [0xa5; 512]);
                drop(running);
                assert_eq!(
                    call(&scope, &owner, handle, 1, pair(&input), output(&mut out)),
                    input.len() as i32
                );
                assert_eq!(&out[..input.len()], input);
                assert!(out[input.len()..].iter().all(|byte| *byte == 0xa5));
            }
        })
        .unwrap();
}

#[test]
fn service_failure_never_copies_partial_scratch_and_does_not_stick_busy() {
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
                        call(&scope, &owner, handle, 1, pair(&input), output(&mut out)),
                        ERR_BUSY
                    );
                    assert_eq!(out, [0xa5; 512]);
                }
            }
        })
        .unwrap();
}

#[test]
fn generic_schema_validation_accepts_multiplicity_but_echo_requires_one_record() {
    let bytes = batch("one");
    assert_eq!(
        validate_batch(&bytes, &[registrations::SERVICE_ECHO_TAG]),
        Ok(())
    );
    let mut two = bytes.clone();
    two[0] = 2;
    two.extend_from_slice(&bytes[1..]);
    assert_eq!(
        validate_batch(&two, &[registrations::SERVICE_ECHO_TAG]),
        Ok(())
    );
    assert_eq!(
        validate_batch(&[0], &[registrations::SERVICE_ECHO_TAG]),
        Ok(())
    );
    assert_eq!(validate_batch(&bytes, &[]), Err(ERR_UNSUPPORTED));
    let key = services_wire::service_schema("core.test-echo", 1).unwrap();
    let op = key.op(1).unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let mut out = [0xa5; 512];
    assert_eq!(echo(&env, op, &two, &mut out), Err(ERR_INVALID));
    assert_eq!(echo(&env, op, &[0], &mut out), Err(ERR_INVALID));
    assert_eq!(out, [0xa5; 512]);
}
