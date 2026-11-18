//! File sync: folders of the card kept in sync with your own WebDAV server
//! (Nextcloud, ownCloud, a NAS and the like), in both directions or from the
//! server only. A file changed on one side goes to the other; changed on both,
//! both are kept; deleted on one side and unchanged on the other, it is
//! deleted on the other too (the card's copy goes to a trash), else it stays.
//! Every file is written whole or not at all, the state is kept after each
//! file and a cut or a lost network is picked up where it stopped. Files are
//! streamed by the host, never held here.
//!
//! This is the interface-7 plugin (`PORTING-7.md`).

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
mod native_entry;
mod native_home;
pub mod native_link;
mod native_look;
pub mod native_names;
pub mod native_pair;
pub mod native_plan;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_run;
pub mod native_state;
pub mod native_sync;
pub mod native_when;
mod native_widgets;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod test_volume;
