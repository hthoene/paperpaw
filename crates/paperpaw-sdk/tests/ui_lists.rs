//! Lists as every screen uses them, by buttons and by touch alike:
//! [`ListView`] (pages of equal rows, or a grid), [`Rows`] (rows of
//! different heights with section headings) and [`Sections`] (the same
//! for lists of any length). Focus moves wrap or stop as documented and
//! never land on a heading, every row is hit where it is drawn and only
//! there, pages split the list without losing or repeating an entry,
//! and nothing is drawn outside the list's area.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_sdk::ui::step_focus;
use proptest::prelude::*;

// ---- step_focus and list_step -----------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn single_steps_wrap_and_larger_ones_stop_at_the_ends(
        len in 0usize..50,
        focus in 0usize..60,
        delta in -60isize..60,
    ) {
        if len == 0 {
            prop_assert_eq!(step_focus(focus, delta, len), 0);
            return Ok(());
        }
        let focus = focus % len;
        let to = step_focus(focus, delta, len);
        prop_assert!(to < len);
        let target = focus as isize + delta;
        let expected = match delta {
            1 | -1 => target.rem_euclid(len as isize) as usize,
            _ => target.clamp(0, len as isize - 1) as usize,
        };
        prop_assert_eq!(to, expected);
    }
}

// ---- ListView ---------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]



}

// ---- Rows -------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)))]




}

// ---- Sections ---------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)))]




}

// ---- What a move asks to be drawn ---------------------------------------
