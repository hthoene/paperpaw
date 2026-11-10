//! Reading progress synced with KOReader's sync server ("kosync"), so a
//! book opens where one stopped on another device, PaperPaw or KOReader.
//!
//! - Books are identified as KOReader does (a partial MD5 of the file), so
//!   the same file matches on every device.
//! - Positions are KOReader's XPointers where the format has them (EPUB),
//!   besides the percentage.
//! - The progress is sent at most once a minute while reading (where
//!   PaperPaw runs the radio itself, only while it is on anyway: turning
//!   pages never keeps it on), and when a book is closed and the device
//!   sleeps; it is fetched when a book opens and when the device wakes. If
//!   another device is further, a dialog offers to go there (once for each
//!   position).
//! - Nothing is sent for a book while its progress is being fetched, nor
//!   while the dialog is open or was left without a choice (offered
//!   again when the server's position moves), nor before the server's
//!   progress was read (a failed fetch is tried again first): a further
//!   position on the server is never overwritten before the user chose
//!   to stay.
//! - *Sync progress* in the reading menu's *More* fetches the latest, then
//!   sends this device's position unless another device is further.
//! - The account (settings → *Progress sync*) keeps the user name in the
//!   settings and only the MD5 of the password, as KOReader does. Its
//!   screen also sets the server: KOReader's or any compatible one, also
//!   below a path (`https://example.org/kosync`) or over plain `http` in a
//!   home network. Another server signs out, however it was set: an
//!   account's key belongs to its server.
//!
//! This is the interface-7 plugin (`PORTING-7.md`): the settings, the account
//! screen, the sync (`native_sync`, carried out through the network, secrets
//! and work services) and the offer. Needs a device with network.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
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
#[doc(hidden)]
pub mod native_account;
mod native_backing;
mod native_entry;
#[doc(hidden)]
pub mod native_flow;
#[doc(hidden)]
pub mod native_model;
/// The interface-7 progress sync.
pub mod native_plugin;
#[doc(hidden)]
pub mod native_store;
#[doc(hidden)]
pub mod native_sync;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
