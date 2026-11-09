//! The review's decisions: which card comes, what each answer keeps, how a
//! card answered *Again* comes back, what is said when none is left; over a
//! storage in memory, a clock the test sets and the plugin's own English.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_i18n::{Arg as Message, Catalog, Translator};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;

use super::*;
use crate::cards::{Texts, add, add_deck};

struct English(Translator);

fn english() -> English {
    let mut translator = Translator::new();
    translator.add(Catalog::parse(include_str!("../i18n/en.ini")).unwrap());
    translator.set_language("en");
    English(translator)
}

impl Words for English {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        Ok(self.0.tr(key).to_string())
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let mut named: Vec<(&str, Message<'_>)> = Vec::new();
        let mut count = None;
        for arg in args {
            match *arg {
                Arg::Text { name, value } => named.push((name, Message::Str(value))),
                Arg::Int { name, value } => named.push((name, Message::Int(value))),
                Arg::Count(n) => count = Some(n),
                _ => {}
            }
        }
        Ok(match count {
            Some(n) => self.0.plural(key, n, &named),
            None => self.0.format(key, &named),
        })
    }
}

/// A clock the test moves; the local time is UTC.
struct Clock(Cell<i64>);

impl Time for Clock {
    fn now(&self) -> i64 {
        self.0.get()
    }
    fn local(&self, unix: i64) -> i64 {
        unix
    }
}

const SETTINGS: Settings = Settings {
    new_per_day: 20,
    retention: 90,
};
const NOON: i64 = 1_800_000_000 / 86_400 * 86_400 + 12 * 3600;

fn texts(front: &str, back: &str) -> Texts {
    Texts {
        front: front.into(),
        back: back.into(),
        example: String::new(),
    }
}

/// A deck with the cards given, all new.
fn deck(storage: &MemoryStorage, cards: &[(&str, &str)]) -> u32 {
    let id = add_deck(storage, "Words").unwrap();
    add(storage, id, cards.iter().map(|(f, b)| texts(f, b))).unwrap();
    id
}

fn start(storage: &MemoryStorage, clock: &Clock, deck: Option<u32>) -> Review {
    Review::new(storage, clock, &mut english(), SETTINGS, deck, None).unwrap()
}

fn front(review: &Review) -> &str {
    &review.current.as_ref().unwrap().1.front
}

#[test]
fn the_cards_come_one_by_one_and_each_answer_is_kept() {
    let storage = MemoryStorage::new();
    deck(&storage, &[("Hund", "dog"), ("Katze", "cat")]);
    let clock = Clock(Cell::new(NOON));
    let mut words = english();
    let mut review = start(&storage, &clock, None);
    assert_eq!(review.left(), 2);
    assert!(!review.revealed);
    // The back and the four waits come with the first step on.
    assert_eq!(review.reveal(&clock, &mut words).unwrap(), Move::Redraw);
    assert!(review.revealed);
    assert!(review.waits.iter().all(|wait| !wait.is_empty()));
    assert_eq!(review.reveal(&clock, &mut words).unwrap(), Move::Ignored);
    let first = front(&review).to_string();
    let answered = review
        .answer(&storage, &clock, &mut words, Grade::Good)
        .unwrap();
    assert!(answered.kept);
    assert_eq!(review.left(), 1);
    assert_ne!(front(&review), first);
    assert!(!review.revealed, "the next card shows its front first");
    review
        .answer(&storage, &clock, &mut words, Grade::Easy)
        .unwrap();
    assert!(review.current.is_none());
    assert_eq!(review.message, "Done: 2 cards reviewed.");
    // Kept at once: a new review finds nothing due now.
    let again = start(&storage, &clock, None);
    assert!(again.current.is_none());
    assert!(
        again.message.starts_with("Nothing to review now"),
        "{}",
        again.message
    );
}

#[test]
fn a_card_answered_again_comes_back_when_its_time_has_come() {
    let storage = MemoryStorage::new();
    deck(&storage, &[("Hund", "dog")]);
    let clock = Clock(Cell::new(NOON));
    let mut words = english();
    let mut review = start(&storage, &clock, None);
    review
        .answer(&storage, &clock, &mut words, Grade::Again)
        .unwrap();
    assert!(review.current.is_none());
    assert_eq!(review.message, "Next card in 10 min.");
    assert_eq!(review.left(), 1);
    // Too early: nothing changes.
    clock.0.set(NOON + 5 * 60);
    assert!(!review.tick(&storage, &clock, &mut words).unwrap());
    clock.0.set(NOON + 11 * 60);
    assert!(review.tick(&storage, &clock, &mut words).unwrap());
    assert_eq!(front(&review), "Hund");
    // A card shown is not replaced by a tick.
    assert!(!review.tick(&storage, &clock, &mut words).unwrap());
}

