//! The index of when each deck's cards are due.
//! Part of [`cards`](super); moved out of it to keep files short.

use super::*;

/// When a deck's cards are due (or a part of that, a line of `due.txt`):
/// its new ones, and the others by quarter hour. A deck's lines add up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Index {
    pub(super) deck: u32,
    pub(super) new: u32,
    pub(super) hours: Vec<(i64, u32)>,
}

/// Hours on a line of the index at most: far below [`MAX_LINE`].
pub(super) const HOURS_A_LINE: usize = 300;

impl Index {
    pub(super) fn count(&mut self, state: &State) -> Result<(), StorageError> {
        if state.is_new() {
            self.new += 1;
            return Ok(());
        }
        let quarter = state.due.div_euclid(QUARTER);
        match self.hours.binary_search_by_key(&quarter, |(q, _)| *q) {
            Ok(at) => {
                if let Some((_, n)) = self.hours.get_mut(at) {
                    *n += 1;
                }
            }
            Err(at) => {
                self.hours
                    .try_reserve(1)
                    .map_err(|_| StorageError::TooLarge)?;
                self.hours.insert(at, (quarter, 1));
            }
        }
        Ok(())
    }

    /// Appends its lines: the new cards on the first, [`HOURS_A_LINE`]
    /// hours a line.
    pub(super) fn push_lines(&self, text: &mut String) -> Result<(), StorageError> {
        let mut parts = self.hours.chunks(HOURS_A_LINE);
        let mut first = true;
        loop {
            let part = parts.next();
            if part.is_none() && !first {
                break;
            }
            let new = if first { self.new } else { 0 };
            push_number(text, self.deck)?;
            push(text, "\t")?;
            push_number(text, new)?;
            push(text, "\t")?;
            for (i, (hour, n)) in part.unwrap_or_default().iter().enumerate() {
                if i > 0 {
                    push(text, " ")?;
                }
                push_signed(text, *hour)?;
                push(text, "=")?;
                push_number(text, *n)?;
            }
            push(text, "\n")?;
            first = false;
        }
        Ok(())
    }

    pub(super) fn parse(line: &str) -> Result<Option<Index>, StorageError> {
        let mut fields = line.split('\t');
        let (Some(deck), Some(new)) = (
            fields.next().and_then(|d| d.parse().ok()),
            fields.next().and_then(|n| n.parse().ok()),
        ) else {
            return Ok(None);
        };
        let mut hours = Vec::new();
        for pair in fields.next().unwrap_or_default().split(' ') {
            let Some((hour, n)) = pair.split_once('=') else {
                continue;
            };
            if let (Ok(hour), Ok(n)) = (hour.parse(), n.parse()) {
                hours.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
                hours.push((hour, n));
            }
        }
        Ok(Some(Index { deck, new, hours }))
    }
}

/// Calls `part` with each line of the index, read one at a time.
pub(super) fn each_index(
    storage: &dyn Storage,
    mut part: impl FnMut(Index),
) -> Result<(), StorageError> {
    let mut failed = Ok(());
    each_line(storage, INDEX, |_, line| match Index::parse(line) {
        Ok(Some(i)) => {
            part(i);
            ControlFlow::Continue(())
        }
        Ok(None) => ControlFlow::Continue(()),
        Err(e) => {
            failed = Err(e);
            ControlFlow::Break(())
        }
    })?;
    failed
}

/// Where the index could not be brought along with a change of the decks
/// (the heap refused something after the deck was put in place), it no
/// longer says what the decks hold: it is taken away (a removal needs no
/// heap), which marks it for a rebuild ([`rebuild_index`], at start and
/// when the cards are chosen). The error goes on.
pub(super) fn keep_index(
    storage: &dyn Storage,
    result: Result<(), StorageError>,
) -> Result<(), StorageError> {
    if result.is_err() {
        let _ = storage.remove(INDEX);
    }
    result
}

