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
fn the_innermost_call_of_an_owner_wins_and_ends_first() {
    let scope = FormatScope::new();
    let outer = scope
        .enter(5, &book("a.epub", BookKind::File, "", 1))
        .unwrap();
    let other = scope
        .enter(6, &book("b.cbz", BookKind::File, "", 2))
        .unwrap();
    let inner = scope
        .enter(5, &book("c.epub", BookKind::File, "", 4))
        .unwrap();
    assert_eq!(seen(&scope, 5).unwrap().3, 4);
    assert_eq!(seen(&scope, 6).unwrap().3, 2);
    drop(inner);
    assert_eq!(seen(&scope, 5).unwrap().3, 1);
    drop(other);
    drop(outer);
    assert_eq!(seen(&scope, 5), None);
}

#[test]
fn nesting_is_bounded() {
    let scope = FormatScope::new();
    let guards: std::vec::Vec<_> = (0..MAX_NESTING)
        .map(|n| {
            scope
                .enter(1, &book("a", BookKind::File, "", n as u32))
                .unwrap()
        })
        .collect();
    assert_eq!(
        scope.enter(1, &book("a", BookKind::File, "", 99)).err(),
        Some(ERR_FULL)
    );
    drop(guards);
    assert!(scope.enter(1, &book("a", BookKind::File, "", 1)).is_ok());
}

#[test]
fn a_scope_in_use_is_busy_and_a_visitor_may_not_enter() {
    let scope = FormatScope::new();
    let _guard = scope.enter(1, &book("a", BookKind::File, "", 1)).unwrap();
    let mut busy = None;
    scope.current(1, &mut |_| {
        busy = scope.enter(1, &book("b", BookKind::File, "", 2)).err();
    });
    assert_eq!(busy, Some(ERR_BUSY));
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
