//! Bluetooth Low Energy (see [ADR 0027](../../../docs/adr/0027-bluetooth.md)):
//! [`Env::bluetooth`](crate::Env::bluetooth) is `Some` where the kernel
//! may use a radio ([`Capabilities::bluetooth`](crate::Capabilities)).
//!
//! A plugin that declares [`Uses::BLUETOOTH`](crate::Uses::BLUETOOTH)
//! asks with the functions of this module, which write
//! [`Command::Bluetooth`], and hears everything as its own events, each a
//! [`BtEvent`]: a scan's on the event it names ([`scan`]), a link's on the
//! link's ([`connect`]), being seen on its own ([`advertise`]), a GATT
//! service's on the service's ([`ServiceSpec`]). Every command is
//! answered, by its result or a [`BtEvent::Failed`] that says which
//! command failed and why.
//!
//! - **Scanning**: [`scan`] finds devices in reach ([`BtEvent::Found`]),
//!   always for a while ([`Scan::lasting`]), by default passively and
//!   seldom ([`ScanPower::Low`]). Several plugins' scans are one scan of
//!   the radio at the cheapest power that serves them all, each plugin
//!   hearing only what passes its own filters.
//! - **A GATT client**: [`connect`] (by default at [`LinkPower::Low`]),
//!   [`discover`] the services, [`read`], [`write()`], [`subscribe`] to
//!   notifications ([`BtEvent::Value`]), or [`subscribe_to`] a service's
//!   characteristic by its UUIDs, handles found by the kernel.
//! - **Pairing** ([`pair`]) is always confirmed by the reader, in the
//!   kernel's own question: a plugin only asks.
//! - **Being seen**: [`advertise`] for a while, and a GATT server of
//!   services declared at registration
//!   ([`Registrar::add_gatt_service`](crate::Registrar::add_gatt_service)):
//!   centrals' reads and writes come as events, answered with [`respond`]
//!   or [`refuse`], and [`notify_centrals`] sends to those subscribed.
//! - **Keys of a remote or a keyboard**: a bonded device that types is the
//!   kernel's: [`remote_keys`] connects it and its keys arrive as buttons
//!   of the key maps (`button.hid.right`, `button.hid.page-down` …), with
//!   no plugin parsing its reports. A plugin a device is given to may ask
//!   for the reports themselves ([`hid_reports`]).
//!
//! A plugin sees only devices its own scans found and bonded devices the
//! reader gave it; anything else fails with [`BtError::NotFound`].
//! Devices are known by a [`DeviceId`], never by their address. The
//! Bluetooth manager (the plugin whose screen is bound to
//! [`slots::BLUETOOTH`](crate::builtin::slots::BLUETOOTH)) sees every
//! bond; forgetting and giving otherwise count only as the reader's own
//! doing. Events written out for others (observers, rules) carry no
//! payloads. Scans, links and advertising end with the screen that was
//! showing when they were asked for (unless declared work asked for
//! them), and all before the device sleeps; the radio is off whenever
//! nothing needs it.
//!
//! ```
//! use paperpaw_sdk::bluetooth::{self, BtEvent, Uuid};
//! use paperpaw_sdk::{Event, Registrar, RegisterError};
//!
//! const FOUND: Event<BtEvent> = Event::new("pulse.found");
//! const LINK: Event<BtEvent> = Event::new("pulse.link");
//! const HEART_RATE: Uuid = Uuid::sig(0x180D);
//! const MEASUREMENT: Uuid = Uuid::sig(0x2A37);
//!
//! fn register(r: &mut Registrar) -> Result<(), RegisterError> {
//!     // A scan for the first heart-rate belt, then its measurements.
//!     r.on(FOUND, |_, event: &BtEvent, out| {
//!         if let BtEvent::Found(device) = event {
//!             out.push(bluetooth::connect(device.id, &LINK).command());
//!         }
//!     })?;
//!     r.on(LINK, |_, event: &BtEvent, out| match event {
//!         BtEvent::Connected { device, .. } => {
//!             out.push(bluetooth::subscribe_to(*device, HEART_RATE, MEASUREMENT));
//!         }
//!         BtEvent::Value { bytes, .. } => {
//!             // Flags first; the rate is one byte, or two where flag 0 is set.
//!             let bpm = match bytes.as_slice() {
//!                 [flags, low, high, ..] if flags & 1 == 1 => u16::from_le_bytes([*low, *high]),
//!                 [_, rate, ..] => u16::from(*rate),
//!                 _ => return,
//!             };
//!             let _ = bpm;
//!         }
//!         _ => {}
//!     })
//! }
//! # let _ = bluetooth::scan(&FOUND).service(HEART_RATE).first().command();
//! ```

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::time::Duration;

use crate::event::{Event, EventData, ParamValue};
use crate::work::Refusal;
use crate::{Command, Id, Params};

mod builders;
mod event;

/// A Bluetooth UUID: of a service, a characteristic or a descriptor.
/// Those of the Bluetooth SIG are 16-bit numbers in a base UUID
/// ([`Uuid::sig`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uuid(u128);

/// The Bluetooth base UUID, `00000000-0000-1000-8000-00805f9b34fb`.
const BASE: u128 = 0x0000_0000_0000_1000_8000_0080_5f9b_34fb;

