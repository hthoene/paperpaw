//! How a file is merged into a deck: matching its lines to the deck's cards,
//! writing the slices, and the failures of a step.
//! Part of [`sync`](super); moved out of it to keep files short.

use super::*;

/// The deck an answer names: that of the file's name, or a new one beside
/// it. The inner error is the deck list being full.
pub(super) fn deck_for_answer(
    storage: &dyn Storage,
    file: &str,
    listed: &[String],
    link: bool,
) -> Result<Result<u32, Problem>, StorageError> {
    let name = deck_name_among(file, listed)?;
    Ok(if link {
        cards::deck_named(storage, &name)
    } else {
        let mut other = copy(trimmed_cut(&name, MAX_NAME - 4))?;
        push(&mut other, " (2)")?;
        cards::add_deck(storage, &other)
    })
}

/// Room for `count` of a kind in one allocation, or a storage failure: a
/// refused allocation is not the file's fault (`Crowded` is, and is never
/// tried again), so the file is `Unreadable` and tried again next time.
pub(super) fn room<T>(count: usize) -> Result<Vec<T>, Failure> {
    let mut vec = Vec::new();
    vec.try_reserve_exact(count).map_err(|_| Failure::Storage)?;
    Ok(vec)
}

/// The deck's cards of the slice that no line has yet, by key. At most
/// [`CROWD`] of them: a deck whose cards share a front fills a slice.
pub(super) fn deck_entries(
    storage: &dyn Storage,
    work: &Work,
    by_front: bool,
    in_slice: &dyn Fn(u64) -> bool,
) -> Result<Vec<Entry>, Failure> {
    let most = CROWD.min(work.cards);
    let mut entries: Vec<Entry> = room(most)?;
    let mut order = 0usize;
    let mut crowded = false;
    cards::each_card(storage, work.deck, |_, card| {
        let this = order;
        order += 1;
        if work.matched_cards.get(this) {
            return ControlFlow::Continue(());
        }
        let key = if by_front {
            front_key(&card.front)
        } else {
            u64::from(card.state.id)
        };
        if !in_slice(key) {
            return ControlFlow::Continue(());
        }
        if entries.len() >= most {
            crowded = true;
            return ControlFlow::Break(());
        }
        let text = if by_front {
            texts_hash(&[&card.back, &card.example])
        } else {
            texts_hash(&[&card.front, &card.back, &card.example])
        };
        entries.push(Entry {
            key,
            id: card.state.id,
            order: this as u32,
            text,
        });
        ControlFlow::Continue(())
    })?;
    if crowded {
        return Err(Failure::Outcome(Outcome::Crowded));
    }
    entries.sort_unstable_by_key(|e| (e.key, e.order));
    Ok(entries)
}

/// The file's lines of the slice: matched with a card of `entries`, or new.
/// What is to be written, and how many lines were the same as their card.
pub(super) fn match_lines(
    storage: &dyn Storage,
    work: &mut Work,
    entries: Vec<Entry>,
    by_front: bool,
    in_slice: &dyn Fn(u64) -> bool,
) -> Result<(Vec<Edit>, usize), Failure> {
    let mut used = Bits::new(entries.len())?;
    let most = CROWD.min(work.lines);
    let mut edits: Vec<Edit> = room(most)?;
    let (mut same, mut crowded, mut index) = (0usize, false, 0usize);
    table::each(storage, &work.path, &work.layout, None, |item| {
        let Item::Record(record) = item else {
            return ControlFlow::Continue(());
        };
        let this = index;
        index += 1;
        if work.matched_lines.get(this) {
            return ControlFlow::Continue(());
        }
        // The key is made of the text the card will have.
        let key = if by_front {
            Some(front_key(trimmed_cut(&record.front, MAX_TEXT)))
        } else {
            record.id.map(u64::from)
        };
        let Some(key) = key.filter(|k| in_slice(*k)) else {
            return ControlFlow::Continue(());
        };
        let offset = u32::try_from(record.offset).unwrap_or(u32::MAX);
        let start = entries.partition_point(|e| e.key < key);
        let hit = (start..entries.len())
            .take_while(|&i| entries.get(i).is_some_and(|e| e.key == key))
            .find(|&i| !used.get(i));
        let entry = hit.and_then(|i| entries.get(i));
        if let (Some(i), Some(entry)) = (hit, entry) {
            used.set(i);
            work.matched_lines.set(this);
            work.matched_cards.set(entry.order as usize);
            let text = if by_front {
                texts_hash(&[&record.back, &record.example])
            } else {
                texts_hash(&[&record.front, &record.back, &record.example])
            };
            if text == entry.text {
                same += 1;
            } else if edits.len() < most {
                edits.push(Edit {
                    id: entry.id,
                    offset,
                    add: false,
                });
            } else {
                crowded = true;
                return ControlFlow::Break(());
            }
        } else if by_front {
            work.matched_lines.set(this);
            if edits.len() >= most {
                crowded = true;
                return ControlFlow::Break(());
            }
            edits.push(Edit {
                id: 0,
                offset,
                add: true,
            });
        }
        ControlFlow::Continue(())
    })?;
    if crowded {
        return Err(Failure::Outcome(Outcome::Crowded));
    }
    Ok((edits, same))
}

