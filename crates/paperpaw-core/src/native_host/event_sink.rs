//! Kernel-owned producer endpoint for the single native runtime.
use super::event_producers::EventSink;
use alloc::boxed::Box;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};

impl crate::Kernel {
    /// Producers borrow only this endpoint. Calls queue data, never enter plugins.
    pub(crate) fn native_event_sink(&self) -> Option<&dyn EventSink> {
        self.shared.native_events.as_deref()
    }

    /// Install the real runtime endpoint once, after its fallible construction.
    /// No endpoint is synthesized while native activation is disabled.
    pub(crate) fn install_native_event_sink(
        &mut self,
        sink: impl EventSink + 'static,
    ) -> Result<(), i32> {
        if self.shared.native_events.is_some() {
            return Err(ERR_STATE);
        }
        let sink: Box<dyn EventSink> = super::shared::try_box(sink).ok_or(ERR_NO_MEMORY)?;
        self.shared.native_events = Some(sink);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
    use super::*;
    use crate::native_host::{
        event_pending::{EventBudget, Pending, Ready, Visibility},
        registrations as wire,
    };
    use alloc::rc::Rc;
    use core::cell::RefCell;

    struct Sink(Rc<RefCell<Pending>>);
    impl EventSink for Sink {
        fn setting(&self, now: u64, key: &str, old: Result<&[u8], i32>) {
            self.0
                .borrow_mut()
                .setting(now, key, old, Visibility::Readable)
                .unwrap();
        }
        fn hint(&self, tag: u32) {
            self.0.borrow_mut().hint(tag).unwrap();
        }
    }

    #[test]
    fn optional_endpoint_routes_to_installed_pending_state_without_replacement() {
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        assert!(kernel.native_event_sink().is_none());
        let budget = EventBudget::try_new().unwrap();
        let pending = Rc::new(RefCell::new(budget.pending().unwrap()));
        let replacement = Rc::new(RefCell::new(budget.pending().unwrap()));
        kernel
            .install_native_event_sink(Sink(pending.clone()))
            .unwrap();
        assert_eq!(
            kernel.install_native_event_sink(Sink(replacement.clone())),
            Err(ERR_STATE)
        );
        let sink = kernel.native_event_sink().unwrap();
        sink.hint(wire::EVENT_STATUS_CHANGED_TAG);
        assert!(matches!(
            pending.borrow_mut().take(),
            Some(Ready::Hint(wire::EVENT_STATUS_CHANGED_TAG))
        ));
        assert!(pending.borrow_mut().take().is_none());
        assert!(replacement.borrow_mut().take().is_none());
    }
}