impl Uuid {
    /// A number of the Bluetooth SIG's, e.g. `0x180D` (heart rate) or
    /// `0x1812` (HID).
    pub const fn sig(number: u16) -> Uuid {
        Uuid(BASE | ((number as u128) << 96))
    }

    pub const fn from_u128(value: u128) -> Uuid {
        Uuid(value)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// Its 16-bit number, where it is one of the SIG's.
    pub const fn short(self) -> Option<u16> {
        if self.0 & !(0xffff_u128 << 96) == BASE {
            Some((self.0 >> 96) as u16)
        } else {
            None
        }
    }

    /// From `180d` or `0x180D` (a SIG number), or the 36 characters of a
    /// full UUID.
    pub fn parse(text: &str) -> Option<Uuid> {
        let text = text.trim();
        let short = text.strip_prefix("0x").unwrap_or(text);
        if short.len() == 4 {
            return u16::from_str_radix(short, 16).ok().map(Uuid::sig);
        }
        let hex: String = text.chars().filter(|c| *c != '-').collect();
        if text.len() != 36 || hex.len() != 32 {
            return None;
        }
        u128::from_str_radix(&hex, 16).ok().map(Uuid)
    }
}

impl core::fmt::Display for Uuid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let v = self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (v >> 96) as u32,
            (v >> 80) as u16,
            (v >> 64) as u16,
            (v >> 48) as u16,
            v & 0xffff_ffff_ffff
        )
    }
}

/// A device as the kernel knows it: a number of its own, stable across
/// starts for a bonded device (a plugin may keep it, e.g. in a setting).
/// Its address is never shown to plugins: it may be random and change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(u32);

impl DeviceId {
    /// The device numbered `number` by the kernel.
    #[cfg(any(feature = "kernel", feature = "testing", test))]
    pub const fn new(number: u32) -> DeviceId {
        DeviceId(number)
    }

    pub const fn number(self) -> u32 {
        self.0
    }
}

impl ParamValue for DeviceId {
    fn to_param(&self) -> String {
        self.0.to_string()
    }

    fn from_param(text: &str) -> Option<Self> {
        text.parse().ok().map(DeviceId)
    }
}

/// An attribute's place in a device's attribute table (a characteristic's
/// value or a descriptor), from [`BtEvent::Discovered`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle(u16);

impl Handle {
    #[cfg(any(feature = "kernel", feature = "testing", test))]
    pub const fn new(handle: u16) -> Handle {
        Handle(handle)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A central's read or write of the device's GATT server, to answer with
/// [`respond`] or [`refuse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GattRequest(u32);

impl GattRequest {
    #[cfg(any(feature = "kernel", feature = "testing", test))]
    pub const fn new(number: u32) -> GattRequest {
        GattRequest(number)
    }

    pub const fn number(self) -> u32 {
        self.0
    }
}

/// An ATT error (Core Specification Vol 3, Part F, 3.4.1.1): what a
/// device answered, or what the plugin's GATT server answers
/// ([`refuse`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttError(u8);

impl AttError {
    pub const INVALID_HANDLE: AttError = AttError(0x01);
    pub const READ_NOT_PERMITTED: AttError = AttError(0x02);
    pub const WRITE_NOT_PERMITTED: AttError = AttError(0x03);
    pub const INSUFFICIENT_AUTHENTICATION: AttError = AttError(0x05);
    pub const REQUEST_NOT_SUPPORTED: AttError = AttError(0x06);
    pub const INVALID_OFFSET: AttError = AttError(0x07);
    pub const INVALID_LENGTH: AttError = AttError(0x0D);
    pub const UNLIKELY: AttError = AttError(0x0E);
    pub const INSUFFICIENT_ENCRYPTION: AttError = AttError(0x0F);
    /// The value is not one the service takes (the first of the codes
    /// left to applications).
    pub const VALUE_NOT_ALLOWED: AttError = AttError(0x80);

    pub const fn new(code: u8) -> AttError {
        AttError(code)
    }

    pub const fn code(self) -> u8 {
        self.0
    }
}

/// What can be done with a characteristic: a set of the bits of its
/// declaration (Core Specification Vol 3, Part G, 3.3.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Properties(u8);

impl Properties {
    pub const NONE: Properties = Properties(0);
    pub const BROADCAST: Properties = Properties(0x01);
    pub const READ: Properties = Properties(0x02);
    pub const WRITE_WITHOUT_RESPONSE: Properties = Properties(0x04);
    pub const WRITE: Properties = Properties(0x08);
    pub const NOTIFY: Properties = Properties(0x10);
    pub const INDICATE: Properties = Properties(0x20);

    pub const fn contains(self, other: Properties) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(bits: u8) -> Properties {
        Properties(bits)
    }

    const fn with(self, other: Properties) -> Properties {
        Properties(self.0 | other.0)
    }
}

impl core::ops::BitOr for Properties {
    type Output = Properties;

    fn bitor(self, other: Properties) -> Properties {
        self.with(other)
    }
}

/// What a device is, from its appearance and its services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DeviceKind {
    /// It types: a keyboard.
    Keyboard,
    /// A remote, a page-turner or a media remote (an HID device that is no
    /// keyboard).
    Remote,
    /// A sensor (heart rate, temperature …).
    Sensor,
    /// A phone or a computer.
    Phone,
    #[default]
    Other,
}

impl DeviceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            DeviceKind::Keyboard => "keyboard",
            DeviceKind::Remote => "remote",
            DeviceKind::Sensor => "sensor",
            DeviceKind::Phone => "phone",
            DeviceKind::Other => "other",
        }
    }

    pub fn parse(text: &str) -> DeviceKind {
        match text {
            "keyboard" => DeviceKind::Keyboard,
            "remote" => DeviceKind::Remote,
            "sensor" => DeviceKind::Sensor,
            "phone" => DeviceKind::Phone,
            _ => DeviceKind::Other,
        }
    }

    /// Whether its keys can go to the key maps ([`remote_keys`]).
    pub const fn can_type(self) -> bool {
        matches!(self, DeviceKind::Keyboard | DeviceKind::Remote)
    }
}

