//! Tests of the read ops (ops 1-7, 13, 15, 16, 22, 25): every input rule, the
//! paging rules, what passes through from the host and the checks of its
//! answers, over a scripted fake.
use super::*;
use crate::abi7::context::RowsEnd;
use crate::native_host::lib_service::{
    query::{LibQuery, One, Rows},
    write::LibWrite,
};
use alloc::{boxed::Box, string::String};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use wire::{
    ServiceLibCover as Cover, ServiceLibEntry as Entry, ServiceLibPage as PageAsk,
    ServiceLibQuery as Query, ServiceLibRef as Ref, ServiceLibTextAt as TextAt,
};

fn path_ref(path: &str) -> Record<'_> {
    Record::ServiceLibRef(Ref {
        path,
        generation: 0,
        key: None,
    })
}
fn key_ref(key: u32, generation: u32) -> Record<'static> {
    Record::ServiceLibRef(Ref {
        path: "",
        generation,
        key: Some(key),
    })
}
fn book_ref() -> Record<'static> {
    path_ref("books/a.epub")
}
fn query(edit: impl FnOnce(&mut Query<'static>)) -> Record<'static> {
    let mut query = Query {
        show: 0,
        sort: 0,
        flags: 0,
        words: "",
        author: "",
        series: "",
        tag: "",
        folder: "",
        added_since: 0,
        cursor: 0,
        generation: 0,
        facet: 0,
    };
    edit(&mut query);
    Record::ServiceLibQuery(query)
}
fn text_at(kind: u32, index: u32, offset: u32) -> Record<'static> {
    Record::ServiceLibTextAt(TextAt {
        kind,
        index,
        offset,
    })
}
fn page_ask(cursor: u32, generation: u32) -> Record<'static> {
    Record::ServiceLibPage(PageAsk {
        cursor,
        annotation_generation: generation,
    })
}
fn field_page_ask(cursor: u32, generation: u32) -> Record<'static> {
    Record::ServiceLibFieldPage(wire::ServiceLibFieldPage { cursor, generation })
}
fn host(fake: &Rc<Fake>) -> Rc<dyn LibHost> {
    fake.clone()
}
fn ask(fake: &Rc<Fake>, op: u32, records: &[Record<'_>]) -> Result<Vec<u8>, i32> {
    call(&host(fake), op, records)
}
/// An input batch past the op's own record count, as a hostile caller sends it.
fn loose(records: &[Record<'_>]) -> Vec<u8> {
    let mut buffer = std::vec![0; 8192];
    let used = wire::records_encode_counted(records, 8, 8192, &mut buffer).unwrap();
    buffer.truncate(used);
    buffer
}
fn only(reply: &[u8]) -> Record<'_> {
    wire::record_decode_one(reply).unwrap()
}

// Op 1.

#[test]
fn state_answers_the_hosts_record_and_takes_no_input() {
    let fake = Fake::new();
    fake.say(&[Record::ServiceLibState(wire::ServiceLibState {
        state: 1,
        books: 12,
        generation: 9,
        walk_ppm: 400_000,
    })]);
    let reply = ask(&fake, 1, &[]).unwrap();
    assert!(matches!(only(&reply), Record::ServiceLibState(s) if s.books == 12));
    assert_eq!(
        call_raw(&host(&fake), 1, &loose(&[book_ref()])),
        Err(ERR_INVALID)
    );
    assert_eq!(call_raw(&host(&fake), 1, &[]), Err(ERR_INVALID));
}

#[test]
fn state_refuses_a_malformed_answer() {
    let fake = Fake::new();
    let state = |state, books, generation, walk_ppm| {
        Record::ServiceLibState(wire::ServiceLibState {
            state,
            books,
            generation,
            walk_ppm,
        })
    };
    for bad in [
        state(3, 1, 1, 0),
        state(2, 1, 0, 0),
        state(2, 1, 1, 1_000_001),
        state(0, 5, 1, 0),
    ] {
        fake.say(&[bad]);
        assert_eq!(ask(&fake, 1, &[]), Err(ERR_INVALID));
    }
    // Nothing, two records, another record.
    fake.say(&[]);
    assert_eq!(ask(&fake, 1, &[]), Err(ERR_INVALID));
    fake.say(&[state(2, 1, 1, 0), state(2, 1, 1, 0)]);
    assert_eq!(ask(&fake, 1, &[]), Err(ERR_INVALID));
    fake.say(&[book_ref()]);
    assert_eq!(ask(&fake, 1, &[]), Err(ERR_INVALID));
    fake.fails.set(Some(ERR_IO));
    assert_eq!(ask(&fake, 1, &[]), Err(ERR_IO));
}

// Ops 2 and 5: the query record.

#[test]
fn a_query_goes_to_the_host_with_its_values() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..3).map(entry_row).collect();
    let reply = ask(
        &fake,
        2,
        &[query(|q| {
            q.show = 5;
            q.sort = 4;
            q.flags = 3;
            q.words = "tolstoy war";
            q.author = "Leo";
            q.folder = "novels";
            q.added_since = 19_000;
        })],
    )
    .unwrap();
    assert_eq!(end_of(&reply).total, 3);
    assert_eq!(rows_in(&reply), 3);
    assert_eq!(
        fake.log.borrow().as_slice(),
        ["query 5 4 3 tolstoy war|Leo||novels 19000 0 0 0"]
    );
}

