//! KOReader: the bookmarks and highlights of a book go back and forth
//! with KOReader, for books read in both on one device (or one library
//! folder synced between devices).
//!
//! KOReader keeps what it knows of a book beside it, in
//! `Book.sdr/metadata.epub.lua`. When PaperPaw opens a book that has such
//! a file, and when its bookmarks or highlights change, this plugin reads
//! it, takes over what is new there, and leaves what is new here in
//! `Book.sdr/Book.epub.annotations.lua`: KOReader takes that over the
//! next time it opens the book, and removes the file. Positions are
//! KOReader's XPointers
//! ([`Document::xpointer`](paperpaw_sdk::document::Document)), so EPUBs
//! only.
//!
//! What is removed on one side goes on the other too: the plugin keeps
//! which annotations both had at the last exchange, and their notes then.
//! A note changed on one side only goes to the other; one changed on both
//! keeps the later change.
//!
//! A file of either side that is there but cannot be read or understood
//! stops the exchange: nothing changes on either side until it can be.
//! So does a missing one of KOReader (as while a sync has brought the
//! folder but not yet the file in it), and one side without any of what
//! both had (book data moved halfway, a sync not complete), unless that
//! side removed them all: here the reader said so, and KOReader's file
//! keeps an empty list where it was seen with annotations at the last
//! exchange. KOReader writes an empty list for every book it opened, so
//! one in a file never seen with any (a fresh one from another device)
//! removes nothing: what both had is offered to KOReader again. Where
//! KOReader's file, seen with annotations before, has none any more
//! (removed there, or a fresh file synced in from another device), the
//! reader is asked in the book whether to remove them here too; nothing
//! changes before, *Keep* (where the focus starts) offers them to
//! KOReader again. Before everything is removed here, the annotations
//! are kept aside (`<key>.txt.removed` beside them). What could
//! not be written for KOReader is not taken as known, so it goes again
//! next time. Annotations go to KOReader with the time they were made:
//! while neither they nor the clock tell it, they wait here.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod native_entry;
#[doc(hidden)]
pub mod native_exchange;
#[doc(hidden)]
pub mod native_model;
/// The KOReader exchange, as the native plugin (see `PORTING-7.md`).
pub mod native_plugin;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_model_tests;
