//! Each allocation of a format call refuses before the format is entered.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::{NORMAL, arm_failure, reset};
use crate::native_host::format_call::{
    Target, call,
    tests::{Behavior, Harness},
};
use paperpaw_native::host_glue::ERR_NO_MEMORY;

#[test]
fn a_format_call_allocation_failure_never_enters_the_format() {
    reset(NORMAL);
    let mut harness = Harness::new();
    let plugin = harness.add("format-epub", Behavior::Good);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let target = Target { plugin, token: 7 };
    let mut out = [0_u8; 64];
    // The framed call, the one allocation: the format answers into `out`, no
    // reply buffer of its own (#248).
    arm_failure(1);
    let result = call(&harness.registry, target, &[1, 2], &mut out, &env);
    arm_failure(0);
    assert_eq!(result, Err(ERR_NO_MEMORY));
    assert!(harness.seen.borrow().is_empty());
    assert_eq!(harness.registry.depth(), 0);
    assert_eq!(out, [0; 64]);
    assert_eq!(
        call(&harness.registry, target, &[1, 2], &mut out, &env),
        Ok(5)
    );
}
