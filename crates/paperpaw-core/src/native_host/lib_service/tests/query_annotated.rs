#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Service-level tests of ops 15 and 16 (annotations, anchors) and op 22
//! (fields) over the fake host of `query.rs`.
use super::*;

// Ops 15, 16: annotations and anchors.

#[test]
fn annotations_page_under_the_books_annotation_generation() {
    let fake = Fake::new();
    fake.generation.set(11);
    *fake.rows.borrow_mut() = (0..10).map(|n| annotation_row(n, n)).collect();
    let (pages, ids) = walk_book(&fake, 15);
    assert_eq!(
        pages,
        [3, 3, 3, 1],
        "three whole rows of 1,100 bytes a page"
    );
    assert_eq!(ids, (0..10).collect::<Vec<_>>());
    let page = |cursor, generation| ask(&fake, 15, &[book_ref(), page_ask(cursor, generation)]);
    assert_eq!(page(3, 0), Err(ERR_STATE));
    assert_eq!(page(3, 10), Err(ERR_STATE));
    assert!(page(3, 11).is_ok());
    fake.lax.set(true);
    assert_eq!(page(0, 10), Err(ERR_STATE), "the service compares too");
    assert!(
        fake.log
            .borrow()
            .iter()
            .any(|line| line == "annotations Path(\"books/a.epub\") 3 11")
    );
}

#[test]
fn annotation_rows_are_in_reading_order_with_unique_ids() {
    let fake = Fake::new();
    let say = |rows: &[(u32, u32)]| {
        *fake.rows.borrow_mut() = rows
            .iter()
            .map(|(id, at)| annotation_row(*id, *at))
            .collect();
        ask(&fake, 15, &[book_ref(), page_ask(0, 0)])
    };
    assert!(
        say(&[(1, 5), (2, 5), (3, 9)]).is_ok(),
        "equal places keep their order"
    );
    assert_eq!(say(&[(1, 9), (2, 5)]), Err(ERR_INVALID), "out of order");
    assert_eq!(say(&[(1, 5), (1, 6)]), Err(ERR_INVALID), "a repeated id");
    *fake.rows.borrow_mut() = std::vec![anchor_row(1, 3)];
    assert_eq!(
        ask(&fake, 15, &[book_ref(), page_ask(0, 0)]),
        Err(ERR_INVALID)
    );
}

#[test]
fn annotation_fields_are_checked() {
    let fake = Fake::new();
    let say = |edit: fn(&mut wire::ServiceLibAnnotation<'static>)| {
        let mut row = annotation_fields(1, 5);
        edit(&mut row);
        *fake.rows.borrow_mut() = std::vec![stream(&[Record::ServiceLibAnnotation(row)])];
        ask(&fake, 15, &[book_ref(), page_ask(0, 0)])
    };
    assert!(say(|_| ()).is_ok());
    assert!(say(|a| a.flags = 0b1_1111).is_ok());
    assert_eq!(say(|a| a.flags = 0b10_0000), Err(ERR_INVALID));
    assert_eq!(
        say(|a| a.end_offset = 0),
        Err(ERR_INVALID),
        "a highlight ends before it starts"
    );
    // Only a highlight runs forward: a bookmark or another kind may end first,
    // as the write side takes them.
    assert!(say(|a| (a.kind, a.end_offset) = (0, 0)).is_ok(), "bookmark");
    assert!(say(|a| (a.kind, a.end_offset) = (2, 0)).is_ok(), "other(0)");
    assert_eq!(say(|a| a.percent = 101), Err(ERR_INVALID));
    assert_eq!(
        say(|a| a.conversion = 7),
        Err(ERR_INVALID),
        "no conversion flagged"
    );
    assert!(say(|a| (a.flags, a.conversion) = (0b1001, 7)).is_ok());
    assert_eq!(
        say(|a| a.flags = 0),
        Err(ERR_INVALID),
        "a note but not noted"
    );
    assert!(say(|a| (a.flags, a.note) = (0, "")).is_ok());
}

