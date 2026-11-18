//! What the widgets show: the weather, or why there is none, and the place's
//! name and the weather's age. The plugin builds it from its state when a
//! widget is measured or drawn; the widgets only read it.
use crate::native_weather::Weather;

/// What a widget tells while it has no weather to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Waiting {
    /// No place is set.
    NoPlace,
    /// Fetching it.
    Fetching,
    /// It could not be fetched.
    Failed,
    /// It comes once there is a network.
    NoNetwork,
}

impl Waiting {
    /// The translation key of what the widget says.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Waiting::NoPlace => "weather.no-place",
            Waiting::Fetching => "weather.fetching",
            Waiting::Failed => "weather.failed",
            Waiting::NoNetwork => "weather.no-network",
        }
    }
}

/// What the widgets show now, borrowed from the plugin's state.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Shown<'a> {
    /// The weather kept for the place set; `None` while there is none (or no place).
    pub weather: Option<&'a Weather>,
    /// Why there is no weather, while there is none.
    pub waiting: Waiting,
    /// The place's name, trimmed.
    pub place: &'a str,
    /// Its age in seconds (whole minutes), when it is old (`native_weather::is_old`).
    pub old: Option<i64>,
    /// Today, in days since 1970, if the device knows it.
    pub today: Option<i32>,
}

impl<'a> Shown<'a> {
    /// What is shown when `waiting` and nothing else is known.
    #[must_use]
    pub const fn waiting(waiting: Waiting) -> Self {
        Self {
            weather: None,
            waiting,
            place: "",
            old: None,
            today: None,
        }
    }
}