/// How a link is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Security {
    #[default]
    None,
    /// Encrypted, with keys from pairing.
    Encrypted,
    /// Encrypted, and pairing compared or typed a number.
    Authenticated,
}

/// Who may read or write a characteristic of the plugin's GATT server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Access {
    /// Anyone in reach.
    Open,
    /// A central paired with the device (pairing the reader confirmed).
    #[default]
    Encrypted,
    /// A central paired with a number compared or typed.
    Authenticated,
}

/// How a value came: read, or sent by the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueKind {
    Read,
    Notified,
    Indicated,
}

/// How much a scan may cost: the kernel scans at the highest power any
/// plugin asks for, never more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[non_exhaustive]
pub enum ScanPower {
    /// Passive, a small part of the time: finds what advertises often.
    #[default]
    Low,
    /// Half the time.
    Balanced,
    /// All the time, for a few seconds (at most 5): only as the reader's
    /// own doing.
    Fast,
}

/// How much a link may cost: the kernel keeps a link at the highest power
/// any of its users asks for, never more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[non_exhaustive]
pub enum LinkPower {
    /// Long intervals, the device allowed to skip some: for values that
    /// come now and then.
    #[default]
    Low,
    /// Short intervals: keys, a reading while it is shown.
    Responsive,
    /// The shortest, while data flows; back to [`LinkPower::Low`] when it
    /// stops.
    Burst,
}

/// How much being seen may cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[non_exhaustive]
pub enum AdvertisePower {
    /// About once a second.
    #[default]
    Low,
    /// A few times a second.
    Balanced,
    /// As often as allowed, for at most 30 seconds.
    Fast,
}

/// Data a device advertises for its maker: the company's number
/// (assigned by the Bluetooth SIG), then the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ManufacturerData {
    pub company: u16,
    pub bytes: Vec<u8>,
}

impl ManufacturerData {
    pub fn new(company: u16, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            company,
            bytes: bytes.into(),
        }
    }
}

/// Data a device advertises for one of its services, e.g. a broadcast
/// sensor's readings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceData {
    pub service: Uuid,
    pub bytes: Vec<u8>,
}

impl ServiceData {
    pub fn new(service: Uuid, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            service,
            bytes: bytes.into(),
        }
    }
}

/// A device found by a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Device {
    pub id: DeviceId,
    /// The name it gives, where it gives one.
    pub name: Option<String>,
    /// Its signal, in dBm (e.g. -50 strong, -90 weak), where known.
    pub signal: Option<i8>,
    /// The services it says it offers.
    pub services: Vec<Uuid>,
    pub manufacturer_data: Vec<ManufacturerData>,
    pub service_data: Vec<ServiceData>,
    /// The power it sends at, in dBm, where it says.
    pub tx_power: Option<i8>,
    /// Its GAP appearance, where it gives one.
    pub appearance: Option<u16>,
    /// It is bonded with this device (and given to the plugin).
    pub bonded: bool,
    pub kind: DeviceKind,
}

impl Device {
    /// A device found, for a plugin's own tests.
    #[cfg(any(feature = "kernel", feature = "testing", test))]
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            name: None,
            signal: None,
            services: Vec::new(),
            manufacturer_data: Vec::new(),
            service_data: Vec::new(),
            tx_power: None,
            appearance: None,
            bonded: false,
            kind: DeviceKind::Other,
        }
    }

    /// Its signal in bars, 1 to 4, as for Wi-Fi networks.
    pub fn bars(&self) -> u8 {
        match self.signal {
            Some(s) if s >= -60 => 4,
            Some(s) if s >= -70 => 3,
            Some(s) if s >= -80 => 2,
            _ => 1,
        }
    }
}

/// A descriptor of a characteristic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GattDescriptor {
    pub uuid: Uuid,
    pub handle: Handle,
}

/// A characteristic of a device's service.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GattCharacteristic {
    pub uuid: Uuid,
    /// Where its value is read, written and subscribed to.
    pub handle: Handle,
    pub properties: Properties,
    pub descriptors: Vec<GattDescriptor>,
}

/// A service of a device, with its characteristics.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GattService {
    pub uuid: Uuid,
    pub characteristics: Vec<GattCharacteristic>,
}

#[cfg(any(feature = "kernel", feature = "testing", test))]
impl GattDescriptor {
    pub fn new(uuid: Uuid, handle: Handle) -> Self {
        Self { uuid, handle }
    }
}

