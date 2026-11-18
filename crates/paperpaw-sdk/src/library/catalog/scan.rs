//! The catalog scanned and queried without an infallible allocation: one
//! reused line buffer of at most [`LINE_CAP`] bytes, every copy of a text
//! and every key reserved fallibly, and the reads' errors returned as errors
//! (a card that fails is not the end of the list). A device short of memory
//! gets [`Error::OutOfMemory`] instead of an abort, and nothing is returned
//! half made.
//!
//! The results are those of [`super::query_in`] and [`super::each_at`]. Where
//! they differ, it is in the lines that cannot be books: a line over
//! [`LINE_CAP`] bytes (damage; a line has at most a few kilobytes), one that
//! is not UTF-8 and one without a path are skipped, never read lossily; and
//! the lowercase of a text (keys, words) is made by the `casing` module, byte
//! for byte as `str::to_lowercase` makes it (the final sigma included), so
//! the order and the cursors of a query are those of the stored catalog.
