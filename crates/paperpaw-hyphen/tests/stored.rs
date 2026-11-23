//! The card-backed reader gives the hyphenation of the table in memory, and
//! fails cleanly on a table or a card that misbehaves.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Failed, Mem, Rng, Shipped, shipped, show, words};
use paperpaw_hyphen::{
    CheckError, MAX_BLOCK_BYTES, MAX_EXCEPTION_BYTES, OpenError, Patterns, StoredPatterns,
    TableError, check_table, compile,
};

fn open(table: &[u8], cache: usize) -> StoredPatterns<Mem> {
    StoredPatterns::open(Mem::new(table.to_vec()), 7, cache).unwrap()
}

fn breaks_of(patterns: &Patterns<'_>, word: &str) -> Vec<usize> {
    let mut breaks = Vec::new();
    patterns.hyphenate(word, &mut |at| breaks.push(at));
    breaks
}

fn stored_breaks(reader: &mut StoredPatterns<Mem>, word: &str) -> Vec<usize> {
    let mut breaks = Vec::new();
    reader.hyphenate(word, &mut |at| breaks.push(at));
    breaks
}

#[test]
fn every_shipped_table_hyphenates_like_the_slice() {
    for set in shipped() {
        let patterns = Patterns::new(&set.table).unwrap();
        // One block cached: every change of block evicts.
        for cache in [1, 3] {
            let mut reader = open(&set.table, cache);
            assert_eq!(reader.minimums(), patterns.minimums(), "{}", set.name);
            let mut hyphenated = 0;
            for word in words(&set, 3000, 0x9e37_79b9) {
                let want = breaks_of(&patterns, &word);
                hyphenated += usize::from(!want.is_empty());
                assert_eq!(
                    stored_breaks(&mut reader, &word),
                    want,
                    "{} cache {cache}: {}",
                    set.name,
                    show(&word, &want)
                );
            }
            assert!(!reader.is_failed(), "{}", set.name);
            assert!(
                hyphenated > 100,
                "{}: the word list finds no breaks",
                set.name
            );
        }
    }
}

#[test]
fn minimums_and_exceptions_are_the_slices() {
    let table = compile("1b", "ab-ab", 2, 2);
    let patterns = Patterns::new(&table).unwrap();
    for (l, r) in [(0, 0), (1, 1), (1, 0), (0, 1), (3, 0), (1000, 1000)] {
        let want = patterns.with_minimums(l, r);
        let mut reader = open(&table, 2).with_minimums(l, r);
        assert_eq!(reader.minimums(), want.minimums());
        // One reader, the minimums given word by word: the same, and the
        // reader's own stay as they were.
        let mut shared = open(&table, 2);
        for word in ["abab", "ababab", "bbbb", "a", ""] {
            assert_eq!(
                stored_breaks(&mut reader, word),
                breaks_of(&want, word),
                "{word}"
            );
            let mut breaks = Vec::new();
            shared.hyphenate_with_minimums(l, r, word, &mut |at| breaks.push(at));
            assert_eq!(breaks, breaks_of(&want, word), "{word}");
        }
        assert_eq!(shared.minimums(), patterns.minimums());
    }
}

#[test]
fn empty_and_tiny_tables() {
    for table in [
        compile("", "", 1, 1),
        compile("a1", "", 1, 1),
        compile("", "ab-c", 1, 1),
    ] {
        let patterns = Patterns::new(&table).unwrap();
        let mut reader = open(&table, 2);
        for word in ["abc", "aaa", "ab", "ababab"] {
            assert_eq!(stored_breaks(&mut reader, word), breaks_of(&patterns, word));
        }
        assert!(!reader.is_failed());
    }
}

fn de() -> Shipped {
    shipped().into_iter().find(|s| s.name == "de").unwrap()
}

/// The header, index and exception boundaries of a table.
fn boundaries(table: &[u8]) -> Vec<usize> {
    let word = |at: usize| u32::from_le_bytes(table[at..at + 4].try_into().unwrap()) as usize;
    let (blocks, exc_at) = (word(12), word(16));
    let mut cuts = vec![0, 1, 4, 5, 8, 12, 16, 20, 23, 24, 25, 27];
    for at in [24 + 4 * blocks, exc_at, table.len()] {
        cuts.extend([at.saturating_sub(1), at, at + 1]);
    }
    cuts.extend([24 + 2 * blocks, 24 + 4 * blocks - 1]);
    cuts.retain(|&c| c < table.len());
    cuts
}

