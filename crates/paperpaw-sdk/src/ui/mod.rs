//! Building blocks for plugin user interfaces.
//!
//! All sizes derive from the device's density and the `core.ui-size`
//! setting, so every plugin's UI looks consistent and scales together when
//! the user chooses larger text.
//!
//! A screen draws with a [`Ui`]: the canvas, the environment and the style
//! together, with the interface's parts as its methods (`ui.button(..)`,
//! `ui.message(..)`, `ui.focus(..)`). Text of the interface is a [`Label`],
//! and a row of a list is a [`Row`]. The parts that keep state (a
//! [`Dialog`], [`Chips`], a [`ListView`]) draw with the `Ui`, and those that
//! take the buttons' actions and gestures (a [`Dialog`], a [`TextView`])
//! answer with a [`Response`].

mod address;
mod button;
mod chips;
pub mod choice;
pub mod confirm;
mod context;
mod dialog;
mod fields;
mod header;
mod hints;
mod image;
pub mod items;
mod label;
pub(crate) mod layout;
mod list;
pub mod look;
mod look_extra;
mod number_edit;
pub mod offline;
mod paging;
mod picker;
mod progress;
pub mod reorder;
mod response;
mod row;
mod rows;
pub mod sections;
mod setting_rows;
pub mod slider;
mod style;
mod text;
mod thumbnail;
pub mod widget_list;

pub use address::*;
pub use button::*;
pub use chips::Chips;
pub use context::Ui;
pub use dialog::Dialog;
pub use fields::*;
pub use header::*;
pub use hints::*;
pub use image::*;
pub use label::Label;
pub use layout::*;
pub use list::*;
pub use number_edit::{NumberEdit, editing_hint};
pub use paging::Paging;
pub use picker::{DateOrder, DatePicker, PickerPart, TimePicker};
pub(crate) use progress::*;
pub use response::{Response, focus_shown};
pub use row::{Row, RowKind, RowPart};
pub use rows::Rows;
pub use setting_rows::*;
pub use style::*;
pub use text::*;
pub use thumbnail::Thumbnail;

use crate::semantics::Role;

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_document::ImageRef;
use paperpaw_graphics::Canvas;
use paperpaw_image::GrayFile;
use paperpaw_platform::{Color, Point, Rect, Storage};

use crate::builtin::{actions, fonts, settings};
use crate::env::Env;
use crate::fonts::FontHandle;
use crate::screen::Hint;
use crate::settings::{SettingKind, SettingSpec, SettingValue};
use paperpaw_text::{TextInk, ToneCurve};

/// Whether a part drawn in `rect` can show on `canvas`: it meets the
/// canvas's clip (one band of the frame), or the canvas only tells what
/// is drawn, as for a screen reader, which hears every part. Parts that
/// cannot show are left out before their text is looked up and measured.
pub(crate) fn shows(canvas: &Canvas<'_>, rect: Rect) -> bool {
    canvas.is_describing() || canvas.is_blind() || canvas.clip().intersection(&rect).is_some()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Sizes for tests: no fonts needed, nothing rounded.
    pub(crate) const STYLE: UiStyle = UiStyle::PLAIN;

    #[test]
    fn list_pages_focus_and_hits() {
        let mut list = ListView::new(Rect::new(0, 100, 200, 300), 100);
        assert_eq!(list.rows(), 3);
        assert_eq!(list.pages(7), 3);
        assert!(list.move_focus(1, 7).is_focus());
        assert_eq!(list.move_focus(2, 7), ListChange::Page);
        assert_eq!(list.page(), 1);
        assert_eq!(list.move_focus(100, 7), ListChange::Page);
        assert_eq!(list.focus(), 6);
        // Page 2 shows only index 6.
        assert_eq!(list.hit(Point::new(10, 150), 7), Some(6));
        assert_eq!(list.hit(Point::new(10, 250), 7), None);
        assert_eq!(list.hit(Point::new(10, 50), 7), None);
        // One step past either end goes round; a page stops at the end.
        assert_eq!(list.move_focus(1, 7), ListChange::Page);
        assert_eq!(list.focus(), 0);
        assert_eq!(list.move_focus(-3, 7), ListChange::None);
        assert_eq!(list.move_focus(-1, 7), ListChange::Page);
        assert_eq!(list.focus(), 6);
        assert_eq!(list.move_focus(1, 0), ListChange::None);
    }

    #[test]
    fn a_hidden_focus_is_shown_before_a_button_chooses() {
        let mut list = ListView::new(Rect::new(0, 0, 200, 300), 100);
        list.set_shown(false);
        list.set_focus(2);
        // The first press shows the focus; the second chooses.
        assert_eq!(list.confirm(), None);
        assert!(list.shown());
        assert_eq!(list.confirm(), Some(2));
        // Shorter lists keep the focus on their last item.
        list.set_len(2);
        assert_eq!(list.focus(), 1);
        list.set_len(5);
        assert_eq!(list.focus(), 1);
        list.set_len(0);
        assert_eq!(list.focus(), 0);
    }

    #[test]
    fn lists_with_sizes_out_of_range_do_not_panic() {
        // Rows asked 0 high are 1 high.
        let list = ListView::new(Rect::new(0, 0, 200, 300), 0);
        assert_eq!(list.row(), 1);
        assert_eq!(list.rows(), 300);
        assert_eq!(list.hit(Point::new(10, 2), 5), Some(2));
        assert_eq!(list.hit(Point::new(10, 10), 5), None);
        assert_eq!(list.row_rect(2).height, 1);
        // More columns than a u16 holds, or than the list is wide.
        let grid = ListView::new(Rect::new(0, 0, 200, 300), 100).columns(70_000);
        assert_eq!(grid.column_count(), usize::from(u16::MAX));
        assert_eq!(grid.row_rect(1).width, 1);
        assert_eq!(grid.hit(Point::new(150, 10), 3), None);
        assert_eq!(grid.hit(Point::new(0, 10), 3), Some(0));
    }
}
