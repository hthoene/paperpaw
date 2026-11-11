//! Mail: read and send e-mail over IMAP and SMTP with a password or an app
//! password (no browser login).

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration tests also use the builders"
)]
mod declarations;
mod native_entry;
pub mod native_model;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin (slice 1: its settings; see `PORTING-7.md`).
pub mod native_plugin;
mod native_reader;
mod native_screens;
mod native_socket;
mod native_values;
mod native_words;
mod native_writer;

pub mod account;
pub mod browser;
pub mod codec;
pub mod compose;
pub mod driver;
pub mod header;
pub mod html;
pub mod imap;
pub mod job;
mod plugin;
pub mod provider;
pub(crate) mod reader;
pub mod render;
pub mod session;
pub mod settings;
pub mod smtp;
pub mod store;
pub mod texts;
pub mod widgets;
pub(crate) mod writer;

pub use plugin::Mail;

#[cfg(test)]
mod tests;

use paperpaw_sdk::Id;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{Confirmed, SocketNotice, Typed};

pub const PLUGIN: Id = Id::new_static("mail");
/// Opens the folders.
pub const OPEN: Id = Id::new_static("mail.open");
/// Writes a mail.
pub const WRITE: Id = Id::new_static("mail.write");
pub const BOXES: Id = Id::new_static("mail.boxes");
pub const LIST: Id = Id::new_static("mail.list");
pub const READ: Id = Id::new_static("mail.read");
pub const COMPOSE: Id = Id::new_static("mail.compose");
/// The socket of a screen's connection, and what it tells. Every kind of
/// screen has its own: the kernel closes a socket that is connected again
/// (and every screen hears every event), so screens sharing one would end
/// each other's connections.
#[derive(Debug)]
pub struct Channel {
    pub socket: Id,
    pub events: Event<SocketNotice>,
}

/// The folders.
pub const BOXES_CHANNEL: Channel = Channel {
    socket: Id::new_static("mail.socket.boxes"),
    events: Event::new("mail.events.boxes"),
};
/// The mails of a folder.
pub const LIST_CHANNEL: Channel = Channel {
    socket: Id::new_static("mail.socket.list"),
    events: Event::new("mail.events.list"),
};
/// A mail being read.
pub const READER_CHANNEL: Channel = Channel {
    socket: Id::new_static("mail.socket.reader"),
    events: Event::new("mail.events.reader"),
};
/// A mail being written.
pub const COMPOSE_CHANNEL: Channel = Channel {
    socket: Id::new_static("mail.socket.compose"),
    events: Event::new("mail.events.compose"),
};
/// The answer to *Trust this server?*.
pub const PIN_ANSWER: Event<Confirmed> = Event::new("mail.pin-answer");
/// Text typed for a part of a mail being written.
pub const TYPED: Event<Typed> = Event::new("mail.typed");

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/mail_settings.rs"));
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