#[test]
fn query_values_are_checked_and_the_limits_are_exact() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..2).map(entry_row).collect();
    let with = |edit: fn(&mut Query<'static>)| ask(&fake, 2, &[query(edit)]);
    assert!(with(|q| q.show = 5).is_ok());
    assert_eq!(with(|q| q.show = 6), Err(ERR_INVALID));
    assert!(with(|q| q.sort = 4).is_ok());
    assert_eq!(with(|q| q.sort = 5), Err(ERR_INVALID));
    assert!(with(|q| q.flags = 3).is_ok());
    assert_eq!(with(|q| q.flags = 4), Err(ERR_INVALID));
    // Op 2 has no facet.
    assert_eq!(with(|q| q.facet = 1), Err(ERR_INVALID));
    // A cursor needs the generation it came from.
    assert_eq!(with(|q| q.cursor = 1), Err(ERR_STATE));
    let words: &'static str = Box::leak("w".repeat(256).into_boxed_str());
    assert!(ask(&fake, 2, &[query(|q| q.words = words)]).is_ok());
    assert_eq!(ask(&fake, 1, &[query(|_| ())]), Err(ERR_INVALID));
    assert_eq!(ask(&fake, 2, &[book_ref()]), Err(ERR_INVALID));
    let double = loose(&[query(|_| ()), query(|_| ())]);
    assert_eq!(call_raw(&host(&fake), 2, &double), Err(ERR_INVALID));
}

#[test]
fn a_search_over_256_bytes_is_no_space_never_shortened() {
    let fake = Fake::new();
    let words = "Z".repeat(256);
    let words: &'static str = Box::leak(words.into_boxed_str());
    let mut bytes = loose(&[query(|q| q.words = words)]);
    lengthen(&mut bytes, "Z");
    assert_eq!(call_raw(&host(&fake), 2, &bytes), Err(ERR_NO_SPACE));
    assert_eq!(call_raw(&host(&fake), 5, &bytes), Err(ERR_NO_SPACE));
    assert!(fake.log.borrow().is_empty(), "the host never saw it");
}

#[test]
fn facets_take_a_facet_up_to_four() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..2).map(facet_row).collect();
    for facet in 0..=4 {
        assert!(ask(&fake, 5, &[query(|q| q.facet = facet)]).is_ok());
    }
    assert_eq!(ask(&fake, 5, &[query(|q| q.facet = 5)]), Err(ERR_INVALID));
    let rows = ask(&fake, 5, &[query(|q| q.facet = 2)]).unwrap();
    assert_eq!((rows_in(&rows), end_of(&rows).total), (2, 2));
    assert!(
        fake.log
            .borrow()
            .last()
            .unwrap()
            .starts_with("facets 0 0 0")
    );
    // Entry rows are not facets, nor the other way round.
    *fake.rows.borrow_mut() = (0..2).map(entry_row).collect();
    assert_eq!(ask(&fake, 5, &[query(|_| ())]), Err(ERR_INVALID));
    *fake.rows.borrow_mut() = (0..2).map(facet_row).collect();
    assert_eq!(ask(&fake, 2, &[query(|_| ())]), Err(ERR_INVALID));
}

// Paging of the catalog.

#[test]
fn query_pages_by_count_without_gaps_or_repeats() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..100).map(entry_row).collect();
    let (pages, keys) = walk(&fake, 2, |cursor, generation| {
        query(|q| {
            q.cursor = cursor;
            q.generation = generation;
        })
    });
    assert_eq!(pages, [32, 32, 32, 4]);
    assert_eq!(keys, (0..100).collect::<Vec<_>>());
    *fake.rows.borrow_mut() = Vec::new();
    let reply = ask(&fake, 2, &[query(|_| ())]).unwrap();
    assert_eq!((rows_in(&reply), end_of(&reply).flags), (0, 1));
}

#[test]
fn query_pages_by_bytes_whole_rows_only() {
    let fake = Fake::new();
    // Rows at their caps (about 400 bytes): ten and a bit to a page.
    *fake.rows.borrow_mut() = (0..25).map(fat_entry_row).collect();
    let (pages, keys) = walk(&fake, 2, |cursor, generation| {
        query(|q| {
            q.cursor = cursor;
            q.generation = generation;
        })
    });
    let (last, full) = pages.split_last().unwrap();
    assert!(full.iter().all(|rows| (9..=10).contains(rows)), "{pages:?}");
    assert!((1..=10).contains(last));
    assert_eq!(keys, (0..25).collect::<Vec<_>>());
}

#[test]
fn a_stale_generation_or_a_bare_cursor_is_a_state_error() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..40).map(entry_row).collect();
    let page = |cursor, generation| {
        ask(
            &fake,
            2,
            &[query(|q| {
                q.cursor = cursor;
                q.generation = generation;
            })],
        )
    };
    assert_eq!(page(32, 0), Err(ERR_STATE));
    assert_eq!(page(32, 4), Err(ERR_STATE), "not the host's generation");
    assert!(page(32, 5).is_ok());
    assert_eq!(
        page(0, 4),
        Err(ERR_STATE),
        "a generation alone is checked too"
    );
    // A host that does not check: the service does.
    fake.lax.set(true);
    assert_eq!(page(32, 4), Err(ERR_STATE));
}

