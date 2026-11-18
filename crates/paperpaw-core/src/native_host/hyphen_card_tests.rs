#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The card tables of `text.hyphenate`.
extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;

fn card() -> (Rc<MemoryStorage>, CardTables) {
    let storage = Rc::new(MemoryStorage::new());
    let tables = CardTables::new(storage.clone());
    (storage, tables)
}

#[test]
fn a_language_is_found_case_insensitively_and_the_greatest_plugin_wins() {
    let (_, t) = card();
    t.replace(
        [
            ("de", "plugins/a/de.hyph", 3),
            ("DE", "plugins/b/de.hyph", 9),
            ("en-gb", "plugins/a/en.hyph", 1),
        ]
        .into_iter(),
    )
    .unwrap();
    assert_eq!(t.find("de"), Some(1));
    assert_eq!(t.find("De"), Some(1));
    assert_eq!(t.find("EN-GB"), Some(2));
    assert_eq!(t.find("fr"), None);
}

#[test]
fn a_table_opens_from_the_card_with_an_identity_that_follows_the_file() {
    let (storage, t) = card();
    storage.write("plugins/a/de.hyph", b"0123456789").unwrap();
    t.replace([("de", "plugins/a/de.hyph", 1)].into_iter())
        .unwrap();
    let key = t.find("de").unwrap();
    let (mut file, id) = t.open(key).unwrap();
    assert_eq!(file.size(), 10);
    let mut buf = [0_u8; 4];
    assert!(file.read_at(3, &mut buf));
    assert_eq!(&buf, b"3456");
    assert!(
        !file.read_at(8, &mut buf),
        "past the end: a read that fails"
    );
    assert!(!file.read_at(u64::MAX, &mut buf));
    storage.write("plugins/a/de.hyph", b"0123456789ab").unwrap();
    let (_, changed) = t.open(key).unwrap();
    assert_ne!(id, changed);
    // No file: not found; a key from before a replace is not trusted.
    storage.remove("plugins/a/de.hyph").unwrap();
    assert_eq!(t.open(key).err(), Some(ERR_NOT_FOUND));
    assert_eq!(t.open(77).err(), Some(ERR_NOT_FOUND));
}

#[test]
fn replacing_is_all_or_nothing_and_changes_the_epoch() {
    let (_, t) = card();
    let before = t.card_epoch();
    t.replace([("de", "p/de.hyph", 1)].into_iter()).unwrap();
    assert_ne!(t.card_epoch(), before);
    let after = t.card_epoch();
    let long = "x".repeat(40);
    assert_eq!(
        t.replace([("en", "p/en.hyph", 1), (long.as_str(), "p/x", 1)].into_iter()),
        Err(ERR_INVALID)
    );
    assert_eq!(t.card_epoch(), after, "a refused replace changes nothing");
    assert_eq!(t.find("de"), Some(0));
    assert_eq!(t.find("en"), None);
    t.card_changed();
    assert_ne!(t.card_epoch(), after);
}
