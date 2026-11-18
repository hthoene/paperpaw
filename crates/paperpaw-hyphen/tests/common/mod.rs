//! The shipped pattern sets, word lists and sources for the reader tests.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::path::PathBuf;

use paperpaw_hyphen::{Source, compile};

/// (plugin folder, patterns, exceptions, left, right), as in the plugins'
/// `build.rs`.
pub const SHIPPED: &[(&str, &str, Option<&str>, u8, u8)] = &[
    (
        "hyphenation",
        "hyph-en-us.pat.txt",
        Some("hyph-en-us.hyp.txt"),
        2,
        3,
    ),
    ("hyphenation-da", "hyph-da.pat.txt", None, 2, 2),
    ("hyphenation-de", "hyph-de-1996.pat.txt", None, 2, 2),
    ("hyphenation-es", "hyph-es.pat.txt", None, 2, 2),
    ("hyphenation-fr", "hyph-fr.pat.txt", None, 2, 2),
    ("hyphenation-it", "hyph-it.pat.txt", None, 2, 2),
    (
        "hyphenation-nb",
        "hyph-nb.pat.txt",
        Some("hyph-nb.hyp.txt"),
        2,
        2,
    ),
    (
        "hyphenation-nl",
        "hyph-nl.pat.txt",
        Some("hyph-nl.hyp.txt"),
        2,
        2,
    ),
    (
        "hyphenation-pl",
        "hyph-pl.pat.txt",
        Some("hyph-pl.hyp.txt"),
        2,
        2,
    ),
    (
        "hyphenation-pt",
        "hyph-pt.pat.txt",
        Some("hyph-pt.hyp.txt"),
        2,
        3,
    ),
    (
        "hyphenation-ru",
        "hyph-ru.pat.txt",
        Some("hyph-ru.hyp.txt"),
        2,
        2,
    ),
    ("hyphenation-sv", "hyph-sv.pat.txt", None, 2, 2),
    ("hyphenation-uk", "hyph-uk.pat.txt", None, 2, 2),
];

pub struct Shipped {
    pub name: &'static str,
    pub patterns: String,
    pub exceptions: String,
    pub table: Vec<u8>,
}

fn plugin_file(folder: &str, file: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "plugins",
        folder,
        "patterns",
        file,
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn shipped() -> Vec<Shipped> {
    SHIPPED
        .iter()
        .map(|&(folder, pat, hyp, left, right)| {
            let patterns = plugin_file(folder, pat);
            let exceptions = hyp.map(|h| plugin_file(folder, h)).unwrap_or_default();
            let table = compile(&patterns, &exceptions, left, right);
            Shipped {
                name: folder.trim_start_matches("hyphenation-"),
                patterns,
                exceptions,
                table,
            }
        })
        .collect()
}

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Words made of the letters of the patterns and exceptions: every
/// exception, and words glued from pattern keys (many are not words, which
/// is fine: the two readers must agree on everything).
pub fn words(set: &Shipped, count: usize, seed: u64) -> Vec<String> {
    let mut rng = Rng(seed | 1);
    let keys: Vec<String> = set
        .patterns
        .split_whitespace()
        .map(|p| {
            p.chars()
                .filter(|c| !c.is_ascii_digit() && *c != '.')
                .collect::<String>()
        })
        .filter(|k| !k.is_empty())
        .collect();
    let mut out: Vec<String> = set
        .exceptions
        .split_whitespace()
        .map(|w| w.chars().filter(|&c| c != '-').collect())
        .collect();
    for _ in 0..count {
        let mut word = String::new();
        let parts = 1 + rng.below(4);
        for _ in 0..parts {
            word.push_str(&keys[rng.below(keys.len().max(1))]);
        }
        if rng.below(5) == 0 {
            word = word.to_uppercase();
        }
        out.push(word);
    }
    out
}

/// A table in memory that counts reads and can fail at the `fail_at`th.
pub struct Mem {
    pub bytes: Vec<u8>,
    pub reads: u64,
    pub fail_at: Option<u64>,
    /// Pretends to be this long (for a file that changed).
    pub claimed: Option<u64>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Failed;

impl Mem {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            reads: 0,
            fail_at: None,
            claimed: None,
        }
    }
}

impl Source for Mem {
    type Error = Failed;

    fn len(&self) -> u64 {
        self.claimed.unwrap_or(self.bytes.len() as u64)
    }

    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), Failed> {
        self.reads += 1;
        if self.fail_at == Some(self.reads) {
            return Err(Failed);
        }
        let from = usize::try_from(at).map_err(|_| Failed)?;
        let slice = self.bytes.get(from..from + buf.len()).ok_or(Failed)?;
        buf.copy_from_slice(slice);
        Ok(())
    }
}

/// The breaks `word` gets, as the word with `-` inserted.
pub fn show(word: &str, breaks: &[usize]) -> String {
    let mut out = String::new();
    for (i, c) in word.char_indices() {
        if breaks.contains(&i) {
            out.push('-');
        }
        out.push(c);
    }
    out
}