#[test]
fn the_hosts_position_is_checked() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..40).map(entry_row).collect();
    let page = || ask(&fake, 2, &[query(|_| ())]);
    fake.generation.set(0);
    assert_eq!(page(), Err(ERR_INVALID), "a generation is never 0");
    fake.generation.set(5);
    fake.next.set(Some(7));
    assert_eq!(page(), Err(ERR_INVALID), "next is not cursor + rows");
    fake.next.set(Some(41));
    assert_eq!(page(), Err(ERR_INVALID), "next is past the total");
    fake.next.set(None);
    fake.stubborn.set(true);
    assert_eq!(page(), Err(ERR_INVALID), "a host that ignores Break");
    fake.stubborn.set(false);
    assert!(page().is_ok());
    fake.fails.set(Some(ERR_IO));
    assert_eq!(page(), Err(ERR_IO), "a card failure: no partial rows");
}

#[test]
fn a_row_that_fits_no_page_is_a_refusal_not_a_loop() {
    let fake = Fake::new();
    *fake.rows.borrow_mut() = (0..3).map(entry_row).collect();
    // A scratch that holds the `end` but no row.
    let bytes = input(2, &[query(|_| ())]).unwrap();
    let mut out = std::vec![0; 40];
    let result = with(host(&fake), |frame| {
        frame.run(schema_op(2), &bytes, &mut out)
    });
    assert_eq!(result, Err(ERR_NO_SPACE));
    // A scratch that cannot even hold the `end`.
    let mut out = std::vec![0; 8];
    let result = with(host(&fake), |frame| {
        frame.run(schema_op(2), &bytes, &mut out)
    });
    assert_eq!(result, Err(ERR_NO_SPACE));
}

#[test]
fn rows_are_checked_before_they_are_passed_on() {
    let fake = Fake::new();
    let one = |row: Entry<'_>| {
        *fake.rows.borrow_mut() = std::vec![stream(&[Record::ServiceLibEntry(row)])];
        ask(&fake, 2, &[query(|_| ())])
    };
    let base = || entry(1);
    assert!(one(base()).is_ok());
    assert!(
        one(Entry {
            progress: 255,
            ..base()
        })
        .is_ok()
    );
    assert_eq!(
        one(Entry {
            progress: 101,
            ..base()
        }),
        Err(ERR_INVALID)
    );
    assert!(
        one(Entry {
            flags: 0xff,
            ..base()
        })
        .is_ok()
    );
    assert_eq!(
        one(Entry {
            flags: 0x100,
            ..base()
        }),
        Err(ERR_INVALID)
    );
    assert!(
        one(Entry {
            badges: "a\nb",
            ..base()
        })
        .is_ok()
    );
    assert_eq!(
        one(Entry {
            badges: "a\nb\nc",
            ..base()
        }),
        Err(ERR_INVALID)
    );
    // A repeated catalog handle.
    *fake.rows.borrow_mut() = std::vec![entry_row(3), entry_row(3)];
    assert_eq!(ask(&fake, 2, &[query(|_| ())]), Err(ERR_INVALID));
}

// Ops naming a book.

#[test]
fn a_book_is_named_by_path_or_by_handle_and_nothing_else() {
    let fake = Fake::new();
    fake.say(&[book_record("books/a.epub", 4)]);
    assert!(ask(&fake, 3, &[book_ref()]).is_ok());
    assert!(ask(&fake, 3, &[key_ref(4, 5)]).is_ok());
    let asked: Vec<String> = fake
        .log
        .borrow()
        .iter()
        .filter(|line| line.starts_with("book "))
        .cloned()
        .collect();
    assert_eq!(
        asked,
        [
            "book Path(\"books/a.epub\")",
            "book Key { key: 4, generation: 5 }"
        ]
    );
    let both = Record::ServiceLibRef(Ref {
        path: "books/a.epub",
        generation: 5,
        key: Some(4),
    });
    let neither = path_ref("");
    assert_eq!(ask(&fake, 3, &[both]), Err(ERR_INVALID));
    assert_eq!(ask(&fake, 3, &[neither]), Err(ERR_INVALID));
    assert_eq!(ask(&fake, 3, &[key_ref(4, 0)]), Err(ERR_STATE));
    for bad in ["../a.epub", "a\nb.epub", "/a.epub", "a/", "a:b"] {
        assert_eq!(ask(&fake, 3, &[path_ref(bad)]), Err(ERR_INVALID), "{bad}");
    }
}

#[test]
fn lib_book_flag_missing_is_the_hosts_lookup_not_the_catalogs() {
    let fake = Fake::new();
    let flags = |fake: &Rc<Fake>| match only(&ask(fake, 3, &[book_ref()]).unwrap()) {
        Record::ServiceLibBook(row) => row.flags,
        _ => panic!(),
    };
    // On the card: bit 8 clear, and the lookup got the book's own path.
    fake.say(&[book_record("books/a.epub", 4)]);
    assert_eq!(flags(&fake), 0);
    assert!(fake.log.borrow().contains(&"present books/a.epub".into()));
    // Gone: bit 8 set, the catalog's other bits kept.
    fake.present.set(Ok(false));
    let mut row = book_fields("books/a.epub", 4);
    row.flags = 0b1001;
    fake.say(&[Record::ServiceLibBook(row)]);
    assert_eq!(flags(&fake), 0b1_0000_1001);
    // The catalog cannot claim it either way: the lookup decides.
    fake.present.set(Ok(true));
    let mut row = book_fields("books/a.epub", 4);
    row.flags = 1 << 8;
    fake.say(&[Record::ServiceLibBook(row)]);
    assert_eq!(flags(&fake), 0);
    // A card failure is the reply; nothing is made up.
    fake.present.set(Err(ERR_IO));
    assert_eq!(ask(&fake, 3, &[book_ref()]), Err(ERR_IO));
    // A host without the lookup refuses the op rather than guess.
    fake.present.set(Err(ERR_UNSUPPORTED));
    assert_eq!(ask(&fake, 3, &[book_ref()]), Err(ERR_UNSUPPORTED));
    // The entry rows keep their eight bits: bit 8 there is invalid.
    let mut row = entry(4);
    row.flags = 1 << 8;
    fake.say(&[Record::ServiceLibEntry(row)]);
    assert!(ask(&fake, 6, &[book_ref()]).is_err());
}

