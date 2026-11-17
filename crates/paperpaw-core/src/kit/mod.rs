//! The kernel's kit: the registries its built-ins fill (actions, menus,
//! status items, decorations, setup steps, widgets) and the drawing,
//! library and time helpers its screens use, read through the kernel's
//! [`Place`](crate::Place) (#300, Decision 209). They were the SDK's old
//! in-process API; plugins see none of it.

pub mod action;
pub mod ask;
pub mod contribution;
pub mod decorations;
pub mod help;
pub mod install;
pub mod library;
pub mod logo;
pub mod menus;
pub mod readers;
pub mod setup;
pub mod status;
pub mod time;
pub mod ui;
pub mod widgets;
