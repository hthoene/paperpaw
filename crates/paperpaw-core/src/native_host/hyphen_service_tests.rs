#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! `text.hyphenate` over a fake card: the tag fallback, the checks, the open
//! tables' limits and the failure rules of `document.md` section 6.1.
extern crate std;
use super::*;
use crate::native_host::resources::HostResources;
use alloc::rc::Rc;
use core::cell::{Cell, RefCell};
use paperpaw_hyphen::compile;
use std::collections::BTreeMap;

struct File {
    data: Vec<u8>,
    fail: Rc<Cell<bool>>,
}
impl HyphenFile for File {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> bool {
        if self.fail.get() {
            return false;
        }
        let Some(part) = usize::try_from(at)
            .ok()
            .and_then(|at| self.data.get(at..at.checked_add(buf.len())?))
        else {
            return false;
        };
        buf.copy_from_slice(part);
        true
    }
}

struct Card {
    /// lowercase tag -> (key, table bytes)
    tables: RefCell<BTreeMap<String, (u64, Vec<u8>)>>,
    epoch: Cell<u64>,
    fail: Rc<Cell<bool>>,
    opens: Cell<u32>,
}
impl Card {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            tables: RefCell::new(BTreeMap::new()),
            epoch: Cell::new(1),
            fail: Rc::new(Cell::new(false)),
            opens: Cell::new(0),
        })
    }
    fn add(&self, tag: &str, bytes: Vec<u8>) {
        let key = self.tables.borrow().len() as u64 + 1;
        self.tables
            .borrow_mut()
            .insert(tag.to_ascii_lowercase(), (key, bytes));
    }
}
impl HyphenHost for Card {
    fn find(&self, tag: &str) -> Option<u64> {
        self.tables
            .borrow()
            .get(&tag.to_ascii_lowercase())
            .map(|t| t.0)
    }
    fn open(&self, key: u64) -> Result<(Box<dyn HyphenFile>, u64), i32> {
        self.opens.set(self.opens.get() + 1);
        let tables = self.tables.borrow();
        let (_, bytes) = tables.values().find(|t| t.0 == key).ok_or(ERR_NOT_FOUND)?;
        Ok((
            Box::new(File {
                data: bytes.clone(),
                fail: self.fail.clone(),
            }),
            key,
        ))
    }
    fn card_epoch(&self) -> u64 {
        self.epoch.get()
    }
}

/// Breaks before every `b` (and `c` where asked), at least one letter each side.
fn table(extra: &str) -> Vec<u8> {
    compile(&std::format!("1b {extra}"), "", 1, 1)
}

fn state(card: &Rc<Card>) -> HyphenState {
    HyphenState::new(card.clone())
}

fn ask(state: &HyphenState, tag: &str, word: &str, left: u32, right: u32) -> Result<Vec<u8>, i32> {
    let mut out = [0u8; 64];
    hyphenate(
        state,
        tag.as_bytes(),
        word.as_bytes(),
        left,
        right,
        &mut out,
    )
    .map(|n| out[..n].to_vec())
}

#[test]
fn breaks_are_byte_offsets_ascending_and_tags_fall_back() {
    let card = Card::new();
    card.add("de", table(""));
    let s = state(&card);
    assert_eq!(ask(&s, "de", "abab", 1, 1), Ok(std::vec![1, 3]));
    // Case is ignored in the tag and the word; the tag loses its last part.
    assert_eq!(ask(&s, "DE-ch-1996", "ABAB", 1, 1), Ok(std::vec![1, 3]));
    assert_eq!(ask(&s, "en", "abab", 1, 1), Err(ERR_NOT_FOUND));
    assert_eq!(ask(&s, "de-x", "abab", 1, 1), Ok(std::vec![1, 3]));
    // Minimums: the table's own (0), or the caller's.
    assert_eq!(ask(&s, "de", "abab", 0, 0), Ok(std::vec![1, 3]));
    assert_eq!(ask(&s, "de", "abab", 2, 1), Ok(std::vec![3]));
    // A character boundary, in bytes: "äb" breaks after two bytes.
    assert_eq!(ask(&s, "de", "äbä", 1, 1), Ok(std::vec![2]));
}