#[test]
fn the_path_limit_is_512_bytes() {
    let fake = Fake::new();
    fake.say(&[book_record("books/a.epub", 4)]);
    let path = [
        "p".repeat(100),
        "p".repeat(100),
        "p".repeat(100),
        "p".repeat(100),
    ]
    .join("/")
        + "/"
        + &"q".repeat(108);
    assert_eq!(path.len(), 512);
    assert!(ask(&fake, 3, &[path_ref(&path)]).is_ok());
    let marked = "Z".repeat(512);
    let mut bytes = loose(&[path_ref(&marked)]);
    lengthen(&mut bytes, "Z");
    assert_eq!(call_raw(&host(&fake), 3, &bytes), Err(ERR_NO_SPACE));
}

#[test]
fn book_ops_take_a_ref_first_and_exactly_their_records() {
    let fake = Fake::new();
    fake.say(&[book_record("books/a.epub", 4)]);
    let plain = |op| call_raw(&host(&fake), op, &loose(&[]));
    for op in [3, 6, 13, 25] {
        assert_eq!(plain(op), Err(ERR_INVALID), "no record at all, op {op}");
        let extra = loose(&[book_ref(), page_ask(0, 0)]);
        assert_eq!(call_raw(&host(&fake), op, &extra), Err(ERR_INVALID), "{op}");
        let twice = loose(&[book_ref(), book_ref()]);
        assert_eq!(call_raw(&host(&fake), op, &twice), Err(ERR_INVALID), "{op}");
        let wrong = loose(&[page_ask(0, 0)]);
        assert_eq!(call_raw(&host(&fake), op, &wrong), Err(ERR_INVALID), "{op}");
    }
    for (op, own) in [
        (4, text_at(0, 0, 0)),
        (7, cover_ask(0)),
        (15, page_ask(0, 0)),
        (16, page_ask(0, 0)),
        (22, field_page_ask(0, 0)),
    ] {
        let call = |records: &[Record<'_>]| call_raw(&host(&fake), op, &loose(records));
        assert_eq!(
            call(&[book_ref()]),
            Err(ERR_INVALID),
            "own record missing, op {op}"
        );
        assert_eq!(call(&[own]), Err(ERR_INVALID), "no ref, op {op}");
        assert_eq!(call(&[own, book_ref()]), Err(ERR_INVALID), "order, op {op}");
        assert_eq!(call(&[book_ref(), own, own]), Err(ERR_INVALID), "{op}");
        assert_eq!(call(&[book_ref(), book_ref()]), Err(ERR_INVALID), "{op}");
        // Another op's record in its place.
        let alien = if op == 4 {
            cover_ask(0)
        } else {
            text_at(0, 0, 0)
        };
        assert_eq!(call(&[book_ref(), alien]), Err(ERR_INVALID), "{op}");
    }
}

#[test]
fn the_hosts_refusals_pass_through() {
    let fake = Fake::new();
    for error in [ERR_NOT_FOUND, ERR_STATE, ERR_IO, ERR_BUSY] {
        fake.fails.set(Some(error));
        assert_eq!(ask(&fake, 3, &[book_ref()]), Err(error));
        assert_eq!(ask(&fake, 6, &[key_ref(1, 1)]), Err(error));
        assert_eq!(ask(&fake, 7, &[book_ref(), cover_ask(0)]), Err(error));
        assert_eq!(ask(&fake, 13, &[book_ref()]), Err(error));
        assert_eq!(ask(&fake, 15, &[book_ref(), page_ask(0, 0)]), Err(error));
        assert_eq!(ask(&fake, 16, &[book_ref(), page_ask(0, 0)]), Err(error));
        let fields = [book_ref(), field_page_ask(0, 0)];
        assert_eq!(ask(&fake, 22, &fields), Err(error));
        assert_eq!(ask(&fake, 25, &[book_ref()]), Err(error));
    }
}

#[test]
fn book_answers_are_checked() {
    let fake = Fake::new();
    let book = |edit: fn(&mut wire::ServiceLibBook<'static>)| {
        let mut row = book_fields("books/a.epub", 4);
        edit(&mut row);
        fake.say(&[Record::ServiceLibBook(row)]);
        ask(&fake, 3, &[key_ref(4, 5)])
    };
    assert!(book(|_| ()).is_ok());
    assert_eq!(
        book(|b| b.key = 9),
        Err(ERR_INVALID),
        "another book than asked"
    );
    assert_eq!(book(|b| b.generation = 0), Err(ERR_INVALID));
    assert_eq!(book(|b| b.path = "../x"), Err(ERR_INVALID));
    assert_eq!(book(|b| b.progress = 150), Err(ERR_INVALID));
    assert!(book(|b| b.flags = 0x1ff).is_ok(), "bit 8 is `missing`");
    assert_eq!(book(|b| b.flags = 0x2ff), Err(ERR_INVALID));
    // The whole length is never shorter than the text, and says when it is cut.
    assert_eq!(book(|b| b.title_bytes = 1), Err(ERR_INVALID));
    assert_eq!(book(|b| b.author_bytes = 900), Err(ERR_INVALID));
    assert!(
        book(|b| {
            b.author_bytes = 900;
            b.flags |= 1 << 6;
        })
        .is_ok()
    );
    fake.say(&[]);
    assert_eq!(ask(&fake, 3, &[book_ref()]), Err(ERR_INVALID));
    fake.say(&[book_record("a", 1), book_record("a", 1)]);
    assert_eq!(ask(&fake, 3, &[book_ref()]), Err(ERR_INVALID));
    fake.say(&[Record::ServiceLibKey(wire::ServiceLibKey {
        key: "0123456789abcdef",
        fingerprint: "",
        size: 0,
    })]);
    assert_eq!(ask(&fake, 3, &[book_ref()]), Err(ERR_INVALID));
}

#[test]
fn text_takes_a_kind_an_index_and_an_offset() {
    let fake = Fake::new();
    let say = |kind, index, total, text: &'static str| {
        fake.say(&[Record::ServiceLibText(wire::ServiceLibText {
            kind,
            index,
            total,
            text,
        })]);
    };
    for kind in 0..=6 {
        say(kind, 0, 3, "abc");
        let reply = ask(&fake, 4, &[book_ref(), text_at(kind, 0, 0)]);
        assert!(reply.is_ok(), "kind {kind}");
    }
    assert_eq!(
        ask(&fake, 4, &[book_ref(), text_at(7, 0, 0)]),
        Err(ERR_INVALID)
    );
    // Whole texts have no index; lists do.
    for kind in 0..=2 {
        assert_eq!(
            ask(&fake, 4, &[book_ref(), text_at(kind, 1, 0)]),
            Err(ERR_NOT_FOUND)
        );
    }
    say(5, 77, 3, "abc");
    assert!(ask(&fake, 4, &[key_ref(2, 2), text_at(5, 77, 0)]).is_ok());
    assert!(fake.log.borrow().last().unwrap().ends_with("5 77 0"));
}

