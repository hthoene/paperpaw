//! The shape of widgets (#206): how round their corners are
//! (`<widget>.corners`), how thick their frame's line is
//! (`<widget>.frame-line`) and whether their plate casts a shadow
//! (`<widget>.drop-shadow`), each also for every widget whose own the
//! reader did not choose ([`WIDGET_CORNERS`], [`WIDGET_FRAME_LINE`],
//! [`WIDGET_SHADOW`]); the spacing inside every widget
//! ([`WIDGET_PADDING`]) and the gap between them ([`gap`]).
//! [`widget_look`](super::widget_look) and
//! [`draw_frame`](super::draw_frame) follow them.
//!
//! [`WIDGET_CORNERS`]: crate::builtin::settings::WIDGET_CORNERS
//! [`WIDGET_FRAME_LINE`]: crate::builtin::settings::WIDGET_FRAME_LINE
//! [`WIDGET_SHADOW`]: crate::builtin::settings::WIDGET_SHADOW
//! [`WIDGET_PADDING`]: crate::builtin::settings::WIDGET_PADDING

use alloc::format;

use paperpaw_platform::{Color, Rect};

use super::colours::{self, ColourSpec};
use super::look::Frame;
use crate::builtin::settings::{
    self, WIDGET_CORNERS, WIDGET_FRAME_LINE, WIDGET_GAP, WIDGET_PADDING, WIDGET_SHADOW,
};
use crate::decorations::Tone;
use crate::env::Env;
use crate::id::Id;
use crate::settings::SettingSpec;
use crate::ui::UiStyle;




























