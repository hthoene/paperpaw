//! Drawing helpers for interface-7 plugin screens.
//!
//! A native plugin has no `Canvas`, `Env` or `ListView`: it reads the stock
//! look through settings and the snapshot, lays a screen out as owned
//! [`Paint`] operations and replays them in its draw callback. This crate
//! holds what screens share: [`Style`] (the stock look, read from the
//! settings), [`Messages`] (catalog texts), [`text`] fitting, the [`header`]
//! (title, back arrow, pager) with its tap regions, and a [`ListRenderer`]
//! for list screens with hit-testing. Where the interface language is written
//! right to left ([`Style::rtl`]) the header, rows, items, dialog, chips and the
//! sheet's pager are mirrored; a screen lays itself out once from the left and
//! mirrors its own parts with [`Style::flip`], [`Style::flip_x`] and
//! [`text::label_in`]. It uses `paperpaw_sdk::native` only,
//! allocates fallibly and keeps nothing callback-scoped.
#![no_std]
#![cfg_attr(test, allow(clippy::unwrap_used, reason = "assertions"))]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../../plugins/test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

pub mod chips;
pub mod covers;
pub mod dashboard;
pub mod dialog;
pub mod event;
pub mod files;
pub mod focus;
pub mod geometry;
pub mod header;
pub mod icons;
pub mod items;
pub mod json;
pub mod list;
pub mod mem;
pub mod messages;
pub mod paint;
pub mod screens;
pub mod sheet;
pub mod status_items;
pub mod status_line;
pub mod style;
pub mod text;
pub mod uitext;
pub mod widget;
pub mod window;

pub use chips::Chips;
pub use dialog::Dialog;
pub use focus::{ListFocus, ListInput, Step};
pub use items::{Footer, Item, ItemList, ItemStep, RowItem};
pub use list::ListRenderer;
pub use messages::Messages;
pub use paint::{Page, Paint};
pub use screens::Screens;
pub use style::Style;
pub use window::{RowSource, WindowList};