#[test]
fn the_buttons_move_the_focus_round_and_a_step_on_shows_the_back() {
    let storage = MemoryStorage::new();
    deck(&storage, &[("Hund", "dog")]);
    let clock = Clock(Cell::new(NOON));
    let mut words = english();
    let mut review = start(&storage, &clock, None);
    assert_eq!(review.step(&clock, &mut words, -1).unwrap(), Move::Ignored);
    assert_eq!(review.step(&clock, &mut words, 1).unwrap(), Move::Redraw);
    assert!(review.revealed);
    assert_eq!(review.focus, 2, "it starts on Good");
    review.step(&clock, &mut words, 1).unwrap();
    review.step(&clock, &mut words, 1).unwrap();
    assert_eq!(review.focus, 0, "Easy is followed by Again");
    review.step(&clock, &mut words, -1).unwrap();
    assert_eq!(review.focus, 3);
}

#[test]
fn a_deck_named_is_reviewed_alone_and_a_card_named_comes_first() {
    let storage = MemoryStorage::new();
    let one = deck(&storage, &[("eins", "one")]);
    let two = add_deck(&storage, "Others").unwrap();
    add(
        &storage,
        two,
        [texts("zwei", "two"), texts("drei", "three")],
    )
    .unwrap();
    let clock = Clock(Cell::new(NOON));
    assert_eq!(start(&storage, &clock, Some(one)).left(), 1);
    assert_eq!(start(&storage, &clock, Some(two)).left(), 2);
    let all = start(&storage, &clock, None);
    assert_eq!(all.left(), 3);
    // `drei` (id 2 of the deck `two`) first.
    let card = cards::due(&storage, NOON + 86_400, 20, 10, Some(two)).unwrap();
    let wanted = card.iter().find(|due| due.state.id == 2).unwrap();
    let review = Review::new(
        &storage,
        &clock,
        &mut english(),
        SETTINGS,
        None,
        Some((two, wanted.state.id)),
    )
    .unwrap();
    assert_eq!(front(&review), "drei");
    assert_eq!(review.left(), 3);
}

#[test]
fn new_cards_are_shown_as_many_as_the_day_allows() {
    let storage = MemoryStorage::new();
    deck(&storage, &[("a", "1"), ("b", "2"), ("c", "3")]);
    let clock = Clock(Cell::new(NOON));
    let few = Settings {
        new_per_day: 2,
        ..SETTINGS
    };
    let review = Review::new(&storage, &clock, &mut english(), few, None, None).unwrap();
    assert_eq!(review.left(), 2);
    assert_eq!(due_now(&storage, &clock, 2), 2);
    assert_eq!(due_now(&storage, &clock, 20), 3);
}

#[test]
fn what_is_said_where_no_card_is_left() {
    let storage = MemoryStorage::new();
    let clock = Clock(Cell::new(NOON));
    let review = start(&storage, &clock, None);
    assert!(review.current.is_none());
    assert!(
        review.message.starts_with("No cards to review"),
        "{}",
        review.message
    );
}

#[test]
fn a_wait_is_said_in_minutes_days_months_and_years() {
    let mut words = english();
    for (seconds, said) in [
        (0, "1 min"),
        (600, "10 min"),
        (3 * 86_400, "3 d"),
        (90 * 86_400, "3 mo"),
        (800 * 86_400, "2 y"),
    ] {
        assert_eq!(wait_text(&mut words, seconds).unwrap(), said);
    }
}

#[test]
fn the_hand_over_names_a_deck_and_a_card() {
    let ok = |pairs: &'static [(&'static str, &'static str)]| {
        named(pairs.iter().map(|(k, v)| Ok::<_, Error>((*k, *v))))
    };
    assert_eq!(ok(&[]).unwrap(), (None, None));
    assert_eq!(ok(&[("deck", "7")]).unwrap(), (Some(7), None));
    assert_eq!(
        ok(&[("first", "4 12"), ("other", "x")]).unwrap(),
        (None, Some((4, 12)))
    );
    // Anything else names nothing; a key given twice differently is refused.
    assert_eq!(ok(&[("deck", "seven")]).unwrap(), (None, None));
    assert_eq!(ok(&[("first", "4")]).unwrap(), (None, None));
    assert_eq!(ok(&[("deck", "1"), ("deck", "1")]).unwrap().0, Some(1));
    assert_eq!(ok(&[("deck", "1"), ("deck", "2")]), Err(Error::Invalid));
}
