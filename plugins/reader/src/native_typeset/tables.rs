//! What a book's pages remember of the host's fonts: the width of each letter
//! and the kerning of each pair of letters, per font and size (a context). They
//! are made when the first page of a book is set and kept until a setting or
//! the screen changes (or the book is closed), so that only the first pages
//! ask the host; the pages after them find what they need. A `Tables::default()`
//! has no room and keeps nothing: every ask goes to the host (slower, as
//! before).
//!
//! The letters met are numbered (up to [`ALPHABET`] at once; a new one takes
//! the number of the one used least lately if it has not been used for a while
//! ([`STALE`]), so that the alphabet follows the script being read: a Cyrillic
//! book with a Latin title page is kerned in Cyrillic from its third page on). A context has a width for each letter
//! and a bit for each pair that says it is known; the kerning of a known pair is
//! zero unless the small table of kerned pairs has it. Nothing is hashed there,
//! so what is found is always what was kept. Characters that have no number
//! (fonts do not kern ideographs, kana and Hangul: [`kerns`]; or all numbers are
//! in use) have their widths kept in a small table of exact entries (the whole key is compared; a new entry
//! takes the place of one of the two it may be in), made at the first such
//! character. Memory, made at once and never grown: about 9 KiB, and 2 KiB more
//! for a book of such characters.
use super::Role;
use alloc::{rc::Rc, vec::Vec};
use core::cell::{RefCell, RefMut};
use paperpaw_sdk::native::Error;

/// Letters (and other characters) kept, in the order they are met.
const ALPHABET: usize = 96;
/// Fonts at sizes kept at once; the least used is forgotten for a new one.
const CONTEXTS: usize = 4;
/// A letter that has not been asked for in this many lookups (about two pages of
/// text) may give its number to a new one: a Latin title page lets go of its
/// letters once the book goes on in Cyrillic, but a working set of more than
/// [`ALPHABET`] letters (Vietnamese) does not take numbers from each other at
/// every page. Letters without a number have their widths kept in the table of
/// the others and are set without kerning.
const STALE: u32 = 3_000;
/// Entries of the table of kerned pairs (a power of two, 4 bytes each): sets of
/// two, a pair is in either entry of its set (a page of English has about a
/// hundred and fifty kerned pairs, and one entry for each lost a third of them
/// to others at every page).
const KERNED: usize = 512;
/// The pairs of one context, as bits.
const PAIR_BYTES: usize = ALPHABET * ALPHABET / 8;
/// Entries of the table of the characters after the alphabet (a power of two,
/// 8 bytes each, a key in one of two).
const EXTRA: usize = 256;

#[derive(Default)]
struct State {
    /// The font and size of each context (0: none) and when it was last used.
    heads: [(u64, u32); CONTEXTS],
    clock: u32,
    /// The number of each ASCII character met (0: not met, else number + 1),
    /// and the others met: the character and its number (`c << 8 | number`).
    ascii: Vec<u8>,
    others: Vec<u32>,
    /// The number the next new character gets while some are free; the
    /// character of each number and when it was last used.
    next: usize,
    chars: Vec<u32>,
    used: Vec<u32>,
    /// The widths of each context's letters (NaN: not known).
    widths: Vec<f32>,
    /// A bit for each pair of each context: its kerning is known.
    known: Vec<u8>,
    /// The kerned pairs: the context (2 bits), the pair (14) and the kerning in
    /// sixty-fourths of a pixel (16); 0 is no entry.
    kerned: Vec<u32>,
    /// Widths of what is not among the first letters (see
    /// [`extra_key`]), none until one is met; `no_extra`: there was no room.
    extra: Vec<u64>,
    no_extra: bool,
}

/// What the tables know of the kerning of a pair.
pub(super) enum Kerning {
    LeftOut,
    Unknown,
    Known(f32),
}

/// The tables of one book, shared by the pages that are set from them. None
/// (`default`) keeps nothing.
#[derive(Clone, Default)]
pub struct Tables(Option<Rc<RefCell<State>>>);

impl core::fmt::Debug for Tables {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Tables")
    }
}

