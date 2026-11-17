//! Decorations of a book's words (ADR 0021): a role's look read from its
//! settings, whatever they hold; marks combined, the earlier winning;
//! lines, frames and backgrounds drawn round the words' boxes and not
//! onto the lines next to them; decorators asked in their order, those
//! switched off left out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::decorations::{AS_IS, LineKind, NONE, Tone};
use proptest::prelude::*;

// ---- A role's look from its settings -----------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]


}

#[test]
fn the_none_values_are_no_tones_or_lines() {
    for value in [
        NONE,
        AS_IS,
        "",
        "core.tone.",
        "core.tone.yellow ",
        "CORE.TONE.YELLOW",
    ] {
        assert_eq!(Tone::from_id(value), None, "{value:?}");
        assert_eq!(LineKind::from_id(value), None, "{value:?}");
    }
}

// ---- Combining ---------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

}

// ---- Drawing -----------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]



}

// ---- Decorators --------------------------------------------------------
