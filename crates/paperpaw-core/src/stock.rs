//! The kernel's stock screens, made as layers (#244, slice D6b): the
//! status items and lines chooser, and the editor of a list of widgets above
//! each other with its gallery. The choice and the yes-or-no question are the
//! dialog plugin's (`core.choice`, `core.confirm`; #293).

pub mod items;
pub mod widget_list;