#[inline(never)]
fn filled<T: Clone>(entries: usize, empty: T) -> Result<Vec<T>, Error> {
    let mut table = Vec::new();
    table
        .try_reserve_exact(entries)
        .map_err(|_| Error::NoMemory)?;
    table.resize(entries, empty);
    Ok(table)
}

impl Tables {
    /// The tables: of widths and of pairs, whatever the settings are (a word
    /// drawn in pieces asks for its pairs too). Each part is one allocation of
    /// its own size, made here.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new() -> Result<Self, Error> {
        let mut others = Vec::new();
        others
            .try_reserve_exact(ALPHABET)
            .map_err(|_| Error::NoMemory)?;
        Ok(Self(Some(Rc::new(RefCell::new(State {
            heads: [(0, 0); CONTEXTS],
            clock: 0,
            ascii: filled(128, 0)?,
            others,
            next: 0,
            chars: filled(ALPHABET, 0)?,
            used: filled(ALPHABET, 0)?,
            widths: filled(CONTEXTS * ALPHABET, f32::NAN)?,
            known: filled(CONTEXTS * PAIR_BYTES, 0)?,
            kerned: filled(KERNED, 0)?,
            extra: Vec::new(),
            no_extra: false,
        })))))
    }

    /// Whether there is room to keep anything (a `Tables::default()` has none).
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.0
            .as_ref()
            .and_then(|state| state.try_borrow().ok())
            .is_some_and(|state| !state.widths.is_empty())
    }

    fn state(&self) -> Option<RefMut<'_, State>> {
        self.0.as_ref()?.try_borrow_mut().ok()
    }

    /// What is kept of the kerning of a pair of letters in one look: `LeftOut`
    /// where the pair cannot be kept now and is not asked of the host (one of
    /// the letters has no number and none can be given: all are in use; it is
    /// set unkerned, as before kerning was set at all), `Known` where it is
    /// kept, else `Unknown`.
    pub(super) fn kerning(&self, role: Role, size: f32, (left, right): (char, char)) -> Kerning {
        let Some(mut state) = self.state() else {
            return Kerning::Unknown;
        };
        let (a, b) = (state.letter(left, false), state.letter(right, false));
        let (Some(a), Some(b)) = (a, b) else {
            let out = state.next >= ALPHABET && state.oldest().is_none();
            return if out {
                Kerning::LeftOut
            } else {
                Kerning::Unknown
            };
        };
        state
            .kept_pair(role, size, a * ALPHABET + b)
            .map_or(Kerning::Unknown, Kerning::Known)
    }

    /// Whether the pair is left out (see [`Tables::kerning`]).
    #[cfg(test)]
    pub(super) fn leaves_out(&self, pair: (char, char)) -> bool {
        matches!(self.kerning(Role::Reading, 0.0, pair), Kerning::LeftOut)
    }

    /// The width kept for the letter `c` in `role` at `size`.
    pub(super) fn width(&self, role: Role, size: f32, c: char) -> Option<f32> {
        let mut state = self.state()?;
        let context = state.context(role, size, false)?;
        let Some(letter) = state.letter(c, false) else {
            return state.extra_get(extra_key(context, c)).map(unit);
        };
        let width = *state.widths.get(context * ALPHABET + letter)?;
        (!width.is_nan()).then_some(width)
    }

    /// Keeps the width of `c`; gives the width the tables will answer with from
    /// now on (that of the host, but for a character after the alphabet, whose
    /// width is kept in sixty-fourths of a pixel).
    pub(super) fn keep_width(&self, role: Role, size: f32, c: char, width: f32) -> f32 {
        let Some(mut state) = self.state() else {
            return width;
        };
        let Some(context) = state.context(role, size, true) else {
            return width;
        };
        match state.letter(c, true) {
            Some(letter) => {
                if let Some(slot) = state.widths.get_mut(context * ALPHABET + letter) {
                    *slot = width;
                }
                width
            }
            None => {
                let kept = quantised(width);
                let exact = f32::from(kept) / 64.0;
                // A width the table cannot hold is used as the host gave it.
                if (exact - width).abs() > 1.0 || !state.extra_put(extra_key(context, c), kept) {
                    return width;
                }
                exact
            }
        }
    }

    /// The kerning kept for `left` then `right` in `role` at `size`.
    #[cfg(test)]
    pub(super) fn pair(&self, role: Role, size: f32, pair: (char, char)) -> Option<f32> {
        match self.kerning(role, size, pair) {
            Kerning::Known(kern) => Some(kern),
            _ => None,
        }
    }

    pub(super) fn keep_pair(&self, role: Role, size: f32, (left, right): (char, char), kern: f32) {
        let Some(mut state) = self.state() else {
            return;
        };
        let Some(context) = state.context(role, size, true) else {
            return;
        };
        let kerning = quantised(kern);
        let (Some(a), Some(b)) = (state.letter(left, true), state.letter(right, true)) else {
            return;
        };
        let pair = a * ALPHABET + b;
        let key = (context as u32) << 14 | pair as u32;
        let at = slot(key);
        let name = (key + 1) & 0xffff;
        let Some(two) = state.kerned.get(at..at + 2) else {
            return;
        };
        let (mine, free) = (
            two.iter().position(|&entry| entry >> 16 == name),
            two.iter().position(|&entry| entry == 0),
        );
        if kerning != 0 {
            let new = name << 16 | u32::from(kerning as u16);
            match mine.or(free) {
                Some(place) => state.kerned[at + place] = new,
                None => {
                    // Both are other pairs': the second is not known any more,
                    // the first moves to its place and the new one is the first.
                    let (first, second) = (state.kerned[at], state.kerned[at + 1]);
                    state.forget_pair(second);
                    state.kerned[at + 1] = first;
                    state.kerned[at] = new;
                }
            }
        } else if let Some(place) = mine {
            state.kerned[at + place] = 0;
        }
        if let Some(byte) = state.known.get_mut(context * PAIR_BYTES + pair / 8) {
            *byte |= 1 << (pair % 8);
        }
    }
}

