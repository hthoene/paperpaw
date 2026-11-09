//! A plugin of the library's JSON is read with its long texts cut, not
//! dropped; nothing of an untrusted value panics.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_plugin_store::native_item::{Item, cut, date};
use paperpaw_plugin_store::native_model::{MAX_NAME, MAX_SUMMARY};

fn read(item: &mut Item, key: &[&str], value: &str) {
    item.read(key, value, "x4").unwrap();
}

#[test]
fn long_values_are_cut_and_the_plugin_is_kept() {
    let mut item = Item::new();
    read(&mut item, &["id"], "airy-theme");
    read(&mut item, &["name"], &"n".repeat(500));
    read(&mut item, &["summary"], &"s".repeat(5_000));
    read(&mut item, &["author"], &"a".repeat(81));
    read(&mut item, &["latest", "version"], &"9".repeat(200));
    assert_eq!(item.entry.id, "airy-theme");
    assert_eq!(item.entry.name.len(), MAX_NAME);
    assert_eq!(item.entry.summary.len(), MAX_SUMMARY);
    assert_eq!(item.entry.author.len(), MAX_NAME);
    assert_eq!(item.entry.version.len(), MAX_NAME);
}

#[test]
fn a_cut_ends_at_a_character() {
    // Two-byte letters: 80 bytes is 40 of them, 81 cuts to 80.
    assert_eq!(cut(&"é".repeat(41), 81).unwrap().len(), 80);
    assert_eq!(cut(&"é".repeat(41), 80).unwrap().len(), 80);
    assert_eq!(cut("日本語", 4).unwrap(), "日");
    assert_eq!(cut("日本語", 2).unwrap(), "");
    assert_eq!(cut("abc", 0).unwrap(), "");
}

#[test]
fn the_selection_keys_are_read() {
    let mut item = Item::new();
    read(&mut item, &["kind"], "code");
    read(&mut item, &["built_in"], "true");
    read(&mut item, &["devices", "[]"], "x4 pro");
    assert!(item.devices && !item.device);
    read(&mut item, &["devices", "[]"], "x4");
    assert!(item.device);
    read(&mut item, &["latest", "formats"], "not a number");
    assert_eq!(item.formats, u32::MAX);
    read(&mut item, &["latest", "size"], "not a number");
    assert_eq!(item.entry.size, 0);
    read(&mut item, &["latest", "date"], "2026-03-14");
    assert_eq!(item.entry.updated, 20_260_314);
    read(&mut item, &["group"], "no such group");
    assert_eq!(
        item.entry.group,
        paperpaw_plugin_store::native_model::GROUPS.len() - 1
    );
}

#[test]
fn dates_are_a_day_or_a_year() {
    assert_eq!(date("2026-03-14"), 20_260_314);
    assert_eq!(date("2026"), 20_260_101);
    assert_eq!(date("26"), 0);
    assert_eq!(date(""), 0);
    assert_eq!(date("2026-03-14T10:00:00Z"), 20_260_314);
}

#[test]
fn paths_of_any_depth_and_text_of_any_length_do_not_panic() {
    let long = "é".repeat(100_000);
    let mut item = Item::new();
    for key in [
        &[][..],
        &["id"],
        &["latest"],
        &["latest", "version", "x"],
        &["a", "b", "c", "d", "e"],
        &["devices", "[]", "x"],
    ] {
        item.read(key, &long, "x4").unwrap();
    }
    item.read(&["name"], &long, "x4").unwrap();
    assert_eq!(item.entry.name.len(), MAX_NAME);
}
