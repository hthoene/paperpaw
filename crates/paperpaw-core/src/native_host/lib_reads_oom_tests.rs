//! Each allocation of a catalog read refuses in turn: the read is
//! `ERR_NO_MEMORY` (never an abort, never "not found"), and the read that
//! survives the last allocation answers what the unrefused read did.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::{NORMAL, arm_failure, reset};
use crate::native_host::lib_reads::CatalogReads;
use crate::native_host::lib_reads::tests::{Card, annotation_file, card, put_annotations};
use crate::native_host::lib_service::{BookRef, LibQuery};
use crate::native_host::registrations::{self as wire, Record};
extern crate std;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::Storage as _;

/// What a read answered, as numbers (a visitor must not allocate here).
type Seen = Option<[u64; 6]>;

/// Runs `read` with its 1st, 2nd, ... allocation refused until one passes
/// them all; returns how many it made.
fn sweep(read: impl Fn(&CatalogReads, &mut Seen) -> Result<(), i32>, card: &Card) -> u32 {
    sweep_to(read, card, 64)
}

/// [`sweep`] for a read that makes up to `limit - 1` allocations.
fn sweep_to(
    read: impl Fn(&CatalogReads, &mut Seen) -> Result<(), i32>,
    card: &Card,
    limit: u32,
) -> u32 {
    let reads = card.reads();
    let mut whole: Seen = None;
    read(&reads, &mut whole).unwrap();
    assert!(whole.is_some());
    for nth in 1..limit {
        let mut seen: Seen = None;
        arm_failure(nth);
        let result = read(&reads, &mut seen);
        arm_failure(0);
        match result {
            Err(status) => {
                assert_eq!(status, ERR_NO_MEMORY, "allocation {nth}");
                assert_eq!(seen, None, "nothing is answered after a refusal");
            }
            Ok(()) => {
                assert_eq!(seen, whole, "allocation {nth} was not needed");
                return nth - 1;
            }
        }
    }
    panic!("a read makes more than {} allocations", limit - 1);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one table of the sweeps of every read"
)]
fn every_allocation_of_a_catalog_read_refuses_cleanly() {
    reset(NORMAL);
    let card = card();
    let generation = card.generation();
    let a = u32::try_from(card.a).unwrap();
    {
        use paperpaw_sdk::document::Locator;
        use paperpaw_sdk::positions::{self, Position};
        let kept = Position::new(Locator::new(7, 1234), 41.5).xpointer(Some("/body/p[3]".into()));
        positions::save(&*card.storage, "books/a.epub", &kept).unwrap();
        put_annotations(&card, &annotation_file("a note"));
        // The book's file is there: a gone one is looked for as a directory
        // by the storage, whose own allocations are not the host's.
        card.storage.write("books/b.epub", &[7_u8; 10]).unwrap();
    }
    let by_path = BookRef::Path("books/b.epub");
    let by_key = BookRef::Key { key: a, generation };
    let counts = [
        sweep(
            |reads, seen| {
                reads.state(&mut |record| {
                    let Record::ServiceLibState(s) = record else {
                        panic!()
                    };
                    *seen = Some([
                        s.state.into(),
                        s.books.into(),
                        s.generation.into(),
                        s.walk_ppm.into(),
                        0,
                        0,
                    ]);
                })
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                reads.book(by_path, &mut |record| {
                    let Record::ServiceLibBook(b) = record else {
                        panic!()
                    };
                    *seen = Some([
                        b.key.into(),
                        b.flags.into(),
                        b.title.len() as u64,
                        b.path.len() as u64,
                        b.format.len() as u64,
                        b.size,
                    ]);
                })
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                reads.book(by_key, &mut |record| {
                    let Record::ServiceLibBook(b) = record else {
                        panic!()
                    };
                    *seen = Some([b.key.into(), b.flags.into(), b.title.len() as u64, 0, 0, 0]);
                })
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                let ask = wire::ServiceLibTextAt {
                    kind: 3,
                    index: 1,
                    offset: 0,
                };
                reads.text(by_path, &ask, &mut |record| {
                    let Record::ServiceLibText(t) = record else {
                        panic!()
                    };
                    *seen = Some([t.total.into(), t.text.len() as u64, 0, 0, 0, 0]);
                })
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                reads.position(by_key, &mut |record| {
                    let Record::ServiceLibPosition(p) = record else {
                        panic!()
                    };
                    *seen = Some([
                        p.section.into(),
                        p.offset.into(),
                        p.progress_ppm.into(),
                        p.xpointer.len() as u64,
                        p.flags.into(),
                        0,
                    ]);
                })
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                reads.book_key(by_key, &mut |record| {
                    let Record::ServiceLibKey(k) = record else {
                        panic!()
                    };
                    *seen = Some([
                        k.key.len() as u64,
                        k.fingerprint.len() as u64,
                        k.size,
                        0,
                        0,
                        0,
                    ]);
                })
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                let mut total = 0_u64;
                let end =
                    reads.annotations(BookRef::Path("books/a.epub"), 1, 0, &mut |record| {
                        let Record::ServiceLibAnnotation(a) = record else {
                            panic!()
                        };
                        total += u64::from(a.id) + a.text.len() as u64;
                        core::ops::ControlFlow::Continue(())
                    })?;
                *seen = Some([
                    total,
                    end.total.into(),
                    end.next.into(),
                    end.generation.into(),
                    0,
                    0,
                ]);
                Ok(())
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                let mut total = 0_u64;
                let end = reads.anchors(BookRef::Path("books/a.epub"), 0, 0, &mut |record| {
                    let Record::ServiceLibAnchor(a) = record else {
                        panic!()
                    };
                    total += u64::from(a.id) + a.start.len() as u64;
                    core::ops::ControlFlow::Continue(())
                })?;
                *seen = Some([
                    total,
                    end.total.into(),
                    end.next.into(),
                    end.generation.into(),
                    0,
                    0,
                ]);
                Ok(())
            },
            &card,
        ),
        sweep(
            |reads, seen| {
                let ask = wire::ServiceLibTextAt {
                    kind: 6,
                    index: 2,
                    offset: 0,
                };
                reads.text(BookRef::Path("books/a.epub"), &ask, &mut |record| {
                    let Record::ServiceLibText(t) = record else {
                        panic!()
                    };
                    *seen = Some([t.total.into(), t.text.len() as u64, 0, 0, 0, 0]);
                })
            },
            &card,
        ),
    ];
    // Each read reserves something, so each has an allocation to refuse.
    assert!(counts.iter().all(|&n| n >= 1), "{counts:?}");
    assert!(
        counts[1] >= 4,
        "the book reserves its buffer and the view's reads: {counts:?}"
    );
}

