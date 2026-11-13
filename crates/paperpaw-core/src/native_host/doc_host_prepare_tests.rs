//! The preparing loop of the production document host: a format that answers
//! `open` with state 1 is called with op 3 between inputs, its answers are
//! checked and recorded, the opener hears of changes with
//! `event.document_changed`, and a finished preparation stamps the cache.
use super::*;
use crate::native_host::{
    doc_service::{FAILED, PREPARING, PrepareStep, READY},
    fmt_call::FrameRoute,
};
use paperpaw_native::host_glue::ERR_BUSY;

fn fmt_state(state: u32, progress: u32, status: i32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtState(local::FmtState::new(
            0,
            state,
            progress,
            status,
            0,
            5,
            "epub.epub",
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    bytes
}

/// A world with one book whose format answered `open` with "preparing".
fn preparing(w: &World<'_>, path: &str) -> u32 {
    *w.reply.borrow_mut() = Ok(fmt_state(PREPARING, 0, 0));
    w.open(1, path).unwrap()
}

fn step(w: &World<'_>) -> PrepareStep {
    w.state.prepare_step(&w.hosts, &FrameRoute(w))
}

fn answer(w: &World<'_>, state: u32, progress: u32, status: i32) {
    *w.reply.borrow_mut() = Ok(fmt_state(state, progress, status));
}

/// The oldest `event.document_changed` of the opener: (document, state,
/// progress).
fn changed_event(w: &World<'_>) -> Option<(u32, u32, u32)> {
    let mut out = [0_u8; 64];
    let length = w
        .state
        .next_event(OPENER, &mut out)
        .unwrap_or_else(|e| panic!("next_event {e}"))?;
    match crate::native_host::codec::decode_any_single(&out[..length]).unwrap() {
        Record::EventDocumentChanged(event) => {
            Some((event.document, event.state, event.progress_ppm))
        }
        other => panic!("{other:?}"),
    }
}

fn ops(w: &World<'_>) -> Vec<u32> {
    w.calls.borrow().iter().map(|call| call.2).collect()
}

#[test]
fn a_preparing_document_is_stepped_until_ready_and_the_opener_hears_each_change() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        assert_eq!(step(w), PrepareStep::Idle, "nothing prepares yet");
        let document = preparing(w, "books/a.epub");
        assert_eq!(changed_event(w), None, "the open's state is its answer");
        answer(w, PREPARING, 400_000, 0);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), Some((document, PREPARING, 400_000)));
        assert_eq!(changed_event(w), None);
        // The same progress again: no event.
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), None);
        answer(w, READY, 1_000_000, 0);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), Some((document, READY, 1_000_000)));
        assert_eq!(
            step(w),
            PrepareStep::Idle,
            "a ready document is not stepped"
        );
        assert_eq!(ops(w), [2, 3, 3, 3]);
    });
}

#[test]
fn steps_between_two_takes_are_one_event_with_the_latest_values() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = preparing(w, "books/a.epub");
        for progress in [100_000, 200_000, 300_000] {
            answer(w, PREPARING, progress, 0);
            assert_eq!(step(w), PrepareStep::More);
        }
        assert_eq!(changed_event(w), Some((document, PREPARING, 300_000)));
        assert_eq!(changed_event(w), None);
    });
}

#[test]
fn the_documents_of_a_device_with_room_take_turns() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let first = preparing(w, "books/a.epub");
        let second = preparing(w, "books/b.epub");
        assert_ne!(first, second);
        answer(w, PREPARING, 100_000, 0);
        step(w);
        answer(w, PREPARING, 200_000, 0);
        step(w);
        // One step each: both documents moved, the first one's event first.
        assert_eq!(changed_event(w), Some((first, PREPARING, 100_000)));
        assert_eq!(changed_event(w), Some((second, PREPARING, 200_000)));
        assert_eq!(changed_event(w), None);
    });
}

#[test]
fn a_format_that_answers_an_error_or_nonsense_fails_the_document() {
    for bad in [Err(ERR_IO), Ok(vec![0xff_u8; 9])] {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            let document = preparing(w, "books/a.epub");
            *w.reply.borrow_mut() = bad.clone();
            assert_eq!(step(w), PrepareStep::More);
            assert_eq!(changed_event(w), Some((document, FAILED, 0)));
            assert_eq!(
                step(w),
                PrepareStep::Idle,
                "a failed document is not stepped"
            );
        });
    }
}

#[test]
fn a_step_that_cannot_be_made_now_is_tried_again_and_the_document_still_prepares() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = preparing(w, "books/a.epub");
        *w.reply.borrow_mut() = Err(ERR_BUSY);
        assert_eq!(step(w), PrepareStep::Later);
        assert_eq!(changed_event(w), None);
        answer(w, PREPARING, 500_000, 0);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), Some((document, PREPARING, 500_000)));
    });
}

#[test]
fn a_step_made_later_still_passes_the_turn_to_the_next_document() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let first = preparing(w, "books/a.epub");
        let second = preparing(w, "books/b.epub");
        // The first one's step cannot be made now: the next step is the
        // second one's, not the first one's again (it would starve the rest).
        *w.reply.borrow_mut() = Err(ERR_BUSY);
        assert_eq!(step(w), PrepareStep::Later);
        answer(w, PREPARING, 200_000, 0);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), Some((second, PREPARING, 200_000)));
        // And then the first one has its turn.
        answer(w, PREPARING, 100_000, 0);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), Some((first, PREPARING, 100_000)));
        assert_eq!(changed_event(w), None);
    });
}

