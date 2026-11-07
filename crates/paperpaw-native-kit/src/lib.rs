//! Drawing helpers for interface-7 plugin screens.
//!
//! A native plugin has no `Canvas`, `Env` or `ListView`: it reads the stock
//! look through settings and the snapshot, lays a screen out as owned
//! [`Paint`] operations and replays them in its draw callback. This crate
//! holds what screens share: [`Style`] (the stock look, read from the
//! settings), [`Messages`] (catalog texts), [`text`] fitting, the [`header`]
//! (title, back arrow, pager) with its tap regions, and a [`ListRenderer`]
//! for list screens with hit-testing. It uses `paperpaw_sdk::native` only,
//! allocates fallibly and keeps nothing callback-scoped.
#![no_std]

extern crate alloc;

pub mod focus;
pub mod header;
pub mod list;
pub mod messages;
pub mod paint;
pub mod style;
pub mod text;

pub use focus::{ListFocus, ListInput, Step};
pub use list::ListRenderer;
pub use messages::Messages;
pub use paint::{Page, Paint};
pub use style::Style;
