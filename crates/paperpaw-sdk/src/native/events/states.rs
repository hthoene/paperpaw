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
    pub(super) const fn decode(state: u32) -> Self {
        match state {
            0 => Self::Off,
            2 => Self::Connecting,
            3 => Self::Connected,
            4 => Self::AccessPoint,
            _ => Self::On,
        }
    }
}

/// Why a `join` or `host` request ended without the network (`docs/abi/device.md`
/// section 4). Values this SDK does not know are [`Self::Other`], as the contract
/// requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WifiFailure {
    /// The network was not found.
    NotFound,
    /// The password was refused.
    Password,
    /// The attempt timed out.
    Timeout,
    /// A newer `join` or `host` replaced this request.
    Superseded,
    /// The device cannot do this here.
    Unsupported,
    /// The user has to act first: the request came without a recent input.
    NeedsUser,
    /// Another failure, or a reason this SDK does not know.
    Other,
}
impl WifiFailure {
    const fn decode(reason: u32) -> Self {
        match reason {
            1 => Self::NotFound,
            2 => Self::Password,
            3 => Self::Timeout,
            4 => Self::Superseded,
            5 => Self::Unsupported,
            6 => Self::NeedsUser,
            _ => Self::Other,
        }
    }
}

/// How one `join` or `host` request ended (`event.wifi_done`): exactly once,
/// success or failure, to the plugin that sent it. It says what happened to
/// this request; the current radio state is what `event.wifi_changed` and the
/// device state read say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiDone<'a> {
    pub(super) now_ms: u64,
    pub(super) request: u32,
    pub(super) ssid: &'a str,
    pub(super) status: i32,
    pub(super) reason: u32,
    pub(super) state: u32,
}
impl<'a> WifiDone<'a> {
    /// Emission time in monotonic milliseconds.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }
    /// The sender's request number, or `None` for a join the host started itself
    /// (a remembered network at wake): its failure goes to the network manager.
    #[must_use]
    pub const fn request(&self) -> Option<crate::native::device::DeviceRequest> {
        crate::native::device::DeviceRequest::new(self.request)
    }
    /// The network's name.
    #[must_use]
    pub const fn ssid(&self) -> &'a str {
        self.ssid
    }
    /// `Ok` when the network is up; else the error the request ended with.
    ///
    /// # Errors
    /// The host's status for a request that did not end with the network.
    pub const fn status(&self) -> Result<(), crate::native::Error> {
        if self.status == 0 {
            Ok(())
        } else {
            Err(crate::native::Error::from_status(self.status))
        }
    }
    /// The reason of a failed request; `None` when it succeeded.
    #[must_use]
    pub const fn failure(&self) -> Option<WifiFailure> {
        if self.status == 0 {
            None
        } else {
            Some(WifiFailure::decode(self.reason))
        }
    }
    /// The radio state the request reached when it succeeded (connected or
    /// hosting); [`WifiLinkState::Off`] for a failure.
    #[must_use]
    pub const fn state(&self) -> WifiLinkState {
        if self.status == 0 {
            WifiLinkState::decode(self.state)
        } else {
            WifiLinkState::Off
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

/// What a Bluetooth event says happened (`event.bluetooth`, `docs/abi/device.md`).
/// Values this SDK does not know are [`Self::Other`]: ignore them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BluetoothKind {
    /// The radio state or the bonded list changed: read them again. No other field is set.
    Changed,
    /// A scan found a device: `device`, `name`, `device_kind` and `signal` are set.
    Found,
    /// The scan ended: its time ran out, it was stopped, or it found what it looked for.
    ScanEnded,
    /// A device connected: `device`.
    Connected,
    /// A link ended: `device` and `reason` (`None` where the plugin asked for it).
    Disconnected,
    /// A device is bonded and given to the plugin that asked: `device`, `name`, `device_kind`.
    Paired,
    /// A command with no result of its own finished (forget, give, take back, keys): `device`.
    Done,
    /// A command failed: `device` (0 if none, e.g. a failed scan) and `reason`.
    Failed,
    /// A kind this SDK does not know.
    Other,
}
impl BluetoothKind {
    const fn decode(kind: u32) -> Self {
        match kind {
            1 => Self::Changed,
            2 => Self::Found,
            3 => Self::ScanEnded,
            4 => Self::Connected,
            5 => Self::Disconnected,
            6 => Self::Paired,
            7 => Self::Done,
            8 => Self::Failed,
            _ => Self::Other,
        }
    }
}

/// What kind of device it is; unknown values are [`Self::Other`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BluetoothDeviceKind {
    /// Another kind of device.
    Other,
    /// A keyboard.
    Keyboard,
    /// A remote.
    Remote,
    /// A sensor.
    Sensor,
    /// A phone.
    Phone,
}
impl BluetoothDeviceKind {
    const fn decode(kind: u32) -> Self {
        match kind {
            1 => Self::Keyboard,
            2 => Self::Remote,
            3 => Self::Sensor,
            4 => Self::Phone,
            _ => Self::Other,
        }
    }
}

