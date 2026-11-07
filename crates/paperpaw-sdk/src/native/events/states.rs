//! Small event state views carry no owned strings or callback host bindings.
use crate::power::PowerLevel;

/// A reading-session edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookKind {
    /// A book opened, including a restored reading session.
    Opened,
    /// A book closed, including sleep or shutdown.
    Closed,
    /// The open book finished.
    Finished,
}

/// A radio state without network identity or address information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WifiLinkState {
    /// Radio off.
    Off,
    /// Radio on, without a connection. Also used for reserved wire values.
    On,
    /// Connecting to a network.
    Connecting,
    /// Connected to a network.
    Connected,
    /// Hosting the device's own transfer network.
    AccessPoint,
}
impl WifiLinkState {
    pub(super) fn decode(state: u32) -> Self {
        match state {
            0 => Self::Off,
            2 => Self::Connecting,
            3 => Self::Connected,
            4 => Self::AccessPoint,
            _ => Self::On,
        }
    }
}

/// A borrowed change to the device's power mode, preserving interface-6 meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerChange<'a> {
    pub(super) now_ms: u64,
    pub(super) level: PowerLevel,
    pub(super) automatic: bool,
    pub(super) stretch: u32,
    pub(super) mode: &'a str,
}
impl<'a> PowerChange<'a> {
    /// Emission time in monotonic milliseconds.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }
    /// Energy level; reserved wire levels become Normal.
    #[must_use]
    pub fn level(&self) -> PowerLevel {
        self.level
    }
    /// Whether the battery selected this mode automatically.
    #[must_use]
    pub fn automatic(&self) -> bool {
        self.automatic
    }
    /// How many times longer declared work may wait.
    #[must_use]
    pub fn stretch(&self) -> u32 {
        self.stretch
    }
    /// Borrowed built-in mode or preset id.
    #[must_use]
    pub fn mode(&self) -> &'a str {
        self.mode
    }
}

/// Charging-state bits; unknown future bits are ignored by the getters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeFlags(u32);
impl ChargeFlags {
    /// Borrow the meaning of the host-stamped bits without rejecting future bits.
    #[must_use]
    pub fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    /// The battery is charging.
    #[must_use]
    pub fn charging(&self) -> bool {
        self.0 & 1 != 0
    }
    /// External power is present.
    #[must_use]
    pub fn external_power(&self) -> bool {
        self.0 & 2 != 0
    }
    /// The battery is below the host's low threshold.
    #[must_use]
    pub fn battery_low(&self) -> bool {
        self.0 & 4 != 0
    }
}
