//! The host's own call to a format: refused inside a callback, metered to the
//! callee, a reply that is not whole reported with `dropped`.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::*;
use crate::native_host::format_call::tests::{Behavior, Harness};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND};

fn run(h: &Harness, kernel: &crate::Kernel, plugin: u64, out: &mut [u8]) -> Result<usize, i32> {
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    HostRoute {
        registry: &h.registry,
        env: &env,
        meter: &kernel.shared.meter,
    }
    .call(Target { plugin, token: 7 }, b"\x01\x02", out)
}

fn kernel() -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

#[test]
fn the_format_runs_as_its_own_and_is_metered_to_itself() {
    let kernel = kernel();
    let mut h = Harness::new();
    let plugin = h.add("format-epub", Behavior::Good);
    let mut out = [0_u8; 64];
    assert_eq!(run(&h, &kernel, plugin, &mut out), Ok(5));
    assert_eq!(h.seen.borrow().len(), 1);
    assert_eq!(kernel.shared.meter.calls_of("format-epub"), 1);
}

#[test]
fn inside_a_callback_nothing_is_called() {
    let kernel = kernel();
    let mut h = Harness::new();
    let plugin = h.add("format-epub", Behavior::Good);
    h.registry.test_enter_callback();
    let mut out = [0_u8; 64];
    assert_eq!(run(&h, &kernel, plugin, &mut out), Err(ERR_BUSY));
    h.registry.test_leave_callback();
    assert!(h.seen.borrow().is_empty());
    assert_eq!(kernel.shared.meter.calls_of("format-epub"), 0);
}

#[test]
fn an_unknown_format_is_not_found_and_a_reply_that_is_not_whole_is_reported() {
    let kernel = kernel();
    let mut h = Harness::new();
    let mut out = [0_u8; 64];
    assert_eq!(run(&h, &kernel, 99, &mut out), Err(ERR_NOT_FOUND));
    let plugin = h.add("format-bad", Behavior::Truncated);
    assert_eq!(run(&h, &kernel, plugin, &mut out), Err(ERR_INVALID));
    assert_eq!(h.dropped.borrow().as_slice(), [(7, ERR_INVALID)]);
}

#[test]
fn the_formats_own_status_passes_through() {
    let kernel = kernel();
    let mut h = Harness::new();
    let plugin = h.add("format-epub", Behavior::Status(-12));
    let mut out = [0_u8; 64];
    assert_eq!(run(&h, &kernel, plugin, &mut out), Err(-12));
    assert!(h.dropped.borrow().is_empty());
}