#[test]
fn the_calls_are_checked_before_any_table_is_read() {
    let card = Card::new();
    card.add("de", table(""));
    let s = state(&card);
    assert_eq!(ask(&s, "", "ab", 1, 1), Err(ERR_INVALID));
    assert_eq!(ask(&s, "d_e", "ab", 1, 1), Err(ERR_INVALID));
    assert_eq!(ask(&s, &"a".repeat(36), "ab", 1, 1), Err(ERR_INVALID));
    assert_eq!(ask(&s, "de", &"a".repeat(193), 1, 1), Err(ERR_INVALID));
    assert_eq!(ask(&s, "de", "ab", 17, 1), Err(ERR_INVALID));
    assert_eq!(ask(&s, "de", "ab", 1, 17), Err(ERR_INVALID));
    let mut out = [0u8; 8];
    assert_eq!(
        hyphenate(&s, b"de", &[0xff, 0xfe], 1, 1, &mut out),
        Err(ERR_UTF8)
    );
    assert_eq!(card.opens.get(), 0, "nothing was opened for a bad call");
}

#[test]
fn an_empty_word_asks_whether_there_is_a_table() {
    let card = Card::new();
    card.add("de", table(""));
    let s = state(&card);
    assert_eq!(ask(&s, "de", "", 0, 0), Ok(std::vec![]));
    assert_eq!(ask(&s, "fr", "", 0, 0), Err(ERR_NOT_FOUND));
    assert_eq!(card.opens.get(), 0);
}

#[test]
fn short_and_long_words_are_left_whole() {
    let card = Card::new();
    card.add("de", table(""));
    let s = state(&card);
    assert_eq!(
        ask(&s, "de", "ab", 2, 2),
        Ok(std::vec![]),
        "below left + right"
    );
    let long = "ab".repeat(25);
    assert_eq!(
        ask(&s, "de", &long, 1, 1),
        Ok(std::vec![]),
        "over 48 characters"
    );
    let edge = "ab".repeat(24);
    assert_eq!(
        ask(&s, "de", &edge, 1, 1).unwrap().len(),
        24,
        "48 characters"
    );
}

