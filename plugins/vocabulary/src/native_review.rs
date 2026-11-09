//! Reviewing flash cards on interface 7 (interface 6: `review.rs`): the cards
//! due, one at a time. The front first; *Select* or a tap shows the back and
//! the example and four answers, *Again*, *Hard*, *Good*, *Easy*, each with
//! when the card would come again. With buttons the focus starts on *Good*
//! and goes left and right, *Select* answers; by touch a tap does.
//!
//! A card answered *Again* comes again in this review once its time has come
//! (the 10 minutes its button says), before the next card due; while only such
//! cards are left the screen says when the next comes and shows it then (at
//! the next minute tick, or later from home). Each answer is kept at once
//! ([`cards::record`]); the decks are written with them when the review closes.
//!
//! This file is the state and the decisions, over a storage, a clock and the
//! words, so it is made and tested without a host; `native_review_screen`
//! draws it and takes the input.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_ui_kit::mem::push;
use paperpaw_ui_kit::text::copy;

use crate::cards::{self, Card, Day, Due};
use crate::fsrs::{Fx, Grade};
use crate::native_words::Words;

/// The screen's id (relative to the plugin: `review`).
pub(crate) const SCREEN: &str = "vocabulary.review";
/// The hand-over key naming the deck to review, else all are.
pub(crate) const DECK: &str = "deck";
/// The hand-over key naming the card to show first, `<deck> <id>`.
pub(crate) const FIRST: &str = "first";
/// Cards due taken into a review at most.
const MOST: usize = 200;
/// The answer the focus starts on.
const GOOD: usize = 2;

/// The keys of the four answers' words.
pub(crate) const ANSWER_KEYS: [&str; 4] = [
    "vocabulary.review.again",
    "vocabulary.review.hard",
    "vocabulary.review.good",
    "vocabulary.review.easy",
];

/// The clock: the time now and the local time of a moment.
pub(crate) trait Time {
    /// Seconds since 1970 now (the last one known where the device has none).
    fn now(&self) -> i64;
    /// The local time of the moment `unix`, in seconds.
    fn local(&self, unix: i64) -> i64;
}

fn day(time: &impl Time, at: i64) -> Day {
    Day::of(at, time.local(at))
}

/// The cards due now, and new ones to show today, as the screens count them.
pub(crate) fn due_now(storage: &dyn Storage, time: &impl Time, new_per_day: u32) -> usize {
    let today = day(time, time.now());
    let new = cards::new_left(storage, today, new_per_day);
    cards::due_count(storage, today.end, new).unwrap_or(0)
}

/// What the screen does about an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Move {
    /// Not for the review.
    Ignored,
    /// The screen goes back.
    Back,
    /// Something changed: draw again.
    Redraw,
}

/// A review under way.
#[derive(Debug)]
pub(crate) struct Review {
    /// The cards due, the next one at `at`.
    queue: Vec<Due>,
    at: usize,
    /// Cards answered *Again*, until their time has come.
    waiting: Vec<Due>,
    /// The card shown and its texts.
    pub(crate) current: Option<(Due, Card)>,
    pub(crate) revealed: bool,
    /// The answer chosen with the buttons.
    pub(crate) focus: usize,
    answered: usize,
    retention: Fx,
    /// When the card shown would come again after each answer, once its back
    /// shows.
    pub(crate) waits: [String; 4],
    /// What the screen says once no card is left.
    pub(crate) message: String,
}

/// The settings a review reads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    /// New cards a day (0 to 1,000).
    pub new_per_day: u32,
    /// The retention aimed at, in percent (70 to 99).
    pub retention: i64,
}

impl Review {
    /// The review of `deck` (else of all), with the card named by `first` (the
    /// one shown asleep) at the front if it is among those due.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what reading the cards says as [`Error::NoMemory`]
    /// (nothing is shown half chosen).
    pub(crate) fn new(
        storage: &dyn Storage,
        time: &impl Time,
        words: &mut impl Words,
        settings: Settings,
        deck: Option<u32>,
        first: Option<(u32, u32)>,
    ) -> Result<Self, Error> {
        let now = time.now();
        let today = day(time, now);
        let new = cards::new_left(storage, today, settings.new_per_day);
        let mut queue =
            cards::due(storage, today.end, new, MOST, deck).map_err(|_| Error::NoMemory)?;
        if let Some((deck, id)) = first
            && let Some(at) = queue
                .iter()
                .position(|due| due.deck == deck && due.state.id == id)
        {
            let card = queue.remove(at);
            queue.insert(0, card);
        }
        let mut review = Self {
            queue,
            at: 0,
            waiting: Vec::new(),
            current: None,
            revealed: false,
            focus: GOOD,
            answered: 0,
            retention: Fx::hundredths(settings.retention.clamp(70, 99)),
            waits: Default::default(),
            message: String::new(),
        };
        review.next(storage, time, words)?;
        Ok(review)
    }

