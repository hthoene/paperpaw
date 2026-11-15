//! Where a word may be broken at the end of a line (`core.hyphenate`): the
//! places the host's table for the book's language gives, kept for the words
//! met so that a page laid again, or the page before and after, does not ask the
//! host for the same word twice. One fixed allocation per book (about 1 KiB),
//! made at the first page, never grown and freed with the book (or a changed
//! setting). Where there is no room the reader carries on without it: every
//! word is asked of the host.
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::native::{Error, Host, text as host_text};

/// Words kept (a power of two; a word takes the entry its hash names).
const ENTRIES: usize = 64;
/// The longest word kept, in bytes: its places are bits of one `u64`.
const LONGEST: usize = 63;

/// A word's hash (never 0) and the places it may be broken at, one bit for each
/// byte offset.
#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    key: u64,
    places: u64,
}

/// The places of the words met.
#[derive(Clone, Debug, Default)]
pub struct Breaks(Option<Rc<RefCell<Vec<Entry>>>>);

/// The hash of a word in `language` with the minimums `left` and `right`.
fn key_of(language: &str, (left, right): (u32, u32), word: &str) -> u64 {
    let mut hash = paperpaw_sdk::digest::fnv1a64(language);
    for byte in left
        .to_le_bytes()
        .into_iter()
        .chain(right.to_le_bytes())
        .chain(word.bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash | 1
}

/// Tells the layout where a word may break: the `places` the host gave (the
/// first `asked` of them), none where it refused (a language with no table, a
/// word it could not read: the word stays whole).
pub fn break_places(asked: Result<usize, Error>, places: &[u8], out: &mut dyn FnMut(usize)) {
    for place in places.iter().take(asked.unwrap_or(0)) {
        out(usize::from(*place));
    }
}

impl Breaks {
    /// Room for [`ENTRIES`] words.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new() -> Result<Self, Error> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(ENTRIES)
            .map_err(|_| Error::NoMemory)?;
        entries.resize(ENTRIES, Entry::default());
        Ok(Self(Some(Rc::new(RefCell::new(entries)))))
    }

    /// Whether there is room to keep anything.
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.0.is_some()
    }

    /// Tells `out` the byte offsets `word` may be broken at, keeping at least
    /// `left` letters before and `right` after (0: the table's own): from what
    /// was kept, else asked of the host and kept. A refusal is no place; only a
    /// language the host has no table for is kept as that.
    pub fn places(
        &self,
        host: &Host<'_>,
        language: &str,
        (left, right): (u32, u32),
        word: &str,
        out: &mut dyn FnMut(usize),
    ) {
        let key = key_of(language, (left, right), word);
        let slot = (key >> 40) as usize & (ENTRIES - 1);
        if let Some(entries) = &self.0
            && let Ok(entries) = entries.try_borrow()
            && let Some(entry) = entries.get(slot)
            && entry.key == key
        {
            for at in (0..LONGEST).filter(|at| (entry.places >> at) & 1 == 1) {
                out(at);
            }
            return;
        }
        let mut places = [0u8; host_text::MAX_BREAKS];
        let asked = host_text::hyphenate(host, language, word, left, right, &mut places);
        break_places(asked, &places, out);
        let kept = match asked {
            Ok(count) => places.iter().take(count).try_fold(0u64, |mask, &at| {
                (usize::from(at) < LONGEST).then(|| mask | (1u64 << at))
            }),
            Err(Error::NotFound) => Some(0),
            Err(_) => None,
        };
        if word.len() <= LONGEST
            && let Some(places) = kept
            && let Some(entries) = &self.0
            && let Ok(mut entries) = entries.try_borrow_mut()
            && let Some(entry) = entries.get_mut(slot)
        {
            *entry = Entry { key, places };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_told_as_far_as_the_host_gave_and_none_where_it_refused() {
        let mut told = Vec::new();
        // `asked` places of the buffer; what lies beyond them is not told.
        break_places(Ok(2), &[3, 6, 9], &mut |at| told.push(at));
        assert_eq!(told, [3, 6]);
        for refused in [Err(Error::NotFound), Err(Error::Invalid), Ok(0)] {
            let mut told = Vec::new();
            break_places(refused, &[3, 6, 9], &mut |at| told.push(at));
            assert!(told.is_empty());
        }
    }

    #[test]
    fn a_word_has_one_key_for_its_language_and_minimums() {
        let key = key_of("en", (0, 0), "typography");
        assert_eq!(key, key_of("en", (0, 0), "typography"));
        assert_ne!(key, key_of("de", (0, 0), "typography"));
        assert_ne!(key, key_of("en", (2, 0), "typography"));
        assert_ne!(key, key_of("en", (0, 2), "typography"));
        assert_ne!(key, key_of("en", (0, 0), "typographs"));
        assert_ne!(key, 0);
    }

    #[test]
    fn the_cache_is_one_fixed_allocation_and_none_without_room() {
        assert!(Breaks::new().is_ok_and(|breaks| breaks.has_room()));
        assert!(!Breaks::default().has_room());
    }
}
