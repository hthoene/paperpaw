//! What a book's pages remember of the host's fonts: the width of each letter
//! and the kerning of each pair of letters, per font and size (a context). They
//! are made when the first page of a book is set and kept until a setting or
//! the screen changes (or the book is closed), so that only the first pages
//! ask the host; the pages after them find what they need. A `Tables::default()`
//! has no room and keeps nothing: every ask goes to the host (slower, as
//! before).
//!
//! The letters met are numbered in the order they come (up to [`ALPHABET`]; a
//! later one is not kept). A context has a width for each letter and a bit for
//! each pair that says it is known; the kerning of a known pair is zero unless
//! the small table of kerned pairs has it. Nothing is hashed, so what is found
//! is always what was kept. Memory, all of it made at once and never grown:
//! about 7 KiB.
use super::{Letters, Role};
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::native::Error;

/// Letters (and other characters) kept, in the order they are met.
const ALPHABET: usize = 96;
/// Fonts at sizes kept at once; the least used is forgotten for a new one.
const CONTEXTS: usize = 4;
/// Entries of the table of kerned pairs (a power of two, 4 bytes each).
const KERNED: usize = 256;
/// The pairs of one context, as bits.
const PAIR_BYTES: usize = ALPHABET * ALPHABET / 8;

#[derive(Debug, Default)]
struct State {
    /// The font and size of each context (0: none) and when it was last used.
    heads: [(u64, u32); CONTEXTS],
    clock: u32,
    /// The number of each ASCII character met (0: not met, else number + 1),
    /// and the others met: the character and its number (`c << 8 | number`).
    ascii: Vec<u8>,
    others: Vec<u32>,
    /// The number the next new character gets.
    next: usize,
    /// The widths of each context's letters (NaN: not known).
    widths: Vec<f32>,
    /// A bit for each pair of each context: its kerning is known.
    known: Vec<u8>,
    /// The kerned pairs: the context (2 bits), the pair (14) and the kerning in
    /// sixty-fourths of a pixel (16); 0 is no entry.
    kerned: Vec<u32>,
}

/// The tables of one book, shared by the pages that are set from them.
#[derive(Clone, Debug, Default)]
pub struct Tables(Rc<RefCell<State>>);

fn filled<T: Clone>(entries: usize, empty: T) -> Result<Vec<T>, Error> {
    let mut table = Vec::new();
    table
        .try_reserve_exact(entries)
        .map_err(|_| Error::NoMemory)?;
    table.resize(entries, empty);
    Ok(table)
}

impl Tables {
    /// The tables for `letters`: of widths always, of pairs where kerning or
    /// ligatures are on. Each part is one allocation of its own size, made here.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(letters: Letters) -> Result<Self, Error> {
        let pairs = letters.kerning || letters.ligatures;
        let mut others = Vec::new();
        others
            .try_reserve_exact(ALPHABET)
            .map_err(|_| Error::NoMemory)?;
        Ok(Self(Rc::new(RefCell::new(State {
            heads: [(0, 0); CONTEXTS],
            clock: 0,
            ascii: filled(128, 0)?,
            others,
            next: 0,
            widths: filled(CONTEXTS * ALPHABET, f32::NAN)?,
            known: filled(if pairs { CONTEXTS * PAIR_BYTES } else { 0 }, 0)?,
            kerned: filled(if pairs { KERNED } else { 0 }, 0)?,
        }))))
    }

    /// Whether there is room to keep anything (a `Tables::default()` has none).
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.0
            .try_borrow()
            .is_ok_and(|state| !state.widths.is_empty())
    }

    /// The width kept for the letter `c` in `role` at `size`.
    pub(super) fn width(&self, role: Role, size: f32, c: char) -> Option<f32> {
        let mut state = self.0.try_borrow_mut().ok()?;
        let (context, letter) = state.place(role, size, c, false)?;
        let width = *state.widths.get(context * ALPHABET + letter)?;
        (!width.is_nan()).then_some(width)
    }

    pub(super) fn keep_width(&self, role: Role, size: f32, c: char, width: f32) {
        if let Ok(mut state) = self.0.try_borrow_mut()
            && let Some((context, letter)) = state.place(role, size, c, true)
            && let Some(slot) = state.widths.get_mut(context * ALPHABET + letter)
        {
            *slot = width;
        }
    }

    /// The kerning kept for `left` then `right` in `role` at `size`.
    pub(super) fn pair(&self, role: Role, size: f32, (left, right): (char, char)) -> Option<f32> {
        let mut state = self.0.try_borrow_mut().ok()?;
        let (context, a) = state.place(role, size, left, false)?;
        let b = state.letter(right, false)?;
        let pair = a * ALPHABET + b;
        if state.known.get(context * PAIR_BYTES + pair / 8)? >> (pair % 8) & 1 == 0 {
            return None;
        }
        let key = (context as u32) << 14 | pair as u32;
        let entry = *state.kerned.get(slot(key))?;
        Some(if entry >> 16 == (key + 1) & 0xffff {
            f32::from(entry as u16 as i16) / 64.0
        } else {
            0.0
        })
    }

    pub(super) fn keep_pair(&self, role: Role, size: f32, (left, right): (char, char), kern: f32) {
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return;
        };
        let Some((context, a)) = state.place(role, size, left, true) else {
            return;
        };
        let Some(b) = state.letter(right, true) else {
            return;
        };
        let pair = a * ALPHABET + b;
        let key = (context as u32) << 14 | pair as u32;
        let kerning = quantised(kern);
        let at = slot(key);
        let Some(&before) = state.kerned.get(at) else {
            return;
        };
        let name = (key + 1) & 0xffff;
        let mine = before >> 16 == name;
        if kerning != 0 {
            if before != 0 && !mine {
                // The entry is another pair's: it is not known any more.
                state.forget_pair(before);
            }
            state.kerned[at] = name << 16 | u32::from(kerning as u16);
        } else if mine {
            state.kerned[at] = 0;
        }
        if let Some(byte) = state.known.get_mut(context * PAIR_BYTES + pair / 8) {
            *byte |= 1 << (pair % 8);
        }
    }
}