#[test]
fn truncation_and_padding_are_refused() {
    for table in [de().table, compile("a1 b2", "ab-ab cd-ef", 1, 1)] {
        let mut cuts = boundaries(&table);
        // Every length near the end walks through the exceptions.
        cuts.extend(table.len().saturating_sub(40)..table.len());
        for cut in cuts {
            assert!(check_table(&table[..cut]).is_err(), "cut at {cut}");
            let opened = StoredPatterns::open(Mem::new(table[..cut].to_vec()), 0, 2);
            assert!(opened.is_err(), "cut at {cut}");
        }
        let mut padded = table.clone();
        padded.push(0);
        assert_eq!(check_table(&padded), Err(TableError::Trailing));
        assert!(check_table(&table).is_ok());
    }
}

#[test]
fn header_faults_name_their_part() {
    let table = compile("a1 b2 c3", "ab-ab", 1, 1);
    let with = |at: usize, value: u32| {
        let mut t = table.clone();
        t[at..at + 4].copy_from_slice(&value.to_le_bytes());
        t
    };
    assert_eq!(check_table(b"nope"), Err(TableError::Magic));
    let mut old = table.clone();
    old[0] = b'Q';
    assert_eq!(check_table(&old), Err(TableError::Magic));
    assert_eq!(check_table(&table[..10]), Err(TableError::Header));
    assert_eq!(check_table(&with(12, u32::MAX)), Err(TableError::Header));
    assert_eq!(check_table(&with(16, u32::MAX)), Err(TableError::Header));
    assert_eq!(check_table(&with(16, 24)), Err(TableError::Header));
    assert_eq!(check_table(&with(20, 99)), Err(TableError::Exceptions));
    assert_eq!(check_table(&with(20, 0)), Err(TableError::Trailing));
    // Index: not ascending, or behind the patterns.
    assert_eq!(check_table(&with(24, 1_000)), Err(TableError::Index));
    let mut equal = table.clone();
    equal[24..28].copy_from_slice(&0u32.to_le_bytes());
    assert!(check_table(&equal).is_ok());
    let many = compile(
        "a1 b2 c3 d4 e5 f6 g7 h8 i9 j1 k2 l3 m4 n5 o6 p7 q8 r9 s1 t2",
        "",
        1,
        1,
    );
    let blocks = u32::from_le_bytes(many[12..16].try_into().unwrap());
    assert_eq!(blocks, 2);
    let mut flat = many.clone();
    flat[28..32].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(check_table(&flat), Err(TableError::Index));
    // Exceptions that are not UTF-8.
    let mut bad = table.clone();
    let n = bad.len();
    bad[n - 1] = 0xff;
    assert_eq!(check_table(&bad), Err(TableError::Exceptions));
}

#[test]
fn small_scratch_is_refused() {
    let table = compile("a1", "", 1, 1);
    let mut src = Mem::new(table);
    let mut scratch = [0u8; 255];
    assert_eq!(
        paperpaw_hyphen::check_source(&mut src, &mut scratch).unwrap_err(),
        CheckError::Scratch
    );
}

#[test]
fn random_mutations_never_panic() {
    let small = compile(
        "hy3ph he2n hena4 hen5at 1na n2at 1tio 2io o2n",
        "ta-ble re-cord",
        2,
        3,
    );
    let big = de().table;
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let probes = [
        "hyphenation",
        "table",
        "record",
        "Donaudampfschifffahrt",
        "Mädchen",
        "ab",
    ];
    for (table, runs) in [(&small, 4000), (&big, 400)] {
        for _ in 0..runs {
            let mut t = table.clone();
            for _ in 0..1 + rng.below(4) {
                let at = if rng.below(2) == 0 {
                    rng.below(40.min(t.len()))
                } else {
                    rng.below(t.len())
                };
                t[at] = rng.next() as u8;
            }
            if rng.below(8) == 0 {
                t.truncate(rng.below(t.len()));
            }
            let sound = check_table(&t).is_ok();
            let Ok(mut reader) = StoredPatterns::open(Mem::new(t.clone()), 1, 2) else {
                continue;
            };
            assert!(sound);
            for word in probes {
                let mut out = Vec::new();
                reader.hyphenate(word, &mut |at| out.push(at));
                if reader.is_failed() {
                    assert!(out.is_empty());
                }
            }
            // The slice reader does not panic on it either.
            if let Ok(patterns) = Patterns::new(&t) {
                for word in probes {
                    patterns.hyphenate(word, &mut |_| ());
                }
            }
        }
    }
}

