//! An overlay at the bottom with a title and a few choices, e.g. what to
//! do with a file or whether to delete it: the screen of slot
//! [`CHOICE`](paperpaw_sdk::builtin::slots::CHOICE), which
//! [`ask::choice`](paperpaw_sdk::ask::choice) asks. The choice comes back as
//! the plugin's reply event, a [`Picked`] with the context it was asked
//! for and the option chosen. Tapping outside, or *Back*, chooses
//! nothing.
//!
//! The sheet is as wide as the screen's readable column (`ui::readable`):
//! on a screen wider than that it stands in the middle, and what lies
//! beside it is outside, as what lies above it is: a tap there closes the
//! choice (the screen below is dimmed, and not usable meanwhile).

/// The option chosen.
pub const OPTION: &str = "option";

paperpaw_sdk::event_data! {
    /// What was chosen: the option, for the context the choice was
    /// opened for.
    pub struct Picked {
        /// The context ([`ask::choice`](paperpaw_sdk::ask::choice)'s `context`).
        pub path: alloc::string::String,
        pub option: alloc::string::String,
    }
}
/// The options, separated by commas.
pub const OPTIONS: &str = "options";
/// Translation keys of the options: this, then the option.
pub const LABELS: &str = "labels";
