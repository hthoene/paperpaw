//! The sleep card: the front of the card due next and how many are due, the
//! card kept for the review that opens on waking, and what is left of the last
//! sleep when nothing is due.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use paperpaw_sdk::MemoryStorage;

use super::*;
use crate::alloc_refusal_tests::Quiet;
use crate::cards::{Texts, add, add_deck};
use crate::sync::tests::heap::{armed, fixture};

/// A clock that stands still; the local time is UTC.
struct Noon;

impl Time for Noon {
    fn now(&self) -> i64 {
        NOON
    }
    fn local(&self, unix: i64) -> i64 {
        unix
    }
}

const NOON: i64 = 1_800_000_000 / 86_400 * 86_400 + 12 * 3600;

fn deck(storage: &MemoryStorage, fronts: &[&str]) -> u32 {
    let id = add_deck(storage, "Words").unwrap();
    let texts = fronts.iter().map(|front| Texts {
        front: (*front).into(),
        back: "back".into(),
        example: String::new(),
    });
    add(storage, id, texts).unwrap();
    id
}

#[test]
fn the_card_due_next_is_shown_and_kept_for_waking() {
    let storage = MemoryStorage::new();
    let id = deck(&storage, &["Hund", "Katze"]);
    let asleep = next(&storage, &Noon, 20).unwrap();
    assert_eq!(asleep.front.as_deref(), Some("Hund"));
    assert_eq!(asleep.due, 2);
    let (deck, _) = shown(&storage).unwrap();
    assert_eq!(deck, id);
}

#[test]
fn with_nothing_due_nothing_is_shown_and_the_last_card_is_forgotten() {
    let storage = MemoryStorage::new();
    deck(&storage, &["Hund"]);
    assert!(next(&storage, &Noon, 20).unwrap().front.is_some());
    assert!(shown(&storage).is_some());
    // No new card a day: nothing is due; what was kept is gone.
    let asleep = next(&storage, &Noon, 0).unwrap();
    assert_eq!(asleep, Asleep::default());
    assert!(shown(&storage).is_none());
}

#[test]
fn without_decks_there_is_nothing_to_show() {
    let storage = MemoryStorage::new();
    assert_eq!(next(&storage, &Noon, 20).unwrap(), Asleep::default());
}

#[test]
fn when_the_heap_refuses_nothing_panics() {
    let make = || {
        fixture(|| {
            let storage = Quiet(MemoryStorage::new());
            deck(&storage.0, &["Hund", "Katze"]);
            storage
        })
    };
    let kept = make();
    let (whole, count) = armed(usize::MAX, || next(&kept, &Noon, 20));
    assert!(whole.is_ok());
    assert!(count > 0);
    for at in 1..=count {
        let storage = make();
        let (result, _) = armed(at, || next(&storage, &Noon, 20));
        if let Err(error) = result {
            assert!(matches!(error, Error::NoMemory), "refusing allocation {at}");
        }
    }
}
