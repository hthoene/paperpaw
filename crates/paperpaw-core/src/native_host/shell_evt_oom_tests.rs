//! Fail actual queue allocations through the existing thread-local allocator.
use super::arm_failure;
use crate::native_host::event_pending::{EventBudget, Ready, Visibility};
use paperpaw_native::host_glue::ERR_NO_MEMORY;

#[test]
fn evt_budget_and_both_edge_allocations_refuse_and_recover() {
    arm_failure(1);
    let failed = EventBudget::try_new();
    arm_failure(0);
    assert!(matches!(failed, Err(ERR_NO_MEMORY)));
    for allocation in [1, 2] {
        let budget = EventBudget::try_new().unwrap();
        let mut pending = budget.pending().unwrap();
        arm_failure(allocation);
        let result = pending.setting(1, "plugin.value", Ok(&[]), Visibility::Readable);
        arm_failure(0);
        assert_eq!(result, Err(ERR_NO_MEMORY));
        assert_eq!(budget.used(), 0);
        assert!(matches!(pending.take(), Some(Ready::Reread)));
        assert!(pending.take().is_none());
        pending
            .setting(2, "plugin.value", Ok(&[]), Visibility::Readable)
            .unwrap();
        assert!(budget.used() > 0);
        assert!(matches!(pending.take(), Some(Ready::Edge(_))));
        assert_eq!(budget.used(), 0);
    }
}