/// Why a Bluetooth command failed or a link ended; unknown values are [`Self::Other`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BluetoothReason {
    /// Another reason, or none said.
    Other,
    /// The radio is off.
    Off,
    /// Not allowed.
    NotAllowed,
    /// The device was not found.
    NotFound,
    /// The radio is busy.
    Busy,
    /// Timed out.
    Timeout,
    /// The reader declined (the pairing confirmation).
    Declined,
    /// The link was lost.
    Lost,
    /// An attribute-protocol error (unused in 1.0: GATT is not carried).
    Att,
    /// Held by another plugin.
    Held,
    /// Not supported on this device.
    Unsupported,
}
impl BluetoothReason {
    const fn decode(reason: u32) -> Self {
        match reason {
            1 => Self::Off,
            2 => Self::NotAllowed,
            3 => Self::NotFound,
            4 => Self::Busy,
            5 => Self::Timeout,
            6 => Self::Declined,
            7 => Self::Lost,
            8 => Self::Att,
            9 => Self::Held,
            10 => Self::Unsupported,
            _ => Self::Other,
        }
    }
}

/// A change to a Bluetooth device or a scan, sent to the plugin bound to the
/// host slot `core.bluetooth` (`event.bluetooth`, `docs/abi/device.md`). The
/// fields a kind does not use are 0 or empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BluetoothChange<'a> {
    pub(super) now_ms: u64,
    pub(super) kind: u32,
    pub(super) device: u32,
    pub(super) name: &'a str,
    pub(super) device_kind: u32,
    pub(super) signal: i32,
    pub(super) reason: u32,
}
impl<'a> BluetoothChange<'a> {
    /// Emission time in monotonic milliseconds.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }
    /// What happened.
    #[must_use]
    pub fn kind(&self) -> BluetoothKind {
        BluetoothKind::decode(self.kind)
    }
    /// The host's id of the device; 0 where the event is about no device.
    #[must_use]
    pub fn device(&self) -> u32 {
        self.device
    }
    /// The device's name, borrowed from the event.
    #[must_use]
    pub fn name(&self) -> &'a str {
        self.name
    }
    /// The device's category.
    #[must_use]
    pub fn device_kind(&self) -> BluetoothDeviceKind {
        BluetoothDeviceKind::decode(self.device_kind)
    }
    /// The signal strength in dBm, on a found device; `None` where not known.
    #[must_use]
    pub fn signal(&self) -> Option<i32> {
        (self.signal != 0).then_some(self.signal)
    }
    /// Why, on a failure or a link the plugin did not end; `None` where the
    /// plugin asked for it (reason 0 on a disconnect) or none is said.
    #[must_use]
    pub fn reason(&self) -> Option<BluetoothReason> {
        match self.reason {
            0 => None,
            other => Some(BluetoothReason::decode(other)),
        }
    }
}