/// The heap one read holds at its most (the card here is read without
/// allocating, so this is the adapter's and the SDK readers' own).
fn peak(read: impl FnOnce(&CatalogReads), card: &Card) -> usize {
    let reads = card.reads();
    super::HEAP_LIVE.with(|live| live.set(0));
    super::HEAP_PEAK.with(|peak| peak.set(0));
    read(&reads);
    super::HEAP_PEAK.with(core::cell::Cell::get)
}

#[test]
fn the_heap_one_catalog_read_holds_at_its_most_is_small() {
    reset(NORMAL);
    let card = card();
    {
        use paperpaw_sdk::document::Locator;
        use paperpaw_sdk::positions::{self, Position};
        let kept = Position::new(Locator::new(7, 1234), 41.5).xpointer(Some("/body/p[3]".into()));
        positions::save(&*card.storage, "books/a.epub", &kept).unwrap();
        put_annotations(&card, &annotation_file("a note"));
    }
    // The book's file is there: a gone one is looked for by the storage,
    // whose own allocations are not the host's (and not what is measured).
    card.storage.write("books/b.epub", &[7_u8; 10]).unwrap();
    let path = BookRef::Path("books/b.epub");
    let ask = wire::ServiceLibTextAt {
        kind: 0,
        index: 0,
        offset: 0,
    };
    let peaks = [
        peak(|r| r.state(&mut |_| ()).unwrap(), &card),
        peak(|r| r.book(path, &mut |_| ()).unwrap(), &card),
        peak(|r| r.text(path, &ask, &mut |_| ()).unwrap(), &card),
        peak(
            |r| {
                r.book_key(BookRef::Path("books/a.epub"), &mut |_| ())
                    .unwrap()
            },
            &card,
        ),
        peak(
            |r| {
                r.position(BookRef::Path("books/a.epub"), &mut |_| ())
                    .unwrap()
            },
            &card,
        ),
        peak(
            |r| {
                r.annotations(BookRef::Path("books/a.epub"), 0, 0, &mut |_| {
                    core::ops::ControlFlow::Continue(())
                })
                .unwrap();
            },
            &card,
        ),
        peak(
            |r| {
                r.anchors(BookRef::Path("books/a.epub"), 0, 0, &mut |_| {
                    core::ops::ControlFlow::Continue(())
                })
                .unwrap();
            },
            &card,
        ),
        peak(
            |r| {
                let ask = wire::ServiceLibTextAt {
                    kind: 6,
                    index: 2,
                    offset: 0,
                };
                r.text(BookRef::Path("books/a.epub"), &ask, &mut |_| ())
                    .unwrap();
            },
            &card,
        ),
    ];
    std::eprintln!(
        "heap peaks (state, book, text, book_key, position, annotations, anchors, note): {peaks:?}"
    );
    // The book's buffers are 1,952 bytes, and the log of changes (12 bytes
    // a line, two lines here) is held while a line is read.
    assert!(peaks[1] <= 2_048, "{peaks:?}");
    assert!(
        peaks[2] <= 1_152
            && peaks[0] <= 256
            && peaks[3] <= 256
            && peaks[4] <= 512
            && peaks[5] <= 1_280
            && peaks[6] <= 2_304
            && peaks[7] <= 1_280,
        "{peaks:?}"
    );
}