/// The changes and the new cards of a slice written, the deck anew beside
/// itself.
pub(super) fn write_slice(
    storage: &dyn Storage,
    work: &mut Work,
    by_front: bool,
    mut edits: Vec<Edit>,
) -> Result<(), Failure> {
    // The changes by card number, then the new lines in the file's order.
    edits.sort_unstable_by_key(|e| (e.add, e.id, e.offset));
    let split = edits.partition_point(|e| !e.add);
    let (changes, adds) = edits.split_at(split);
    let (path, layout) = (&work.path, &work.layout);
    let read = |offset: u32| table::record_at(storage, path, layout, u64::from(offset));
    let mut changed = 0usize;
    // A line that cannot be read now, or texts that cannot be made, leave
    // the card as it is; the update then fails and is tried again.
    let failed = core::cell::Cell::new(false);
    let mut pending = adds.iter();
    let added = cards::rewrite_with(
        storage,
        work.deck,
        |card| {
            if failed.get() {
                return true;
            }
            if let Ok(i) = changes.binary_search_by_key(&card.state.id, |c| c.id)
                && let Some(change) = changes.get(i)
            {
                match read(change.offset) {
                    Ok(Some(record)) => {
                        let made =
                            (|| -> Result<(Option<String>, String, String), StorageError> {
                                Ok((
                                    if by_front {
                                        None
                                    } else {
                                        Some(copy(trimmed_cut(&record.front, MAX_TEXT))?)
                                    },
                                    copy(trimmed_cut(&record.back, MAX_TEXT))?,
                                    copy(trimmed_cut(&record.example, MAX_TEXT))?,
                                ))
                            })();
                        match made {
                            Ok((front, back, example)) => {
                                if let Some(front) = front {
                                    card.front = front;
                                }
                                card.back = back;
                                card.example = example;
                                changed += 1;
                            }
                            Err(_) => failed.set(true),
                        }
                    }
                    Ok(None) => {}
                    Err(_) => failed.set(true),
                }
            }
            true
        },
        &mut || loop {
            if failed.get() {
                return None;
            }
            let edit = pending.next()?;
            match read(edit.offset) {
                Ok(Some(record)) => {
                    return Some(Texts {
                        front: record.front,
                        back: record.back,
                        example: record.example,
                    });
                }
                Ok(None) => {}
                Err(_) => failed.set(true),
            }
        },
    )
    .map_err(|_| Failure::Storage)?;
    if failed.get() {
        return Err(Failure::Storage);
    }
    work.report.changed += changed;
    work.report.added += added;
    work.report.no_room += adds.len() - added.min(adds.len());
    Ok(())
}

/// A file that cannot be a deck as it is, kept in the ledger by its size
/// and time (hash 0), so that it is told and not tried at every opening. A
/// file linked to a deck keeps its deck.
pub(super) fn note_failed(storage: &dyn Storage, found: &Found) {
    let Ok(mut ledger) = sources(storage) else {
        return;
    };
    let (deck, mirror, left_out) = ledger
        .find(&found.name)
        .map_or((0, false, 0), |s| (s.deck, s.mirror, s.left_out));
    ledger.sources.retain(|s| s.name != found.name);
    let (Ok(name), Ok(())) = (copy(&found.name), lines::reserve(&mut ledger.sources, 1)) else {
        return;
    };
    ledger.sources.push(Source {
        name,
        deck,
        size: found.size,
        modified: found.modified.unwrap_or(0),
        hash: 0,
        mirror,
        left_out,
    });
    let _ = write_sources(storage, &ledger);
}

/// The ledger without what can be of no use: a link to a deck that is gone,
/// a failed file that is not in the folder any more.
pub(crate) fn tidy(storage: &dyn Storage, ledger: &mut Ledger, listed: impl Fn(&str) -> bool) {
    let Ok(decks) = cards::decks(storage) else {
        return;
    };
    ledger.sources.retain(|s| {
        if s.deck == 0 {
            listed(&s.name)
        } else {
            decks.iter().any(|d| d.id == s.deck)
        }
    });
}

/// What a deck that could not be read comes to.
pub(super) fn fault_of(fault: apkg::Fault) -> Failure {
    match fault {
        apkg::Fault::NoNotes => Failure::Outcome(Outcome::NoNotes),
        apkg::Fault::NewerAnki => Failure::Outcome(Outcome::NewerAnki),
        apkg::Fault::TooLarge => Failure::Outcome(Outcome::DeckTooLarge),
        // A refused allocation is not the deck's fault: it is tried again.
        apkg::Fault::Memory | apkg::Fault::Storage => Failure::Storage,
    }
}

/// Why a step could not go on.
#[derive(Debug)]
pub(super) enum Failure {
    Outcome(Outcome),
    Storage,
}

impl From<StorageError> for Failure {
    fn from(_: StorageError) -> Self {
        Failure::Storage
    }
}

impl From<Problem> for Failure {
    fn from(_: Problem) -> Self {
        Failure::Storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused allocation is not the deck's fault, so it is not told as
    /// "too large" (which is never tried again): the file is tried again.
    #[test]
    fn a_deck_the_heap_refused_is_tried_again_and_one_too_large_is_not() {
        assert!(matches!(fault_of(apkg::Fault::Memory), Failure::Storage));
        assert!(matches!(fault_of(apkg::Fault::Storage), Failure::Storage));
        assert!(matches!(
            fault_of(apkg::Fault::TooLarge),
            Failure::Outcome(Outcome::DeckTooLarge)
        ));
        assert!(matches!(
            fault_of(apkg::Fault::NoNotes),
            Failure::Outcome(Outcome::NoNotes)
        ));
        assert!(Outcome::DeckTooLarge.is_the_file());
    }
}
