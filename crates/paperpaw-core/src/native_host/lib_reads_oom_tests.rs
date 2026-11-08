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

/// What a read answered, as numbers (a visitor must not allocate here).
type Seen = Option<[u64; 6]>;

/// Runs `read` with its 1st, 2nd, ... allocation refused until one passes
/// them all; returns how many it made.
fn sweep(read: impl Fn(&CatalogReads, &mut Seen) -> Result<(), i32>, card: &Card) -> u32 {
    let reads = card.reads();
    let mut whole: Seen = None;
    read(&reads, &mut whole).unwrap();
    assert!(whole.is_some());
    for nth in 1..64 {
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
    panic!("a read makes more than 63 allocations");
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
