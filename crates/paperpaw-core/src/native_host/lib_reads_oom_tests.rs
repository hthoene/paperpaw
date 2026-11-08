//! Each allocation of a catalog read refuses in turn: the read is
//! `ERR_NO_MEMORY` (never an abort, never "not found"), and the read that
//! survives the last allocation answers what the unrefused read did.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::{NORMAL, arm_failure, reset};
use crate::native_host::lib_reads::CatalogReads;
use crate::native_host::lib_reads::tests::{Card, card};
use crate::native_host::lib_service::{BookRef, LibQuery};
use crate::native_host::registrations::{self as wire, Record};
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
fn every_allocation_of_a_catalog_read_refuses_cleanly() {
    reset(NORMAL);
    let card = card();
    let generation = card.generation();
    let a = u32::try_from(card.a).unwrap();
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
    ];
    // Each read reserves something, so each has an allocation to refuse.
    assert!(counts.iter().all(|&n| n >= 1), "{counts:?}");
    assert!(
        counts[1] >= 4,
        "the book reserves its buffer and the view's reads: {counts:?}"
    );
}