#[cfg(any(feature = "kernel", feature = "testing", test))]
impl GattCharacteristic {
    pub fn new(uuid: Uuid, handle: Handle, properties: Properties) -> Self {
        Self {
            uuid,
            handle,
            properties,
            descriptors: Vec::new(),
        }
    }
}

#[cfg(any(feature = "kernel", feature = "testing", test))]
impl GattService {
    pub fn new(uuid: Uuid, characteristics: Vec<GattCharacteristic>) -> Self {
        Self {
            uuid,
            characteristics,
        }
    }
}

/// Characteristic `characteristic` of service `service` among `services`
/// (those [`BtEvent::Discovered`] lists).
pub fn characteristic(
    services: &[GattService],
    service: Uuid,
    characteristic: Uuid,
) -> Option<&GattCharacteristic> {
    services
        .iter()
        .filter(|s| s.uuid == service)
        .flat_map(|s| s.characteristics.iter())
        .find(|c| c.uuid == characteristic)
}

/// Why something with Bluetooth did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BtError {
    /// There is no radio here, or it could not be switched on.
    Off,
    /// The plugin does not declare [`Uses::BLUETOOTH`](crate::Uses), or
    /// asked what only the reader or the Bluetooth manager may.
    NotAllowed,
    /// The device is not one of the plugin's, not in reach or not
    /// connected; the handle or the UUIDs are not the device's.
    NotFound,
    /// No link is left ([`BluetoothView::max_links`]), or the device's
    /// memory cannot take the radio now.
    Busy,
    /// The device did not answer in time.
    Timeout,
    /// The reader said no (to pairing).
    Declined,
    /// The link was lost.
    Lost,
    /// The device answered with this ATT error.
    Att(AttError),
    /// Not now: asked again too soon, or the mode holds it; it says when
    /// to try again.
    Held(Refusal),
    /// Not done here (e.g. advertising on a shell that cannot).
    Unsupported,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl BtError {
    /// Its name, e.g. `not-found` (an ATT error and a hold by their kind
    /// alone).
    pub const fn as_str(self) -> &'static str {
        match self {
            BtError::Off => "off",
            BtError::NotAllowed => "not-allowed",
            BtError::NotFound => "not-found",
            BtError::Busy => "busy",
            BtError::Timeout => "timeout",
            BtError::Declined => "declined",
            BtError::Lost => "lost",
            BtError::Att(_) => "att",
            BtError::Held(_) => "held",
            BtError::Unsupported => "unsupported",
            BtError::Other(_) => "other",
        }
    }
}

impl core::fmt::Display for BtError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BtError::Att(error) => write!(f, "att-{:02x}", error.code()),
            BtError::Other(code) => write!(f, "other-{code}"),
            other => f.write_str(other.as_str()),
        }
    }
}

impl ParamValue for BtError {
    fn to_param(&self) -> String {
        self.to_string()
    }

    fn from_param(text: &str) -> Option<Self> {
        Some(match text {
            "off" => BtError::Off,
            "not-allowed" => BtError::NotAllowed,
            "not-found" => BtError::NotFound,
            "busy" => BtError::Busy,
            "timeout" => BtError::Timeout,
            "declined" => BtError::Declined,
            "lost" => BtError::Lost,
            "unsupported" => BtError::Unsupported,
            "held" => BtError::Held(Refusal::new(
                crate::work::Reason::TooOften,
                crate::work::Retry::WhenAllowed,
            )),
            other => match other.strip_prefix("other-") {
                Some(code) => BtError::Other(code.parse().ok()?),
                None => BtError::Att(AttError(
                    u8::from_str_radix(other.strip_prefix("att-")?, 16).ok()?,
                )),
            },
        })
    }
}

/// Which command a [`BtFailure`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BtOp {
    Scan,
    Connect,
    Disconnect,
    Discover,
    Read,
    Write,
    Subscribe,
    Unsubscribe,
    Pair,
    Forget,
    Give,
    Keys,
    HidReports,
    Advertise,
    Notify,
    Respond,
    Extended,
}

const OPS: [BtOp; 17] = [
    BtOp::Scan,
    BtOp::Connect,
    BtOp::Disconnect,
    BtOp::Discover,
    BtOp::Read,
    BtOp::Write,
    BtOp::Subscribe,
    BtOp::Unsubscribe,
    BtOp::Pair,
    BtOp::Forget,
    BtOp::Give,
    BtOp::Keys,
    BtOp::HidReports,
    BtOp::Advertise,
    BtOp::Notify,
    BtOp::Respond,
    BtOp::Extended,
];

impl BtOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            BtOp::Scan => "scan",
            BtOp::Connect => "connect",
            BtOp::Disconnect => "disconnect",
            BtOp::Discover => "discover",
            BtOp::Read => "read",
            BtOp::Write => "write",
            BtOp::Subscribe => "subscribe",
            BtOp::Unsubscribe => "unsubscribe",
            BtOp::Pair => "pair",
            BtOp::Forget => "forget",
            BtOp::Give => "give",
            BtOp::Keys => "keys",
            BtOp::HidReports => "hid-reports",
            BtOp::Advertise => "advertise",
            BtOp::Notify => "notify",
            BtOp::Respond => "respond",
            BtOp::Extended => "extended",
        }
    }

    pub fn parse(text: &str) -> Option<BtOp> {
        OPS.into_iter().find(|op| op.as_str() == text)
    }
}

