#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The scope of format calls: who is inside one, for which book, and that
//! it ends with its guard.
extern crate std;
use super::*;
use std::string::{String, ToString};

fn book<'a>(path: &'a str, kind: BookKind, opened: &'a str, document: u32) -> FormatBook<'a> {
    FormatBook {
        path,
        kind,
        opened,
        document,
        cache: "",
        css: "",
        cache_cap: 0,
        quota: None,
    }
}

fn seen(
    scope: &FormatScope,
    owner: u64,
) -> Option<(std::string::String, BookKind, std::string::String, u32)> {
    let mut seen = None;
    scope.current(owner, &mut |b| {
        seen = Some((b.path.to_string(), b.kind, b.opened.to_string(), b.document));
    });
    seen
}

#[test]
fn flags_name_how_the_format_reads_its_book() {
    assert_eq!(BookKind::of_flags(0), BookKind::File);
    assert_eq!(BookKind::of_flags(0b1_0000), BookKind::Siblings);
    assert_eq!(BookKind::of_flags(0b100), BookKind::Folder);
    // prepared, sniffs and whole_pages do not change it.
    assert_eq!(BookKind::of_flags(0b1011), BookKind::File);
}

#[test]
fn a_caller_is_inside_a_call_only_while_its_guard_lives() {
    let scope = FormatScope::new();
    assert_eq!(seen(&scope, 5), None);
    let guard = scope
        .enter(5, &book("books/a/", BookKind::Siblings, "p1.png", 3))
        .unwrap();
    assert_eq!(
        seen(&scope, 5),
        Some((
            "books/a/".to_string(),
            BookKind::Siblings,
            "p1.png".to_string(),
            3
        ))
    );
    // Another plugin is not inside it.
    assert_eq!(seen(&scope, 6), None);
    drop(guard);
    assert_eq!(seen(&scope, 5), None);
}

#[test]
fn calls_of_different_owners_nest_and_end_in_any_order() {
    let scope = FormatScope::new();
    let outer = scope
        .enter(5, &book("a.epub", BookKind::File, "", 1))
        .unwrap();
    let other = scope
        .enter(6, &book("b.cbz", BookKind::File, "", 2))
        .unwrap();
    assert_eq!(seen(&scope, 5).unwrap().3, 1);
    assert_eq!(seen(&scope, 6).unwrap().3, 2);
    // The outer call ends first; the other one is untouched.
    drop(outer);
    assert_eq!(seen(&scope, 5), None);
    assert_eq!(seen(&scope, 6).unwrap().3, 2);
    drop(other);
    assert_eq!(seen(&scope, 6), None);
}

#[test]
fn an_owner_has_one_call_at_a_time() {
    let scope = FormatScope::new();
    let first = scope
        .enter(5, &book("a.epub", BookKind::File, "", 1))
        .unwrap();
    // A second call of the same plugin is its own re-entry: refused, and the
    // call under way is still the one that is answered.
    assert_eq!(
        scope.enter(5, &book("c.epub", BookKind::File, "", 4)).err(),
        Some(ERR_BUSY)
    );
    assert_eq!(seen(&scope, 5).unwrap().3, 1);
    drop(first);
    assert!(
        scope
            .enter(5, &book("c.epub", BookKind::File, "", 4))
            .is_ok()
    );
}

#[test]
fn nesting_is_bounded() {
    let scope = FormatScope::new();
    let guards: std::vec::Vec<_> = (0..MAX_NESTING)
        .map(|n| {
            scope
                .enter(n as u64 + 1, &book("a", BookKind::File, "", n as u32))
                .unwrap()
        })
        .collect();
    assert_eq!(
        scope.enter(99, &book("a", BookKind::File, "", 99)).err(),
        Some(ERR_FULL)
    );
    drop(guards);
    assert!(scope.enter(99, &book("a", BookKind::File, "", 1)).is_ok());
}

