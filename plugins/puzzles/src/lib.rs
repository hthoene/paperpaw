//! Puzzles: sudoku, crosswords and chess puzzles, offline.
//!
//! - *Sudoku*: puzzles made on the device with exactly one solution, three
//!   levels, button-only input (move, pick a digit), pencil marks, check,
//!   undo; the game is kept. A *Puzzle of the day* (the same sudoku all day)
//!   has a home widget.
//! - *Crosswords*: `.puz` (Across Lite) and `ipuz` files from the card's
//!   folder `puzzles`: the grid, the clues, typing a word on the keyboard,
//!   check and reveal; the letters are kept per file. None is bundled.
//! - *Chess puzzles*: a small bundled set (Lichess puzzle database, CC0):
//!   pick a piece and where it goes, a hint, the next puzzle.
//! - Statistics: the puzzles solved.
//!
//! What each decides without a host is in `sudoku`, `puz`, `chess` and `state`
//! (with their tests); the screen is `native_view` (what keys mean) and
//! `native_draw` (only the cells of the band being drawn), the widget
//! `native_widgets`, and `native_plugin` the glue (`PORTING-7.md`).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod chess;
#[path = "../declarations.rs"]
pub mod declarations;
mod native_draw;
mod native_entry;
pub mod native_plugin;
mod native_view;
mod native_widgets;
pub mod puz;
pub mod state;
pub mod sudoku;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("puzzles");

/// Help text published by the host during atomic native admission.
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];
/// Catalog sources published before native callbacks.
pub static CATALOGS: [(&str, &str); 2] = [
    ("en", include_str!("../i18n/en.ini")),
    ("de", include_str!("../i18n/de.ini")),
];

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
