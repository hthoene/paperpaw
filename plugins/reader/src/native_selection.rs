//! Choosing text on the page, and what is done with it (native).
pub mod live;
#[cfg(test)]
mod live_tests;
pub mod moves;
#[cfg(test)]
mod moves_tests;

pub use moves::{Mode, Selection, continues, line_boxes};
