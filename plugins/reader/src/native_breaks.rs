//! Where a word may be broken at the end of a line (`core.hyphenate`): the
//! places the host's table for the book's language gives, kept for the words
//! met so that a page laid again, or the page before and after, does not ask the
//! host for the same word twice. A language the host has no table for is
//! remembered too (one more entry): no word of the book is asked then, until the
//! tables are dropped (the book is closed, a setting changes, another screen
//! covers the reader), so a pack installed meanwhile is found when the reader
//! is shown again. One fixed allocation per book (about 2 KiB),
//! made at the first page, never grown and freed with the book (or a changed
//! setting). Where there is no room the reader carries on without it: every
//! word is asked of the host.
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::native::{Error, text as host_text};

/// Words kept (a power of two; a word takes the entry its hash names).
const ENTRIES: usize = 128;
/// What asks the host for the places of a word (the number of them).
pub type Ask<'a> = dyn Fn(&str, &mut [u8]) -> Result<usize, Error> + 'a;
/// The entry after the words': the key of the language the host has no table for.
const NO_TABLE: usize = ENTRIES;
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
    /// Room for 128 words and a language without a table.
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
        let table = paperpaw_sdk::digest::fnv1a64(language) | 1;
        if let Some(entries) = &self.0
            && let Ok(entries) = entries.try_borrow()
            && entries
                .get(NO_TABLE)
                .is_some_and(|entry| entry.key == table)
        {
            return;
        }
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
        let asked = ask(word, &mut places);
        break_places(asked, &places, out);
        let kept = match asked {
            Ok(count) => places.iter().take(count).try_fold(0u64, |mask, &at| {
                (usize::from(at) < LONGEST).then(|| mask | (1u64 << at))
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
            self.keep(slot, Entry { key, places });
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
    fn a_hundred_and_twenty_eight_words_are_kept_for_the_pages_around() {
        use alloc::string::String;
        let (breaks, asks) = (Breaks::new().unwrap(), Cell::new(0));
        // Words with a slot each: two pages of a book's long words and more.
        let mut taken = [false; ENTRIES];
        let words: Vec<String> = (0..100_000)
            .map(|n| alloc::format!("word{n}"))
            .filter(|word| {
                let slot = (key_of("ru", (0, 0), word) >> 40) as usize & (ENTRIES - 1);
                !core::mem::replace(&mut taken[slot], true)
            })
            .collect();
        assert_eq!(words.len(), 128);
        for word in &words {
            told(&breaks, true, &asks, word);
        }
        assert_eq!(asks.get(), 128);
        for word in &words {
            told(&breaks, true, &asks, word);
        }
        assert_eq!(asks.get(), 128);
    }
}