#[test]
fn a_visitor_may_enter_another_call_and_ending_never_needs_a_borrow() {
    let scope = FormatScope::new();
    let _guard = scope.enter(1, &book("a", BookKind::File, "", 1)).unwrap();
    let mut inner = None;
    scope.current(1, &mut |_| {
        inner = scope.enter(2, &book("b", BookKind::File, "", 2)).ok();
    });
    assert_eq!(seen(&scope, 2).unwrap().3, 2);
    // Ended while the entry is being looked at: gone for everyone at once.
    let mut guard = inner.take();
    let mut during = None;
    scope.current(2, &mut |_| {
        drop(guard.take());
        during = Some(seen(&scope, 2));
    });
    assert_eq!(during, Some(None));
    assert_eq!(seen(&scope, 2), None);
}

#[test]
fn a_call_that_unwinds_leaves_no_entry() {
    let scope = FormatScope::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = scope
            .enter(7, &book("a.epub", BookKind::File, "", 1))
            .unwrap();
        assert!(scope.current(7, &mut |_| ()));
        panic!("the format panics");
    }));
    assert!(result.is_err());
    assert_eq!(seen(&scope, 7), None);
    // And the place is free again.
    assert!(
        scope
            .enter(7, &book("a.epub", BookKind::File, "", 2))
            .is_ok()
    );
}

#[test]
fn the_folder_of_a_book_is_the_one_root_3_reaches() {
    let folder = |path, kind| book(path, kind, "", 1).folder().map(ToString::to_string);
    assert_eq!(folder("books/a.epub", BookKind::File), None);
    assert_eq!(
        folder("comics/one/", BookKind::Folder),
        Some("comics/one".to_string())
    );
    assert_eq!(
        folder("comics/one", BookKind::Folder),
        Some("comics/one".to_string())
    );
    assert_eq!(
        folder("comics/one/p1.png", BookKind::Siblings),
        Some("comics/one".to_string())
    );
    // A book at the top of the card has the top as its folder.
    assert_eq!(folder("p1.png", BookKind::Siblings), Some(String::new()));
}

#[test]
fn the_bytes_of_a_cache_are_counted_per_folder_whoever_writes_to_it() {
    let scope = FormatScope::new();
    scope.charge("c/a", 6, 0, 10).unwrap();
    // Another document of the same conversion writes to the same folder.
    assert_eq!(scope.charge("c/a", 6, 0, 10), Err(ERR_NO_SPACE));
    scope.charge("c/b", 6, 0, 10).unwrap();
    assert_eq!((scope.total("c/a"), scope.total("c/b")), (6, 6));
    // A reopen finds a finished cache's bytes in its stamp; counted ones stay.
    scope.seed("c/new", 7);
    scope.seed("c/a", 1);
    assert_eq!((scope.total("c/new"), scope.total("c/a")), (7, 6));
    // A folder made new starts at 0.
    scope.reset("c/a");
    assert_eq!(scope.total("c/a"), 0);
    scope.charge("c/a", 10, 0, 10).unwrap();
    assert_eq!(scope.charge("c/a", 1, 0, 10), Err(ERR_NO_SPACE));
    // What is credited is not counted; the count cannot go below 0.
    scope.charge("c/a", 0, 99, 10).unwrap();
    assert_eq!(scope.total("c/a"), 0);
}

#[test]
fn a_full_count_table_drops_the_folder_used_least_recently() {
    let scope = FormatScope::new();
    for n in 0..MAX_COUNTED {
        scope.charge(&std::format!("c/{n}"), 1, 0, 10).unwrap();
    }
    // The first is used again: the second is the least recent now.
    scope.charge("c/0", 1, 0, 10).unwrap();
    scope.charge("c/new", 1, 0, 10).unwrap();
    assert_eq!(scope.total("c/0"), 2);
    assert_eq!(scope.total("c/1"), 0);
    assert_eq!(scope.total("c/new"), 1);
}

#[test]
fn the_places_of_the_calls_are_made_at_the_first_call_and_kept() {
    let scope = FormatScope::new();
    // Before any call nothing is held, and nothing is inside one.
    assert_eq!(scope.places_len(), 0);
    assert_eq!(seen(&scope, 5), None);
    let guard = scope
        .enter(5, &book("books/a.epub", BookKind::File, "", 1))
        .unwrap();
    assert_eq!(scope.places_len(), MAX_NESTING);
    let at = scope.places_at();
    drop(guard);
    // The next call takes the same places.
    let _again = scope
        .enter(6, &book("books/b.epub", BookKind::File, "", 2))
        .unwrap();
    assert_eq!(scope.places_at(), at);
}
