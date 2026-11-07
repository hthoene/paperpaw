//! Every broker preparation allocation refuses without dispatch or lazy publication.
use super::{NORMAL, arm_failure, reset};
use crate::native_host::stage_broker::{
    Run,
    tests::{Behavior, Harness, input, row, schema},
};
use paperpaw_native::host_glue::ERR_NO_MEMORY;

#[test]
fn broker_allocation_failure_keeps_callbacks_and_lazy_start_unpublished() {
    reset(NORMAL);
    let mut harness = Harness::new();
    let plugin = harness.add("probe", Behavior::Good);
    let lazy = harness
        .registry
        .insert(paperpaw_sdk::Id::new_static("lazy"), None, true)
        .unwrap();
    let schema = schema();
    let input = input();
    for nth in 1..=4 {
        arm_failure(nth);
        let result = Run::prepare(
            &schema,
            &harness.depth,
            &input,
            &harness.registry,
            [row(plugin, 1, 0, 0), row(lazy, 2, 0, 0)],
        );
        arm_failure(0);
        assert_eq!(result.err(), Some(ERR_NO_MEMORY), "allocation {nth}");
        assert!(harness.calls.borrow().is_empty());
        assert_eq!(harness.registry.depth(), 0);
        assert_eq!(harness.registry.take_start(), None);
    }
    let run = Run::prepare(
        &schema,
        &harness.depth,
        &input,
        &harness.registry,
        [row(plugin, 1, 0, 0), row(lazy, 2, 0, 0)],
    )
    .unwrap();
    assert_eq!(harness.registry.take_start(), None);
    drop(run);
    assert_eq!(harness.registry.take_start(), Some(lazy));
}
