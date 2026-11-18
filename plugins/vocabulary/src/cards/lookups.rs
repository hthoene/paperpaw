//! Words looked up and waiting to become cards.
//! Part of [`cards`](super); moved out of it to keep files short.

use super::*;

/// The longest the list of words waiting to become cards grows.
pub(super) const MAX_LOOKUPS: u64 = 64 * 1024;
/// Words taken from that list into the deck at a time.
pub(super) const LOOKUPS_AT_ONCE: usize = 64;

/// Keeps `word` looked up in `sentence`, to become a card when the flash
/// cards open next: a line appended, nothing read.
pub(crate) fn queue_lookup(
    storage: &dyn Storage,
    word: &str,
    sentence: &str,
) -> Result<(), StorageError> {
    if storage.size(LOOKUPS).unwrap_or(0) >= MAX_LOOKUPS {
        return Ok(());
    }
    let word = clean(word, MAX_TEXT)?;
    if word.is_empty() {
        return Ok(());
    }
    let mut line = String::new();
    push_escaped(&mut line, &word)?;
    push(&mut line, "\t")?;
    push_escaped(&mut line, &clean(sentence, MAX_TEXT)?)?;
    push(&mut line, "\n")?;
    storage.append(LOOKUPS, line.as_bytes())
}

/// How many words wait to become cards (some may be there already).
pub(super) fn lookups_waiting(storage: &dyn Storage) -> usize {
    let mut n = 0;
    let _ = each_line(storage, LOOKUPS, |_, line| {
        n += usize::from(!line.is_empty());
        ControlFlow::Continue(())
    });
    n
}

/// The words looked up as cards of the deck `deck_name` (made where there
/// is none), each front once, its sentence the example; a part at a time,
/// one pass over the deck each. How many became cards.
pub(crate) fn take_lookups(storage: &dyn Storage, deck_name: &str) -> Result<usize, Problem> {
    let mut added = 0;
    while storage.exists(LOOKUPS) {
        // A part of the list, each word once.
        let mut part: Vec<Texts> = Vec::new();
        let mut rest = String::new();
        let mut failed = Ok(());
        each_line(storage, LOOKUPS, |_, line| {
            let mut take = || -> Result<(), StorageError> {
                if part.len() >= LOOKUPS_AT_ONCE {
                    push(&mut rest, line)?;
                    return push(&mut rest, "\n");
                }
                let (word, sentence) = line.split_once('\t').unwrap_or((line, ""));
                let front = unescape(word)?;
                if !front.is_empty() && !part.iter().any(|t| same_folded(&t.front, &front)) {
                    part.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
                    part.push(Texts {
                        front,
                        back: String::new(),
                        example: unescape(sentence)?,
                    });
                }
                Ok(())
            };
            match take() {
                Ok(()) => ControlFlow::Continue(()),
                Err(e) => {
                    failed = Err(e);
                    ControlFlow::Break(())
                }
            }
        })?;
        failed?;
        let deck = deck_named(storage, deck_name)?;
        // Those the deck has already are left out, seen in the pass that
        // counts its cards.
        let mut there: Vec<bool> = Vec::new();
        there
            .try_reserve_exact(part.len())
            .map_err(|_| StorageError::TooLarge)?;
        there.resize(part.len(), false);
        let mut adder = Adder::seeing(storage, deck, |card| {
            if let Some(at) = part.iter().position(|t| same_folded(&t.front, &card.front))
                && let Some(slot) = there.get_mut(at)
            {
                *slot = true;
            }
        })?;
        for (texts, there) in part.into_iter().zip(there) {
            if !there && !adder.push(texts)? {
                break;
            }
        }
        added += adder.finish()?;
        if rest.is_empty() {
            storage.remove(LOOKUPS)?;
        } else {
            write_whole(storage, LOOKUPS, rest.as_bytes())?;
        }
    }
    Ok(added)
}
