//! Where a word may be broken at the end of a line (`core.hyphenate`): the
//! places the host's table for the book's language gives, kept for the words
//! met so that a page laid again, or the page before and after, does not ask the
//! host for the same word twice (each ask is a card-backed table lookup, about
//! ten milliseconds on the X4). A language the host has no table for is
//! remembered too (one more entry): no word of the book is asked then, until the
//! tables are dropped (the book is closed, a setting changes, another screen
//! covers the reader), so a pack installed meanwhile is found when the reader
//! is shown again. One fixed allocation per book (`ENTRIES` + 1 of 8 bytes: 2,056
//! bytes), made at the first page, never grown and freed with the book (or a
//! changed setting). Where there is no room the reader carries on without it:
//! every word is asked of the host.
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::native::{Error, text as host_text};

/// Words kept (a power of two): in sets of two, a word is in either entry of
/// the set its hash names.
const ENTRIES: usize = 256;
/// What asks the host for the places of a word (the number of them).
pub type Ask<'a> = dyn Fn(&str, &mut [u8]) -> Result<usize, Error> + 'a;
/// The entry after the words': the key of the language the host has no table for.
const NO_TABLE: usize = ENTRIES;
/// The longest word kept, in bytes: its places are bits of one `u32`.
const LONGEST: usize = 31;

/// A word's hash (never 0) and the places it may be broken at, one bit for each
/// byte offset.
#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    key: u32,
    places: u32,
}

/// The places of the words met.
#[derive(Clone, Debug, Default)]
pub struct Breaks(Option<Rc<RefCell<Vec<Entry>>>>);

/// A 64-bit FNV-1a hash folded to 32 bits (never 0).
fn fold(hash: u64) -> u32 {
    ((hash ^ hash >> 32) as u32) | 1
}

/// The hash of a word in `language` with the minimums `left` and `right`.
fn key_of(language: &str, (left, right): (u32, u32), word: &str) -> u32 {
    let mut hash = paperpaw_sdk::digest::fnv1a64(language);
    for byte in left
        .to_le_bytes()
        .into_iter()
        .chain(right.to_le_bytes())
        .chain(word.bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    fold(hash)
}

/// The first entry of the set of a word's hash.
const fn set_of(key: u32) -> usize {
    (key >> 8) as usize & (ENTRIES - 2)
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
    /// Room for 256 words and a language without a table.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new() -> Result<Self, Error> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(ENTRIES + 1)
            .map_err(|_| Error::NoMemory)?;
        entries.resize(ENTRIES + 1, Entry::default());
        Ok(Self(Some(Rc::new(RefCell::new(entries)))))
    }

    /// Whether there is room to keep anything.
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.0.is_some()
    }

    /// Tells `out` the byte offsets `word` may be broken at, keeping at least
    /// `left` letters before and `right` after (0: the table's own): from what
    /// was kept, else asked of the host and kept. A refusal is no place; a language
    /// the host has no table for is kept as that, and no word of the book is
    /// asked after it.
    pub fn places(
        &self,
        ask: &Ask<'_>,
        language: &str,
        (left, right): (u32, u32),
        word: &str,
        out: &mut dyn FnMut(usize),
    ) {
        let table = fold(paperpaw_sdk::digest::fnv1a64(language));
        if let Some(entries) = &self.0
            && let Ok(entries) = entries.try_borrow()
            && entries
                .get(NO_TABLE)
                .is_some_and(|entry| entry.key == table)
        {
            return;
        }
        let key = key_of(language, (left, right), word);
        let set = set_of(key);
        if let Some(entries) = &self.0
            && let Ok(entries) = entries.try_borrow()
            && let Some(entry) = entries
                .get(set..set + 2)
                .and_then(|two| two.iter().find(|entry| entry.key == key))
        {
            for at in (0..LONGEST).filter(|at| (entry.places >> at) & 1 == 1) {
                out(at);
            }
            return;
        }
        let mut places = [0u8; host_text::MAX_BREAKS];
        let asked = ask(word, &mut places);
        break_places(asked, &places, out);
        let kept = match asked {
            Ok(count) => places.iter().take(count).try_fold(0u32, |mask, &at| {
                (usize::from(at) < LONGEST).then(|| mask | (1u32 << at))
            }),
            Err(Error::NotFound) => {
                self.keep(
                    NO_TABLE,
                    Entry {
                        key: table,
                        places: 0,
                    },
                );
                None
            }
            Err(_) => None,
        };
        if word.len() <= LONGEST
            && let Some(places) = kept
        {
            self.keep_in_set(set, Entry { key, places });
        }
    }

    fn keep(&self, slot: usize, new: Entry) {
        if let Some(entries) = &self.0
            && let Ok(mut entries) = entries.try_borrow_mut()
            && let Some(entry) = entries.get_mut(slot)
        {
            *entry = new;
        }
    }

    /// Keeps `new` in a free entry of the set, else in the first, the first's
    /// word moving to the second (the older one is forgotten).
    fn keep_in_set(&self, set: usize, new: Entry) {
        if let Some(entries) = &self.0
            && let Ok(mut entries) = entries.try_borrow_mut()
            && let Some([first, second]) = entries.get_mut(set..set + 2)
        {
            if first.key != 0 {
                *second = *first;
            }
            *first = new;
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

    use core::cell::Cell;

    /// The words broken where `table`, counting what was asked of the "host".
    fn told(breaks: &Breaks, table: bool, asks: &Cell<u32>, word: &str) -> Vec<usize> {
        let ask = |_: &str, out: &mut [u8]| {
            asks.set(asks.get() + 1);
            if !table {
                return Err(Error::NotFound);
            }
            out[0] = 2;
            Ok(1)
        };
        let mut told = Vec::new();
        breaks.places(&ask, "ru", (0, 0), word, &mut |at| told.push(at));
        told
    }

    #[test]
    fn a_language_without_a_table_is_asked_once_for_the_book() {
        let (breaks, asks) = (Breaks::new().unwrap(), Cell::new(0));
        for word in ["один", "два", "три", "один"] {
            assert!(told(&breaks, false, &asks, word).is_empty());
        }
        assert_eq!(asks.get(), 1);
    }

    #[test]
    fn a_pack_installed_meanwhile_is_found_when_the_tables_are_made_again() {
        let asks = Cell::new(0);
        let breaks = Breaks::new().unwrap();
        assert!(told(&breaks, false, &asks, "слово").is_empty());
        // The reader was covered: the break places are dropped with the tables.
        let breaks = Breaks::new().unwrap();
        assert_eq!(told(&breaks, true, &asks, "слово"), [2]);
        assert_eq!(told(&breaks, true, &asks, "слово"), [2]);
        assert_eq!(asks.get(), 2);
    }

    #[test]
    fn a_set_of_two_words_is_kept_for_the_pages_around() {
        use alloc::string::String;
        let (breaks, asks) = (Breaks::new().unwrap(), Cell::new(0));
        // Words with two to a set: all of the entries but not more than two of a set.
        let mut taken = [0u8; ENTRIES];
        let words: Vec<String> = (0..400_000)
            .map(|n| alloc::format!("word{n}"))
            .filter(|word| {
                let set = set_of(key_of("ru", (0, 0), word));
                taken[set] = taken[set].saturating_add(1);
                taken[set] <= 2
            })
            .collect();
        assert_eq!(words.len(), ENTRIES);
        for word in &words {
            told(&breaks, true, &asks, word);
        }
        assert_eq!(asks.get() as usize, ENTRIES);
        for word in &words {
            told(&breaks, true, &asks, word);
        }
        assert_eq!(asks.get() as usize, ENTRIES);
    }
}
