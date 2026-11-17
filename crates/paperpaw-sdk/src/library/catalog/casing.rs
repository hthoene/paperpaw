//! The lowercase of a text as `str::to_lowercase` makes it, into a string
//! reserved fallibly (`str::to_lowercase` allocates without a way to refuse).
//!
//! `str::to_lowercase` is `char::to_lowercase` for each char but for the
//! capital sigma, which is a final `ς` at the end of a word: after a cased
//! letter and before none, skipping the case-ignorable characters (an
//! apostrophe, a combining mark, a dot…) on both sides. The standard
//! library keeps `Case_Ignorable` and `Cased` private, so [`IGNORABLE`] is
//! a copy of the first, and the second is built from what `char` offers;
//! the tests compare both with the standard library for every char, so a
//! toolchain with newer Unicode data fails there, not silently in the
//! order of a library.








