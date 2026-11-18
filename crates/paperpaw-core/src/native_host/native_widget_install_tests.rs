//! A maker registers for `event.widget_changed` (86): admitted at the install.
use super::*;

/// A real plugin registers for `event.widget_changed` (86): admitted at the
/// install, not refused as an event with no delivery.
#[test]
fn a_registration_for_widget_changed_is_admitted() {
    let (kernel, result) = regs_kernel(13);
    result.unwrap();
    let key = kernel.shared.native_instances.find("factory").unwrap();
    assert!(
        kernel
            .shared
            .native_instances
            .subscribed(key, wire::EVENT_WIDGET_CHANGED_TAG)
    );
}
