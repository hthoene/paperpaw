//! A list shown while a fresh list arrives is picked again from it (slice 3,
//! host-free): the plugins a view holds are places in the index, and a fresh
//! list takes the index and the hits away. The screen itself needs a host; here
//! are the pieces it decides with: what a stale pick shows, the pick again, what
//! tells a fresh list (`stamp`) and where the focus goes (`kept_focus`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_index::{Index, Order, take};
use paperpaw_plugin_store::native_item::Device;
use paperpaw_plugin_store::native_list::Hooks;
use paperpaw_plugin_store::native_screen_list::Say;
use paperpaw_plugin_store::native_screen_view::{Source, Which, kept_focus, pick, stamp, tops};
use paperpaw_plugin_store::native_screens::{Installs, Step};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{Error, Fingerprint, INTERFACE};
use paperpaw_sdk::{MemoryStorage, Storage};

/// The words as their keys.
struct Words;

impl Say for Words {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        Ok(key.to_string())
    }
    fn format(&mut self, key: &str, _: &[Arg<'_>]) -> Result<String, Error> {
        Ok(key.to_string())
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        Ok(text.to_string())
    }
}

struct Nothing;

impl Installs for Nothing {
    fn step(&self, _: &str) -> Step {
        Step::Idle
    }
    fn queued(&self) -> bool {
        false
    }
}

const READING: Which = Which::Group(0);
/// The rows a page holds in these tests.
const PER_PAGE: usize = 6;

fn nobody(_: &str) -> Option<[u8; 32]> {
    None
}
fn none() -> Vec<String> {
    Vec::new()
}

/// A plugin of `group`, first published and last updated on the given days.
fn entry(id: &str, group: &str, added: &str, date: &str) -> String {
    let mut name = id.to_string();
    name[..1].make_ascii_uppercase();
    format!(
        r#"{{"id": "{id}", "name": "{name}", "summary": "Does {id}.", "group": "{group}",
  "author": "A", "licence": "MIT", "kind": "data", "added": "{added}",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 300, "date": "{date}"}}}}"#
    )
}

