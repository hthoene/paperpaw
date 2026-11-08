//! Which events mean what, and when the figure is written.

use paperpaw_sdk::native::events::Event;

use super::{EVENTS, Edge, TODAY, edge};
use crate::model::Kept;

fn stage(schema: u32, payload: &[u8]) -> Event<'_> {
    Event::StageCall {
        stage: "stats.read-today",
        schema,
        payload,
    }
}

fn told(day: i32, seconds: u32) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&day.to_le_bytes());
    bytes[4..].copy_from_slice(&seconds.to_le_bytes());
    bytes
}

#[test]
fn the_statistics_tell_a_day_and_the_seconds() {
    assert_eq!(
        edge(TODAY, &stage(1, &told(20_000, 750))),
        Some(Edge::Told(20_000, 750))
    );
    assert_eq!(
        edge(TODAY, &stage(1, &told(-3, u32::MAX))),
        Some(Edge::Told(-3, u32::MAX))
    );
}

#[test]
fn what_is_not_exactly_that_is_nothing() {
    // Another schema, a short or a long payload, none at all.
    assert_eq!(edge(TODAY, &stage(2, &told(1, 2))), None);
    assert_eq!(edge(TODAY, &stage(1, &told(1, 2)[..7])), None);
    assert_eq!(edge(TODAY, &stage(1, &[0; 9])), None);
    assert_eq!(edge(TODAY, &stage(1, &[])), None);
    // The stage at the other token, or the other events at the stage's.
    assert_eq!(edge(EVENTS, &stage(1, &told(1, 2))), None);
    assert_eq!(edge(TODAY, &Event::Suspend { now_ms: 1 }), None);
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
    // Told with no book open: now.
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
