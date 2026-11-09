//! Readwise: the highlights of a book go to Readwise when its notes are
//! exported (*Export as Markdown* at the end of a book's bookmarks and
//! highlights), with the notes on them and where they are in the book.
//! Readwise keeps each highlight once, so exporting again sends only
//! what is new.
//!
//! Readwise needs one's access token (readwise.io/access_token): the
//! first export asks for it on the keyboard (or a phone, where offered);
//! it is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`). Shipped switched off: switch it on in the
//! plugins.
//!
//! What cannot be sent (no connection, Readwise down) waits and is sent
//! later: with the next export, and when Wi-Fi comes back where PaperPaw
//! manages it. The notes are read a line at a time and the request
//! written to a file and sent from there, so notes of any length are
//! sent on small devices too.
//!
//! It reads the Markdown the reader writes (see
//! [`native_readwise::Parser`]), so it works with any reader that writes the
//! same and runs the reader's export stage. The plugin is a native
//! interface-7 plugin only (`native_plugin`; `PORTING-7.md`).

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
pub mod declarations;
mod native_entry;
pub mod native_plugin;
pub mod native_readwise;
pub mod native_send;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_golden_tests;
#[cfg(test)]
mod native_send_tests;
#[cfg(test)]
mod test_volume;
