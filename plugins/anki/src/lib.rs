//! Anki: study the cards that are due in Anki on your computer on the
//! reader, and send the answers back, through the AnkiConnect add-on.
//!
//! *Anki* in the tools: *Sync* sends the answers waiting (a queue that is
//! kept on the reader, also with no Wi-Fi) to Anki, then fetches the cards due
//! (and a few new ones, as the settings say) of the chosen deck; *Study*
//! shows them one after the other, offline. Anki decides what is due: the
//! reader keeps no schedule. AnkiConnect (add-on 2055492159) listens on the
//! computer's own address only unless `webBindAddress` is set to `0.0.0.0` in
//! its configuration; its `apiKey`, if set, is entered here and kept among
//! the passwords and keys. Only AnkiConnect's documented protocol (version 6)
//! is used. Nothing goes through AnkiWeb.
//!
//! No network is used by a key press: a sync runs on request, and where the
//! setting says, when the screen opens. At most 50 cards are kept, one read
//! into memory at a time.
//!
//! This is the interface-7 plugin (`PORTING-7.md`). Needs a device with network.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
pub mod native_anki;
pub mod native_cards;
mod native_entry;
mod native_keep;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_study;
mod native_study_screen;
pub mod native_sync;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_sync_tests;
#[cfg(test)]
mod test_volume;