#[test]
fn anchors_page_and_hold_only_annotations_that_have_them() {
    let fake = Fake::new();
    fake.generation.set(3);
    // Two XPointers of 1,024 bytes: one row a page.
    *fake.rows.borrow_mut() = (0..4).map(|id| anchor_row(id, 3)).collect();
    let (pages, ids) = walk_book(&fake, 16);
    assert_eq!(pages, [1, 1, 1, 1]);
    assert_eq!(ids, [0, 1, 2, 3]);
    // Short ones: all in one page of at most 32.
    let short = |id| stream(&[Record::ServiceLibAnchor(anchor_fields(id, 3, "/a", "/b"))]);
    *fake.rows.borrow_mut() = (0..40).map(short).collect();
    let (pages, _) = walk_book(&fake, 16);
    assert_eq!(pages, [32, 8]);
}

#[test]
fn anchor_rows_are_checked() {
    let fake = Fake::new();
    let say = |edit: fn(&mut wire::ServiceLibAnchor<'static>)| {
        let mut row = anchor_fields(1, 3, "/a", "/b");
        edit(&mut row);
        *fake.rows.borrow_mut() = std::vec![stream(&[Record::ServiceLibAnchor(row)])];
        ask(&fake, 16, &[book_ref(), page_ask(0, 0)])
    };
    assert!(say(|_| ()).is_ok());
    assert_eq!(say(|a| a.flags = 0), Err(ERR_INVALID), "nothing to anchor");
    assert_eq!(say(|a| a.flags = 4), Err(ERR_INVALID));
    assert!(say(|a| (a.flags, a.start, a.end, a.conversion) = (1, "", "", 5)).is_ok());
    assert_eq!(
        say(|a| a.start = ""),
        Err(ERR_INVALID),
        "flagged, none given"
    );
    assert_eq!(say(|a| (a.flags, a.conversion) = (2, 5)), Err(ERR_INVALID));
    assert_eq!(
        say(|a| (a.flags, a.start) = (1, "/a")),
        Err(ERR_INVALID),
        "given, not flagged"
    );
    *fake.rows.borrow_mut() = std::vec![anchor_row(1, 3), anchor_row(1, 3)];
    assert_eq!(
        ask(&fake, 16, &[book_ref(), page_ask(0, 0)]),
        Err(ERR_INVALID)
    );
}

// Op 22.

#[test]
fn fields_page_under_the_books_field_generation() {
    let fake = Fake::new();
    fake.generation.set(7);
    *fake.rows.borrow_mut() = (0..10).map(|n| field_row(n, 1024)).collect();
    let (pages, ids) = walk_with(&fake, 22, |cursor, generation| {
        std::vec![book_ref(), field_page_ask(cursor, generation)]
    });
    assert_eq!(pages, [3, 3, 3, 1], "three whole rows at their caps a page");
    assert_eq!(ids, (0..10).collect::<Vec<_>>());
    let page =
        |cursor, generation| ask(&fake, 22, &[book_ref(), field_page_ask(cursor, generation)]);
    assert_eq!(page(3, 0), Err(ERR_STATE), "a bare cursor");
    assert_eq!(page(3, 6), Err(ERR_STATE), "not the host's generation");
    assert!(page(3, 7).is_ok());
    fake.lax.set(true);
    assert_eq!(page(0, 6), Err(ERR_STATE), "the service compares too");
    assert!(
        fake.log
            .borrow()
            .iter()
            .any(|line| line == "fields Path(\"books/a.epub\") 3 7")
    );
    // By key, and the same pages.
    let by_key = ask(&fake, 22, &[key_ref(1, 1), field_page_ask(0, 0)]).unwrap();
    assert_eq!(rows_in(&by_key), 3);
}

