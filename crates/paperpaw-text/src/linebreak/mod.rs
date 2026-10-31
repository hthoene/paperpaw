//! Unicode line breaking (UAX #14) for the wrapping of lines.
//!
//! The design is `paperpaw-notes` `designs/line-breaking-a5.md`. This
//! module is **crate-private** on purpose: `paperpaw-text` crosses into
//! plugins, so a public type with fields would freeze its state into the
//! plugin interface, and each rule UAX #14 adds later may need more state.
//! Only [`UNICODE_VERSION`] and the wrapping functions are public.

mod breaker;
mod table;
#[rustfmt::skip]
mod table_data;

#[cfg(test)]
mod tests;

/// The Unicode version of the line-breaking table: `(major, minor, update)`.
/// One release does not mix versions: the bidi and Arabic tables are of the
/// same one (a test compares them).
pub(crate) use breaker::{LineBreaker, Opportunity, lifts_filter};

pub const UNICODE_VERSION: (u8, u8, u8) = table_data::UNICODE;