    /// The cards left, the one shown included.
    pub(crate) fn left(&self) -> usize {
        self.queue.len() - self.at + self.waiting.len() + usize::from(self.current.is_some())
    }

    /// Shows the next card, if any is left: one answered *Again* whose time has
    /// come, else the next due. A card deleted or changed since is left out.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn next(
        &mut self,
        storage: &dyn Storage,
        time: &impl Time,
        words: &mut impl Words,
    ) -> Result<(), Error> {
        self.revealed = false;
        self.focus = GOOD;
        self.current = None;
        let now = time.now();
        let ready = self
            .waiting
            .iter()
            .enumerate()
            .filter(|(_, due)| due.state.due <= now)
            .min_by_key(|(_, due)| due.state.due)
            .map(|(at, _)| at)
            .map(|at| self.waiting.remove(at));
        let mut candidates = ready.into_iter();
        loop {
            let due = match candidates.next() {
                Some(due) => due,
                None => match self.queue.get(self.at) {
                    Some(due) => {
                        self.at += 1;
                        *due
                    }
                    None => break,
                },
            };
            if let Some(card) = cards::card_at(storage, due.deck, due.offset)
                && card.state.id == due.state.id
            {
                self.current = Some((due, card));
                return Ok(());
            }
        }
        self.message = self.nothing_left(storage, time, words, now)?;
        Ok(())
    }

    /// What the screen says where no card is left.
    fn nothing_left(
        &self,
        storage: &dyn Storage,
        time: &impl Time,
        words: &mut impl Words,
        now: i64,
    ) -> Result<String, Error> {
        if let Some(at) = self.soonest() {
            let wait = wait_text(words, at - now)?;
            words.format(
                "vocabulary.review.next-in",
                &[Arg::Text {
                    name: "wait",
                    value: &wait,
                }],
            )
        } else if self.answered > 0 {
            words.format(
                "vocabulary.review.done",
                &[Arg::Count(self.answered as i64)],
            )
        } else {
            let now = time.now();
            match cards::next_due(storage, now) {
                Some(at) => {
                    let wait = wait_text(words, at - now)?;
                    words.format(
                        "vocabulary.review.nothing-until",
                        &[Arg::Text {
                            name: "wait",
                            value: &wait,
                        }],
                    )
                }
                None => words.copy("vocabulary.review.nothing"),
            }
        }
    }

    /// When the first card answered *Again* is due, if one waits.
    pub(crate) fn soonest(&self) -> Option<i64> {
        self.waiting.iter().map(|due| due.state.due).min()
    }

    /// The minute tick: where only cards answered *Again* are left and one is
    /// due now, it is shown. `true` where the screen changed.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn tick(
        &mut self,
        storage: &dyn Storage,
        time: &impl Time,
        words: &mut impl Words,
    ) -> Result<bool, Error> {
        if self.current.is_some() {
            return Ok(false);
        }
        self.next(storage, time, words)?;
        Ok(self.current.is_some())
    }

    /// Shows the back and the four answers.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn reveal(
        &mut self,
        time: &impl Time,
        words: &mut impl Words,
    ) -> Result<Move, Error> {
        if self.current.is_none() || self.revealed {
            return Ok(Move::Ignored);
        }
        self.waits = self.waits_of(time, words)?;
        self.revealed = true;
        Ok(Move::Redraw)
    }

    /// When the card shown would come again after each answer, as people say it.
    fn waits_of(&self, time: &impl Time, words: &mut impl Words) -> Result<[String; 4], Error> {
        let mut waits: [String; 4] = Default::default();
        let Some((due, _)) = &self.current else {
            return Ok(waits);
        };
        let now = time.now();
        for (grade, wait) in Grade::ALL.into_iter().zip(&mut waits) {
            let after = due
                .state
                .answered(grade, now, same_day(time, due, now), self.retention);
            *wait = wait_text(words, after.due - now)?;
        }
        Ok(waits)
    }

    /// Answers the card shown with `grade`: kept at once, and the next card
    /// shown. `Err(true)` where the answer could not be kept (said by the
    /// caller; the review goes on), `Err(false)` for a refused allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn answer(
        &mut self,
        storage: &dyn Storage,
        time: &impl Time,
        words: &mut impl Words,
        grade: Grade,
    ) -> Result<Answered, Error> {
        let Some((due, _)) = self.current.take() else {
            return Ok(Answered { kept: true });
        };
        let now = time.now();
        let after = due
            .state
            .answered(grade, now, same_day(time, &due, now), self.retention);
        let kept = cards::record(storage, due.deck, due.state.due, &after).is_ok();
        if due.state.is_new() {
            cards::new_shown(storage, day(time, now), now);
        }
        self.answered += 1;
        if grade == Grade::Again {
            push(
                &mut self.waiting,
                Due {
                    state: after,
                    ..due
                },
            )?;
        }
        self.next(storage, time, words)?;
        Ok(Answered { kept })
    }

    /// Moves the focus of the answers by `step` (shown: left and right);
    /// where the back is not shown yet a step forward shows it.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn step(
        &mut self,
        time: &impl Time,
        words: &mut impl Words,
        step: isize,
    ) -> Result<Move, Error> {
        if self.current.is_none() {
            return Ok(Move::Ignored);
        }
        if self.revealed {
            self.focus = (self.focus as isize + step).rem_euclid(4) as usize;
            Ok(Move::Redraw)
        } else if step > 0 {
            self.reveal(time, words)
        } else {
            Ok(Move::Ignored)
        }
    }

    /// The answers go into the decks, once, as the review ends.
    pub(crate) fn close(&self, storage: &dyn Storage) {
        let _ = cards::fold(storage);
    }
}