#[test]
fn a_read_that_fails_poisons_cleanly() {
    let set = de();
    let patterns = Patterns::new(&set.table).unwrap();
    let list = words(&set, 60, 99);
    // Reads while opening, then while hyphenating.
    // The first k that no longer fails the open is the number of reads it
    // makes plus one.
    let open_reads = (1..)
        .find(|&k| {
            let mut src = Mem::new(set.table.clone());
            src.fail_at = Some(k);
            StoredPatterns::open(src, 0, 2).is_ok()
        })
        .unwrap()
        - 1;
    let mut total = 0;
    let mut reader = open(&set.table, 2);
    for w in &list {
        reader.hyphenate(w, &mut |_| ());
    }
    total += reader.reads();
    assert!(total > 20);
    for k in 1..=(open_reads + 40) {
        let mut src = Mem::new(set.table.clone());
        src.fail_at = Some(k);
        match StoredPatterns::open(src, 0, 2) {
            Err(OpenError::Check(CheckError::Source(Failed))) => assert!(k <= open_reads),
            Err(other) => panic!("read {k}: {other:?}"),
            Ok(mut reader) => {
                assert!(k > open_reads);
                let mut failed_at = None;
                for (i, w) in list.iter().enumerate() {
                    let mut out = Vec::new();
                    reader.hyphenate(w, &mut |at| out.push(at));
                    if reader.is_failed() && failed_at.is_none() {
                        failed_at = Some(i);
                        assert!(out.is_empty(), "read {k}: partial breaks");
                    }
                    if failed_at.is_some() {
                        assert!(out.is_empty());
                    } else {
                        assert_eq!(out, breaks_of(&patterns, w));
                    }
                }
                assert!(failed_at.is_some(), "read {k} never reached");
            }
        }
    }
}

#[test]
fn a_replaced_or_shrunk_file_is_noticed_or_fails() {
    let set = de();
    let mut reader = open(&set.table, 2);
    let len = set.table.len() as u64;
    assert!(reader.still_valid(7, len));
    assert!(!reader.still_valid(8, len));
    assert!(!reader.still_valid(7, len + 1));
    assert!(!reader.still_valid(7, len - 1));
    // The card's file shrinks while the reader is open: reads past what the
    // source can give fail instead of being applied to the old index.
    reader.hyphenate("Donaudampfschifffahrt", &mut |_| ());
    assert!(!reader.is_failed());
    let mut src = Mem::new(set.table[..set.table.len() / 2].to_vec());
    src.claimed = Some(len);
    let mut reader = StoredPatterns::open(src, 7, 1);
    // A source that claims more than it has: the check fails at some read.
    assert!(reader.is_err());
    let _ = reader.as_mut().map(|r| r.is_failed());
}

/// A table that reads `then` instead of `first` from the second read of its
/// header on: a file changed between the check and the rest of the open.
struct ChangedAfterCheck {
    first: Mem,
    then: Mem,
    headers: u32,
}

impl paperpaw_hyphen::Source for ChangedAfterCheck {
    type Error = Failed;

    fn len(&self) -> u64 {
        paperpaw_hyphen::Source::len(&self.first)
    }

    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), Failed> {
        self.headers += u32::from(at == 0);
        let src = if self.headers > 1 {
            &mut self.then
        } else {
            &mut self.first
        };
        paperpaw_hyphen::Source::read_at(src, at, buf)
    }
}

