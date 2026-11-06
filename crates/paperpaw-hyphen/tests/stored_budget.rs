//! Measures the shipped tables for the reader's memory and read budget.
//! Run `cargo test -p paperpaw-hyphen --features kernel --test stored_budget
//! -- --nocapture` to see the table.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Mem, Rng, Shipped, shipped};
use paperpaw_hyphen::{MAX_BLOCK_BYTES, MAX_EXCEPTION_BYTES, StoredPatterns, check_source};

/// Short words (one or two pattern keys, 4 to 14 letters): a stand-in for
/// running text, which has no shipped sample in every language.
fn text_words(set: &Shipped, count: usize, seed: u64) -> Vec<String> {
    let mut rng = Rng(seed | 1);
    let mut out = Vec::new();
    let all = common::words(set, 20_000, seed);
    while out.len() < count {
        let w = &all[rng.below(all.len())];
        if (4..=14).contains(&w.chars().count()) && w.chars().all(char::is_lowercase) {
            out.push(w.clone());
        }
    }
    out
}

fn reads_for(reader: &mut StoredPatterns<Mem>, words: &[String]) -> u64 {
    let before = reader.reads();
    for w in words {
        reader.hyphenate(w, &mut |_| ());
    }
    reader.reads() - before
}

#[test]
fn budget_table() {
    println!(
        "{:>3} {:>7} {:>6} {:>5} {:>6} {:>5} {:>5} | {:>6} {:>6} | cache 4: cold/warm reads per page, worst word | cache 8: cold/warm, worst",
        "set", "file", "index", "exc", "blocks", "maxb", "avgb", "heap4", "heap8"
    );
    for set in shipped() {
        let mut src = Mem::new(set.table.clone());
        let mut scratch = [0u8; 512];
        let layout = check_source(&mut src, &mut scratch).unwrap();
        let blocks = (layout.blocks as usize).max(1);
        let patterns = u64::from(layout.exceptions_at) - layout.entries_at();
        assert!(
            (layout.largest_block as usize) <= MAX_BLOCK_BYTES as usize
                && u64::from(layout.exceptions_len) as usize <= MAX_EXCEPTION_BYTES as usize
        );
        print!(
            "{:>3} {:>7} {:>6} {:>5} {:>6} {:>5} {:>5}",
            set.name,
            layout.table_len(),
            layout.entries_at(),
            u64::from(layout.exceptions_len),
            (layout.blocks as usize),
            (layout.largest_block as usize),
            patterns / blocks as u64,
        );
        let words = text_words(&set, 300 * 22, 5);
        let mut cells = Vec::new();
        let mut heaps = Vec::new();
        for cache in [4, 8] {
            let mut reader = StoredPatterns::open(Mem::new(set.table.clone()), 0, cache).unwrap();
            heaps.push(reader.heap_bytes());
            let mut cold = 0;
            for page in words.chunks(300).take(20) {
                let mut fresh =
                    StoredPatterns::open(Mem::new(set.table.clone()), 0, cache).unwrap();
                cold += reads_for(&mut fresh, page);
            }
            // Warm: the reader has seen the previous 20 pages.
            let mut warm = 0;
            reads_for(&mut reader, &words[..300 * 2]);
            for page in words.chunks(300).skip(2).take(20) {
                warm += reads_for(&mut reader, page);
            }
            let mut worst = 0;
            for w in &words[..2000] {
                worst = worst.max(reads_for(&mut reader, std::slice::from_ref(w)));
            }
            cells.push((cold / 20, warm / 20, worst));
        }
        println!(
            " | {:>6} {:>6} | {:>5}/{:>5} {:>4} | {:>5}/{:>5} {:>4}",
            heaps[0],
            heaps[1],
            cells[0].0,
            cells[0].1,
            cells[0].2,
            cells[1].0,
            cells[1].1,
            cells[1].2
        );
    }
}
