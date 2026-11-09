//! The registration cap of a plugin (32): records are stored as they come.
use super::*;

// Mode 15 of `submit_registrations`: ten events in two batches, past the old
// cap of eight registrations.

#[test]
fn more_than_eight_registrations_are_admitted_and_every_allocation_fault_refuses_whole() {
    // Ten records: the 9th and 10th need room the first eight did not take.
    let (mut kernel, result) = regs_kernel(15);
    result.unwrap();
    assert!(kernel.unload_plugin(&Id::new_static("factory")));
    let mut reached_success = false;
    for allocation in 1..96 {
        reset(NORMAL);
        DESTROYED.with(|value| value.set(0));
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        REGS.with(|value| value.set(15));
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = install(&mut kernel, 0);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        REGS.with(|value| value.set(0));
        if result.is_ok() {
            reached_success = true;
            break;
        }
        let shared = &kernel.shared;
        assert!(!shared.registry.screens.contains_key("factory.screen"));
        assert!(shared.native_instances.find("factory").is_none());
        assert_eq!(DESTROYED.with(Cell::get), 1, "{allocation}");
    }
    assert!(reached_success, "the fault walk reached an admitted owner");
}

#[test]
fn the_staged_records_are_not_kept_in_fixed_arrays() {
    use crate::native_host::{Construction, RegistrationBatch};
    use core::mem::size_of;
    // Neither type holds a record inline: a record ten times the size makes
    // neither of them larger, so the heap cost is what a plugin registered.
    type Big = [u64; 16];
    assert_eq!(
        size_of::<RegistrationBatch<'static, (), (), Big>>(),
        size_of::<RegistrationBatch<'static, (), (), u8>>()
    );
    assert_eq!(
        size_of::<Construction<'static, (), (), Big>>(),
        size_of::<Construction<'static, (), (), u8>>()
    );
}
