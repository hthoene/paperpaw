//! Answers kept in a log and folded into the cards.
//! Part of [`cards`](super); moved out of it to keep files short.

use super::*;

/// Keeps the answer that made a card of `deck` `after`; it was due at
/// `before` (0 if new).
pub(crate) fn record(
    storage: &dyn Storage,
    deck: u32,
    before: i64,
    after: &State,
) -> Result<(), StorageError> {
    let mut line = String::new();
    push_number(&mut line, deck)?;
    push(&mut line, "\t")?;
    after.push_fields(&mut line)?;
    push(&mut line, "\t")?;
    push_signed(&mut line, before)?;
    push(&mut line, "\n")?;
    storage.append(ANSWERS, line.as_bytes())?;
    remember_clock(storage, after.last);
    Ok(())
}

/// Each card's last state, and when it was due before, by deck and id
/// (sorted by them).
#[derive(Debug, Default)]
pub(super) struct Answers(Vec<((u32, u32), (State, i64))>);

impl Answers {
    pub(super) fn get(&self, deck: u32, id: u32) -> Option<&(State, i64)> {
        let at = self
            .0
            .binary_search_by_key(&(deck, id), |(key, _)| *key)
            .ok()?;
        self.0.get(at).map(|(_, found)| found)
    }

    /// The last state of the card and when it was due before the first
    /// answer: `state` and `before` where it is the first.
    ///
    /// A sorted `Vec` takes an insert in linear time, so folding `n` answers
    /// of different cards is quadratic in `n` (a memmove of 56 B entries):
    /// fine for the few hundred answers of a review or a day (`answers.txt`
    /// is folded when a review ends and before any change of a deck), but
    /// not a design for tens of thousands.
    pub(super) fn note(
        &mut self,
        deck: u32,
        state: State,
        before: i64,
    ) -> Result<(), StorageError> {
        match self
            .0
            .binary_search_by_key(&(deck, state.id), |(key, _)| *key)
        {
            Ok(at) => {
                if let Some((_, (last, _))) = self.0.get_mut(at) {
                    *last = state;
                }
            }
            Err(at) => {
                self.0.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
                self.0.insert(at, ((deck, state.id), (state, before)));
            }
        }
        Ok(())
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &(State, i64)> {
        self.0.iter().map(|(_, found)| found)
    }
}

/// The answers given since the decks were written: each card's last
/// state, and when it was due before the first of them, by deck and id.
/// An error where they could not be read to their end.
pub(super) fn answered(storage: &dyn Storage) -> Result<Answers, StorageError> {
    let mut found = Answers::default();
    let mut failed = Ok(());
    each_line(storage, ANSWERS, |_, line| {
        let mut fields = line.split('\t');
        let parsed = (|| {
            let deck: u32 = fields.next()?.parse().ok()?;
            let state = State::parse(&mut fields)?;
            let before: i64 = fields.next()?.parse().ok()?;
            Some((deck, state, before))
        })();
        if let Some((deck, state, before)) = parsed
            && let Err(e) = found.note(deck, state, before)
        {
            failed = Err(e);
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    })?;
    failed?;
    Ok(found)
}

/// Writes the answers given into their decks. Answers that cannot be
/// read are kept for the next time.
pub(crate) fn fold(storage: &dyn Storage) -> Result<(), StorageError> {
    if !storage.exists(ANSWERS) {
        return Ok(());
    }
    let answered = answered(storage)?;
    // The entries are sorted by deck: each deck once, in turn.
    let mut done: Option<u32> = None;
    for ((deck, _), _) in &answered.0 {
        let deck = *deck;
        if done == Some(deck) {
            continue;
        }
        done = Some(deck);
        let result = rewrite(storage, deck, |card| {
            if let Some((state, _)) = answered.get(deck, card.state.id) {
                card.state = *state;
            }
            true
        });
        match result {
            Ok(()) | Err(Problem::Missing) => {}
            Err(Problem::Storage(e)) => return Err(e),
            Err(_) => {}
        }
    }
    storage.remove(ANSWERS)
}
