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
        assert!(queue.budget.bytes.get() <= PLUGIN_BYTES);
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
fn the_events_of_subscription_checked_services_can_be_registered() {
    // 90 `image_ready`, 91 `library_changed`, 108 `document_closed` and 110
    // `lookup_ready`: their producers deliver only to a plugin that subscribed
    // (the native `files` plugin is refused at admission without 91).
    for tag in [90, 91, 108, 110] {
        assert!(super::super::events::is_event_tag(tag), "{tag}");
    }
}

#[test]
fn pending_unsupported_events_do_not_enable_admission() {
    // 64 and the hints 70-74 have their delivery (`setting_events`), 86 its own
    // (`widget_events`), and 75 and 76 are the registry's hints (`registry_changed` keeps
    // its bits, `hint` takes 70-74 and 76); the book events 65-69 register (nothing queues
    // them: they are no hints).
    for tag in (64..=76).chain(86..=86) {
        assert!(super::super::events::is_event_tag(tag));
    }
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    for tag in [64, 69, 75, 77, u32::MAX] {
        assert_eq!(queue.hint(tag), Err(ERR_INVALID));
    }
    assert!(queue.take().is_none());
}

#[test]
fn explicit_reread_uses_no_edge_budget_and_is_not_a_drop() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    queue.setting(1, "", Ok(&[]), Readable).unwrap();
    queue.setting(2, "", Ok(&[]), Readable).unwrap();
    assert_eq!(budget.used(), 0);
    assert!(matches!(queue.take(), Some(Ready::Reread)));
    assert!(queue.take().is_none());
}

fn retain_until_full(queue: &mut Pending, held: &mut Vec<Ready>) {
    let old = [0; 512];
    while queue.setting(1, "plugin.value", Ok(&old), Readable).is_ok() {
        held.push(queue.take().unwrap());
    }
}

#[test]
fn detached_events_keep_the_per_plugin_byte_charge_during_enqueue() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    let mut held = Vec::new();
    retain_until_full(&mut queue, &mut held);
    let used = budget.used();
    assert!(used > PLUGIN_BYTES - 600);
    assert!(used <= PLUGIN_BYTES);
    assert!(queue.edges.is_empty());
    assert_eq!(queue.budget.bytes.get(), used);
    assert_eq!(
        queue.setting(2, "plugin.value", Ok(&[0; 512]), Readable),
        Err(ERR_NO_SPACE)
    );
    drop(held.pop());
    queue
        .setting(3, "plugin.value", Ok(&[0; 512]), Readable)
        .unwrap();
    assert_eq!(budget.used(), used);
    queue.clear();
    assert!(budget.used() > 0);
    drop(held);
    assert_eq!(budget.used(), 0);
}

#[test]
fn detached_events_keep_the_shared_charge_across_queue_teardown() {
    let budget = EventBudget::try_new().unwrap();
    let mut a = budget.pending().unwrap();
    let mut b = budget.pending().unwrap();
    let mut c = budget.pending().unwrap();
    let mut held = Vec::new();
    for queue in [&mut a, &mut b, &mut c] {
        retain_until_full(queue, &mut held);
        assert!(budget.used() <= DEVICE_BYTES);
    }
    let used = budget.used();
    assert!(used > DEVICE_BYTES - 600);
    drop(a);
    b.clear();
    assert_eq!(budget.used(), used);
    assert_eq!(
        c.setting(2, "plugin.value", Ok(&[0; 512]), Readable),
        Err(ERR_NO_SPACE)
    );
    drop(held.pop());
    c.setting(3, "plugin.value", Ok(&[0; 512]), Readable)
        .unwrap();
    assert_eq!(budget.used(), used);
    drop(b);
    drop(c);
    assert!(budget.used() > 0);
    drop(held);
    assert_eq!(budget.used(), 0);
}

#[test]
fn repeated_take_and_retain_cannot_bypass_the_held_edge_limit() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    let mut held = Vec::new();
    for at in 0..EDGES {
        raise(&mut queue, at as u64);
        held.push(queue.take().unwrap());
    }
    assert!(queue.edges.is_empty());
    assert_eq!(
        queue.setting(17, "plugin.value", Ok(&[]), Readable),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(queue.budget.edges.get(), EDGES);
    drop(held.pop());
    raise(&mut queue, 18);
    assert_eq!(queue.budget.edges.get(), EDGES);
    drop(queue);
    assert!(budget.used() > 0);
    drop(held);
    assert_eq!(budget.used(), 0);
}

#[test]
fn registry_changed_bits_are_or_ed_until_delivered_and_keep_their_place() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    queue.registry(0);
    assert!(queue.take().is_none(), "no bit, no event");
    queue.registry(1);
    queue.hint(wire::EVENT_STATUS_CHANGED_TAG).unwrap();
    queue.registry(1 << 4);
    // Coalesced: one event with both bits, behind the hint raised before the
    // second bit.
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_STATUS_CHANGED_TAG))
    ));
    assert!(matches!(queue.take(), Some(Ready::Registry(0b1_0001))));
    assert!(queue.take().is_none());
    // Delivered: the next one starts from none.
    queue.registry(1 << 2);
    assert!(matches!(queue.take(), Some(Ready::Registry(0b100))));
    // Edges and registry bits share the order without clashing.
    for at in 0..16 {
        raise(&mut queue, at);
    }
    queue.registry(1);
    queue.clear();
    assert!(queue.take().is_none());
    queue.registry(2);
    assert!(matches!(queue.take(), Some(Ready::Registry(2))));
}

#[test]
fn the_preview_hint_is_one_pending_bit_that_keeps_the_order() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    queue.hint(wire::EVENT_PREVIEW_CHANGED_TAG).unwrap();
    queue.hint(wire::EVENT_STATUS_CHANGED_TAG).unwrap();
    queue.hint(wire::EVENT_PREVIEW_CHANGED_TAG).unwrap();
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_STATUS_CHANGED_TAG))
    ));
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_PREVIEW_CHANGED_TAG))
    ));
    assert!(queue.take().is_none());
    // Every bit and every edge fit the order together.
    for at in 0..16 {
        raise(&mut queue, at);
    }
    for tag in 70..=74 {
        queue.hint(tag).unwrap();
    }
    queue.hint(wire::EVENT_PREVIEW_CHANGED_TAG).unwrap();
    queue.registry(1);
    let mut told = 0;
    while queue.take().is_some() {
        told += 1;
    }
    assert_eq!(told, 16 + 5 + 1 + 1);
}

#[test]
fn registry_and_preview_bits_are_held_while_all_sixteen_edges_are_held() {
    let budget = EventBudget::try_new().unwrap();
    let mut queue = budget.pending().unwrap();
    for at in 0..16 {
        raise(&mut queue, at);
    }
    assert_eq!(
        queue.setting(17, "plugin.value", Ok(&[]), Readable),
        Err(ERR_NO_SPACE)
    );
    // Bits need no edge room: none is lost to the full edge queue.
    queue.registry(1 << 31);
    queue.hint(wire::EVENT_PREVIEW_CHANGED_TAG).unwrap();
    for at in 0..16 {
        assert_eq!(edge_time(queue.take().unwrap()), at);
    }
    // The refused edge left the reread bit, which came first.
    assert!(matches!(queue.take(), Some(Ready::Reread)));
    assert!(matches!(queue.take(), Some(Ready::Registry(0x8000_0000))));
    assert!(matches!(
        queue.take(),
        Some(Ready::Hint(wire::EVENT_PREVIEW_CHANGED_TAG))
    ));
    assert!(queue.take().is_none());
}