/// A query reads every line of the catalog and makes a key for each book
/// it finds: each of those allocations refuses in turn, over two passes (a
/// cursor of 3), and a query that survives them all answers what the
/// unrefused one did.
#[test]
fn every_allocation_of_a_query_refuses_cleanly() {
    reset(NORMAL);
    let card = card();
    let ask = |cursor, sort, words: &'static str| wire::ServiceLibQuery {
        show: 0,
        sort,
        flags: 0,
        words,
        author: "",
        series: "",
        tag: "",
        folder: "",
        added_since: 0,
        cursor,
        generation: 0,
        facet: 0,
    };
    let counts = [(0, 0, ""), (3, 1, ""), (0, 2, "e"), (1, 3, ""), (0, 4, "")].map(
        |(cursor, sort, words)| {
            sweep_to(
                |reads, seen| {
                    let (mut rows, mut keys) = (0_u64, 0_u64);
                    let end = reads.query(&ask(cursor, sort, words), "", &mut |record| {
                        let Record::ServiceLibEntry(e) = record else {
                            panic!()
                        };
                        rows += 1;
                        keys += u64::from(e.key) + e.title.len() as u64;
                        core::ops::ControlFlow::Continue(())
                    })?;
                    *seen = Some([rows, keys, end.total.into(), end.next.into(), 0, 0]);
                    Ok(())
                },
                &card,
                2_000,
            )
        },
    );
    assert!(counts.iter().all(|&n| n >= 1), "{counts:?}");
}

/// The heap of one `query` pass over a shelf of 300 books with the names of
/// a real one (home's recent list, the files list): the page's 32 slots are
/// what it holds, besides the line being read.
#[test]
fn the_heap_one_query_holds_at_its_most_is_the_window_and_one_line() {
    use paperpaw_sdk::MemoryStorage;
    reset(NORMAL);
    let mut text = std::string::String::from("\tcatalog\t2\t1\n");
    for n in 0..300_u32 {
        text.push_str(&std::format!(
            "books/library/Some Folder/{n:03}-a-reasonably-long-file-name.epub\tFirstname Lastname {n}\t\
             A Reasonably Long Book Title, Number {n}\tA Series Name\t{}\t\t{}\t\t19990\t\t123456\n",
            n % 12,
            if n % 3 == 0 { "40" } else { "" },
        ));
    }
    let card = Card {
        storage: std::rc::Rc::new(
            MemoryStorage::new().with(".paperpaw/library/catalog.txt", text.into_bytes()),
        ),
        env: card().env,
        a: 0,
        b: 0,
        c: 0,
        d: 0,
        e: 0,
    };
    let ask = |sort, cursor| wire::ServiceLibQuery {
        show: 0,
        sort,
        flags: 0,
        words: "",
        author: "",
        series: "",
        tag: "",
        folder: "",
        added_since: 0,
        cursor,
        generation: 0,
        facet: 0,
    };
    let run = |sort, cursor| {
        peak(
            |r| {
                r.query(&ask(sort, cursor), "", &mut |_| {
                    core::ops::ControlFlow::Continue(())
                })
                .unwrap();
            },
            &card,
        )
    };
    let peaks = [run(0, 0), run(4, 0), run(0, 100)];
    std::eprintln!("QUERY HEAP PEAKS (title, recent, title from 100): {peaks:?}");
    // The window of 32 slots (64 bytes and two short strings each) and a line.
    assert!(peaks.iter().all(|&peak| peak <= 8_192), "{peaks:?}");
}
