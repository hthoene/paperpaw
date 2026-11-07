#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::super::event_producers::{self, OldValue};
use super::Visibility::{Hidden, Readable, Redacted};
use super::*;
use core::cell::Cell;
use paperpaw_sdk::SettingValue;

fn raise(queue: &mut Pending, at: u64) {
    queue
        .setting(at, "plugin.value", Ok(&[]), Readable)
        .unwrap();
}
fn edge_time(ready: Ready) -> u64 {
    let Some(wire::Record::EventSettingChanged(event)) =
        ready.setting_record(999, |_| Readable).unwrap()
    else {
        panic!()
    };
    event.now_ms
}

#[test]
fn ordering_preserves_edges_and_moves_coalesced_hints_to_newest_position() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    raise(&mut queue, 1);
    queue.hint(wire::EVENT_PLUGINS_CHANGED_TAG).unwrap();
    queue.hint(wire::EVENT_WIFI_CHANGED_TAG).unwrap();
    raise(&mut queue, 2);
    queue.hint(wire::EVENT_PLUGINS_CHANGED_TAG).unwrap();
    assert_eq!(edge_time(queue.take().unwrap()), 1);
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_WIFI_CHANGED_TAG))
    ));
    assert_eq!(edge_time(queue.take().unwrap()), 2);
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_PLUGINS_CHANGED_TAG))
    ));
    assert!(queue.take().is_none());
    assert_eq!(budget.used(), 0);
}

#[test]
fn arbitrary_coalescing_and_overflow_never_wrap_an_old_edges_order() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    raise(&mut queue, 1);
    let bytes = budget.used();
    for _ in 0..70_000 {
        queue.hint(wire::EVENT_STATUS_CHANGED_TAG).unwrap();
        assert_eq!(
            queue.setting(2, "plugin.value", Err(ERR_NO_SPACE), Readable),
            Err(ERR_NO_SPACE)
        );
    }
    assert_eq!(budget.used(), bytes);
    assert_eq!(edge_time(queue.take().unwrap()), 1);
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_STATUS_CHANGED_TAG))
    ));
    assert!(matches!(queue.take(), Some(Ready::Reread)));
    assert!(queue.take().is_none());
}

#[test]
fn sixteen_edges_are_never_coalesced_and_overflow_has_an_independent_bit() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    for at in 0..16 {
        raise(&mut queue, at);
    }
    assert_eq!(
        queue.setting(17, "plugin.value", Ok(&[]), Readable),
        Err(ERR_NO_SPACE)
    );
    queue.hint(wire::EVENT_STATUS_CHANGED_TAG).unwrap();
    assert_eq!(
        queue.setting(18, "plugin.value", Ok(&[]), Readable),
        Err(ERR_NO_SPACE)
    );
    for at in 0..16 {
        assert_eq!(edge_time(queue.take().unwrap()), at);
    }
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_STATUS_CHANGED_TAG))
    ));
    let reread = queue.take().unwrap();
    let Some(wire::Record::EventSettingChanged(event)) =
        reread.setting_record(42, |_| Hidden).unwrap()
    else {
        panic!()
    };
    assert_eq!((event.now_ms, event.key, event.old), (42, "", &[][..]));
    assert!(queue.take().is_none());
}

#[test]
fn shared_byte_budget_and_per_plugin_cap_recover_on_pop_clear_and_drop() {
    let budget = EventBudget::try_new().unwrap();
    let mut a = budget.pending().unwrap();
    let mut b = budget.pending().unwrap();
    let mut c = budget.pending().unwrap();
    let old = [0; 512];
    for queue in [&mut a, &mut b, &mut c] {
        while queue.setting(1, "plugin.value", Ok(&old), Readable).is_ok() {}
        assert!(queue.held_bytes <= PLUGIN_BYTES);
        assert!(budget.used() <= DEVICE_BYTES);
    }
    assert!(c.edges.len() < a.edges.len());
    assert!(a.take().is_some());
    c.setting(2, "plugin.value", Ok(&old), Readable).unwrap();
    a.clear();
    b.clear();
    drop(c);
    assert_eq!(budget.used(), 0);
}

#[test]
fn visibility_filters_before_storage_and_again_before_delivery() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    let old = OldValue::capture(Some(&SettingValue::Bool(true)));
    queue
        .setting(1, "plugin.value", old.bytes(), Hidden)
        .unwrap();
    assert!(queue.take().is_none());
    assert_eq!(budget.used(), 0);
    for enqueue_visibility in [Readable, Redacted] {
        queue
            .setting(1, "plugin.value", old.bytes(), enqueue_visibility)
            .unwrap();
        let edge = queue.take().unwrap();
        assert!(edge.setting_record(2, |_| Hidden).unwrap().is_none());
        let Some(wire::Record::EventSettingChanged(event)) =
            edge.setting_record(2, |_| Redacted).unwrap()
        else {
            panic!()
        };
        assert!(event.old.is_empty());
        let Some(wire::Record::EventSettingChanged(event)) =
            edge.setting_record(2, |_| Readable).unwrap()
        else {
            panic!()
        };
        assert_eq!(event.old.is_empty(), enqueue_visibility == Redacted);
    }
    queue
        .setting(1, "plugin.value", Err(ERR_NO_SPACE), Redacted)
        .unwrap();
    assert!(matches!(queue.take(), Some(Ready::Edge(_))));
    assert!(queue.take().is_none());
}

#[test]
fn producer_reports_every_unrepresentable_edge_and_never_truncates_old() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    let value = SettingValue::Text("x".repeat(501).into());
    let old = OldValue::capture(Some(&value));
    let drops = Cell::new(0);
    assert_eq!(
        event_producers::setting(&mut queue, 1, "plugin.value", &old, Readable, |tag, why| {
            assert_eq!(tag, wire::EVENT_SETTING_CHANGED_TAG);
            assert!(matches!(why, super::super::flat::DropReason::Producer));
            drops.set(drops.get() + 1);
        }),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(drops.get(), 1);
    assert!(matches!(queue.take(), Some(Ready::Reread)));
    assert!(queue.take().is_none());
    assert_eq!(budget.used(), 0);
}

#[test]
fn pending_unsupported_events_do_not_enable_admission() {
    for tag in 64..=74 {
        assert!(!super::super::events::is_event_tag(tag));
    }
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    for tag in [64, 69, 75, u32::MAX] {
        assert_eq!(queue.hint(tag), Err(ERR_INVALID));
    }
    assert!(queue.take().is_none());
}