#[test]
fn a_text_answer_follows_the_offset_and_cut_rules() {
    let fake = Fake::new();
    let long = "é".repeat(600);
    let text_of = |kind, index, total, text: &str, offset| {
        fake.say(&[Record::ServiceLibText(wire::ServiceLibText {
            kind,
            index,
            total,
            text: Box::leak(String::from(text).into_boxed_str()),
        })]);
        ask(&fake, 4, &[book_ref(), text_at(0, 0, offset)])
    };
    // 1,200 bytes from 0: 1,024 of them, whole characters.
    assert!(text_of(0, 0, 1200, &long[..1024], 0).is_ok());
    assert!(
        text_of(0, 0, 1200, &long[..1022], 0).is_ok(),
        "cut at a character"
    );
    assert_eq!(
        text_of(0, 0, 1200, &long[..1000], 0),
        Err(ERR_INVALID),
        "cut short"
    );
    assert_eq!(
        text_of(0, 0, 100, &long[..200], 0),
        Err(ERR_INVALID),
        "longer than the text"
    );
    // From an offset: the rest when it fits.
    assert!(text_of(0, 0, 1200, &long[..200], 1000).is_ok());
    assert_eq!(text_of(0, 0, 1200, &long[..100], 1000), Err(ERR_INVALID));
    assert_eq!(
        text_of(0, 0, 100, "x", 200),
        Err(ERR_INVALID),
        "offset past the end"
    );
    // Another kind or index than asked.
    assert_eq!(text_of(1, 0, 3, "abc", 0), Err(ERR_INVALID));
    assert_eq!(text_of(0, 1, 3, "abc", 0), Err(ERR_INVALID));
}

#[test]
fn next_in_series_is_another_book() {
    let fake = Fake::new();
    fake.say(&[Record::ServiceLibEntry(entry(8))]);
    assert!(ask(&fake, 6, &[key_ref(4, 5)]).is_ok());
    assert_eq!(ask(&fake, 6, &[key_ref(8, 5)]), Err(ERR_INVALID));
    fake.say(&[Record::ServiceLibEntry(Entry {
        progress: 200,
        ..entry(8)
    })]);
    assert_eq!(ask(&fake, 6, &[key_ref(4, 5)]), Err(ERR_INVALID));
    fake.fails.set(Some(ERR_NOT_FOUND));
    assert_eq!(ask(&fake, 6, &[book_ref()]), Err(ERR_NOT_FOUND));
}