/// A command that failed: which, about what, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BtFailure {
    pub op: BtOp,
    pub device: Option<DeviceId>,
    pub handle: Option<Handle>,
    pub error: BtError,
}

impl BtFailure {
    pub const fn new(op: BtOp, error: BtError) -> Self {
        Self {
            op,
            device: None,
            handle: None,
            error,
        }
    }

    #[must_use]
    pub const fn device(mut self, device: DeviceId) -> Self {
        self.device = Some(device);
        self
    }

    #[must_use]
    pub const fn handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }
}

/// What happened, as a plugin's event (see the [module documentation](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BtEvent {
    /// A scan found `device` (each device once a scan, again where its
    /// name, services or data change and [`Scan::updates`] asked so).
    Found(Device),
    /// The scan ended (its time was up, it found what it was to find, it
    /// was stopped, or its screen closed).
    ScanEnded,
    #[non_exhaustive]
    Connected {
        device: DeviceId,
        /// The most bytes a value carries at once on the link.
        mtu: u16,
        security: Security,
    },
    /// The link ended; `reason` where it was not asked for.
    #[non_exhaustive]
    Disconnected {
        device: DeviceId,
        reason: Option<BtError>,
    },
    /// The device's services, after [`discover`].
    #[non_exhaustive]
    Discovered {
        device: DeviceId,
        services: Vec<GattService>,
    },
    /// The device's services changed: discover them again.
    #[non_exhaustive]
    ServicesChanged { device: DeviceId },
    /// A value read ([`read`]) or sent by the device ([`subscribe`]).
    #[non_exhaustive]
    Value {
        device: DeviceId,
        handle: Handle,
        kind: ValueKind,
        bytes: Vec<u8>,
    },
    /// A [`write()`] with a response was done.
    #[non_exhaustive]
    Written { device: DeviceId, handle: Handle },
    /// A write without response went out: the next may follow.
    #[non_exhaustive]
    Sent { device: DeviceId, handle: Handle },
    /// Pairing ([`pair`]) is done: the device is bonded and given to the
    /// plugin that asked.
    #[non_exhaustive]
    Paired { device: DeviceId },
    /// A command without a result of its own was done (forgetting,
    /// giving, a remote's keys).
    #[non_exhaustive]
    Done { op: BtOp, device: DeviceId },
    /// A command could not be done.
    Failed(BtFailure),
    /// The device is advertised ([`advertise`]).
    Advertising,
    /// Advertising ended (its time was up, it was stopped, its screen
    /// closed).
    AdvertisingEnded,
    /// A central reads a characteristic of the plugin's service: answer
    /// with [`respond`] or [`refuse`].
    #[non_exhaustive]
    ReadRequest {
        request: GattRequest,
        service: Uuid,
        characteristic: Uuid,
        central: DeviceId,
    },
    /// A central wrote `bytes` to a characteristic of the plugin's
    /// service; `request` where it waits for an answer.
    #[non_exhaustive]
    WriteRequest {
        request: Option<GattRequest>,
        service: Uuid,
        characteristic: Uuid,
        central: DeviceId,
        bytes: Vec<u8>,
    },
    /// A central subscribed to a characteristic of the plugin's service.
    #[non_exhaustive]
    Subscribed {
        service: Uuid,
        characteristic: Uuid,
        central: DeviceId,
    },
    /// A central no longer subscribes.
    #[non_exhaustive]
    Unsubscribed {
        service: Uuid,
        characteristic: Uuid,
        central: DeviceId,
    },
    /// An HID device's report map ([`hid_reports`]), once.
    #[non_exhaustive]
    HidReportMap { device: DeviceId, bytes: Vec<u8> },
    /// An input report of an HID device ([`hid_reports`]), as it came,
    /// with its report id where the device numbers its reports.
    #[non_exhaustive]
    HidReport {
        device: DeviceId,
        report: Option<u8>,
        bytes: Vec<u8>,
    },
    /// What a later feature tells (see [`extended`]).
    #[non_exhaustive]
    Extended { key: Id, params: Params },
}

/// The parameters of a [`BtEvent`] written out for observers and rules:
/// what happened, never a payload.
pub mod bt_params {
    pub const KIND: &str = "kind";
    pub const DEVICE: &str = "device";
    pub const HANDLE: &str = "handle";
    pub const OP: &str = "op";
    pub const ERROR: &str = "error";
    /// How many bytes a value or a report carried (not the bytes).
    pub const LENGTH: &str = "length";
    pub const SERVICE: &str = "service";
    pub const CHARACTERISTIC: &str = "characteristic";
}

/// The commands as the kernel gets them (plugins write them with the
/// functions of this module).
mod request;

#[cfg(feature = "kernel")]
pub use request::{BtCommand, Response};
#[cfg(not(feature = "kernel"))]
use request::{BtCommand, Response};

impl BtCommand {
    fn command(self) -> Command {
        Command::Bluetooth(Box::new(BtRequest {
            command: self,
            from: None,
        }))
    }
}

/// A Bluetooth command on its way to the kernel, with the plugin that
/// sent it (filled in by the kernel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtRequest {
    command: BtCommand,
    from: Option<Id>,
}

