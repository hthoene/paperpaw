//! SDK prechecks must follow each slot's own schema bound.
#![allow(
    clippy::expect_used,
    reason = "invalid schema fixtures must fail the test"
)]
use super::{fonts, formatting};

fn slot_max_bytes(name: &str) -> usize {
    let schema = include_str!("../../../../abi/abi.def");
    let marker = alloc::format!("\"name\": \"{name}\"");
    let slot = schema.split_once(&marker).expect("slot must exist").1;
    let value = slot
        .split_once("\"max_bytes\":")
        .expect("slot byte limit")
        .1;
    value
        .trim_start()
        .split(|ch: char| !ch.is_ascii_digit())
        .next()
        .expect("numeric limit")
        .parse()
        .expect("usize limit")
}

#[test]
fn font_and_text_bounds_match_their_own_slots() {
    for (name, sdk) in [
        ("font.acquire", fonts::FONT_ACQUIRE_MAX_BYTES),
        ("text.measure", fonts::TEXT_MEASURE_MAX_BYTES),
        ("text.draw", fonts::TEXT_DRAW_MAX_BYTES),
        ("text.ink", fonts::TEXT_INK_MAX_BYTES),
    ] {
        assert_eq!(sdk, slot_max_bytes(name), "{name}");
    }
}

#[test]
fn translation_copy_bound_matches_its_own_slot() {
    assert_eq!(formatting::MAX_MESSAGE_BYTES, slot_max_bytes("i18n.copy"));
}