#[test]
fn cover_side_is_zero_or_up_to_the_screens_long_side() {
    let fake = Fake::new();
    fake.say(&[Record::ServiceDocImage(image(7, 300, 400))]);
    for side in [0, 1, 800] {
        assert!(
            ask(&fake, 7, &[book_ref(), cover_ask(side)]).is_ok(),
            "{side}"
        );
    }
    assert_eq!(
        ask(&fake, 7, &[book_ref(), cover_ask(801)]),
        Err(ERR_INVALID)
    );
    // The host learns who asks and what.
    assert_eq!(
        fake.log.borrow().last().unwrap(),
        "cover 7 files Path(\"books/a.epub\") 800"
    );
    for bad in [image(0, 3, 4), image(7, 0, 4), image(7, 3, 0)] {
        fake.say(&[Record::ServiceDocImage(bad)]);
        assert_eq!(ask(&fake, 7, &[book_ref(), cover_ask(0)]), Err(ERR_INVALID));
    }
    fake.say(&[Record::ServiceDocImage(wire::ServiceDocImage {
        flags: 2,
        ..image(7, 3, 4)
    })]);
    assert_eq!(ask(&fake, 7, &[book_ref(), cover_ask(0)]), Err(ERR_INVALID));
    fake.say(&[]);
    assert_eq!(ask(&fake, 7, &[book_ref(), cover_ask(0)]), Err(ERR_INVALID));
    // No screen size known.
    fake.long_side.set(Err(ERR_IO));
    assert_eq!(ask(&fake, 7, &[book_ref(), cover_ask(0)]), Err(ERR_IO));
}

#[test]
fn position_answers_are_checked() {
    let fake = Fake::new();
    let position = |edit: fn(&mut wire::ServiceLibPosition<'static>)| {
        let mut row = wire::ServiceLibPosition {
            section: 2,
            offset: 10,
            insert: 0,
            progress_ppm: 250_000,
            conversion: 0,
            updated: 1_700_000_000,
            xpointer: "",
            flags: 0,
        };
        edit(&mut row);
        fake.say(&[Record::ServiceLibPosition(row)]);
        ask(&fake, 13, &[book_ref()])
    };
    assert!(position(|_| ()).is_ok());
    assert!(position(|p| p.progress_ppm = 1_000_000).is_ok());
    assert_eq!(position(|p| p.progress_ppm = 1_000_001), Err(ERR_INVALID));
    assert_eq!(position(|p| p.flags = 8), Err(ERR_INVALID));
    assert!(position(|p| (p.flags, p.xpointer) = (1, "/body/p[1]")).is_ok());
    assert_eq!(
        position(|p| p.flags = 1),
        Err(ERR_INVALID),
        "flagged, none given"
    );
    assert_eq!(
        position(|p| p.xpointer = "/body"),
        Err(ERR_INVALID),
        "given, not flagged"
    );
    assert!(
        position(|p| (p.flags, p.conversion) = (2, 0)).is_ok(),
        "absent and 0 differ"
    );
    assert!(position(|p| (p.flags, p.conversion) = (6, 9)).is_ok());
    assert_eq!(position(|p| p.conversion = 9), Err(ERR_INVALID));
}

#[test]
fn book_key_is_sixteen_and_thirty_two_hex_digits() {
    let fake = Fake::new();
    let key = |key: &str, fingerprint: &str| {
        fake.say(&[Record::ServiceLibKey(wire::ServiceLibKey {
            key,
            fingerprint,
            size: 1234,
        })]);
        ask(&fake, 25, &[book_ref()])
    };
    let print = "0123456789abcdef0123456789abcdef";
    assert!(key("0123456789abcdef", print).is_ok());
    assert_eq!(key("0123456789abcde", print), Err(ERR_INVALID));
    assert_eq!(key("0123456789ABCDEF", print), Err(ERR_INVALID));
    assert_eq!(key("0123456789abcdeg", print), Err(ERR_INVALID));
    assert_eq!(key("0123456789abcdef", "abc"), Err(ERR_INVALID));
    assert_eq!(
        key("0123456789abcdef", &print.replace('0', "G")),
        Err(ERR_INVALID)
    );
}

#[path = "query_annotated.rs"]
mod annotated;

// The fake host and the records it answers with.

static FILL: [u8; 1100] = [b'x'; 1100];
/// `n` bytes of text with the lifetime of a fixture.
fn filler(n: usize) -> &'static str {
    core::str::from_utf8(&FILL[..n]).unwrap()
}

