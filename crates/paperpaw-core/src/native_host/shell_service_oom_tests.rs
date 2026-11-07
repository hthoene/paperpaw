//! Real service-handle and scratch allocations fail without publishing output.
use super::arm_failure;
use crate::native_host::host_services::Caller;
use crate::{
    abi7::{context::Frame, services_wire::ServiceOp},
    native_host::{
        HostResources, call_services, declared_services::DeclaredServices,
        environment::EnvironmentScope, registrations,
    },
};
use core::pin::pin;
use paperpaw_native::{
    host_glue::{self as abi, PpBuffer, PpBytes},
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
fn pair(bytes: &[u8]) -> PpBytes {
    PpBytes {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

#[test]
fn failed_handle_and_reply_allocations_recover_without_sticking_busy() {
    let pool = HostResources::with_test_service(echo).unwrap();
    let mut owner = pool.try_owner().unwrap();
    owner.bind_caller("fixture").unwrap();
    let scope = pin!(EnvironmentScope::with_resources(owner.try_view().unwrap()));
    let declared = DeclaredServices::try_from_iter(core::iter::once(Declared {
        key: "core.test-echo",
        major: 1,
        semantic_revision: 0,
    }))
    .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let mut input = [0; 64];
    let length = registrations::records_encode(
        &[registrations::Record::ServiceEcho(
            registrations::ServiceEcho { text: "recovery" },
        )],
        &mut input,
    )
    .unwrap();
    scope
        .as_ref()
        .scope(&env, || {
            let mut handle = 0x9876;
            arm_failure(1);
            // SAFETY: all fixture pairs are live, bounded and disjoint.
            let refused = unsafe {
                call_services::open(
                    &scope,
                    &owner,
                    &declared,
                    pair(b"core.test-echo"),
                    1,
                    0,
                    &mut handle,
                )
            };
            arm_failure(0);
            assert_eq!(refused, abi::ERR_NO_MEMORY);
            assert_eq!(handle, 0x9876);
            // SAFETY: the same valid fixture storage remains live.
            assert_eq!(
                unsafe {
                    call_services::open(
                        &scope,
                        &owner,
                        &declared,
                        pair(b"core.test-echo"),
                        1,
                        0,
                        &mut handle,
                    )
                },
                abi::OK
            );
            let mut out = [0xa5; 512];
            arm_failure(1);
            // SAFETY: live input and exclusive output storage do not overlap.
            let refused = unsafe {
                call_services::call(
                    &scope,
                    &owner,
                    "files",
                    handle,
                    1,
                    pair(&input[..length]),
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: out.len(),
                    },
                )
            };
            arm_failure(0);
            assert_eq!(refused, abi::ERR_NO_MEMORY);
            assert_eq!(out, [0xa5; 512]);
            // SAFETY: the same valid fixture storage remains live.
            let count = unsafe {
                call_services::call(
                    &scope,
                    &owner,
                    "files",
                    handle,
                    1,
                    pair(&input[..length]),
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: out.len(),
                    },
                )
            };
            assert_eq!(count, (length - 1) as i32);
            assert_eq!(&out[..length - 1], &input[1..length]);
            assert!(out[length - 1..].iter().all(|byte| *byte == 0xa5));
            owner.release(handle).unwrap();
        })
        .unwrap();
}

#[test]
fn failed_identity_binding_leaves_the_owner_unresolved_and_can_retry() {
    let pool = HostResources::with_test_service(echo).unwrap();
    let mut owner = pool.try_owner().unwrap();
    arm_failure(1);
    let result = owner.bind_caller("fixture");
    arm_failure(0);
    assert_eq!(result, Err(abi::ERR_NO_MEMORY));
    assert!(matches!(owner.bound_caller(), Err(abi::ERR_DENIED)));
    owner.bind_caller("fixture").unwrap();
    assert_eq!(owner.bound_caller().unwrap().as_str(), "fixture");
}