#[test]
fn a_failed_answer_must_carry_its_error_and_a_progress_is_at_most_a_million() {
    // State 2 without a status, and progress above 1,000,000 ppm: malformed.
    for (state, progress, status) in [(FAILED, 0, 0), (PREPARING, 1_000_001, 0)] {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            let document = preparing(w, "books/a.epub");
            answer(w, state, progress, status);
            assert_eq!(step(w), PrepareStep::More);
            assert_eq!(changed_event(w), Some((document, FAILED, 0)));
        });
    }
}

#[test]
fn a_ready_answer_stamps_the_cache_and_a_no_space_answer_removes_it() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        preparing(w, "books/a.epub");
        let folder = seen_cache(w, 0);
        let stamp = std::format!("{folder}/{}", crate::native_host::doc_cache::STAMP);
        assert!(w.card.get(&stamp).is_none(), "not finished yet");
        answer(w, PREPARING, 500_000, 0);
        step(w);
        assert!(w.card.get(&stamp).is_none());
        answer(w, READY, 1_000_000, 0);
        step(w);
        assert!(w.card.get(&stamp).is_some(), "ready: stamped");
    });
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        let document = preparing(w, "books/a.epub");
        let folder = seen_cache(w, 0);
        answer(w, PREPARING, 100_000, 0);
        step(w);
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_some());
        *w.reply.borrow_mut() = Err(ERR_NO_SPACE);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), Some((document, FAILED, 100_000)));
        assert!(
            w.card.get(&std::format!("{folder}/pages/1")).is_none(),
            "the partial cache is removed"
        );
    });
}

#[test]
fn a_closed_document_is_not_stepped_and_leaves_no_event() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = preparing(w, "books/a.epub");
        answer(w, PREPARING, 100_000, 0);
        step(w);
        w.call(
            AdmittedFeatures::NONE,
            18,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        )
        .unwrap();
        assert_eq!(changed_event(w), None);
        assert_eq!(step(w), PrepareStep::Idle);
    });
}

#[test]
fn a_device_with_two_places_prepares_one_book_at_a_time_in_open_order() {
    assert!(Limits::X4.one_preparation());
    assert!(!Limits::LARGE.one_preparation());
}

#[test]
fn a_format_that_stopped_while_a_book_prepares_ends_it_and_the_opener_hears_it() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = preparing(w, "books/a.epub");
        w.stop("epub");
        // The next call brings the index in line: reason 1, the format stopped.
        assert_eq!(w.format_for("a.epub"), Err(ERR_NOT_FOUND));
        assert_eq!(
            step(w),
            PrepareStep::Idle,
            "a closed document is not stepped"
        );
        let mut out = [0_u8; 64];
        let length = w.state.next_event(OPENER, &mut out).unwrap().unwrap();
        match crate::native_host::codec::decode_any_single(&out[..length]).unwrap() {
            Record::EventDocumentClosed(event) => {
                assert_eq!((event.document, event.reason), (document, 1))
            }
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn a_table_that_says_nothing_prepares_is_asked_nothing() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        assert!(!w.state.preparing());
        preparing(w, "books/a.epub");
        assert!(w.state.preparing());
        answer(w, READY, 1_000_000, 0);
        step(w);
        assert!(!w.state.preparing());
    });
}

/// The opener's view of the document after `steps` answers of `progress`.
fn run_steps(w: &World<'_>, steps: u32, progress: impl Fn(u32) -> u32) -> Option<(u32, u32, u32)> {
    let mut last = None;
    for at in 1..=steps {
        answer(w, PREPARING, progress(at), 0);
        assert_eq!(step(w), PrepareStep::More);
        while let Some(event) = changed_event(w) {
            last = Some(event);
        }
    }
    last
}

#[test]
fn a_format_that_answers_more_without_progress_32_times_fails_with_a_timeout() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = preparing(w, "books/a.epub");
        // 31 answers without rising above the start: still preparing.
        run_steps(w, 31, |_| 0);
        assert!(w.state.preparing());
        // The 32nd fails the document, as if the format had said so.
        answer(w, PREPARING, 0, 0);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(changed_event(w), Some((document, FAILED, 0)));
        assert!(!w.state.preparing());
        assert_eq!(step(w), PrepareStep::Idle);
    });
}

#[test]
fn progress_of_one_ppm_a_call_keeps_the_preparation_going_and_falling_back_counts() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = preparing(w, "books/a.epub");
        let last = run_steps(w, 100, |at| at);
        assert_eq!(last, Some((document, PREPARING, 100)));
        assert!(w.state.preparing());
        // Falling back below the best is no progress: 32 of them fail it.
        run_steps(w, 31, |at| 100 - at);
        assert!(w.state.preparing());
        answer(w, PREPARING, 0, 0);
        step(w);
        assert!(!w.state.preparing());
    });
}

#[test]
fn a_rise_starts_the_count_over() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        preparing(w, "books/a.epub");
        run_steps(w, 31, |_| 0);
        run_steps(w, 1, |_| 10);
        run_steps(w, 31, |_| 10);
        assert!(w.state.preparing(), "the count started over at the rise");
    });
}