/// Whether fonts kern `c` with its neighbours: not the ideographs, the kana, the
/// Hangul syllables and the full-width forms, which are set one after the other
/// (a page of them has no pair to ask the host about).
pub(super) const fn kerns(c: char) -> bool {
    !matches!(
        c as u32,
        0x2E80..=0xA4CF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x2_0000..=0x3_FFFF
    )
}

/// A value kept in sixty-fourths of a pixel, as pixels.
fn unit(kept: i16) -> f32 {
    f32::from(kept) / 64.0
}

/// The key of the width of `c` in a context, in the table of the characters
/// after the alphabet: the context (2 bits) and the character (21 bits) above
/// the low 16 bits, which are the value. Bit 63 is set: an empty entry is 0.
fn extra_key(context: usize, c: char) -> u64 {
    1 << 63 | (context as u64 & 3) << 61 | u64::from(c as u32) << 40
}

/// The first of the two entries a key may be in, in the table of the
/// characters after the alphabet.
fn extra_slot(key: u64) -> usize {
    ((key >> 16).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 56) as usize & (EXTRA - 2)
}

/// The first of the two entries of a pair's set in the table of kerned pairs.
const fn slot(key: u32) -> usize {
    (key.wrapping_mul(0x9E37_79B1) >> 20) as usize & (KERNED - 2)
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
    /// The kerning kept for the pair of numbers `pair` (`a * ALPHABET + b`).
    fn kept_pair(&mut self, role: Role, size: f32, pair: usize) -> Option<f32> {
        let context = self.context(role, size, false)?;
        if self.known.get(context * PAIR_BYTES + pair / 8)? >> (pair % 8) & 1 == 0 {
            return None;
        }
        let key = (context as u32) << 14 | pair as u32;
        let at = slot(key);
        let name = (key + 1) & 0xffff;
        let entry = self
            .kerned
            .get(at..at + 2)?
            .iter()
            .find(|&&e| e >> 16 == name);
        Some(entry.map_or(0.0, |&e| f32::from(e as u16 as i16) / 64.0))
    }

    /// The number of `c` among the characters met; it is numbered now if
    /// `assign` (the number of the letter used least lately where all are
    /// taken). `None` for a character that is not kept.
    fn letter(&mut self, c: char, assign: bool) -> Option<usize> {
        if self.widths.is_empty() || !kerns(c) {
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
        if let Some(number) = found {
            *self.used.get_mut(number)? = self.clock;
            return Some(number);
        }
        if !assign {
            return None;
        }
        let number = if self.next < ALPHABET {
            self.next += 1;
            self.next - 1
        } else {
            self.free_oldest()?
        };
        if ascii {
            *self.ascii.get_mut(c as usize)? = (number + 1) as u8;
        } else {
            // Room: a number has one character, and each is in the list once.
            if self.others.len() >= self.others.capacity() {
                return None;
            }
            self.others.push((c as u32) << 8 | number as u32);
        }
        *self.chars.get_mut(number)? = c as u32;
        *self.used.get_mut(number)? = self.clock;
        Some(number)
    }

    /// The number of the letter used least lately, if it is stale.
    fn oldest(&self) -> Option<usize> {
        let age = |i: usize| {
            self.clock
                .wrapping_sub(self.used.get(i).copied().unwrap_or(0))
        };
        let number = (0..ALPHABET).max_by_key(|&i| age(i))?;
        (age(number) >= STALE).then_some(number)
    }

    /// Takes the number of the letter used least lately away from it and
    /// forgets what was kept under it in every context.
    fn free_oldest(&mut self) -> Option<usize> {
        let number = self.oldest()?;
        let old = *self.chars.get(number)?;
        if old < 128 {
            *self.ascii.get_mut(old as usize)? = 0;
        } else {
            self.others.retain(|&o| (o & 0xff) as usize != number);
        }
        for context in 0..CONTEXTS {
            if let Some(width) = self.widths.get_mut(context * ALPHABET + number) {
                *width = f32::NAN;
            }
            for other in 0..ALPHABET {
                for pair in [number * ALPHABET + other, other * ALPHABET + number] {
                    if let Some(byte) = self.known.get_mut(context * PAIR_BYTES + pair / 8) {
                        *byte &= !(1 << (pair % 8));
                    }
                }
            }
        }
        Some(number)
    }

    /// The context of `role` at `size`. One that is not there is made only if
    /// `assign` (the one used least long ago is emptied for it).
    fn context(&mut self, role: Role, size: f32, assign: bool) -> Option<usize> {
        if self.widths.is_empty() {
            return None;
        }
        let key = (role as u64) << 32 | u64::from(size.to_bits()) | 1 << 40;
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        if let Some(at) = self.heads.iter().position(|&(k, _)| k == key) {
            self.heads[at].1 = clock;
            return Some(at);
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
        for entry in &mut self.extra {
            if *entry >> 61 & 3 == at as u64 {
                *entry = 0;
            }
        }
        Some(at)
    }

    /// The value kept under `key` in the table of the characters after the
    /// alphabet.
    fn extra_get(&self, key: u64) -> Option<i16> {
        let at = extra_slot(key);
        self.extra
            .get(at..at + 2)?
            .iter()
            .find(|&&entry| entry >> 16 == key >> 16)
            .map(|&entry| entry as u16 as i16)
    }

    /// Keeps `value` under `key`: in its entry if it has one, else in a free
    /// one of its two, else in the place of one of them. The table is made at
    /// the first. Whether it is kept (not where there is no room).
    fn extra_put(&mut self, key: u64, value: i16) -> bool {
        if self.extra.is_empty() && !self.no_extra {
            self.no_extra = true;
            if let Ok(table) = filled(EXTRA, 0) {
                self.extra = table;
                self.no_extra = false;
            }
        }
        let at = extra_slot(key);
        let Some(two) = self.extra.get_mut(at..at + 2) else {
            return false;
        };
        let new = key | u64::from(value as u16);
        let place = two
            .iter()
            .position(|&entry| entry >> 16 == key >> 16)
            .or_else(|| two.iter().position(|&entry| entry == 0));
        match (place, two) {
            (Some(place), two) => two[place] = new,
            // Both are taken by others: the first one moves to the second,
            // the new one is the first.
            (None, [first, second]) => {
                *second = *first;
                *first = new;
            }
            _ => return false,
        }
        true
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