#[test]
fn the_service_passes_the_hosts_field_order_on_across_pages() {
    // The host keeps the order contract (by owning plugin id, then field
    // name) the same on every page: the service neither sorts nor can check
    // an order across pages, so what a reader walks is the host's order.
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..10).rev().map(|n| field_row(n, 1024)).collect();
    let (pages, ids) = walk_with(&fake, 22, |cursor, generation| {
        std::vec![book_ref(), field_page_ask(cursor, generation)]
    });
    assert_eq!(pages, [3, 3, 3, 1]);
    assert_eq!(ids, (0..10).rev().collect::<Vec<_>>());
}

#[test]
fn a_field_page_holds_as_many_whole_rows_as_fit() {
    let fake = Fake::new();
    // 32 small fields are the most a page names; the rest is the next page.
    *fake.rows.borrow_mut() = (0..40).map(|n| field_row(n, 1)).collect();
    let first = ask(&fake, 22, &[book_ref(), field_page_ask(0, 0)]).unwrap();
    let end = end_of(&first);
    assert_eq!(
        (rows_in(&first), end.total, end.next, end.flags),
        (32, 40, 32, 0)
    );
    let rest = ask(&fake, 22, &[book_ref(), field_page_ask(32, end.generation)]).unwrap();
    let end = end_of(&rest);
    assert_eq!((rows_in(&rest), end.next, end.flags & 1), (8, 40, 1));
    // No field: one empty last page. A cursor past the end: the same.
    *fake.rows.borrow_mut() = Vec::new();
    let none = ask(&fake, 22, &[book_ref(), field_page_ask(0, 0)]).unwrap();
    assert_eq!((rows_in(&none), end_of(&none).flags & 1), (0, 1));
    *fake.rows.borrow_mut() = (0..3).map(|n| field_row(n, 1)).collect();
    let past = ask(&fake, 22, &[book_ref(), field_page_ask(9, 5)]).unwrap();
    assert_eq!((rows_in(&past), end_of(&past).flags & 1), (0, 1));
}

#[test]
fn the_hosts_field_position_is_checked() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..5).map(|n| field_row(n, 1024)).collect();
    let page = || ask(&fake, 22, &[book_ref(), field_page_ask(0, 0)]);
    fake.generation.set(0);
    assert_eq!(page(), Err(ERR_INVALID), "a generation is never 0");
    fake.generation.set(5);
    fake.next.set(Some(9));
    assert_eq!(page(), Err(ERR_INVALID), "next is not cursor + rows");
    fake.next.set(None);
    assert!(page().is_ok());
    fake.stubborn.set(true);
    assert_eq!(page(), Err(ERR_INVALID), "a host that ignores Break");
    fake.stubborn.set(false);
    // A scratch that holds the `end` but no row: a refusal, not a loop.
    let bytes = input(22, &[book_ref(), field_page_ask(0, 0)]).unwrap();
    let mut out = std::vec![0; 40];
    let result = with(host(&fake), |frame| {
        frame.run(schema_op(22), &bytes, &mut out)
    });
    assert_eq!(result, Err(ERR_NO_SPACE));
}

#[test]
fn field_rows_are_checked() {
    let fake = Fake::new();
    let say = |ids: &[&str]| {
        *fake.rows.borrow_mut() = ids
            .iter()
            .map(|id| {
                stream(&[Record::ServiceLibField(wire::ServiceLibField {
                    id,
                    title: "Rating",
                    value: "5",
                })])
            })
            .collect();
        ask(&fake, 22, &[book_ref(), field_page_ask(0, 0)])
    };
    assert!(say(&["stats.rating", "files.loan"]).is_ok());
    assert_eq!(say(&["rating"]), Err(ERR_INVALID), "not <plugin>.<name>");
    assert_eq!(say(&[".rating"]), Err(ERR_INVALID));
    assert_eq!(say(&["stats."]), Err(ERR_INVALID));
    assert_eq!(say(&["stats.rating", "stats.rating"]), Err(ERR_INVALID));
    *fake.rows.borrow_mut() = std::vec![entry_row(1)];
    assert_eq!(
        ask(&fake, 22, &[book_ref(), field_page_ask(0, 0)]),
        Err(ERR_INVALID)
    );
}
