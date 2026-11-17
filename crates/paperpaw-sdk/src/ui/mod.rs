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
pub(crate) mod button;
mod chips;
pub mod choice;
pub mod confirm;
mod context;
mod dialog;
mod fields;
pub(crate) mod header;
mod hints;
mod image;
pub(crate) mod label;
pub(crate) mod layout;
pub(crate) mod list;
pub mod look;
mod look_extra;
pub(crate) mod offline;
mod paging;
mod picker;
mod progress;
mod readable;
pub mod reorder;
mod response;
mod responsive;
pub(crate) mod row;
mod rows;
mod search;
pub(crate) mod sections;
pub(crate) mod setting_rows;
mod setting_values;
pub mod slider;
pub(crate) mod style;
pub(crate) mod text;
mod thumbnail;

pub(crate) use button::*;
pub use hints::*;
pub use image::*;
pub use list::*;
pub use responsive::{COMPACT_BELOW_MM, EXPANDED_FROM_MM, ScreenClass};
pub use setting_rows::*;
pub use style::*;
pub use text::*;

use crate::semantics::Role;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_document::ImageRef;
use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point, Rect, Storage};

use crate::builtin::{fonts, settings};
use crate::env::Env;
use crate::fonts::FontHandle;
use crate::screen::Hint;
use paperpaw_text::{TextInk, ToneCurve};

/// Whether a part drawn in `rect` can show on `canvas`: it meets the
/// canvas's clip (one band of the frame), or the canvas only tells what
/// is drawn, as for a screen reader, which hears every part. Parts that
/// cannot show are left out before their text is looked up and measured.
pub(crate) fn shows(canvas: &Canvas<'_>, rect: Rect) -> bool {
    canvas.is_describing() || canvas.is_blind() || canvas.clip().intersection(&rect).is_some()
}