#[test]
fn an_index_that_goes_back_after_the_check_is_refused() {
    let set = de();
    // The third block said to start before the second, the header as it was.
    let mut then = set.table.clone();
    let second = u32::from_le_bytes(then[28..32].try_into().unwrap());
    then[32..36].copy_from_slice(&(second - 1).to_le_bytes());
    let src = ChangedAfterCheck {
        first: Mem::new(set.table.clone()),
        then: Mem::new(then),
        headers: 0,
    };
    assert_eq!(
        StoredPatterns::open(src, 7, 1).err(),
        Some(OpenError::from(TableError::Header))
    );
}

#[test]
fn caps_hold_for_the_shipped_tables() {
    for set in shipped() {
        let mut src = Mem::new(set.table.clone());
        let mut scratch = [0u8; 512];
        let layout = paperpaw_hyphen::check_source(&mut src, &mut scratch).unwrap();
        // Room to grow: at least a third more than any shipped table.
        assert!(
            (layout.largest_block as usize) * 4 <= MAX_BLOCK_BYTES as usize * 3,
            "{}",
            set.name
        );
        assert!(
            u64::from(layout.exceptions_len) as usize * 2 <= MAX_EXCEPTION_BYTES as usize,
            "{}",
            set.name
        );
    }
}

/// A table with the given index offsets, `patterns` bytes of patterns and
/// `exceptions` bytes after them.
fn raw(offsets: &[u32], patterns: usize, exceptions: &[u8], count: u32) -> Vec<u8> {
    let entries = 24 + 4 * offsets.len();
    let mut table = b"PPHP\x01\x02\x02\x05\0\0\0\0".to_vec();
    let blocks = u32::try_from(offsets.len()).unwrap();
    let at = u32::try_from(entries + patterns).unwrap();
    for word in [blocks, at, count] {
        table.extend_from_slice(&word.to_le_bytes());
    }
    for offset in offsets {
        table.extend_from_slice(&offset.to_le_bytes());
    }
    table.resize(entries + patterns, b'a');
    table.extend_from_slice(exceptions);
    table
}

#[test]
fn the_reader_refuses_what_the_shared_limits_refuse() {
    let too_large = Err(OpenError::Check(CheckError::Table(TableError::TooLarge)));
    let refuse = |table: Vec<u8>| StoredPatterns::open(Mem::new(table), 7, 1).map(|_| ());
    let ones = |n: u32| (0..n).collect::<Vec<u32>>();
    // At each limit the size is accepted (the filler bytes are no real patterns,
    // so opening may still fail for that), one more is refused for its size.
    let n = paperpaw_hyphen::MAX_BLOCKS;
    assert_ne!(refuse(raw(&ones(n), n as usize, &[], 0)), too_large);
    assert_eq!(refuse(raw(&ones(n + 1), n as usize + 1, &[], 0)), too_large);
    let n = MAX_BLOCK_BYTES as usize;
    assert_ne!(refuse(raw(&[0], n, &[], 0)), too_large);
    assert_eq!(refuse(raw(&[0], n + 1, &[], 0)), too_large);
    // Exceptions: 64 words of 127 bytes are exactly 8 KiB; one more byte is refused.
    let mut words = Vec::new();
    for _ in 0..64 {
        words.push(127);
        words.resize(words.len() + 127, b'a');
    }
    assert_eq!(words.len(), MAX_EXCEPTION_BYTES as usize);
    assert!(refuse(raw(&[], 0, &words, 64)).is_ok());
    words.push(0);
    assert_eq!(refuse(raw(&[], 0, &words, 65)), too_large);
    // The slice check and the reader agree.
    assert_eq!(
        check_table(&raw(&[0], n + 1, &[], 0)),
        Err(TableError::TooLarge)
    );
}

/// A [`Mem`] that counts its reads where the test can see them.
struct Counted(Mem, std::rc::Rc<std::cell::Cell<u64>>);

impl paperpaw_hyphen::Source for Counted {
    type Error = Failed;

    fn len(&self) -> u64 {
        paperpaw_hyphen::Source::len(&self.0)
    }

    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), Failed> {
        self.1.set(self.1.get() + 1);
        paperpaw_hyphen::Source::read_at(&mut self.0, at, buf)
    }
}