#[cfg(feature = "kernel")]
impl BtRequest {
    pub fn command(&self) -> &BtCommand {
        &self.command
    }

    /// Says that plugin `plugin` sent it.
    pub fn set_from(&mut self, plugin: Id) {
        self.from = Some(plugin);
    }

    pub fn into_parts(self) -> (BtCommand, Option<Id>) {
        (self.command, self.from)
    }
}

/// The longest a scan runs ([`Scan::lasting`]).
const SCAN_MOST: Duration = Duration::from_secs(60);
/// The longest a fast scan runs.
const FAST_SCAN_MOST: Duration = Duration::from_secs(5);

/// A scan being written ([`scan`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Scan {
    pub events: Id,
    /// Only devices that say they offer one of these (all, where empty).
    pub services: Vec<Uuid>,
    /// Only devices whose name begins so.
    pub name_prefix: Option<String>,
    /// Only devices with data of this company.
    pub manufacturer: Option<u16>,
    pub power: ScanPower,
    /// Asks devices for more (their names): it costs them and this device
    /// a little.
    pub active: bool,
    /// Ends at the first device found.
    pub first: bool,
    /// Tells a device found again where its name, services or data
    /// changed (a broadcast sensor's readings).
    pub updates: bool,
    /// How long it runs (at most a minute; a fast scan at most 5 seconds).
    pub lasting: Duration,
}

/// A link being asked for ([`connect`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Connect {
    pub device: DeviceId,
    pub events: Id,
    pub power: LinkPower,
    /// Let go when nothing went over it this long.
    pub idle: Option<Duration>,
}

/// A discovery being asked for ([`discover`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Discover {
    pub device: DeviceId,
    /// Only these services (all, where empty): less to find, less radio.
    pub services: Vec<Uuid>,
}

/// A write being written ([`write()`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    device: DeviceId,
    handle: Handle,
    bytes: Vec<u8>,
    response: Response,
}

/// Being seen, being written ([`advertise`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Advertising {
    pub events: Id,
    pub name: Option<Cow<'static, str>>,
    pub services: Vec<Uuid>,
    pub manufacturer_data: Vec<ManufacturerData>,
    pub service_data: Vec<ServiceData>,
    pub appearance: Option<u16>,
    /// Centrals may connect (to the plugin's GATT server).
    pub connectable: bool,
    pub power: AdvertisePower,
    /// How long, at most 10 minutes (a fast one at most 30 seconds).
    pub lasting: Duration,
}

/// The longest a plugin is advertised at a time.
const ADVERTISE_MOST: Duration = Duration::from_secs(600);
const FAST_ADVERTISE_MOST: Duration = Duration::from_secs(30);

/// Looks for devices for a while (10 seconds unless [`Scan::lasting`]
/// says otherwise), passively and seldom unless [`Scan::power`] says
/// otherwise; each found comes as `events` ([`BtEvent::Found`]), then
/// [`BtEvent::ScanEnded`].
pub fn scan(events: &Event<BtEvent>) -> Scan {
    Scan {
        events: events.id().clone(),
        services: Vec::new(),
        name_prefix: None,
        manufacturer: None,
        power: ScanPower::Low,
        active: false,
        first: false,
        updates: false,
        lasting: Duration::from_secs(10),
    }
}

/// Ends the scan whose results go to `events`.
pub fn stop_scan(events: &Event<BtEvent>) -> Command {
    BtCommand::StopScan {
        events: events.id().clone(),
    }
    .command()
}

/// Connects to `device`, at [`LinkPower::Low`] unless
/// [`Connect::power`] says otherwise; all about the link comes as
/// `events`, the answers of the commands on it too.
pub fn connect(device: DeviceId, events: &Event<BtEvent>) -> Connect {
    Connect {
        device,
        events: events.id().clone(),
        power: LinkPower::Low,
        idle: None,
    }
}

pub fn disconnect(device: DeviceId) -> Command {
    BtCommand::Disconnect { device }.command()
}

/// Finds the services of a connected device and their characteristics
/// ([`BtEvent::Discovered`]); [`Discover::service`] finds only those
/// needed.
pub fn discover(device: DeviceId) -> Discover {
    Discover {
        device,
        services: Vec::new(),
    }
}

/// Reads the value or descriptor at `handle` ([`BtEvent::Value`]).
pub fn read(device: DeviceId, handle: Handle) -> Command {
    BtCommand::Read { device, handle }.command()
}

/// Writes `bytes` at `handle`, with a response ([`BtEvent::Written`])
/// unless [`Write::without_response`]. Values longer than the link
/// carries at once are written in parts by the kernel.
pub fn write(device: DeviceId, handle: Handle, bytes: impl Into<Vec<u8>>) -> Write {
    Write {
        device,
        handle,
        bytes: bytes.into(),
        response: Response::Wanted,
    }
}

/// Notifications (or indications) of the characteristic at `handle` come
/// as [`BtEvent::Value`].
pub fn subscribe(device: DeviceId, handle: Handle) -> Command {
    BtCommand::Subscribe { device, handle }.command()
}

/// [`subscribe`] to characteristic `characteristic` of service `service`,
/// found by the kernel: no discovery and no handle needed.
pub fn subscribe_to(device: DeviceId, service: Uuid, characteristic: Uuid) -> Command {
    BtCommand::SubscribeTo {
        device,
        service,
        characteristic,
    }
    .command()
}

