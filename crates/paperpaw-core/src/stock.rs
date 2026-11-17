//! The kernel's stock screens, made as layers (#244, slice D6b): the
//! choice and the yes-or-no question every plugin asks
//! ([`paperpaw_sdk::ask`]), the status items and lines chooser, and the
//! editor of a list of widgets above each other with its gallery.

pub mod choice;
pub mod confirm;
pub mod items;
pub mod widget_list;
