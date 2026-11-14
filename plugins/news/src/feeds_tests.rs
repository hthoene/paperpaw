//! `feeds.txt`: what version 1 wrote is written alike, and any list of
//! feeds round trips, one line a feed, whatever the names hold.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use proptest::prelude::*;

use crate::native_news::{Feed, feeds, feeds_text};

#[test]
fn feeds_of_version_1_are_written_alike() {
    let v1 = include_str!("../testdata/feeds.txt.v1");
    let list = feeds(v1).unwrap();
    assert_eq!(feeds_text(&list).unwrap(), v1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn feeds_round_trip(list in prop::collection::vec(("https://[a-z./?=&]{1,20}", prop_oneof![".{0,30}", "[\t\n\r a-z]{0,8}"]), 0..8)) {
        let feeds_list: Vec<Feed> = list
            .iter()
            .map(|(url, name)| Feed { url: url.clone(), name: name.clone() })
            .collect();
        let text = feeds_text(&feeds_list).unwrap();
        let expected: Vec<Feed> = feeds_list
            .iter()
            .map(|f| Feed {
                url: f.url.clone(),
                // Tabs and line breaks become spaces; a carriage return
                // within a name stays.
                name: String::from(f.name.replace(['\t', '\n'], " ").trim()),
            })
            .collect();
        prop_assert_eq!(crate::native_news::feeds(&text).unwrap(), expected);
    }
}
