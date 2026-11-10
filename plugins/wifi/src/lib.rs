//! Wi-Fi, where PaperPaw manages it itself (the X4): when the radio is on,
//! which network it joins, and the page to choose networks (see
//! `paperpaw_sdk::wifi`). Another plugin may take its place: the shell
//! only does what it is told, and the networks joined are kept with the
//! passwords and keys, the same for any plugin.
//!
//! By default the radio is off and comes on only when a plugin wants the
//! network (a catalog, a sync, a transfer), joins the network joined
//! before that the user picked last, or the strongest ([`JOIN_FIRST`]),
//! and goes off again a minute after nothing uses it, or
//! at once when home or a book shows: it costs battery and memory.

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
mod native_entry;
pub mod native_ini;
pub mod native_known;
#[doc(hidden)]
pub mod native_model;
pub mod native_own;
pub mod native_phone;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod native_policy;
pub mod native_radio;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
#[cfg(test)]
mod native_secret_tests;

use paperpaw_sdk::Id;
use paperpaw_sdk::setting::{Setting, Text};

/// The plugin's id.
pub const PLUGIN: Id = Id::new_static("wifi");
/// The settings group, shown as the Wi-Fi page.
pub const GROUP: Id = Id::new_static("wifi.group");
/// The Wi-Fi page.
pub const PAGE: Id = Id::new_static("wifi.page");
/// The page of the networks in reach, opened from the Wi-Fi page.
pub const NETWORKS: Id = Id::new_static("wifi.networks");
/// Opens the Wi-Fi page: what a tap on Wi-Fi's status item runs.
pub const OPEN: Id = Id::new_static("wifi.open");
/// Setting up Wi-Fi with a phone (see `phone`).
pub const PHONE: Id = Id::new_static("wifi.phone");
/// A file on the card with networks to join, read once at a start, kept
/// with the passwords and removed (e.g. written by the flasher): lines
/// `ssid = …` and `password = …`, a pair for each network.
pub const WIFI_FILE: &str = "wifi.ini";
/// The status item: the Wi-Fi sign while the device is in a network.
pub const STATUS: Id = Id::new_static("wifi.status");
/// The radio comes on by itself when a plugin wants the network.
pub const ON_DEMAND: Setting<bool> = Setting::new("wifi.on-demand");
/// How long after the last use the radio goes off.
pub const IDLE_OFF: Setting<Text> = Setting::new("wifi.idle-off");
/// The radio stays on (in the network joined before) while charging.
pub const WHILE_CHARGING: Setting<bool> = Setting::new("wifi.while-charging");
/// Which network joined before is joined first: the one picked last on
/// the page (the default), or the strongest.
pub const JOIN_FIRST: Setting<Text> = Setting::new("wifi.join-first");
/// The choice of [`JOIN_FIRST`]: the network picked last.
pub const FIRST_PICKED: &str = "wifi.join-first.picked";
/// The choice of [`JOIN_FIRST`]: the strongest network.
pub const FIRST_STRONGEST: &str = "wifi.join-first.strongest";
