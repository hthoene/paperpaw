//! Picture frame: your own photos as the sleep screen, as a slideshow and as
//! the widget *Photo of the day*, from a folder of the card and, if you like,
//! from your own photo server (WebDAV: Nextcloud, ownCloud and the like).
//!
//! The host decodes the photos and draws them in bands in the panel's grays
//! (`image_prepare`, `draw_image`), so no photo is ever held in this plugin's
//! memory; contrast and fit are settings. The server's photos are cached on
//! the card within a size cap and fetched only on request, with the charger,
//! or where Wi-Fi is up anyway: nothing here turns the radio on at a wake.
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
pub mod native_cache;
pub mod native_dav;
mod native_entry;
mod native_home;
mod native_keep;
mod native_link;
pub mod native_photos;
mod native_pics;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_state;
pub mod native_sync;
pub mod native_when;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod test_volume;
