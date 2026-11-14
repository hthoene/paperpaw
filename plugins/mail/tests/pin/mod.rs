//! Answers pinned from Beta 3 (interface 6).
//!
//! The native code was compared with the interface-6 code on fixed and on
//! generated inputs while that code was still here. Its answers were written
//! down then, and the tests now check the native code against them: a
//! [`Pin`] is the transcript of one test, in `testdata/pins/<test>.pin`, one
//! line for each answer (a long answer is its length and hash). A test that
//! reads many generated inputs [`fold`](Pin::fold)s their answers into one
//! hash, so the file stays small.
//!
//! A change of what the plugin answers fails a test here; it is reviewed, and
//! `PAPERPAW_BLESS=1 cargo test -p paperpaw-plugin-mail` writes the files anew.
#![allow(dead_code, reason = "each test file uses part of the helper")]
use std::fmt::Debug;
use std::path::PathBuf;

/// Answers longer than this are pinned as their length and hash.
const LONG: usize = 160;

/// FNV-1a over `bytes`, from `hash`.
fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash = (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The transcript of one test.
pub struct Pin {
    path: PathBuf,
    expected: Vec<String>,
    seen: Vec<String>,
    bless: bool,
    folded: u64,
    hash: u64,
}

impl Pin {
    /// The transcript of the test `test` (the name of its function).
    pub fn new(test: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/pins")
            .join(format!("{test}.pin"));
        let bless = std::env::var_os("PAPERPAW_BLESS").is_some();
        let expected = if bless {
            Vec::new()
        } else {
            std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e} (PAPERPAW_BLESS=1 writes it)", path.display()))
                .lines()
                .map(str::to_owned)
                .collect()
        };
        Self {
            path,
            expected,
            seen: Vec::new(),
            bless,
            folded: 0,
            hash: 0xcbf2_9ce4_8422_2325,
        }
    }

    fn text(value: &impl Debug) -> String {
        let text = format!("{value:?}");
        if text.len() > LONG {
            format!(
                "#{}:{:016x}",
                text.len(),
                fnv(0xcbf2_9ce4_8422_2325, text.as_bytes())
            )
        } else {
            text
        }
    }

    /// The answer `value` for `what` is the pinned one.
    pub fn check(&mut self, what: impl Debug, value: impl Debug) {
        let line = format!("{}\t{}", Self::text(&what), Self::text(&value));
        assert!(
            !line.contains('\n'),
            "a pinned line holds no line break: {line:?}"
        );
        if !self.bless {
            let at = self.seen.len();
            assert_eq!(
                self.expected.get(at),
                Some(&line),
                "answer {at} of {} differs from the one pinned",
                self.path.display()
            );
        }
        self.seen.push(line);
    }

    /// Adds the answer `value` for `what` to the hash of this test's many
    /// generated inputs.
    pub fn fold(&mut self, what: impl Debug, value: impl Debug) {
        self.hash = fnv(self.hash, format!("{what:?}\t{value:?}\n").as_bytes());
        self.folded += 1;
    }

    /// The end of the test: all pinned answers were met, and the hash of the
    /// folded ones is the pinned one.
    pub fn finish(mut self) {
        if self.folded > 0 {
            let line = format!("fold\t{}\t{:016x}", self.folded, self.hash);
            if !self.bless {
                let at = self.seen.len();
                assert_eq!(
                    self.expected.get(at),
                    Some(&line),
                    "the folded answers of {} differ from the ones pinned",
                    self.path.display()
                );
            }
            self.seen.push(line);
        }
        if self.bless {
            let mut text = self.seen.join("\n");
            text.push('\n');
            std::fs::create_dir_all(self.path.parent().unwrap()).unwrap();
            std::fs::write(&self.path, text).unwrap();
        } else {
            assert_eq!(
                self.seen.len(),
                self.expected.len(),
                "{} pins answers the test did not give",
                self.path.display()
            );
        }
    }
}
