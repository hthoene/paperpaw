//! The bidi state at the starts of the pieces of long paragraphs (#33).

use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_text::bidi;

/// Starts kept at most: 32 pieces of 4,000 bytes, a paragraph of about
/// 128 KB, in about 5.4 KB.
const MAX_KEPT: usize = 32;
/// Bytes before a state: its section and offset.
const KEY: usize = 8;

/// The bidi state (UAX #9) at the starts of the pieces of long
/// paragraphs (a block split in pieces, see
/// [`Block::continues`](paperpaw_document::Block::continues)), kept from
/// page to page by what lays them out (the reader, for its open book).
/// With it, a page that starts inside such a paragraph resolves its
/// piece from that piece's start, not from the paragraph's: see
/// [`LayoutContext::pieces`](crate::LayoutContext::pieces). The states
/// are the layout's own; at most a few dozen are kept, the oldest given
/// up first, and one not kept is found again by walking from the
/// paragraph's start.
#[derive(Default)]
pub struct PieceStarts {
    kept: RefCell<Vec<u8>>,
}

impl PieceStarts {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kept: RefCell::new(Vec::new()),
        }
    }

    /// Forgets all: for another book.
    pub fn clear(&self) {
        if let Ok(mut kept) = self.kept.try_borrow_mut() {
            *kept = Vec::new();
        }
    }

    /// How many starts are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.kept
            .try_borrow()
            .map_or(0, |kept| kept.len() / record())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copies the state kept for the piece at `offset` of `section` into
    /// `state`; false where none is.
    pub(crate) fn get(&self, section: u32, offset: u32, state: &mut [u8]) -> bool {
        let Ok(kept) = self.kept.try_borrow() else {
            return false;
        };
        let key = key(section, offset);
        let found = kept
            .chunks_exact(record())
            .find(|r| r.get(..KEY) == Some(&key[..]));
        match found.and_then(|r| r.get(KEY..)) {
            Some(bytes) if bytes.len() == state.len() => {
                state.copy_from_slice(bytes);
                true
            }
            _ => false,
        }
    }

    /// The paragraph level of the state kept nearest before `offset` (or
    /// at it) in `section`, if any.
    pub(crate) fn nearest_paragraph(&self, section: u32, offset: u32) -> Option<bidi::Level> {
        let kept = self.kept.try_borrow().ok()?;
        kept.chunks_exact(record())
            .filter_map(|r| {
                let at = |i: usize| {
                    r.get(i..i + 4)
                        .and_then(|b| b.try_into().ok())
                        .map(u32::from_le_bytes)
                };
                let (s, o) = (at(0)?, at(4)?);
                (s == section && o <= offset).then_some((o, r.get(KEY..)?))
            })
            .max_by_key(|(o, _)| *o)
            .and_then(|(_, state)| bidi::state_paragraph(state))
    }

    /// Keeps `state` for the piece at `offset` of `section`, in place of
    /// one kept for it before; the oldest goes where there is no room.
    /// Nothing where memory is short.
    pub(crate) fn put(&self, section: u32, offset: u32, state: &[u8]) {
        let Ok(mut kept) = self.kept.try_borrow_mut() else {
            return;
        };
        if state.len() != bidi::state_len() {
            return;
        }
        let key = key(section, offset);
        let size = record();
        if let Some(at) = kept
            .chunks_exact(size)
            .position(|r| r.get(..KEY) == Some(&key[..]))
        {
            let from = at * size + KEY;
            if let Some(slot) = kept.get_mut(from..from + state.len()) {
                slot.copy_from_slice(state);
            }
            return;
        }
        if kept.len() / size >= MAX_KEPT {
            kept.drain(..size);
        } else if kept.try_reserve(size).is_err() {
            return;
        }
        kept.extend_from_slice(&key);
        kept.extend_from_slice(state);
    }
}

impl core::fmt::Debug for PieceStarts {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PieceStarts")
            .field("kept", &self.len())
            .finish()
    }
}

fn record() -> usize {
    KEY + bidi::state_len()
}

fn key(section: u32, offset: u32) -> [u8; KEY] {
    let [a, b, c, d] = section.to_le_bytes();
    let [e, f, g, h] = offset.to_le_bytes();
    [a, b, c, d, e, f, g, h]
}
