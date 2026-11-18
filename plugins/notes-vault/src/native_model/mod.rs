//! What the native notes vault reads and decides without host calls: lines of
//! a file, a note's title, excerpt and tags, the windows of a list, where the
//! entries of a list come from, and the vault's index. Each part is the
//! interface-6 code made fallible and free of slicing that could panic, with a
//! test that both give the same answers (`native_parity_tests.rs`); every
//! allocation may be refused (`NoMemory`, `tests/native_alloc_refusal.rs`).
//!
//! Two storages are named in every call that needs both: the *vault* (the
//! card, read only) and the plugin's *data* folder (the index, `index.txt`,
//! `tags.txt` and `state.txt`, as plain names inside it).

pub mod index;
pub mod lines;
pub mod note;
pub mod page;
pub mod source;
pub mod tags;
mod text;

pub use paperpaw_sdk::native::Error;