/// The entry of a pair in the table of kerned pairs.
const fn slot(key: u32) -> usize {
    (key.wrapping_mul(0x9E37_79B1) >> 20) as usize & (KERNED - 1)
}

/// A kerning in sixty-fourths of a pixel, as the table keeps it.
pub(super) fn quantised(kern: f32) -> i16 {
    // Rounded away from 0; the cast keeps to the range of an i16.
    let scaled = kern * 64.0;
    (if scaled < 0.0 {
        scaled - 0.5
    } else {
        scaled + 0.5
    }) as i16
}

impl State {
    /// The number of `c` among the characters met; it is numbered now if
    /// `assign` and there is room. `None` for a character that is not kept.
    fn letter(&mut self, c: char, assign: bool) -> Option<usize> {
        if self.widths.is_empty() {
            return None;
        }
        let ascii = (c as u32) < 128;
        let found = if ascii {
            self.ascii
                .get(c as usize)
                .filter(|&&n| n != 0)
                .map(|&n| usize::from(n) - 1)
        } else {
            self.others
                .iter()
                .find(|&&o| o >> 8 == c as u32)
                .map(|&o| (o & 0xff) as usize)
        };
        if found.is_some() || !assign || self.next >= ALPHABET {
            return found;
        }
        let number = self.next;
        if ascii {
            *self.ascii.get_mut(c as usize)? = (number + 1) as u8;
        } else {
            if self.others.len() >= self.others.capacity() {
                return None;
            }
            self.others.push((c as u32) << 8 | number as u32);
        }
        self.next += 1;
        Some(number)
    }

    /// The context of `role` at `size` and the letter `c` in it. A context that
    /// is not there is made only if `assign` (the one used least long ago is
    /// emptied for it).
    fn place(&mut self, role: Role, size: f32, c: char, assign: bool) -> Option<(usize, usize)> {
        let letter = self.letter(c, assign)?;
        let key = (role as u64) << 32 | u64::from(size.to_bits()) | 1 << 40;
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        if let Some(at) = self.heads.iter().position(|&(k, _)| k == key) {
            self.heads[at].1 = clock;
            return Some((at, letter));
        }
        if !assign {
            return None;
        }
        // A free context, else the one that has not been used for the longest.
        let at = (0..CONTEXTS)
            .min_by_key(|&i| {
                let (k, used) = self.heads[i];
                (k != 0, u32::MAX - clock.wrapping_sub(used))
            })
            .unwrap_or(0);
        self.heads[at] = (key, clock);
        if let Some(widths) = self.widths.get_mut(at * ALPHABET..(at + 1) * ALPHABET) {
            widths.fill(f32::NAN);
        }
        if let Some(bits) = self.known.get_mut(at * PAIR_BYTES..(at + 1) * PAIR_BYTES) {
            bits.fill(0);
        }
        Some((at, letter))
    }

    /// A kerned pair's entry was taken by another pair: that one is not known
    /// any more (the entry names its context and pair).
    fn forget_pair(&mut self, entry: u32) {
        let key = (entry >> 16).wrapping_sub(1);
        let (context, pair) = ((key >> 14) as usize, (key & 0x3fff) as usize);
        if let Some(byte) = self.known.get_mut(context * PAIR_BYTES + pair / 8) {
            *byte &= !(1 << (pair % 8));
        }
    }
}