/// Makes the index anew from the decks where there is none (it was taken
/// away, see [`keep_index`], or never made): each deck read once.
pub(crate) fn rebuild_index(storage: &dyn Storage) -> Result<(), StorageError> {
    if storage.exists(INDEX) {
        return Ok(());
    }
    // One deck at a time puts its line in place: where it stops part way
    // the index there is would be a partial one that no later rebuild
    // looks at, so it is taken away again.
    keep_index(storage, make_index(storage))
}

/// The index made anew where it is not there (see [`keep_index`]), as far as
/// the memory allows: what reads the index (the sizes, the cards due) must not
/// take a missing one for a library with nothing in it. A present index costs
/// one `exists`.
pub(crate) fn index_ready(storage: &dyn Storage) {
    match rebuild_index(storage) {
        Ok(()) | Err(_) => {}
    }
}

/// The index of all decks, made one deck at a time.
pub(super) fn make_index(storage: &dyn Storage) -> Result<(), StorageError> {
    for deck in decks(storage)? {
        let mut index = Index {
            deck: deck.id,
            ..Index::default()
        };
        let mut failed = Ok(());
        each_card(storage, deck.id, |_, card| match index.count(&card.state) {
            Ok(()) => ControlFlow::Continue(()),
            Err(e) => {
                failed = Err(e);
                ControlFlow::Break(())
            }
        })?;
        failed?;
        write_index(storage, deck.id, Some(index))?;
    }
    Ok(())
}

/// The first line of the index.
pub(super) fn index_header() -> Result<String, StorageError> {
    let mut out = String::new();
    push(&mut out, DUE_HEADER)?;
    push(&mut out, "\n")?;
    Ok(out)
}

/// `added` new cards more in `deck`: a line of the index more.
pub(super) fn add_to_index(
    storage: &dyn Storage,
    deck: u32,
    added: u32,
) -> Result<(), StorageError> {
    // Without an index (taken away, see [`keep_index`]) one made now from
    // the decks, the new cards in them: never one that knows only these.
    if !storage.exists(INDEX) {
        return rebuild_index(storage);
    }
    let mut line = String::new();
    push_number(&mut line, deck)?;
    push(&mut line, "\t")?;
    push_number(&mut line, added)?;
    push(&mut line, "\t\n")?;
    storage.append(INDEX, line.as_bytes())
}

/// Puts `index` in place of `deck`'s (or takes it out), the others'
/// lines copied a line at a time. Without an index (taken away, see
/// [`keep_index`]) it is made anew from the decks, as they are now.
pub(super) fn set_index(
    storage: &dyn Storage,
    deck: u32,
    index: Option<Index>,
) -> Result<(), StorageError> {
    if !storage.exists(INDEX) {
        return rebuild_index(storage);
    }
    write_index(storage, deck, index)
}

/// [`set_index`] on an index that is there (or is to be made).
pub(super) fn write_index(
    storage: &dyn Storage,
    deck: u32,
    index: Option<Index>,
) -> Result<(), StorageError> {
    storage.write(INDEX_TEMPORARY, index_header()?.as_bytes())?;
    let work = || -> Result<(), StorageError> {
        let mut out = String::new();
        let mut failed = Ok(());
        each_line(storage, INDEX, |_, line| {
            let mut copy_line = || -> Result<(), StorageError> {
                if Index::parse(line)?.is_some_and(|i| i.deck != deck) {
                    push(&mut out, line)?;
                    push(&mut out, "\n")?;
                }
                if out.len() >= CHUNK {
                    storage.append(INDEX_TEMPORARY, out.as_bytes())?;
                    out.clear();
                }
                Ok(())
            };
            match copy_line() {
                Ok(()) => ControlFlow::Continue(()),
                Err(e) => {
                    failed = Err(e);
                    ControlFlow::Break(())
                }
            }
        })?;
        failed?;
        if let Some(index) = index {
            index.push_lines(&mut out)?;
        }
        storage.append(INDEX_TEMPORARY, out.as_bytes())
    };
    work().inspect_err(|_| {
        let _ = storage.remove(INDEX_TEMPORARY);
    })?;
    put_in_place(storage, INDEX_TEMPORARY, INDEX)
}