fn list(entries: &[String]) -> String {
    format!(r#"{{"format": 2, "plugins": [{}]}}"#, entries.join(", "))
}

fn fruit() -> String {
    list(&[
        entry("apple", "reading", "2025-01-01", "2026-01-01"),
        entry("berry", "reading", "2026-05-01", "2026-02-01"),
        entry("cherry", "reading", "2025-06-01", "2026-09-01"),
    ])
}

/// The same fruit after the library added Avocado, the newest of all.
fn fresh_fruit() -> String {
    list(&[
        entry("apple", "reading", "2025-01-01", "2026-01-01"),
        entry("avocado", "reading", "2026-06-01", "2026-01-01"),
        entry("berry", "reading", "2026-05-01", "2026-02-01"),
        entry("cherry", "reading", "2025-06-01", "2026-09-01"),
    ])
}

/// `count` plugins of reading, `p00` on.
fn many(count: usize) -> String {
    let entries: Vec<String> = (0..count)
        .map(|n| entry(&format!("p{n:02}"), "reading", "2026-01-01", "2026-01-01"))
        .collect();
    list(&entries)
}

/// The list as the next visit's fetch leaves it: taken into the index kept.
fn take_list(card: &MemoryStorage, text: &str) -> Index {
    card.write("list.json", text.as_bytes()).unwrap();
    let device = Device {
        name: "x4",
        fingerprint: Some(Fingerprint::of(INTERFACE, 0x4567_89ab_cdef)),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    };
    let hooks = Hooks {
        author_of: &nobody,
        stopped: &none,
        host_checks: false,
    };
    take(card, "list.json", device, hooks)
        .unwrap()
        .expect("a list")
        .0
}

/// The titles of the rows `first..first + max` of a list of `hits` plugins.
fn rows(
    index: &Index,
    card: &MemoryStorage,
    which: &Which,
    hits: usize,
    page: usize,
) -> Vec<String> {
    let top = tops(which, hits, PER_PAGE, false, false).unwrap();
    let source = Source {
        index,
        storage: card,
        installs: &Nothing,
        top: &top,
        hits,
        order: Order::Name,
    };
    let shown = source
        .rows(&mut Words, page * PER_PAGE, PER_PAGE, "")
        .unwrap();
    shown.into_iter().map(|row| row.title).collect()
}

/// The names the pick shows, the rows above them left out.
fn plugins(rows: &[String]) -> Vec<&str> {
    rows.iter()
        .map(String::as_str)
        .filter(|title| !title.starts_with("store."))
        .collect()
}

/// Rows of plugins with nothing in them (the rows above are not counted).
fn assert_left_empty(rows: &[String]) {
    let left = plugins(rows);
    assert!(
        !left.is_empty() && left.iter().all(|t| t.is_empty()),
        "{rows:?}"
    );
}

#[test]
fn a_group_picked_from_the_list_before_is_empty_until_it_is_picked_again() {
    let card = MemoryStorage::new();
    let index = take_list(&card, &fruit());
    let picked = pick(&READING, Order::Name, &index, &card, &Nothing).unwrap();
    let shown = rows(&index, &card, &READING, picked.hits, 0);
    assert_eq!(plugins(&shown), ["Apple", "Berry", "Cherry"]);
    // The fresh list takes the index and the hits away: the rows the view
    // still holds are empty, and only a new pick fills them.
    let fresh = take_list(&card, &fresh_fruit());
    let stale = rows(&fresh, &card, &READING, picked.hits, 0);
    assert_left_empty(&stale);
    let again = pick(&READING, Order::Name, &fresh, &card, &Nothing).unwrap();
    assert_eq!(again.hits, 4);
    let shown = rows(&fresh, &card, &READING, again.hits, 0);
    assert_eq!(plugins(&shown), ["Apple", "Avocado", "Berry", "Cherry"]);
}

#[test]
fn what_tells_a_fresh_list_is_its_size_its_runs_and_its_sections() {
    let card = MemoryStorage::new();
    let index = take_list(&card, &fruit());
    // The same list taken again is no fresh list.
    let same = take_list(&card, &fruit());
    assert_eq!(stamp(&index), stamp(&same));
    // Another plugin, and another group's run.
    let more = take_list(&card, &fresh_fruit());
    assert_ne!(stamp(&index), stamp(&more));
    let other = take_list(
        &card,
        &list(&[
            entry("apple", "reading", "2025-01-01", "2026-01-01"),
            entry("berry", "reading", "2026-05-01", "2026-02-01"),
            entry("cherry", "look", "2025-06-01", "2026-09-01"),
        ]),
    );
    assert_eq!(other.len(), index.len());
    assert_ne!(stamp(&index), stamp(&other));
    // A library that now picks plugins for a start.
    let text = fruit().replace("]}", r#"], "sections": {"recommended": ["apple"]}}"#);
    let picks = take_list(&card, &text);
    assert_eq!(picks.len(), index.len());
    assert_ne!(stamp(&index), stamp(&picks));
}

#[test]
fn a_page_of_a_long_group_has_its_plugins_after_the_pick_again() {
    let card = MemoryStorage::new();
    let index = take_list(&card, &many(40));
    let picked = pick(&READING, Order::Name, &index, &card, &Nothing).unwrap();
    // Page 3 of the 42 rows (the order, a letter and 40 plugins).
    assert!(rows(&index, &card, &READING, picked.hits, 2).len() == PER_PAGE);
    let fresh = take_list(&card, &many(41));
    let stale = rows(&fresh, &card, &READING, picked.hits, 2);
    assert_left_empty(&stale);
    let again = pick(&READING, Order::Name, &fresh, &card, &Nothing).unwrap();
    assert_eq!(again.hits, 41);
    // Every page, the one gone to and those paged on to, has its plugins.
    let pages = (2 + 41usize).div_ceil(PER_PAGE);
    for page in 0..pages {
        let shown = rows(&fresh, &card, &READING, again.hits, page);
        assert!(
            shown.iter().all(|title| !title.is_empty()),
            "{page}: {shown:?}"
        );
    }
    let third = rows(&fresh, &card, &READING, again.hits, 2);
    assert_eq!(plugins(&third), ["P10", "P11", "P12", "P13", "P14", "P15"]);
}

#[test]
fn the_rows_above_follow_the_fresh_list_and_the_focus_stays_on_its_plugin() {
    // The letters row comes with the seventh plugin and goes with it.
    let top = |hits| tops(&READING, hits, PER_PAGE, false, false).unwrap().len();
    assert_eq!((top(6), top(7)), (1, 2));
    // A row more above the plugins: the focus moves down with them.
    assert_eq!(kept_focus(14, 1, 2), 15);
    // A row less: up with them; never below the first row.
    assert_eq!(kept_focus(14, 2, 1), 13);
    assert_eq!(kept_focus(0, 2, 0), 0);
    assert_eq!(kept_focus(5, 1, 1), 5);
}

#[test]
fn the_newest_are_picked_again_from_the_fresh_list() {
    let card = MemoryStorage::new();
    let index = take_list(&card, &fruit());
    let new = Which::New;
    let picked = pick(&new, Order::Added, &index, &card, &Nothing).unwrap();
    let shown = rows(&index, &card, &new, picked.hits, 0);
    assert_eq!(plugins(&shown), ["Berry", "Cherry", "Apple"]);
    let fresh = take_list(&card, &fresh_fruit());
    let stale = rows(&fresh, &card, &new, picked.hits, 0);
    assert_left_empty(&stale);
    let again = pick(&new, Order::Added, &fresh, &card, &Nothing).unwrap();
    let shown = rows(&fresh, &card, &new, again.hits, 0);
    assert_eq!(plugins(&shown), ["Avocado", "Berry", "Cherry", "Apple"]);
}

#[test]
fn a_search_is_picked_again_from_the_fresh_list() {
    let card = MemoryStorage::new();
    let index = take_list(&card, &fruit());
    let search = Which::Search("e".into());
    let picked = pick(&search, Order::Fair, &index, &card, &Nothing).unwrap();
    // Names and summaries with an e: Apple, Berry, Cherry.
    assert_eq!(picked.hits, 3);
    let fresh = take_list(&card, &fresh_fruit());
    let stale = rows(&fresh, &card, &search, picked.hits, 0);
    assert_left_empty(&stale);
    let again = pick(&search, Order::Fair, &fresh, &card, &Nothing).unwrap();
    let shown = rows(&fresh, &card, &search, again.hits, 0);
    assert_eq!(plugins(&shown), ["Apple", "Avocado", "Berry", "Cherry"]);
}