pub fn unsubscribe(device: DeviceId, handle: Handle) -> Command {
    BtCommand::Unsubscribe { device, handle }.command()
}

/// Asks the reader to pair with connected `device` ([`BtEvent::Paired`],
/// or [`BtError::Declined`]).
pub fn pair(device: DeviceId) -> Command {
    BtCommand::Pair { device }.command()
}

/// Forgets bonded `device`: its keys and what it was given. The reader's
/// own doing, or the Bluetooth manager's.
pub fn forget(device: DeviceId, events: &Event<BtEvent>) -> Command {
    BtCommand::Forget {
        device,
        events: events.id().clone(),
    }
    .command()
}

/// Gives bonded `device` to `plugin`: it may connect to it. The reader's
/// own doing, or the Bluetooth manager's.
pub fn give(device: DeviceId, plugin: impl Into<Id>, events: &Event<BtEvent>) -> Command {
    BtCommand::Give {
        device,
        plugin: plugin.into(),
        events: events.id().clone(),
    }
    .command()
}

/// Takes bonded `device` back from `plugin`. The reader's own doing, or
/// the Bluetooth manager's.
pub fn take_back(device: DeviceId, plugin: impl Into<Id>, events: &Event<BtEvent>) -> Command {
    BtCommand::TakeBack {
        device,
        plugin: plugin.into(),
        events: events.id().clone(),
    }
    .command()
}

/// The keys of bonded `device` go to the key maps: the kernel connects
/// it, responsively. The Bluetooth manager's, a plugin's the device is
/// given to, or the reader's own doing.
pub fn remote_keys(device: DeviceId, events: &Event<BtEvent>) -> Command {
    BtCommand::KeysOn {
        device,
        events: events.id().clone(),
    }
    .command()
}

/// The keys of `device` no longer go to the key maps: the kernel lets it
/// go.
pub fn remote_keys_off(device: DeviceId, events: &Event<BtEvent>) -> Command {
    BtCommand::KeysOff {
        device,
        events: events.id().clone(),
    }
    .command()
}

/// The report map and input reports of HID `device` (given to the
/// plugin) come as `events` ([`BtEvent::HidReportMap`],
/// [`BtEvent::HidReport`]).
pub fn hid_reports(device: DeviceId, events: &Event<BtEvent>) -> Command {
    BtCommand::HidReports {
        device,
        events: events.id().clone(),
    }
    .command()
}

/// Advertises the device for a while (a minute unless
/// [`Advertising::lasting`] says otherwise), about once a second unless
/// [`Advertising::power`] says otherwise; what happens comes as `events`.
pub fn advertise(events: &Event<BtEvent>) -> Advertising {
    Advertising {
        events: events.id().clone(),
        name: None,
        services: Vec::new(),
        manufacturer_data: Vec::new(),
        service_data: Vec::new(),
        appearance: None,
        connectable: false,
        power: AdvertisePower::Low,
        lasting: Duration::from_secs(60),
    }
}

pub fn stop_advertising() -> Command {
    BtCommand::StopAdvertising.command()
}

/// Sends `bytes` of characteristic `characteristic` of the plugin's
/// service `service` to the centrals subscribed to it.
pub fn notify_centrals(service: Uuid, characteristic: Uuid, bytes: impl Into<Vec<u8>>) -> Command {
    BtCommand::NotifyCentrals {
        service,
        characteristic,
        bytes: bytes.into(),
    }
    .command()
}

/// Answers a central's read or write ([`BtEvent::ReadRequest`],
/// [`BtEvent::WriteRequest`]).
pub fn respond(request: GattRequest, bytes: impl Into<Vec<u8>>) -> Command {
    BtCommand::Respond {
        request,
        bytes: bytes.into(),
    }
    .command()
}

/// Refuses a central's read or write with `error`.
pub fn refuse(request: GattRequest, error: AttError) -> Command {
    BtCommand::Refuse { request, error }.command()
}

/// A command of a feature to come, named by `key`: answered by
/// [`BtEvent::Extended`], or [`BtError::Unsupported`] where the kernel
/// does not know it. Later features come as functions writing it, so
/// they need no new plugin interface.
pub fn extended(key: impl Into<Id>, params: Params) -> Command {
    BtCommand::Extended {
        key: key.into(),
        params,
    }
    .command()
}

/// A characteristic of a service of the device's own GATT server.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CharacteristicSpec {
    pub uuid: Uuid,
    pub properties: Properties,
    pub access: Access,
    /// The longest value it takes.
    pub max_len: u16,
}

impl CharacteristicSpec {
    /// Characteristic `uuid`, nothing allowed yet, open to paired centrals
    /// only ([`Access::Encrypted`]), up to 20 bytes.
    pub const fn new(uuid: Uuid) -> Self {
        Self {
            uuid,
            properties: Properties::NONE,
            access: Access::Encrypted,
            max_len: 20,
        }
    }

    #[must_use]
    pub const fn read(mut self) -> Self {
        self.properties = self.properties.with(Properties::READ);
        self
    }

    #[must_use]
    pub const fn write(mut self) -> Self {
        self.properties = self.properties.with(Properties::WRITE);
        self
    }

