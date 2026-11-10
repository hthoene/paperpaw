//! End of book: a screen when the last page of a book is reached, with three
//! rows:
//!
//! - the next book of the series, the first one after this one that is
//!   not read to the end (the library when there is none, or the book has
//!   no series);
//! - the book's rating, one to five (`4 / 5`; Select goes on, after five
//!   it is not rated again): a book field of the library
//!   (`end-of-book.rating`);
//! - whether the book counts as finished.
//!
//! Two settings: whether the screen opens (`end-of-book.show`, on to begin
//! with) and whether the book is marked finished (`end-of-book.mark`, on
//! to begin with: the third row says *Yes*; off, *No*). What the row says
//! is kept in memory while the book is open and put in the library again
//! when the book closes, because the reader's closing writes how far the
//! book is read as it does for any book.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeEndOfBook`];
//! the interface-6 plugin is gone. What waits on the SDK and the host is in
//! `PORTING-7.md`: reading the rating back, declaring the rating field and
//! its badge, and the host's acceptance of the screen.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
mod native_model;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

/// The book field the rating is kept in: "1" to "5".
pub const RATING: &str = "end-of-book.rating";