/// A host over scripted answers: records for the one-record methods, encoded
/// rows for the lists, and a log of what it was asked.
struct Fake {
    one: RefCell<Vec<Vec<u8>>>,
    rows: RefCell<Vec<Vec<u8>>>,
    log: RefCell<Vec<String>>,
    /// The generation the host reports (catalog and annotations alike).
    generation: Cell<u32>,
    fails: Cell<Option<i32>>,
    /// Does not refuse a stale generation itself.
    lax: Cell<bool>,
    /// Goes on past a `Break`.
    stubborn: Cell<bool>,
    next: Cell<Option<u32>>,
    long_side: Cell<Result<u32, i32>>,
    /// What the host's lookup of a book's file answers.
    present: Cell<Result<bool, i32>>,
}
impl Fake {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            one: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            log: RefCell::new(Vec::new()),
            generation: Cell::new(5),
            fails: Cell::new(None),
            lax: Cell::new(false),
            stubborn: Cell::new(false),
            next: Cell::new(None),
            long_side: Cell::new(Ok(800)),
            present: Cell::new(Ok(true)),
        })
    }
    fn say(&self, records: &[Record<'_>]) {
        *self.one.borrow_mut() = records
            .iter()
            .map(|record| stream(core::slice::from_ref(record)))
            .collect();
    }
    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
    }
    fn deal(&self, visit: One<'_>) -> Result<(), i32> {
        if let Some(error) = self.fails.get() {
            return Err(error);
        }
        for bytes in self.one.borrow().iter() {
            visit(wire::record_decode_one(bytes).unwrap());
        }
        Ok(())
    }
    fn page(&self, cursor: u32, generation: u32, visit: Rows<'_>) -> Result<RowsEnd, i32> {
        if let Some(error) = self.fails.get() {
            return Err(error);
        }
        let rows = self.rows.borrow();
        let total = u32::try_from(rows.len()).unwrap();
        if !self.lax.get() && generation != 0 && generation != self.generation.get() {
            return Err(ERR_STATE);
        }
        let mut next = cursor.min(total);
        for row in rows.iter().skip(next as usize) {
            let flow = visit(wire::record_decode_one(row).unwrap());
            if flow.is_break() && !self.stubborn.get() {
                break;
            }
            next += 1;
        }
        Ok(RowsEnd {
            total,
            generation: self.generation.get(),
            next: self.next.get().unwrap_or(next),
        })
    }
}
fn describe(q: &Query<'_>) -> String {
    std::format!(
        "{} {} {} {}|{}|{}|{} {} {} {} {}",
        q.show,
        q.sort,
        q.flags,
        q.words,
        q.author,
        q.series,
        q.folder,
        q.added_since,
        q.cursor,
        q.generation,
        q.facet
    )
}
impl super::super::LibExcluded for Fake {
    fn excluded(&self, _first_segment: &str) -> bool {
        false
    }
}
impl LibQuery for Fake {
    fn screen_long_side(&self) -> Result<u32, i32> {
        self.long_side.get()
    }
    fn state(&self, visit: One<'_>) -> Result<(), i32> {
        self.deal(visit)
    }
    fn query(&self, q: &Query<'_>, visit: Rows<'_>) -> Result<RowsEnd, i32> {
        self.note(std::format!("query {}", describe(q)));
        self.page(q.cursor, q.generation, visit)
    }
    fn book(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.note(std::format!("book {book:?}"));
        self.deal(visit)
    }
    fn book_file_present(&self, path: &str) -> Result<bool, i32> {
        self.note(std::format!("present {path}"));
        self.present.get()
    }
    fn text(&self, book: BookRef<'_>, at: &TextAt, visit: One<'_>) -> Result<(), i32> {
        self.note(std::format!(
            "text {book:?} {} {} {}",
            at.kind,
            at.index,
            at.offset
        ));
        self.deal(visit)
    }
    fn facets(&self, q: &Query<'_>, visit: Rows<'_>) -> Result<RowsEnd, i32> {
        self.note(std::format!("facets {}", describe(q)));
        self.page(q.cursor, q.generation, visit)
    }
    fn next_in_series(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.note(std::format!("series {book:?}"));
        self.deal(visit)
    }
    fn cover(
        &self,
        who: &Caller<'_>,
        book: BookRef<'_>,
        side: u32,
        _cx: &crate::native_host::lib_cover::CoverCx<'_>,
        visit: One<'_>,
    ) -> Result<(), i32> {
        self.note(std::format!(
            "cover {} {} {book:?} {side}",
            who.owner,
            who.plugin
        ));
        self.deal(visit)
    }
    fn position(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.note(std::format!("position {book:?}"));
        self.deal(visit)
    }
    fn annotations(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.note(std::format!("annotations {book:?} {cursor} {generation}"));
        self.page(cursor, generation, visit)
    }
    fn anchors(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.note(std::format!("anchors {book:?} {cursor} {generation}"));
        self.page(cursor, generation, visit)
    }
    fn fields(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.note(std::format!("fields {book:?} {cursor} {generation}"));
        self.page(cursor, generation, visit)
    }
    fn book_key(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.note(std::format!("key {book:?}"));
        self.deal(visit)
    }
}
impl LibWrite for Fake {}

/// A bare stream of records, as the host answers.
fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = std::vec![0; 4096];
        let used = wire::record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}
fn end_of(reply: &[u8]) -> wire::ServiceEnd {
    let mut at = 0;
    loop {
        let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
        at += used;
        if let Record::ServiceEnd(end) = record {
            assert_eq!(at, reply.len());
            return end;
        }
    }
}
fn rows_in(reply: &[u8]) -> usize {
    ids_in(reply).len()
}
/// The key or id of each row of a page.
fn ids_in(reply: &[u8]) -> Vec<u32> {
    let mut at = 0;
    let mut ids = Vec::new();
    while at < reply.len() {
        let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
        at += used;
        match record {
            Record::ServiceLibEntry(row) => ids.push(row.key),
            Record::ServiceLibAnnotation(row) => ids.push(row.id),
            Record::ServiceLibAnchor(row) => ids.push(row.id),
            Record::ServiceLibFacet(row) => ids.push(row.first),
            Record::ServiceLibField(row) => {
                ids.push(row.id.trim_start_matches("plug.f").parse().unwrap_or(0));
            }
            Record::ServiceEnd(_) => {}
            other => panic!("{other:?}"),
        }
    }
    ids
}

