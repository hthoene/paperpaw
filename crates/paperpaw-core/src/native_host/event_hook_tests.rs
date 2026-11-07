#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::event_pending::{EventBudget, Pending, Ready, Visibility};
use super::event_producers::EventSink;
use super::registrations as wire;
use crate::Kernel;
use crate::abi7::settings_wire::{SettingValueRef, settings_decode};
use alloc::{rc::Rc, string::String};
use core::cell::RefCell;
use paperpaw_platform::{Battery, Instant, SystemStatus};
use paperpaw_sdk::{
    Id, Manifest, Plugin, RegisterError, Registrar, SettingKind, SettingSpec, SettingValue, Version,
};

struct Fixture;
impl Plugin for Fixture {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("evt"), "EVT fixture", Version::new(1, 0, 0))
    }
    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        for (id, scoped) in [("evt.global", false), ("evt.scoped", true)] {
            let mut spec = SettingSpec::new(
                Id::new_static(id),
                "fixture.value",
                SettingKind::Int { min: 0, max: 10 },
                SettingValue::Int(1),
                paperpaw_sdk::builtin::groups::OTHER,
            );
            spec.scoped = scoped;
            registrar.add_setting(spec)?;
        }
        Ok(())
    }
}
struct Sink(Rc<RefCell<Pending>>);
impl EventSink for Sink {
    fn setting(&self, now: u64, key: &str, old: Result<&[u8], i32>) {
        // These small fixture mutations cannot exceed the bounded store.
        self.0
            .borrow_mut()
            .setting(now, key, old, Visibility::Readable)
            .unwrap();
    }
    fn hint(&self, tag: u32) {
        self.0.borrow_mut().hint(tag).unwrap();
    }
}
fn fixture() -> (Kernel, Rc<RefCell<Pending>>) {
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .plugin(Fixture)
        .wifi(true)
        .build()
        .unwrap();
    let budget = EventBudget::try_new().unwrap();
    let pending = Rc::new(RefCell::new(budget.pending().unwrap()));
    kernel
        .install_native_event_sink(Sink(pending.clone()))
        .unwrap();
    (kernel, pending)
}
fn old_int(pending: &RefCell<Pending>, key: &str) -> Option<i64> {
    let event = pending.borrow_mut().take().unwrap();
    let Some(wire::Record::EventSettingChanged(changed)) =
        event.setting_record(99, |_| Visibility::Readable).unwrap()
    else {
        panic!("expected setting edge")
    };
    assert_eq!(changed.key, key);
    if changed.old.is_empty() {
        return None;
    }
    let SettingValueRef::Int(value) = settings_decode(changed.old).unwrap() else {
        panic!("expected old integer")
    };
    Some(value)
}

#[test]
fn setting_commands_capture_before_commit_and_failed_writes_raise_nothing() {
    let (mut kernel, pending) = fixture();
    kernel.set_setting(Instant(1), "evt.global", "3").unwrap();
    assert_eq!(old_int(&pending, "evt.global"), None);
    assert_eq!(kernel.settings().int("evt.global"), 3);
    kernel
        .output
        .commands
        .push(paperpaw_sdk::Command::SetSetting(
            Id::new_static("evt.global"),
            SettingValue::Int(99),
        ));
    kernel.run_commands();
    assert!(pending.borrow_mut().take().is_none());
    assert_eq!(kernel.settings().int("evt.global"), 3);
    kernel.reset_setting(&Id::new_static("evt.global"));
    assert_eq!(old_int(&pending, "evt.global"), Some(3));
    assert_eq!(kernel.settings().int("evt.global"), 1);
}

#[test]
fn scoped_set_reset_and_scope_exit_keep_typed_old_values() {
    let (mut kernel, pending) = fixture();
    kernel.set_scope(Some(String::from("scope.ini")));
    assert!(pending.borrow_mut().take().is_none());
    kernel.set_in_scope(&Id::new_static("core.scope-only"), SettingValue::Bool(true));
    let switch = pending.borrow_mut().take().unwrap();
    let Some(wire::Record::EventSettingChanged(changed)) =
        switch.setting_record(99, |_| Visibility::Readable).unwrap()
    else {
        panic!("expected scope switch edge")
    };
    assert_eq!(changed.key, "core.scope-only");
    kernel.set_in_scope(&Id::new_static("evt.scoped"), SettingValue::Int(3));
    assert_eq!(old_int(&pending, "evt.scoped"), None);
    kernel.reset_setting(&Id::new_static("evt.scoped"));
    assert_eq!(old_int(&pending, "evt.scoped"), Some(3));
    kernel.set_in_scope(&Id::new_static("evt.scoped"), SettingValue::Int(5));
    assert_eq!(old_int(&pending, "evt.scoped"), None);
    kernel.set_scope(None);
    assert!(matches!(pending.borrow_mut().take(), Some(Ready::Reread)));
    assert!(pending.borrow_mut().take().is_none());
    assert_eq!(kernel.settings().int("evt.scoped"), 1);
}

#[test]
fn wifi_status_charge_and_plugin_toggle_raise_hints_in_producer_order() {
    use crate::WifiEvent;
    use paperpaw_sdk::wifi::WifiState;
    let (mut kernel, pending) = fixture();
    kernel.wifi_event(
        Instant(1),
        WifiEvent::State(WifiState::Hosting {
            ssid: "device".into(),
            address: "192.168.4.1".into(),
            guests: 0,
        }),
    );
    assert!(matches!(
        pending.borrow_mut().take(),
        Some(Ready::Hint(wire::EVENT_WIFI_CHANGED_TAG))
    ));
    assert!(matches!(
        pending.borrow_mut().take(),
        Some(Ready::Hint(wire::EVENT_STATUS_CHANGED_TAG))
    ));
    assert!(pending.borrow_mut().take().is_none());
    kernel.set_status(
        Instant(2),
        SystemStatus::new(None, Some(Battery::new(5, true, None))),
    );
    assert!(matches!(
        pending.borrow_mut().take(),
        Some(Ready::Hint(wire::EVENT_STATUS_CHANGED_TAG))
    ));
    assert!(matches!(
        pending.borrow_mut().take(),
        Some(Ready::Hint(wire::EVENT_CHARGE_CHANGED_TAG))
    ));
    assert!(pending.borrow_mut().take().is_none());
    kernel.set_plugin_enabled(&Id::new_static("evt"), false);
    assert!(matches!(
        pending.borrow_mut().take(),
        Some(Ready::Hint(wire::EVENT_PLUGINS_CHANGED_TAG))
    ));
    assert!(pending.borrow_mut().take().is_none());
}

#[test]
fn setting_edges_ignore_preview_and_capture_the_stored_preimage() {
    let (mut kernel, pending) = fixture();
    kernel.set_setting(Instant(1), "evt.global", "3").unwrap();
    assert_eq!(old_int(&pending, "evt.global"), None);
    kernel.shared.settings.set_preview(alloc::vec![(
        Id::new_static("evt.global"),
        Some(SettingValue::Int(9))
    )]);
    assert_eq!(kernel.settings().int("evt.global"), 9);
    assert!(pending.borrow_mut().take().is_none());
    kernel.set_setting(Instant(2), "evt.global", "4").unwrap();
    assert_eq!(old_int(&pending, "evt.global"), Some(3));
    kernel.reset_setting(&Id::new_static("evt.global"));
    assert_eq!(old_int(&pending, "evt.global"), Some(4));
}
