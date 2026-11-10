//! Readwise Reader: the articles saved to read later, fetched as books
//! into the `Reader` folder of the library, and archived in Reader once
//! read.
//!
//! *Readwise Reader* in the tools asks for the access token
//! (readwise.io/access_token, the same one *Readwise* uses; typing it can
//! be done on a phone too, see the keyboard) and fetches: each article in
//! the place chosen (*Later*, or the *Inbox*) comes as an HTML book of
//! its text (Reader gives no pictures in it). Articles read to the end are
//! archived in Reader with the next fetch and their books removed, as are
//! those Reader says are archived or deleted elsewhere; a book with the
//! reader's highlights, notes or bookmarks stays in the library, let go
//! of by Reader. The token is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`).
//!
//! An interface-7 plugin (`native_plugin`): the network is `core.net`, the
//! books go into the library with `Library::import` and come out with
//! `Library::delete`. Answers are saved to storage and read as they stream;
//! the text of an article is copied from the answer to its book in small
//! pieces (`native_read_later::Copier`), so this works on small devices too. Reader allows twenty
//! requests a minute for lists: a fetch asks for at most nineteen and
//! stops where it is told to wait (the next fetch goes on). Shipped
//! switched off: switch it on in the plugins.

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
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
pub mod native_answers;
#[cfg(test)]
mod native_answers_tests;
mod native_entry;
pub mod native_fetch;
#[cfg(test)]
mod native_fetch_tests;
pub mod native_lists;
/// The interface-7 Readwise Reader plugin.
pub mod native_plugin;
pub mod native_read_later;
#[cfg(test)]
mod native_read_later_tests;
pub mod native_secret;
#[cfg(test)]
mod test_volume;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
