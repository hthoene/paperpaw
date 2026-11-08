//! The native reading of a plugin's keys (`native_item`) gives what the
//! interface-6 reading did, for any path, value and device.
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;
use super::{Device, Item};
use crate::native_item as native;
use alloc::{string::String, vec::Vec};
use proptest::prelude::*;

const KEYS: [&str; 22] = [
    "id",
    "name",
    "summary",
    "author",
    "licence",
    "group",
    "kind",
    "waits",
    "built_in",
    "official",
    "unreviewed",
    "added",
    "devices",
    "[]",
    "latest",
    "version",
    "sdk",
    "formats",
    "size",
    "date",
    "files",
    "x",
];

fn path() -> impl Strategy<Value = Vec<String>> {
    proptest::collection::vec(
        proptest::sample::select(KEYS.to_vec()).prop_map(String::from),
        0..4,
    )
}

fn value() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\PC{0,400}",
        "[0-9-]{0,12}",
        "(true|false|data|code|author|update|x4|x4 pro|system|reading)",
        "é{0,200}",
    ]
}

fn same(old: &Item, new: &native::Item) {
    assert_eq!(old.entry.id, new.entry.id);
    assert_eq!(old.entry.name, new.entry.name);
    assert_eq!(old.entry.summary, new.entry.summary);
    assert_eq!(old.entry.group, new.entry.group);
    assert_eq!(old.entry.author, new.entry.author);
    assert_eq!(old.entry.licence, new.entry.licence);
    assert_eq!(old.entry.version, new.entry.version);
    assert_eq!(old.entry.sdk, new.entry.sdk);
    assert_eq!(old.entry.unreviewed, new.entry.unreviewed);
    assert_eq!(old.entry.added, new.entry.added);
    assert_eq!(old.entry.updated, new.entry.updated);
    assert_eq!(old.entry.size, new.entry.size);
    assert_eq!(old.kind, new.kind);
    assert_eq!(old.waits, new.waits);
    assert_eq!(old.built_in, new.built_in);
    assert_eq!(old.official, new.official);
    assert_eq!(old.devices, new.devices);
    assert_eq!(old.device, new.device);
    assert_eq!(old.formats, new.formats);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(400)))]

    #[test]
    fn keys_are_read_as_before(reads in proptest::collection::vec((path(), value()), 0..30)) {
        let device = Device {
            name: "x4",
            fingerprint: None,
            code_file: "code.ppn",
            plugins: None,
            sdk: paperpaw_sdk::SDK_VERSION,
            official: true,
            provides: None,
        };
        let (mut old, mut new) = (Item::new(), native::Item::new());
        for (key, value) in &reads {
            old.read(key, value, &device);
            new.read(key, value, "x4").unwrap();
            same(&old, &new);
        }
    }

    #[test]
    fn dates_are_read_as_before(text in "\\PC{0,20}|[0-9-]{0,12}") {
        prop_assert_eq!(native::date(&text), super::date(&text));
    }

    #[test]
    fn text_is_cut_as_before(text in "\\PC{0,300}|é{0,200}", max in 0usize..120) {
        prop_assert_eq!(native::cut(&text, max).unwrap(), super::cut(&text, max));
    }
}
