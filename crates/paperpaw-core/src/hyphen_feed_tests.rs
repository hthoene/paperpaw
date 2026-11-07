#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The feed hands what the SDK holds of the data plugins' tables to the
//! shell's sink.
extern crate std;
use super::*;
use crate::native_host::{HyphenTables, hyphen_card::CardTables, hyphen_service::HyphenHost};
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_sdk::MemoryStorage;

fn sink() -> HyphenTables {
    HyphenTables::over(Rc::new(CardTables::new(Rc::new(MemoryStorage::new()))))
}

fn declared(tables: &mut Hyphenation, plugin: &str, files: &[(&str, &str)]) {
    tables
        .add_tables(plugin, "plugins/pack", files.iter().copied())
        .unwrap();
}

#[test]
fn what_was_declared_before_the_sink_reaches_it_when_pushed() {
    let mut tables = Hyphenation::new();
    declared(&mut tables, "pack", &[("de", "hyph/de.pphp")]);
    let mut feed = HyphenFeed::new();
    let sink = sink();
    feed.attach(sink.clone());
    assert_eq!(sink.card().find("de"), None);
    feed.push(&tables).unwrap();
    assert!(sink.card().find("de").is_some());
}

#[test]
fn plugins_added_and_removed_reach_the_sink_at_each_push() {
    let mut tables = Hyphenation::new();
    let mut feed = HyphenFeed::new();
    let sink = sink();
    feed.attach(sink.clone());
    declared(&mut tables, "a", &[("de", "de.pphp"), ("en", "en.pphp")]);
    feed.push(&tables).unwrap();
    assert!(sink.card().find("de").is_some());
    assert!(sink.card().find("en").is_some());
    tables.remove_tables("a");
    feed.push(&tables).unwrap();
    assert_eq!(sink.card().find("de"), None);
    assert_eq!(sink.card().find("en"), None);
}

#[test]
fn a_sink_that_refuses_keeps_what_it_had_and_says_so() {
    let mut tables = Hyphenation::new();
    let mut feed = HyphenFeed::new();
    let sink = sink();
    feed.attach(sink.clone());
    declared(&mut tables, "a", &[("de", "de.pphp")]);
    feed.push(&tables).unwrap();
    // A path longer than the sink takes: the whole list is refused.
    let long = "x".repeat(600);
    declared(&mut tables, "b", &[("en", long.as_str())]);
    assert_eq!(feed.push(&tables), Err(ERR_INVALID));
    assert!(sink.card().find("de").is_some());
    assert_eq!(sink.card().find("en"), None);
}

#[test]
fn a_card_change_reaches_the_sink() {
    let mut feed = HyphenFeed::new();
    let sink = sink();
    feed.attach(sink.clone());
    let before = sink.card().card_epoch();
    feed.card_changed();
    assert_ne!(sink.card().card_epoch(), before);
    // No sink: nothing happens, and a push is no failure.
    let none = HyphenFeed::new();
    none.card_changed();
    assert_eq!(none.push(&Hyphenation::new()), Ok(()));
}
