//! Fail actual queue allocations through the existing thread-local allocator.
use super::arm_failure;
use crate::native_host::event_pending::{EventBudget, Ready, Visibility};
use paperpaw_native::host_glue::ERR_NO_MEMORY;

#[test]
fn event_sink_install_oom_drops_input_and_retry_installs_one_endpoint() {
    use crate::native_host::event_producers::EventSink;
    use alloc::rc::Rc;
    use core::cell::Cell;
    struct Sink {
        drops: Rc<Cell<u32>>,
        calls: Rc<Cell<u32>>,
    }
    impl Drop for Sink {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    impl EventSink for Sink {
        fn setting(&self, _: u64, _: &str, _: Result<&[u8], i32>) {}
        fn hint(&self, _: u32) {
            self.calls.set(self.calls.get() + 1);
        }
    }
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let drops = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let sink = Sink {
        drops: drops.clone(),
        calls: calls.clone(),
    };
    arm_failure(1);
    let result = kernel.install_native_event_sink(sink);
    arm_failure(0);
    assert_eq!(result, Err(ERR_NO_MEMORY));
    assert_eq!(drops.get(), 1);
    assert!(kernel.native_event_sink().is_none());
    assert_eq!(calls.get(), 0);
    kernel
        .install_native_event_sink(Sink {
            drops: drops.clone(),
            calls: calls.clone(),
        })
        .unwrap();
    kernel
        .native_event_sink()
        .unwrap()
        .hint(crate::native_host::registrations::EVENT_STATUS_CHANGED_TAG);
    assert_eq!(calls.get(), 1);
    assert_eq!(drops.get(), 1);
    drop(kernel);
    assert_eq!(drops.get(), 2);
}

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
