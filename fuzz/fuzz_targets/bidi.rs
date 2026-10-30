//! Any text through the bidirectional algorithm and Arabic shaping
//! (`paperpaw_text::fuzz::bidi` says what must hold).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| paperpaw_text::fuzz::bidi(data));
