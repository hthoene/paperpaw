//! What a book's pages remember of the host's fonts: the width of each letter,
//! per font and size (a context). They are made when the first page of a book
//! is set and kept until a setting or the screen changes (or the book is
//! closed), so that only the first pages ask the host; the pages after them
//! find what they need. A `Tables::default()` has no room and keeps nothing:
//! every ask goes to the host (slower, as before).
//!
//! The letters met are numbered in the order they come (up to [`ALPHABET`]; a
//! later one is not kept). Nothing is hashed, so what is found is always what
//! was kept. Memory, made at once and never grown: about 2 KiB.
use super::Role;
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::native::Error;

/// Letters (and other characters) kept, in the order they are met.
const ALPHABET: usize = 96;
/// Fonts at sizes kept at once; the least used is forgotten for a new one.
const CONTEXTS: usize = 4;

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
    /// The tables, each part one allocation of its own size, made here.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new() -> Result<Self, Error> {
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
        Some((at, letter))
    }
}
