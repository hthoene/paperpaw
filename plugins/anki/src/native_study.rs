//! The study: the cards of the last sync one after the other, the front, then
//! the back and the four answers (*Again*, *Hard*, *Good*, *Easy*). An answer
//! is written to the queue (`answers.txt`) at once and goes to Anki with the
//! next sync; Anki's own schedule decides what comes back, so a card answered
//! *Again* is asked for at the next sync, not again today on the reader.
//!
//! Only one card is in memory: the places of the lines are kept and the card
//! is read when it is shown.
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::files::Volume;

use crate::native_anki::Card;
use crate::native_cards::{self as store, Place};

/// The answers' texts (catalog keys), by ease 1 to 4.
pub const ANSWER_KEYS: [&str; 4] = ["anki.again", "anki.hard", "anki.good", "anki.easy"];

/// Why there is no card to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Empty {
    /// There are none: sync first.
    NoCards,
    /// All are answered.
    AllDone,
}

/// A study under way.
#[derive(Debug)]
pub struct Study {
    places: Vec<Place>,
    /// Ids of the cards answered (sorted), kept to skip them.
    done: Vec<u64>,
    /// The index in `places` of the card shown.
    at: usize,
    pub current: Option<Card>,
    pub revealed: bool,
    /// The answer in focus, 0 to 3.
    pub focus: usize,
    pub empty: Empty,
}

impl Study {
    /// The study of the cards on the reader.
    ///
    /// # Errors
    /// `NoMemory`; what the card says.
    pub fn open(volume: &dyn Volume) -> Result<Self, Error> {
        let mut study = Self {
            places: store::places(volume)?,
            done: store::answered(volume)?,
            at: 0,
            current: None,
            revealed: false,
            focus: 2,
            empty: Empty::NoCards,
        };
        study.empty = if study.places.is_empty() {
            Empty::NoCards
        } else {
            Empty::AllDone
        };
        study.next(volume)?;
        Ok(study)
    }

    /// How many cards are left, the one shown included.
    #[must_use]
    pub fn left(&self) -> usize {
        if self.current.is_some() {
            self.places.len().saturating_sub(self.at)
        } else {
            0
        }
    }

    /// Shows the first card from `at` on that is not answered.
    fn next(&mut self, volume: &dyn Volume) -> Result<(), Error> {
        self.current = None;
        self.revealed = false;
        self.focus = 2;
        while let Some(place) = self.places.get(self.at).copied() {
            match store::read(volume, place)? {
                Some(card) if self.done.binary_search(&card.id).is_err() => {
                    self.current = Some(card);
                    return Ok(());
                }
                _ => self.at += 1,
            }
        }
        Ok(())
    }

    /// Shows the back and the answers.
    pub fn reveal(&mut self) -> bool {
        if self.current.is_none() || self.revealed {
            return false;
        }
        self.revealed = true;
        true
    }

    /// Moves the focus among the answers by `by`.
    pub fn step(&mut self, by: isize) -> bool {
        if !self.revealed {
            return false;
        }
        let to = self.focus.saturating_add_signed(by).min(3);
        let changed = to != self.focus;
        self.focus = to;
        changed
    }

    /// Answers the card shown with ease `ease` (1 to 4) and shows the next.
    ///
    /// # Errors
    /// `NoSpace` where the queue is full (the card stays); `NoMemory`; what
    /// the card says.
    pub fn answer(&mut self, volume: &dyn Volume, ease: u8) -> Result<(), Error> {
        let Some(card) = self.current.as_ref() else {
            return Ok(());
        };
        let id = card.id;
        store::record(volume, id, ease)?;
        let at = self.done.partition_point(|done| *done < id);
        self.done.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.done.insert(at, id);
        self.at += 1;
        self.next(volume)
    }

    /// The text to show where there is no card.
    #[must_use]
    pub fn message(&self) -> &'static str {
        match self.empty {
            Empty::NoCards => "anki.study.none",
            Empty::AllDone => "anki.study.done",
        }
    }
}

/// How many cards wait to be studied: those of the last sync less those
/// answered (every answer waiting is of a card of the last sync, as a sync
/// that sent answers drops the cards).
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn ready(volume: &dyn Volume) -> Result<usize, Error> {
    let cards = store::places(volume)?.len();
    if cards == 0 {
        return Ok(0);
    }
    Ok(cards.saturating_sub(store::answered(volume)?.len()))
}
