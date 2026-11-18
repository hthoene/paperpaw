//! Any bytes as a substitution table and text, shaped as Bengali and
//! Devanagari (`paperpaw_text::fuzz::indic` says what must hold).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| paperpaw_text::fuzz::indic(data));