#[test]
fn opening_reads_each_sector_of_the_patterns_about_once() {
    for set in shipped() {
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let src = Counted(Mem::new(set.table.clone()), count.clone());
        let blocks = paperpaw_hyphen::check_table(&set.table).unwrap().blocks;
        StoredPatterns::open(src, 7, 1).unwrap();
        let sectors = (set.table.len() as u64).div_ceil(512);
        let reads = count.get();
        println!(
            "{}: {reads} reads to open, {blocks} blocks, {sectors} sectors",
            set.name
        );
        // Not one read per block: one pass makes the fences, the filter and
        // the short patterns, in windows of whole blocks (a window ends
        // before the block that does not fit, so a little over one read per
        // sector), plus the check, the index and the exceptions.
        assert!(
            reads <= sectors + sectors / 4 + 16,
            "{}: {reads} reads",
            set.name
        );
        assert!(
            reads < u64::from(blocks.max(1)) + 16,
            "{}: {reads} reads",
            set.name
        );
    }
}

/// Most of what a word asks of a table is no pattern: where the table is
/// small enough to keep a filter of its keys, such keys read no block, so a
/// word costs a few reads instead of about twenty, and says the same.
#[test]
fn a_small_table_reads_few_blocks_for_a_word() {
    let set = shipped()
        .into_iter()
        .find(|s| s.name == "hyphenation")
        .unwrap();
    let words = common::words(&set, 400, 3);
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut stored =
        StoredPatterns::open(Counted(Mem::new(set.table.clone()), count.clone()), 1, 4).unwrap();
    let whole = Patterns::new(&set.table).unwrap();
    let after_open = count.get();
    for word in &words {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        stored.hyphenate(word, &mut |at| a.push(at));
        whole.hyphenate(word, &mut |at| b.push(at));
        assert_eq!(a, b, "{word}");
    }
    let reads = count.get() - after_open;
    assert!(
        reads < 8 * words.len() as u64,
        "{reads} reads for {} words",
        words.len()
    );
}

/// The large tables (no filter) keep their patterns of one or two letters,
/// so a word costs about half the reads it did (30, 28 and 25 a word for `de`,
/// `nb` and `nl` before): the ceiling pins that, and the breaks are the
/// slice's.
#[test]
fn a_large_table_reads_about_half_the_blocks_for_a_word() {
    for set in shipped()
        .into_iter()
        .filter(|s| ["de", "nb", "nl"].contains(&s.name))
    {
        let words: Vec<String> = words(&set, 6000, 7)
            .into_iter()
            .filter(|w| (6..=14).contains(&w.chars().count()) && w.chars().all(char::is_lowercase))
            .take(1000)
            .collect();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut stored =
            StoredPatterns::open(Counted(Mem::new(set.table.clone()), count.clone()), 1, 8)
                .unwrap();
        let whole = Patterns::new(&set.table).unwrap();
        let after_open = count.get();
        for word in &words {
            let (mut a, mut b) = (Vec::new(), Vec::new());
            stored.hyphenate(word, &mut |at| a.push(at));
            whole.hyphenate(word, &mut |at| b.push(at));
            assert_eq!(a, b, "{}: {word}", set.name);
        }
        let reads = count.get() - after_open;
        assert!(
            reads <= 16 * words.len() as u64,
            "{}: {reads} reads for {} words",
            set.name,
            words.len()
        );
    }
}

/// A table with more patterns of one or two letters than are kept (here 676
/// of 26 letters) keeps none, and says the same as the slice.
#[test]
fn a_large_table_with_too_many_short_patterns_is_read_from_the_card() {
    let letters: Vec<char> = ('a'..='z').collect();
    let mut patterns = String::new();
    for &a in &letters {
        for &b in &letters {
            patterns.push_str(&format!("{a}1{b} "));
            for &c in &letters {
                patterns.push_str(&format!("{a}{b}2{c} "));
            }
        }
    }
    let table = compile(&patterns, "", 1, 1);
    assert!(check_table(&table).unwrap().blocks > 512);
    let mut stored = open(&table, 8);
    let whole = Patterns::new(&table).unwrap();
    let mut rng = Rng(11);
    for _ in 0..300 {
        let word: String = (0..4 + rng.below(8))
            .map(|_| letters[rng.below(letters.len())])
            .collect();
        assert_eq!(
            stored_breaks(&mut stored, &word),
            breaks_of(&whole, &word),
            "{word}"
        );
    }
    assert!(!stored.is_failed());
}
