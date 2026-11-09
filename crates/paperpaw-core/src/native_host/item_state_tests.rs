//! The item-state table: limits on the net result, the default state, the
//! stop and the update.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;

fn push(item: &str, flags: u32, label: &str) -> ItemPush {
    ItemPush::copied(item, flags, (label, b""), ("", b"")).unwrap()
}

fn put(states: &mut ItemStates, plugin: &str, p: ItemPush) {
    states.fits(plugin, &[&p]).unwrap();
    states.reserve(plugin, 1).unwrap();
    states.apply(plugin, p).unwrap();
}

#[test]
fn a_state_is_kept_replaced_and_removed_by_the_default_state() {
    let mut states = ItemStates::default();
    put(&mut states, "demo", push("open", FAINT, ""));
    assert!(states.get("demo", "open").unwrap().faint());
    assert!(states.get("other", "open").is_none());
    put(&mut states, "demo", push("open", SWITCH | ON, "demo.label"));
    assert_eq!(states.len(), 1, "a record replaces the whole state");
    assert_eq!(states.get("demo", "open").unwrap().switch(), Some(true));
    // Flags 0, or only `keep`, and no texts: the default, which removes it.
    put(&mut states, "demo", push("open", KEEP, ""));
    assert!(states.get("demo", "open").is_none());
    assert_eq!(states.len(), 0);
}

#[test]
fn a_plugin_holds_16_items_and_can_still_change_or_clear_them() {
    let mut states = ItemStates::default();
    for n in 0..PER_PLUGIN_ITEMS {
        put(
            &mut states,
            "demo",
            push(&alloc::format!("item{n}"), HIDDEN, ""),
        );
    }
    let seventeenth = push("one-more", HIDDEN, "");
    assert_eq!(states.fits("demo", &[&seventeenth]), Err(ERR_FULL));
    // At the limit, a change of an item and the clearing of one are fine;
    // so is a batch that clears one and adds another (the net result).
    let change = push("item0", FAINT, "");
    states.fits("demo", &[&change]).unwrap();
    let clear = push("item1", 0, "");
    states.fits("demo", &[&clear]).unwrap();
    states.fits("demo", &[&clear, &seventeenth]).unwrap();
    // Two records of one item count once, the last.
    let again = push("item0", HIDDEN, "");
    states.fits("demo", &[&change, &again]).unwrap();
    // Another plugin has its own 16.
    states.fits("other", &[&push("item0", HIDDEN, "")]).unwrap();
}

#[test]
fn the_bytes_of_a_plugin_and_of_the_device_are_bounded() {
    let mut states = ItemStates::default();
    // 2,048 bytes of ids, keys and arguments per plugin.
    let big = push("a", HIDDEN, &"k".repeat(PER_PLUGIN_BYTES));
    assert_eq!(states.fits("demo", &[&big]), Err(ERR_FULL));
    let fits = push("a", HIDDEN, &"k".repeat(PER_PLUGIN_BYTES - 1));
    states.fits("demo", &[&fits]).unwrap();
    states.reserve("demo", 1).unwrap();
    states.apply("demo", fits).unwrap();
    // The plugin's share is full, but it may replace that very item.
    let smaller = push("a", HIDDEN, "k");
    states.fits("demo", &[&smaller]).unwrap();
    assert_eq!(
        states.fits("demo", &[&push("b", HIDDEN, "kk")]),
        Err(ERR_FULL)
    );
    // The device: 6,144 bytes with what each item and each plugin's slot
    // cost besides their bytes. Three plugins at 2,000 bytes fill it; a
    // fourth does not fit.
    let mut states = ItemStates::default();
    for plugin in ["p1", "p2", "p3"] {
        put(&mut states, plugin, push("x", HIDDEN, &"k".repeat(1970)));
    }
    assert_eq!(
        states.fits("p4", &[&push("x", HIDDEN, &"k".repeat(1970))]),
        Err(ERR_FULL)
    );
    states.fits("p4", &[&push("x", HIDDEN, "k")]).unwrap();
}

#[test]
fn the_device_holds_128_items() {
    let mut states = ItemStates::default();
    for plugin in 0..8 {
        for n in 0..PER_PLUGIN_ITEMS {
            put(
                &mut states,
                &alloc::format!("p{plugin}"),
                push(&alloc::format!("i{n}"), HIDDEN, ""),
            );
        }
    }
    assert_eq!(states.len(), DEVICE_ITEMS);
    assert_eq!(states.fits("p9", &[&push("i0", HIDDEN, "")]), Err(ERR_FULL));
}

#[test]
fn an_unload_ends_every_state_of_the_plugin_and_only_its() {
    let mut states = ItemStates::default();
    put(&mut states, "demo", push("plain", HIDDEN, ""));
    put(&mut states, "demo", push("kept", HIDDEN | KEEP, ""));
    put(&mut states, "other", push("plain", HIDDEN, ""));
    states.forget("demo");
    assert!(states.get("demo", "plain").is_none());
    assert!(states.get("demo", "kept").is_none());
    assert!(states.get("other", "plain").is_some());
    assert_eq!(states.len(), 1);
}

#[test]
fn an_entry_is_24_bytes_on_the_device_and_the_budget_charges_the_heap() {
    // A pointer and a length, five 16-bit lengths, the flags and the slot:
    // 24 bytes with 32-bit pointers, as the budget charges.
    assert!(
        core::mem::size_of::<ItemPush>() <= 2 * core::mem::size_of::<usize>() + 16,
        "an entry grew: {} bytes",
        core::mem::size_of::<ItemPush>()
    );
    assert!(core::mem::size_of::<ItemPush>() <= 32);
    assert_eq!(ENTRY_BYTES, 24);
    // The charge: each item's entry and its allocation's header, its bytes,
    // and its plugin's slot (id included) once. Three plugins with one item
    // each fill the device exactly at this size, and not a byte more.
    let n = DEVICE_BYTES / 3 - (ITEM_BYTES + 1 + SLOT_BYTES + 2);
    let mut states = ItemStates::default();
    for plugin in ["p1", "p2"] {
        put(&mut states, plugin, push("x", HIDDEN, &"k".repeat(n)));
    }
    states
        .fits("p3", &[&push("x", HIDDEN, &"k".repeat(n))])
        .unwrap();
    assert_eq!(
        states.fits("p3", &[&push("x", HIDDEN, &"k".repeat(n + 1))]),
        Err(ERR_FULL)
    );
}

#[test]
fn reserve_makes_room_so_that_apply_allocates_nothing() {
    let mut states = ItemStates::default();
    // Without the room (no slot for the plugin) nothing is stored.
    assert_eq!(
        states.apply("demo", push("a", HIDDEN, "")),
        Err(ERR_NO_MEMORY)
    );
    assert_eq!(states.len(), 0);
    states.reserve("demo", 3).unwrap();
    let (cap, slots) = (states.entries.capacity(), states.plugins.capacity());
    assert!(cap >= 3);
    for item in ["a", "b", "c"] {
        states.apply("demo", push(item, HIDDEN, "")).unwrap();
    }
    assert_eq!(
        (states.entries.capacity(), states.plugins.capacity()),
        (cap, slots),
        "no growth while applying"
    );
    // The slot is kept once for many items; a second plugin has its own.
    states.reserve("other", 1).unwrap();
    assert_eq!(states.plugins.len(), 2);
    states.reserve("demo", 1).unwrap();
    assert_eq!(states.plugins.len(), 2);
    // The last unload gives everything back.
    states.forget("demo");
    states.forget("other");
    assert_eq!(
        (states.entries.capacity(), states.plugins.capacity()),
        (0, 0)
    );
}
