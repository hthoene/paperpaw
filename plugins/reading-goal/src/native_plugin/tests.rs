//! Which events mean what, and when the figure is written.

use paperpaw_sdk::native::events::Event;

use paperpaw_sdk::native::Error;

use super::{EVENTS, Edge, edge, figure};
use crate::model::Kept;

fn told(day: i32, seconds: u32) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&day.to_le_bytes());
    bytes[4..].copy_from_slice(&seconds.to_le_bytes());
    bytes
}

#[test]
fn a_reply_is_a_figure_only_if_it_succeeded_and_is_exactly_a_day_and_the_seconds() {
    assert_eq!(figure(Ok(()), &told(20_000, 750)), Some((20_000, 750)));
    assert_eq!(figure(Ok(()), &told(-3, u32::MAX)), Some((-3, u32::MAX)));
    // The statistics not running yet, not knowing the day, busy: no figure,
    // and the last one is kept.
    for error in [Error::Cancelled, Error::NotFound, Error::Busy] {
        assert_eq!(figure(Err(error), &[]), None, "{error:?}");
        assert_eq!(figure(Err(error), &told(1, 2)), None, "{error:?}");
    }
    for len in [0, 1, 4, 7] {
        assert_eq!(figure(Ok(()), &told(1, 2)[..len]), None, "{len}");
    }
    assert_eq!(figure(Ok(()), &[0; 9]), None);
}

#[test]
fn no_figure_leaves_what_was_kept_and_the_next_answer_replaces_it() {
    let mut kept = Kept::new();
    assert!(kept.apply(Edge::Told(20_000, 60)));
    // The statistics did not answer at the next edge: nothing is applied.
    assert_eq!(kept.to_write(Some((20_000, 60))), None);
    assert!(kept.apply(Edge::Told(20_000, 120)));
    assert_eq!(kept.to_write(Some((20_000, 60))), Some((20_000, 120)));
}

#[test]
fn other_events_and_tokens_are_not_edges() {
    assert_eq!(edge(EVENTS, &Event::Resume { now_ms: 1 }), None);
    assert_eq!(edge(7, &Event::Suspend { now_ms: 1 }), None);
}

#[test]
fn the_device_sleeping_or_stopping_is_an_edge() {
    assert_eq!(
        edge(EVENTS, &Event::Suspend { now_ms: 1 }),
        Some(Edge::Sleeping)
    );
    assert_eq!(
        edge(EVENTS, &Event::Stopping { now_ms: 1 }),
        Some(Edge::Sleeping)
    );
}

#[test]
fn it_is_written_when_no_book_is_open_when_one_closes_and_when_the_device_sleeps() {
    let mut kept = Kept::new();
    // Nothing told: sleeping still says to write (the file decides).
    assert!(kept.apply(Edge::Sleeping));
    // Answered with no book open: now.
    assert!(kept.apply(Edge::Told(20_000, 60)));
    // A book is open: not at every page turned.
    assert!(!kept.apply(Edge::BookOpened));
    assert!(!kept.apply(Edge::Told(20_000, 120)));
    assert!(!kept.apply(Edge::Told(20_000, 180)));
    // Asleep or stopped with the book open: now.
    assert!(kept.apply(Edge::Sleeping));
    // The book closes: now, with what was told last.
    assert!(kept.apply(Edge::BookClosed));
    assert_eq!(kept.to_write(None), Some((20_000, 180)));
    assert!(kept.apply(Edge::Told(20_000, 240)));
}
