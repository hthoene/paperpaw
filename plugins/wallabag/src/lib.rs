//! Wallabag: articles saved to read later, fetched as books into the
//! `Wallabag` folder of the library, and archived in Wallabag once read.
//!
//! *Wallabag* in the tools sets up the account (the server, user name
//! and password, and an API client's ID and secret, made in Wallabag
//! under *API clients management*; typing them can be done on a phone
//! too, see the keyboard) and fetches: each unread article comes as an
//! EPUB Wallabag makes, with its pictures. Articles read to the end are
//! archived with the next fetch and their books removed, as are those
//! Wallabag says are archived or deleted elsewhere; a book with the
//! reader's highlights, notes or bookmarks stays in the library, let go
//! of by Wallabag. The password and the client's secret are kept among
//! the passwords and keys (`core.secrets`).
//!
//! An interface-7 plugin (`native_plugin`): the network is `core.net`, the
//! books go into the library with `Library::import` and come out with
//! `Library::delete`, and the answers Wallabag sends are read in pieces by
//! `native_fetch`, so this works on small devices too. Shipped switched off:
//! switch it on in the plugins. `PORTING-7.md` names what waits for the host.

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
mod native_entry;
pub mod native_fetch;
#[cfg(test)]
mod native_fetch_tests;
pub mod native_fetcher;
#[doc(hidden)]
pub mod native_keep;
pub mod native_lists;
/// The interface-7 wallabag plugin.
pub mod native_plugin;
pub mod native_wallabag;
#[cfg(test)]
mod native_wallabag_tests;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
