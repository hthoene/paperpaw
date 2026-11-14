//! The home screen's arranging, drawing and drawer, run with the real kernel and the
//! native plugins linked (`native-linked-reading` and `native-linked-store`): what the
//! interface-6 Home's hosted tests pinned, moved here when that plugin went (#244).

#![cfg(all(
    feature = "native-linked",
    feature = "native-linked-reading",
    feature = "native-linked-store"
))]

mod native_activation;

#[path = "home_native/common.rs"]
mod common;

#[path = "home_native/arrange.rs"]
mod arrange;
#[path = "home_native/colours.rs"]
mod colours;
#[path = "home_native/data.rs"]
mod data;
#[path = "home_native/drawer_focus.rs"]
mod drawer_focus;
#[path = "home_native/drawer_rtl.rs"]
mod drawer_rtl;
#[path = "home_native/edit_touch.rs"]
mod edit_touch;
#[path = "home_native/landscape.rs"]
mod landscape;
#[path = "home_native/layouts.rs"]
mod layouts;
#[path = "home_native/shapes.rs"]
mod shapes;
#[path = "home_native/widget_list.rs"]
mod widget_list;
