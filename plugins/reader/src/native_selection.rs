//! Choosing text on the page, and what is done with it (native).
pub mod moves;
#[cfg(test)]
mod moves_tests;

pub use moves::{Mode, Selection, line_boxes};
