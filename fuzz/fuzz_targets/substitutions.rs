//! Any bytes as a font pack's substitution table
//! (`paperpaw_text::fuzz::substitutions` says what must hold).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| paperpaw_text::fuzz::substitutions(data));