/// Walk every page of op `op`: (rows per page, every id in order).
fn walk_with(
    fake: &Rc<Fake>,
    op: u32,
    make: impl Fn(u32, u32) -> Vec<Record<'static>>,
) -> (Vec<usize>, Vec<u32>) {
    let (mut cursor, mut generation) = (0, 0);
    let (mut pages, mut all) = (Vec::new(), Vec::new());
    loop {
        let reply = ask(fake, op, &make(cursor, generation)).unwrap();
        assert!(reply.len() <= 4096);
        let end = end_of(&reply);
        let ids = ids_in(&reply);
        pages.push(ids.len());
        all.extend(ids);
        assert_ne!(end.generation, 0);
        if end.flags & 1 != 0 {
            assert_eq!(
                (end.total as usize, end.next as usize),
                (all.len(), all.len())
            );
            return (pages, all);
        }
        assert!(end.next > cursor, "a page moves the cursor forward");
        (cursor, generation) = (end.next, end.generation);
    }
}
fn walk(
    fake: &Rc<Fake>,
    op: u32,
    make: impl Fn(u32, u32) -> Record<'static>,
) -> (Vec<usize>, Vec<u32>) {
    walk_with(fake, op, |cursor, generation| {
        std::vec![make(cursor, generation)]
    })
}
fn walk_book(fake: &Rc<Fake>, op: u32) -> (Vec<usize>, Vec<u32>) {
    walk_with(fake, op, |cursor, generation| {
        std::vec![book_ref(), page_ask(cursor, generation)]
    })
}

/// Make `bytes` (one counted record holding a run of `extra` bytes of 128 to
/// 16,383) carry one more byte of the run: a text over its cap, which the
/// encoder refuses to make.
fn lengthen(bytes: &mut Vec<u8>, extra: &str) {
    let mark = extra.as_bytes()[0];
    let start = bytes.iter().position(|b| *b == mark).unwrap();
    let run = bytes[start..].iter().take_while(|b| **b == mark).count();
    assert!((128..16_384).contains(&run));
    let widen = |at: usize, bytes: &mut Vec<u8>| {
        let old = usize::from(bytes[at] & 0x7f) | usize::from(bytes[at + 1]) << 7;
        assert!(bytes[at] & 0x80 != 0 && bytes[at + 1] & 0x80 == 0);
        let new = old + 1;
        assert!(new < 16_384);
        bytes[at] = u8::try_from(new & 0x7f).unwrap() | 0x80;
        bytes[at + 1] = u8::try_from(new >> 7).unwrap();
    };
    widen(start - 2, bytes);
    bytes.insert(start + run, mark);
    // The count, the tag, then the length of the record.
    let mut at = 1;
    while bytes[at] & 0x80 != 0 {
        at += 1;
    }
    widen(at + 1, bytes);
}

fn entry(key: u32) -> Entry<'static> {
    Entry {
        key,
        title: "A title",
        author: "An author",
        series: "",
        number: "",
        progress: 40,
        flags: 0b10,
        added: 19_000,
        badges: "",
    }
}
fn entry_row(key: u32) -> Vec<u8> {
    stream(&[Record::ServiceLibEntry(entry(key))])
}
/// A row at its caps: about 400 bytes.
fn fat_entry_row(key: u32) -> Vec<u8> {
    stream(&[Record::ServiceLibEntry(Entry {
        title: filler(128),
        author: filler(96),
        series: filler(64),
        number: filler(16),
        badges: filler(64),
        ..entry(key)
    })])
}
fn facet_row(n: u32) -> Vec<u8> {
    let text = std::format!("author {n}");
    stream(&[Record::ServiceLibFacet(wire::ServiceLibFacet {
        text: &text,
        count: n + 1,
        first: n,
    })])
}
fn book_fields(path: &'static str, key: u32) -> wire::ServiceLibBook<'static> {
    wire::ServiceLibBook {
        key,
        generation: 5,
        path,
        title: "Title",
        author: "Author",
        series: "",
        number: "",
        progress: 10,
        flags: 0,
        added: 1,
        size: 100,
        format: "epub",
        tags: 2,
        fields: 1,
        title_bytes: 5,
        author_bytes: 6,
        series_bytes: 0,
    }
}
fn book_record(path: &'static str, key: u32) -> Record<'static> {
    Record::ServiceLibBook(book_fields(path, key))
}
fn cover_ask(side: u32) -> Record<'static> {
    Record::ServiceLibCover(Cover { side })
}
fn image(image: u32, width: u32, height: u32) -> wire::ServiceDocImage {
    wire::ServiceDocImage {
        image,
        index: 0,
        width,
        height,
        flags: 0,
    }
}
/// An annotation of 512-byte text and note (about 1,100 bytes).
fn annotation_fields(id: u32, section: u32) -> wire::ServiceLibAnnotation<'static> {
    wire::ServiceLibAnnotation {
        id,
        kind: 1,
        start_section: section,
        start_offset: 5,
        end_section: section,
        end_offset: 10,
        flags: 1,
        percent: 50,
        conversion: 0,
        created: 1,
        updated: 2,
        client_key: "k",
        text: filler(512),
        note: filler(512),
    }
}
fn annotation_row(id: u32, section: u32) -> Vec<u8> {
    stream(&[Record::ServiceLibAnnotation(annotation_fields(id, section))])
}
fn anchor_fields(
    id: u32,
    flags: u32,
    start: &'static str,
    end: &'static str,
) -> wire::ServiceLibAnchor<'static> {
    wire::ServiceLibAnchor {
        id,
        flags,
        conversion: 0,
        start,
        end,
    }
}
/// An anchor of two XPointers of 1,024 bytes (about 2,070 bytes).
fn anchor_row(id: u32, flags: u32) -> Vec<u8> {
    stream(&[Record::ServiceLibAnchor(anchor_fields(
        id,
        flags,
        filler(1024),
        filler(1024),
    ))])
}
fn field_row(n: u32, value: usize) -> Vec<u8> {
    let id = std::format!("plug.f{n}");
    stream(&[Record::ServiceLibField(wire::ServiceLibField {
        id: &id,
        title: "Field",
        value: filler(value),
    })])
}