/// What came of an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Answered {
    /// Whether it was kept; where not, the screen says so.
    pub kept: bool,
}

/// Whether `due` was last answered on the local day `now` is in.
fn same_day(time: &impl Time, due: &Due, now: i64) -> bool {
    !due.state.is_new() && day(time, due.state.last).number == day(time, now).number
}

/// `seconds` as a wait: `10 min`, `3 d`, `5 mo`, `2 y`.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn wait_text(words: &mut impl Words, seconds: i64) -> Result<String, Error> {
    let minutes = seconds.max(60) / 60;
    let days = seconds / (24 * 3600);
    let (key, n) = match days {
        0 => ("vocabulary.wait.minutes", minutes),
        d if d < 60 => ("vocabulary.wait.days", d),
        d if d < 365 => ("vocabulary.wait.months", d / 30),
        d => ("vocabulary.wait.years", d / 365),
    };
    words.format(
        key,
        &[Arg::Int {
            name: "n",
            value: n,
        }],
    )
}

/// The deck and the first card a hand-over names: `deck` a number, `first`
/// `<deck> <id>`; anything else is not named.
///
/// # Errors
/// [`Error::Invalid`] for a key given twice differently.
pub(crate) fn named<'a>(
    params: impl Iterator<Item = Result<(&'a str, &'a str), Error>>,
) -> Result<(Option<u32>, Option<(u32, u32)>), Error> {
    let (mut deck, mut first): (Option<&str>, Option<&str>) = (None, None);
    for param in params {
        let (key, value) = param?;
        let seen = match key {
            DECK => &mut deck,
            FIRST => &mut first,
            _ => continue,
        };
        if seen.is_some_and(|seen| seen != value) {
            return Err(Error::Invalid);
        }
        *seen = Some(value);
    }
    let deck = deck.and_then(|deck| deck.parse().ok());
    let first = first.and_then(|first| {
        let (deck, id) = first.split_once(' ')?;
        Some((deck.parse().ok()?, id.parse().ok()?))
    });
    Ok((deck, first))
}

/// An owned copy of a card's text for drawing.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn text_of(card: &Card) -> Result<(String, String, String), Error> {
    Ok((copy(&card.front)?, copy(&card.back)?, copy(&card.example)?))
}

#[cfg(test)]
#[path = "native_review_tests.rs"]
mod tests;
