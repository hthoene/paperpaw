//! Real service-handle and scratch allocations fail without publishing output.
use super::arm_failure;
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

fn echo(_: &dyn Frame, _: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    out[..input.len()].copy_from_slice(input);
    Ok(input.len())
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
    let owner = pool.try_owner().unwrap();
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
                    handle,
                    1,
                    pair(&input[..length]),
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: out.len(),
                    },
                )
            };
            assert_eq!(count, length as i32);
            assert_eq!(&out[..length], &input[..length]);
            assert!(out[length..].iter().all(|byte| *byte == 0xa5));
            owner.release(handle).unwrap();
        })
        .unwrap();
}
