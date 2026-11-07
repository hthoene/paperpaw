#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! What the kernel registered of the data plugins' hyphenation tables, and
//! the shell's sink.
extern crate std;
use super::*;
use crate::native_host::{HyphenTables, hyphen_card::CardTables, hyphen_service::HyphenHost};
use alloc::rc::Rc;
use paperpaw_sdk::MemoryStorage;

fn sink() -> HyphenTables {
    HyphenTables::over(Rc::new(CardTables::new(Rc::new(MemoryStorage::new()))))
}

#[test]
fn what_was_registered_before_the_sink_is_replayed_to_it() {
    let mut feed = HyphenFeed::new();
    feed.add("pack", "plugins/pack", [("de", "hyph/de.pphp")].into_iter())
        .unwrap();
    let sink = sink();
    assert_eq!(sink.card().find("de"), None);
    feed.attach(sink.clone());
    assert!(sink.card().find("de").is_some());
}

#[test]
fn plugins_added_and_removed_after_the_sink_reach_it() {
    let mut feed = HyphenFeed::new();
    let sink = sink();
    feed.attach(sink.clone());
    feed.add(
        "a",
        "plugins/a",
        [("de", "de.pphp"), ("en", "en.pphp")].into_iter(),
    )
    .unwrap();
    assert!(sink.card().find("de").is_some());
    assert!(sink.card().find("en").is_some());
    feed.remove("a");
    assert_eq!(sink.card().find("de"), None);
    assert_eq!(sink.card().find("en"), None);
    // Removing what is not there changes nothing and tells nothing.
    feed.remove("a");
}

#[test]
fn the_card_path_is_the_folder_and_the_file() {
    let mut feed = HyphenFeed::new();
    feed.add("a", "plugins/a", [("de", "hyph/de.pphp")].into_iter())
        .unwrap();
    assert_eq!(feed.declared[0].path, "plugins/a/hyph/de.pphp");
    assert_eq!(feed.declared[0].plugin, "a");
}

#[test]
fn a_card_change_reaches_the_sink() {
    let mut feed = HyphenFeed::new();
    let sink = sink();
    feed.attach(sink.clone());
    let before = sink.card().card_epoch();
    feed.card_changed();
    assert_ne!(sink.card().card_epoch(), before);
    // No sink: nothing happens.
    HyphenFeed::new().card_changed();
}

#[test]
fn no_sink_keeps_only_the_list() {
    let mut feed = HyphenFeed::new();
    feed.add("a", "plugins/a", [("de", "de.pphp")].into_iter())
        .unwrap();
    assert_eq!(feed.declared.len(), 1);
}
