//! `catalogs.txt` written and read back for any catalog, the password
//! never in it; and what version 1 wrote is still what the writer writes.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use proptest::prelude::*;

use super::{Catalog, encode, parse};

fn field() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{0,30}",
        "[\t\n\r a-z]{0,10}",
        Just(String::from("sync")),
        Just(String::from("webdav")),
    ]
}

fn catalog() -> impl Strategy<Value = Catalog> {
    (
        field(),
        field(),
        field(),
        prop_oneof![Just(String::new()), "§[a-z0-9]{6,12}"],
        field(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(name, url, user, password, folder, sync, webdav)| Catalog {
                name,
                url,
                user,
                password,
                folder,
                sync,
                webdav,
            },
        )
}

/// What a field becomes in the file.
fn cleaned(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn catalogs_round_trip_without_their_passwords(list in prop::collection::vec(catalog(), 0..6)) {
        let text = encode(&list);
        let expected: Vec<Catalog> = list
            .iter()
            .filter(|c| !c.url.is_empty())
            .map(|c| Catalog {
                name: cleaned(&c.name),
                url: cleaned(&c.url),
                user: cleaned(&c.user),
                password: String::new(),
                folder: cleaned(&c.folder),
                sync: c.sync,
                webdav: c.webdav,
            })
            .collect();
        prop_assert_eq!(parse(&text), expected);
        // One line a catalog, seven fields each.
        prop_assert_eq!(text.lines().count(), list.len());
        prop_assert!(text.lines().all(|l| l.split('\t').count() == 7));
        for c in list.iter().filter(|c| !c.password.is_empty()) {
            prop_assert!(!text.contains(&c.password), "{}", text);
        }
    }

    #[test]
    fn any_text_reads_without_panic(text in ".{0,300}") {
        for c in parse(&text) {
            prop_assert!(!c.url.is_empty());
        }
    }
}

#[test]
fn the_writer_still_writes_version_1() {
    let v1 = include_str!("../../testdata/catalogs.txt.v1");
    assert_eq!(encode(&parse(v1)), v1);
}