#[test]
fn an_out_that_is_too_small_gets_nothing() {
    let card = Card::new();
    card.add("de", table(""));
    let s = state(&card);
    let mut out = [9u8; 1];
    assert_eq!(
        hyphenate(&s, b"de", b"abab", 1, 1, &mut out),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(out, [9], "nothing written");
    let mut none: [u8; 0] = [];
    assert_eq!(hyphenate(&s, b"de", b"aaaa", 1, 1, &mut none), Ok(0));
}

#[test]
fn two_tables_stay_open_and_a_third_waits_for_one_to_go_idle() {
    let card = Card::new();
    for tag in ["de", "en", "fr"] {
        card.add(tag, table(""));
    }
    let s = state(&card);
    ask(&s, "de", "abab", 1, 1).unwrap();
    ask(&s, "en", "abab", 1, 1).unwrap();
    assert_eq!(s.open_count(), 2);
    assert!(s.open_heap() <= HEAP_BUDGET);
    // Both are recent: the third language's words are left whole.
    assert_eq!(ask(&s, "fr", "abab", 1, 1), Ok(std::vec![]));
    assert_eq!(s.open_count(), 2);
    // `en` keeps being used, `de` goes idle for 256 words: `fr` replaces it.
    for _ in 0..IDLE_WORDS {
        ask(&s, "en", "abab", 1, 1).unwrap();
    }
    assert_eq!(ask(&s, "fr", "abab", 1, 1), Ok(std::vec![1, 3]));
    assert_eq!(s.open_count(), 2);
    let opens = card.opens.get();
    ask(&s, "en", "abab", 1, 1).unwrap();
    assert_eq!(card.opens.get(), opens, "en was never closed");
}

#[test]
fn a_failed_read_closes_the_table_and_a_second_one_ends_the_language() {
    let card = Card::new();
    card.add("de", table(""));
    let s = state(&card);
    // Words so that the reads are needed: the first fails after opening.
    ask(&s, "de", "abab", 1, 1).unwrap();
    card.fail.set(true);
    // Already open, its cache may serve; force a fresh read by a new epoch-free
    // table: close it by failing the open.
    s.tables.borrow_mut().open.clear();
    assert_eq!(
        ask(&s, "de", "abab", 1, 1),
        Ok(std::vec![]),
        "first failure"
    );
    assert_eq!(
        ask(&s, "de", "abab", 1, 1),
        Ok(std::vec![]),
        "reopened, fails"
    );
    assert_eq!(
        ask(&s, "de", "abab", 1, 1),
        Err(ERR_NOT_FOUND),
        "second failure"
    );
    assert_eq!(ask(&s, "de", "", 0, 0), Err(ERR_NOT_FOUND));
    // Until the card changes.
    card.fail.set(false);
    assert_eq!(ask(&s, "de", "abab", 1, 1), Err(ERR_NOT_FOUND));
    card.epoch.set(2);
    assert_eq!(ask(&s, "de", "abab", 1, 1), Ok(std::vec![1, 3]));
}

#[test]
fn a_table_that_is_not_sound_is_not_found_at_once() {
    let card = Card::new();
    card.add("de", std::vec![0u8; 100]);
    card.add("en", table(""));
    let s = state(&card);
    assert_eq!(ask(&s, "de", "abab", 1, 1), Ok(std::vec![]));
    assert_eq!(ask(&s, "de", "abab", 1, 1), Err(ERR_NOT_FOUND));
    // Another language is untouched; so is a shorter tag for the same word.
    assert_eq!(ask(&s, "en", "abab", 1, 1), Ok(std::vec![1, 3]));
}

#[test]
fn a_table_opened_before_the_card_changed_is_opened_again() {
    let card = Card::new();
    card.add("de", table(""));
    let s = state(&card);
    ask(&s, "de", "abab", 1, 1).unwrap();
    assert_eq!(card.opens.get(), 1);
    ask(&s, "de", "abab", 1, 1).unwrap();
    assert_eq!(card.opens.get(), 1, "kept open");
    card.epoch.set(5);
    ask(&s, "de", "abab", 1, 1).unwrap();
    assert_eq!(card.opens.get(), 2, "the card changed");
}

#[test]
fn the_slot_checks_its_pairs_and_writes_the_places() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let call = |word: &[u8], out: &mut [core::mem::MaybeUninit<u8>]| unsafe {
        slot(
            &owner,
            PpBytes {
                ptr: b"de".as_ptr(),
                len: 2,
            },
            PpBytes {
                ptr: word.as_ptr(),
                len: word.len(),
            },
            1,
            1,
            PpBuffer {
                ptr: out.as_mut_ptr().cast(),
                cap: out.len(),
            },
        )
    };
    let mut out = [core::mem::MaybeUninit::<u8>::uninit(); 64];
    assert_eq!(call(b"abab", &mut out), ERR_UNSUPPORTED, "no host wired");
    let card = Card::new();
    card.add("de", table(""));
    pool.hosts().wire_hyphen(card).unwrap();
    assert_eq!(call(b"abab", &mut out), 2);
    // SAFETY: two bytes were written.
    let written = unsafe { [out[0].assume_init(), out[1].assume_init()] };
    assert_eq!(written, [1, 3]);
    assert_eq!(call(&[0xff], &mut out), ERR_UTF8);
    assert_eq!(call(&[b'a'; 193], &mut out), ERR_INVALID);
    let mut small = [core::mem::MaybeUninit::<u8>::uninit(); 1];
    assert_eq!(call(b"abab", &mut small), ERR_NO_SPACE);
}
