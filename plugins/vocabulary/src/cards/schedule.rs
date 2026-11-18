//! What is due, and how many cards a deck holds.
//! Part of [`cards`](super); moved out of it to keep files short.

use super::*;

/// A card due, as a review holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Due {
    pub deck: u32,
    /// Where its line begins in the deck.
    pub offset: u64,
    pub state: State,
}

/// The cards to review by `until` (of the deck `only`, or of all): at
/// most `most` due ones, those due longest first, then up to `new` new
/// ones, deck by deck. The answers given are folded in first. An error
/// where the heap refuses: nothing is shown half chosen.
pub(crate) fn due(
    storage: &dyn Storage,
    until: i64,
    new: usize,
    most: usize,
    only: Option<u32>,
) -> Result<Vec<Due>, StorageError> {
    let _ = fold(storage);
    let _ = rebuild_index(storage);
    let (mut reviews, mut fresh): (Vec<Due>, Vec<Due>) = (Vec::new(), Vec::new());
    let decks = decks(storage).unwrap_or_default();
    // Ties are broken as the stable sort did: by the deck's place in
    // `decks.txt` (not its id: a file edited by hand or synced may list
    // them in another order), then by where the card is in the deck.
    let position = |deck: u32| {
        decks
            .iter()
            .position(|d| d.id == deck)
            .unwrap_or(usize::MAX)
    };
    let mut failed = Ok(());
    for deck in decks.iter().filter(|d| only.is_none_or(|o| o == d.id)) {
        each_card(storage, deck.id, |offset, card| {
            let state = card.state;
            let due = Due {
                deck: deck.id,
                offset,
                state,
            };
            let room =
                |list: &mut Vec<Due>| list.try_reserve(1).map_err(|_| StorageError::TooLarge);
            if state.is_new() {
                if fresh.len() < new {
                    if let Err(e) = room(&mut fresh) {
                        failed = Err(e);
                        return ControlFlow::Break(());
                    }
                    fresh.push(due);
                }
            } else if state.due < until {
                if let Err(e) = room(&mut reviews) {
                    failed = Err(e);
                    return ControlFlow::Break(());
                }
                reviews.push(due);
                // Keep those due longest, without holding more than twice
                // as many at a time.
                if reviews.len() >= most * 2 {
                    paperpaw_sdk::sort::sort_by_key(&mut reviews, |d: &Due| {
                        (d.state.due, position(d.deck), d.offset)
                    });
                    reviews.truncate(most);
                }
            }
            ControlFlow::Continue(())
        })?;
        failed?;
    }
    paperpaw_sdk::sort::sort_by_key(&mut reviews, |d| (d.state.due, position(d.deck), d.offset));
    reviews.truncate(most);
    reviews
        .try_reserve(fresh.len())
        .map_err(|_| StorageError::TooLarge)?;
    reviews.extend(fresh);
    Ok(reviews)
}

/// The cards due by `until`, and the new ones to show today (at most
/// `new`), as the widget counts them: from the index and the answers
/// given since, without reading the decks.
pub(crate) fn due_count(
    storage: &dyn Storage,
    until: i64,
    new: usize,
) -> Result<usize, StorageError> {
    index_ready(storage);
    let mut reviews: i64 = 0;
    let mut fresh: i64 = 0;
    each_index(storage, |part| {
        fresh += i64::from(part.new);
        // A quarter hour all before `until`: every day begins on one.
        reviews += part
            .hours
            .iter()
            .filter(|(quarter, _)| (quarter + 1) * QUARTER <= until)
            .map(|(_, n)| i64::from(*n))
            .sum::<i64>();
    })?;
    for (state, before) in answered(storage)?.values() {
        match *before {
            0 => fresh -= 1,
            b if b < until => reviews -= 1,
            _ => {}
        }
        if state.due < until {
            reviews += 1;
        }
    }
    // Words looked up that become cards when the flash cards open.
    fresh += lookups_waiting(storage) as i64;
    Ok(reviews.max(0) as usize + (fresh.max(0) as usize).min(new))
}

/// When the next card that is not new comes due after `after`, if any
/// does (from the index: the answers given since are not looked at).
pub(crate) fn next_due(storage: &dyn Storage, after: i64) -> Option<i64> {
    index_ready(storage);
    let mut next: Option<i64> = None;
    let _ = each_index(storage, |part| {
        for (quarter, _) in &part.hours {
            let at = quarter * QUARTER;
            if at > after && next.is_none_or(|n| at < n) {
                next = Some(at);
            }
        }
    });
    next
}

/// How many cards each deck has, by deck (sorted by it).
#[derive(Debug, Default)]
pub(crate) struct Sizes(Vec<(u32, usize)>);

impl Sizes {
    /// The size of `deck`, if it has an entry.
    pub(crate) fn get(&self, deck: &u32) -> Option<&usize> {
        let at = self.0.binary_search_by_key(deck, |(id, _)| *id).ok()?;
        self.0.get(at).map(|(_, n)| n)
    }

    pub(super) fn add(&mut self, deck: u32, n: usize) -> Result<(), StorageError> {
        match self.0.binary_search_by_key(&deck, |(id, _)| *id) {
            Ok(at) => {
                if let Some((_, total)) = self.0.get_mut(at) {
                    *total += n;
                }
            }
            Err(at) => {
                self.0.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
                self.0.insert(at, (deck, n));
            }
        }
        Ok(())
    }
}

/// How many cards each deck has, by deck.
pub(crate) fn sizes(storage: &dyn Storage) -> Result<Sizes, StorageError> {
    index_ready(storage);
    let mut sizes = Sizes::default();
    let mut failed = Ok(());
    each_index(storage, |part| {
        if failed.is_err() {
            return;
        }
        let others: u32 = part.hours.iter().map(|(_, n)| n).sum();
        failed = sizes.add(part.deck, (part.new + others) as usize);
    })?;
    failed?;
    Ok(sizes)
}