    #[must_use]
    pub const fn write_without_response(mut self) -> Self {
        self.properties = self.properties.with(Properties::WRITE_WITHOUT_RESPONSE);
        self
    }

    #[must_use]
    pub const fn notify(mut self) -> Self {
        self.properties = self.properties.with(Properties::NOTIFY);
        self
    }

    #[must_use]
    pub const fn indicate(mut self) -> Self {
        self.properties = self.properties.with(Properties::INDICATE);
        self
    }

    #[must_use]
    pub const fn access(mut self, access: Access) -> Self {
        self.access = access;
        self
    }

    #[must_use]
    pub const fn max_len(mut self, max_len: u16) -> Self {
        self.max_len = max_len;
        self
    }
}

/// A service of the device's own GATT server, declared at registration
/// ([`Registrar::add_gatt_service`](crate::Registrar::add_gatt_service)):
/// centrals' reads, writes and subscriptions of its characteristics come
/// as `events`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceSpec {
    pub uuid: Uuid,
    pub characteristics: Vec<CharacteristicSpec>,
    pub events: Id,
}

impl ServiceSpec {
    pub fn new(uuid: Uuid, events: &Event<BtEvent>) -> Self {
        Self {
            uuid,
            characteristics: Vec::new(),
            events: events.id().clone(),
        }
    }

    #[must_use]
    pub fn characteristic(mut self, characteristic: CharacteristicSpec) -> Self {
        self.characteristics.push(characteristic);
        self
    }
}

/// Where the radio is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RadioState {
    #[default]
    Off,
    On,
}

/// A device bonded with this one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Bonded {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    /// A link to it is up.
    pub connected: bool,
    /// Its keys go to the key maps ([`remote_keys`]).
    pub keys: bool,
    /// The plugins it is given to (seen by the Bluetooth manager alone).
    given: Vec<Id>,
}

impl Bonded {
    #[cfg(any(feature = "kernel", feature = "testing", test))]
    pub fn new(id: DeviceId, name: impl Into<String>, kind: DeviceKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            connected: false,
            keys: false,
            given: Vec::new(),
        }
    }

    #[cfg(any(feature = "kernel", feature = "testing", test))]
    #[must_use]
    pub fn given_to(mut self, plugins: Vec<Id>) -> Self {
        self.given = plugins;
        self
    }
}

/// Bluetooth as the kernel keeps it, for every plugin's view
/// ([`BluetoothView`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Bluetooth {
    pub state: RadioState,
    /// Something uses the radio (a scan, a link, advertising).
    pub busy: bool,
    /// The most links the device keeps at once.
    pub max_links: u8,
    bonded: Vec<Bonded>,
    /// The Bluetooth manager's plugin.
    manager: Option<Id>,
}

#[cfg(any(feature = "kernel", test))]
impl Bluetooth {
    pub fn new(max_links: u8) -> Self {
        Self {
            max_links,
            ..Self::default()
        }
    }

    pub fn set_bonded(&mut self, bonded: Vec<Bonded>) {
        self.bonded = bonded;
    }

    pub fn set_manager(&mut self, manager: Option<Id>) {
        self.manager = manager;
    }
}

/// Bluetooth as the plugin running sees it ([`Env::bluetooth`](crate::Env::bluetooth)):
/// the radio, and the bonded devices given to it (all of them, for the
/// Bluetooth manager).
#[derive(Clone, Copy)]
pub struct BluetoothView<'a> {
    all: &'a Bluetooth,
    running: &'a dyn crate::secrets::Running,
}

impl core::fmt::Debug for BluetoothView<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BluetoothView")
            .field("state", &self.all.state)
            .finish_non_exhaustive()
    }
}

impl<'a> BluetoothView<'a> {
    pub(crate) fn new(all: &'a Bluetooth, running: &'a dyn crate::secrets::Running) -> Self {
        Self { all, running }
    }

    pub fn state(&self) -> RadioState {
        self.all.state
    }

    /// Something uses the radio.
    pub fn busy(&self) -> bool {
        self.all.busy
    }

    /// The most links the device keeps at once.
    pub fn max_links(&self) -> u8 {
        self.all.max_links
    }

    /// The plugin running is the Bluetooth manager.
    pub fn manager(&self) -> bool {
        self.all
            .manager
            .as_ref()
            .is_some_and(|m| self.running.runs(m.as_str()))
    }

    /// The bonded devices the plugin running may use (all of them for the
    /// Bluetooth manager).
    pub fn bonded(&self) -> impl Iterator<Item = &'a Bonded> + '_ {
        let manager = self.manager();
        self.all
            .bonded
            .iter()
            .filter(move |b| manager || b.given.iter().any(|g| self.running.runs(g.as_str())))
    }

    /// Bonded device `id`, where the plugin running may use it.
    pub fn bond(&self, id: DeviceId) -> Option<&'a Bonded> {
        self.bonded().find(|b| b.id == id)
    }

    /// The plugins bonded device `id` is given to: for the Bluetooth
    /// manager alone (empty for others).
    pub fn given(&self, id: DeviceId) -> &'a [Id] {
        if !self.manager() {
            return &[];
        }
        self.all
            .bonded
            .iter()
            .find(|b| b.id == id)
            .map_or(&[], |b| b.given.as_slice())
    }
}

#[cfg(test)]
mod tests;
