//! Transfer by browser: while the screen is open, the device serves a
//! web page to the local network for putting books and other files on
//! it, fetching them and tidying up, from a phone or a computer, no
//! cable or app needed.
//!
//! The screen shows the page's address as text and as a QR code. The
//! address carries a key drawn anew each time from the platform's random
//! source, so others in the network cannot get at the files; a few wrong
//! keys, and no request is answered for a while (see `paperpaw_sdk::served`). Closing
//! the screen ends the transfer.
//!
//! The same way, the plugin lets one type text on a phone for the
//! keyboard (slot `core.remote-keyboard`, see `native_phone`).

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
mod declarations;

mod native_cable;
mod native_draw;
mod native_entry;
mod native_files;
mod native_install;
mod native_install_host;
mod native_key;
mod native_page;
mod native_phone;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod native_transfer;
mod native_web;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The texts of the web page: the name it reads them by, and the translation.
pub(crate) const PAGE_TEXTS: [(&str, &str); 25] = [
    ("title", "transfer.web.title"),
    ("device", "transfer.web.device"),
    ("upload", "transfer.web.upload"),
    ("drop", "transfer.web.drop"),
    ("newFolder", "transfer.web.new-folder"),
    ("folderName", "transfer.web.folder-name"),
    ("delete", "transfer.web.delete"),
    ("deleteQuestion", "transfer.web.delete-question"),
    ("empty", "transfer.web.empty"),
    ("done", "transfer.web.done"),
    ("failed", "transfer.web.failed"),
    ("target", "transfer.web.target"),
    ("outside", "transfer.web.outside"),
    ("toLibrary", "transfer.web.to-library"),
    ("here", "transfer.web.here"),
    ("move", "transfer.web.move"),
    ("rename", "transfer.web.rename"),
    ("moving", "transfer.web.moving"),
    ("moveHere", "transfer.web.move-here"),
    ("cancel", "transfer.web.cancel"),
    ("newName", "transfer.web.new-name"),
    ("waiting", "transfer.web.waiting"),
    ("sent", "transfer.web.sent"),
    ("notSent", "transfer.web.not-sent"),
    ("retry", "transfer.web.retry"),
];

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::SettingSpec;
    include!(concat!(env!("OUT_DIR"), "/transfer_settings.rs"));
}

/// The settings the plugin declares as a table kept in the program (as Beta 3
/// kept them), for a linked build to use in place of a copy on the heap of
/// each (`Kernel::register_linked`). Only a linked plugin has it.
#[cfg(feature = "linked")]
pub static NATIVE_SETTINGS: &[paperpaw_sdk::SettingSpec] = &native_settings::SETTINGS;

/// The setting groups the plugin declares, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_GROUPS: &[paperpaw_sdk::SettingGroupSpec] = &native_settings::GROUPS;

/// The ids the plugin's declarations name, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;
