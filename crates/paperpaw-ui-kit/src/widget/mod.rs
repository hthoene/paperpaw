//! The widget toolkit of interface 7: a widget's settings read at draw time,
//! the look widgets share (frame, paper, ink, corners, line, shadow,
//! padding), and the pieces that draw text and shapes in a widget's place.
//! A widget plugin lays its content out into [`crate::Paint`] operations,
//! with fallible allocation, and replays them in its draw callback.
pub mod cx;
pub mod layout;
pub mod look;
pub mod names;
pub mod options;
pub mod settings;
pub mod shape;
pub mod text;

pub use cx::Cx;
pub use look::{Align, Frame, Look, Padding, Shadow, Shape, TextSize};
pub use settings::{Key, Settings};
