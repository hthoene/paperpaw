//! What the plugin keeps between starts, as small text files in its data
//! folder, and the names of the crossword files it reads.

use crate::sudoku::push_number;
use alloc::string::String;

/// The solved counts and the state of the puzzle of the day.
pub const STATS_FILE: &str = "stats.txt";
/// The free sudoku being played.
pub const SUDOKU_FILE: &str = "sudoku.txt";
/// The puzzle of the day.
pub const DAILY_FILE: &str = "daily.txt";
/// The folder on the card the crosswords are read from.
pub const FOLDER: &str = "puzzles";
/// The most bytes a kept file may have.
pub const KEPT_MAX: usize = 512;

/// The numbers the plugin counts: puzzles solved, the day of the puzzle of
/// the day and how far it is, the next chess puzzle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub sudoku: u32,
    pub crossword: u32,
    pub chess: u32,
    /// The day (days since 1970, local time) the puzzle of the day below is
    /// of; 0 for none.
    pub daily_day: i32,
    /// The squares of it still empty (0 with `daily_done` once solved).
    pub daily_left: u8,
    pub daily_done: bool,
    /// The chess puzzle to show next.
    pub chess_next: u32,
}

impl Stats {
    /// The kept text: `sudoku|crossword|chess|day|left|done|next`.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for n in [
            u64::from(self.sudoku),
            u64::from(self.crossword),
            u64::from(self.chess),
            u64::try_from(self.daily_day).unwrap_or(0),
            u64::from(self.daily_left),
            u64::from(self.daily_done),
            u64::from(self.chess_next),
        ] {
            push_number(&mut out, n);
            out.push('|');
        }
        out.pop();
        out
    }

    /// The stats of a kept text; none for one that is damaged.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut n = text.trim().split('|').map(|p| p.parse::<u32>().ok());
        let mut next = || n.next().flatten();
        let stats = Self {
            sudoku: next()?,
            crossword: next()?,
            chess: next()?,
            daily_day: i32::try_from(next()?).ok()?,
            daily_left: u8::try_from(next()?).ok()?,
            daily_done: next()? != 0,
            chess_next: next()?,
        };
        n.next().is_none().then_some(stats)
    }

    /// Whether the puzzle of the day on file is today's.
    #[must_use]
    pub fn daily_is(&self, today: Option<i32>) -> bool {
        today.is_some_and(|d| d == self.daily_day && d != 0)
    }

    /// Total puzzles solved.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.sudoku
            .saturating_add(self.crossword)
            .saturating_add(self.chess)
    }
}

/// The name of the file a crossword's letters are kept in: a hash of the
/// puzzle file's name, so that any name fits.
#[must_use]
pub fn crossword_file(name: &str) -> String {
    let mut hash: u32 = 0x811C_9DC5;
    for b in name.bytes() {
        hash ^= u32::from(b.to_ascii_lowercase());
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let mut out = String::from("cw-");
    for shift in (0..8).rev() {
        out.push(char::from_digit((hash >> (shift * 4)) & 15, 16).unwrap_or('0'));
    }
    out.push_str(".txt");
    out
}

/// Whether a file name in the folder is a crossword this plugin reads.
#[must_use]
pub fn is_crossword(name: &str) -> bool {
    let lower = name.rsplit_once('.').map(|(_, e)| e);
    !name.starts_with('.')
        && lower.is_some_and(|e| e.eq_ignore_ascii_case("puz") || e.eq_ignore_ascii_case("ipuz"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stats_round_trip_and_a_damaged_text_is_none() {
        let stats = Stats {
            sudoku: 12,
            crossword: 3,
            chess: 40,
            daily_day: 20_400,
            daily_left: 37,
            daily_done: false,
            chess_next: 5,
        };
        assert_eq!(Stats::parse(&stats.text()), Some(stats));
        assert_eq!(
            Stats::parse(&Stats::default().text()),
            Some(Stats::default())
        );
        for bad in [
            "",
            "1|2",
            "1|2|3|4|5|6|x",
            "1|2|3|4|5|6|7|8",
            "1|2|3|4|300|0|1",
        ] {
            assert_eq!(Stats::parse(bad), None, "{bad:?}");
        }
        assert!(stats.daily_is(Some(20_400)));
        assert!(!stats.daily_is(Some(20_401)));
        assert!(!stats.daily_is(None));
        assert_eq!(stats.total(), 55);
    }

    #[test]
    fn crossword_files_are_found_by_their_extension() {
        for yes in ["a.puz", "B.PUZ", "x.ipuz", "week 1.Puz"] {
            assert!(is_crossword(yes), "{yes}");
        }
        for no in [".hidden.puz", "a.txt", "puz", "a.puz.bak", "a.pdf"] {
            assert!(!is_crossword(no), "{no}");
        }
        assert_eq!(crossword_file("A.puz"), crossword_file("a.PUZ"));
        assert_ne!(crossword_file("a.puz"), crossword_file("b.puz"));
        assert!(crossword_file("a.puz").len() < 20);
    }
}
