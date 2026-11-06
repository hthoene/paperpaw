//! Bluetooth (see [ADR 0027](../../../docs/adr/0027-bluetooth.md)): the
//! radio the shell drives, shared by plugins. The kernel keeps each
//! plugin's scans, links and advertising, numbers links for the shell,
//! lets a plugin see only devices it found or was given, answers every
//! command, turns a bonded remote's reports into keys, runs the radio at
//! the cheapest power that serves everyone, and switches it on for the
//! first request and off a little after the last.

mod commands;
mod reports;

pub use commands::Asked;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_hid::Decoder;
use paperpaw_platform::Instant;
use paperpaw_sdk::bluetooth::{
    Access, AdvertisePower, Advertising, Bluetooth, Bonded, BtError, BtEvent, BtFailure, BtOp,
    Device, DeviceId, DeviceKind, GattService, LinkPower, ManufacturerData, Properties, RadioState,
    Scan, ScanPower, Security, ServiceData, ServiceSpec, Uuid, ValueKind,
};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{Command, Id};

/// How long the radio stays on after the last request, for the next one.
pub const RADIO_GRACE_MS: u64 = 3_000;
/// How long the radio rests after it went off by itself (the shell's
/// host failed) before it comes on again for what still wants it.
pub const RADIO_REST_MS: u64 = 10_000;
/// Devices found that nobody bonded, kept at most while the radio is off
/// (the newest).
pub const FOUND_KEPT: usize = 16;
/// The secrets that keep the bonds: `core.bluetooth.<device number>`.
pub const BOND_PREFIX: &str = "core.bluetooth.";
/// How long after a plugin's scan its next one waits, unless the reader
/// asked for it.
pub const SCAN_COOLDOWN_MS: u64 = 60_000;
/// How long a remote's link stays responsive after its last key.
pub const KEYS_IDLE_MS: u64 = 60_000;

/// What the shell is to do with the radio ([`Io::Bluetooth`](super::Io)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtCall {
    /// The radio on or off; report `Radio`.
    Radio {
        on: bool,
    },
    /// The bonds kept (at start and whenever they change), for the
    /// shell's host to know the devices again.
    Bonds(Vec<Bond>),
    /// Scan for devices at `power` (asking them for more where
    /// `active`), or no longer; report each one as `Found`.
    Scan {
        on: bool,
        power: ScanPower,
        active: bool,
    },
    Connect {
        link: u32,
        address: String,
        power: LinkPower,
    },
    /// The link's power changed.
    LinkPower {
        link: u32,
        power: LinkPower,
    },
    Disconnect {
        link: u32,
    },
    /// Find the services (only these, where some are named) and their
    /// characteristics; report `Discovered`.
    Discover {
        link: u32,
        services: Vec<Uuid>,
    },
    Read {
        link: u32,
        handle: u16,
    },
    Write {
        link: u32,
        handle: u16,
        bytes: Vec<u8>,
        response: bool,
    },
    Subscribe {
        link: u32,
        handle: u16,
        on: bool,
    },
    /// Pair and bond; report `Ask` where the device asks something of the
    /// reader, then `Paired` or `Failed`.
    Pair {
        link: u32,
    },
    /// The reader's answer to the pairing's question.
    Answer {
        link: u32,
        accept: bool,
    },
    /// The device's HID reports: report `HidMap` once, then each
    /// `HidReport`.
    Hid {
        link: u32,
        on: bool,
    },
    /// The device's own GATT server: its services and their handles.
    Server(Vec<ServedService>),
    Advertise(Advert),
    Notify {
        handle: u16,
        bytes: Vec<u8>,
    },
    Respond {
        request: u32,
        bytes: Vec<u8>,
    },
    Refuse {
        request: u32,
        error: u8,
    },
    /// Forget the bond with the device at `address`.
    Forget {
        address: String,
    },
    /// Watch for the bonded devices at `addresses` (empty for none):
    /// passively and seldom, while no other scan runs, reporting one that
    /// advertises as `Found`; the kernel connects it then.
    Watch {
        addresses: Vec<String>,
    },
}

/// What is advertised, for everyone asking; `on` false for nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Advert {
    pub on: bool,
    pub name: Option<String>,
    pub services: Vec<Uuid>,
    pub manufacturer_data: Vec<ManufacturerData>,
    pub service_data: Vec<ServiceData>,
    pub appearance: Option<u16>,
    pub connectable: bool,
    pub power: AdvertisePower,
}

/// A bond: the device's address and the keys the shell's host keeps for
/// it, as the host wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bond {
    pub address: String,
    pub keys: String,
}

/// A characteristic of the device's own GATT server, with the handle the
/// kernel gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedCharacteristic {
    pub uuid: Uuid,
    pub properties: Properties,
    pub access: Access,
    pub max_len: u16,
    pub handle: u16,
}

/// A service of the device's own GATT server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedService {
    pub uuid: Uuid,
    pub characteristics: Vec<ServedCharacteristic>,
}

/// What a pairing asks of the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairQuestion {
    /// Only whether to pair (Just Works).
    Confirm,
    /// Whether both show this number (numeric comparison).
    Compare(u32),
    /// The number to type on the other device (passkey display).
    Show(u32),
}

/// A device a scan found, as the shell saw it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Found {
    pub address: String,
    pub name: Option<String>,
    pub signal: Option<i8>,
    pub services: Vec<Uuid>,
    pub manufacturer_data: Vec<ManufacturerData>,
    pub service_data: Vec<ServiceData>,
    pub tx_power: Option<i8>,
    pub appearance: Option<u16>,
}

/// What the shell reports ([`Kernel::bluetooth_event`](crate::Kernel)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtReport {
    Radio {
        on: bool,
    },
    Found(Found),
    Connected {
        link: u32,
        mtu: u16,
        security: Security,
    },
    Disconnected {
        link: u32,
        reason: Option<BtError>,
    },
    Discovered {
        link: u32,
        services: Vec<GattService>,
    },
    ServicesChanged {
        link: u32,
    },
    Value {
        link: u32,
        handle: u16,
        kind: ValueKind,
        bytes: Vec<u8>,
    },
    Written {
        link: u32,
        handle: u16,
    },
    /// A write without response went out.
    Sent {
        link: u32,
        handle: u16,
    },
    /// What was asked on the link failed.
    Failed {
        link: u32,
        op: BtOp,
        handle: Option<u16>,
        error: BtError,
    },
    Ask {
        link: u32,
        question: PairQuestion,
    },
    Paired {
        link: u32,
        keys: String,
    },
    HidMap {
        link: u32,
        map: Vec<u8>,
    },
    HidReport {
        link: u32,
        report: Option<u8>,
        bytes: Vec<u8>,
    },
    ReadRequest {
        request: u32,
        handle: u16,
        central: String,
    },
    WriteRequest {
        request: Option<u32>,
        handle: u16,
        central: String,
        bytes: Vec<u8>,
    },
    Subscribed {
        handle: u16,
        central: String,
        on: bool,
    },
    Advertising {
        on: bool,
    },
}

/// A device the kernel knows: found this start, or bonded.
#[derive(Debug, Clone)]
struct Known {
    id: DeviceId,
    address: String,
    found: Found,
    kind: DeviceKind,
    /// The plugins whose scans found it.
    found_by: Vec<String>,
    /// The bond's keys, where it is bonded.
    bond: Option<String>,
    given: Vec<Id>,
    /// Its keys go to the key maps.
    keys: bool,
    /// A plugin wants its keys now (not kept): it is connected again
    /// when it drops.
    wanted: bool,
}

impl Known {
    fn name(&self) -> Option<&str> {
        self.found.name.as_deref()
    }

    /// The device as `plugin` may see it.
    fn device(&self, plugin: &str) -> Device {
        let mut device = Device::new(self.id);
        device.name.clone_from(&self.found.name);
        device.signal = self.found.signal;
        device.services.clone_from(&self.found.services);
        device
            .manufacturer_data
            .clone_from(&self.found.manufacturer_data);
        device.service_data.clone_from(&self.found.service_data);
        device.tx_power = self.found.tx_power;
        device.appearance = self.found.appearance;
        device.bonded = self.bond.is_some() && self.given_to(plugin);
        device.kind = self.kind;
        device
    }

    fn given_to(&self, plugin: &str) -> bool {
        self.given.iter().any(|g| g.as_str() == plugin)
    }

    /// Whether `plugin` may use it: found by its scan (and nobody's bond),
    /// or given to it.
    fn mine(&self, plugin: &str) -> bool {
        self.given_to(plugin) || (self.bond.is_none() && self.found_by.iter().any(|p| p == plugin))
    }

    /// The text kept as its bond's secret.
    fn written(&self) -> String {
        let given: Vec<&str> = self.given.iter().map(Id::as_str).collect();
        alloc::format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.address,
            self.name().unwrap_or_default().replace('\t', " "),
            self.kind.as_str(),
            u8::from(self.keys),
            given.join(","),
            self.bond.as_deref().unwrap_or_default()
        )
    }

    fn read(number: u32, text: &str) -> Option<Known> {
        let mut parts = text.split('\t');
        let address = parts.next()?.to_string();
        let name = Some(parts.next()?.to_string()).filter(|n| !n.is_empty());
        let kind = DeviceKind::parse(parts.next()?);
        let keys = parts.next()? == "1";
        let given = parts
            .next()?
            .split(',')
            .filter_map(|g| Id::parse(g).ok())
            .collect();
        let bond = parts.next().map(String::from);
        Some(Known {
            id: DeviceId::new(number),
            address,
            found: Found {
                name,
                ..Found::default()
            },
            kind,
            found_by: Vec::new(),
            bond,
            given,
            keys,
            wanted: false,
        })
    }
}

/// Who asked for something, where it goes, and while how many screens.
#[derive(Debug, Clone)]
struct Asker {
    plugin: String,
    events: Id,
    screens: usize,
}

#[derive(Debug)]
struct ScanRequest {
    asker: Asker,
    scan: Scan,
    until: Instant,
}

#[derive(Debug)]
struct AdvertRequest {
    asker: Asker,
    advertising: Advertising,
    until: Instant,
}

/// A plugin using a link.
#[derive(Debug, Clone)]
struct User {
    asker: Asker,
    power: LinkPower,
    /// Let go when nothing went over the link this long.
    idle: Option<u64>,
}

#[derive(Debug)]
struct Link {
    number: u32,
    device: DeviceId,
    up: bool,
    users: Vec<User>,
    /// The kernel's own, for the device's keys: when the last key came.
    keys: Option<Instant>,
    decoder: Option<Decoder>,
    /// Plugins hearing its HID reports as they are.
    raw: Vec<Asker>,
    /// Who waits for what: a read of a handle (`Some`), a discovery
    /// (`None`).
    reads: Vec<(Option<u16>, Id)>,
    writes: Vec<(u16, Id)>,
    /// Who subscribed to which handle.
    subscribed: Vec<(u16, Id)>,
    /// Subscriptions by UUID waiting for the services: service,
    /// characteristic, events.
    pending: Vec<(Uuid, Uuid, Id)>,
    /// The services found last.
    services: Vec<GattService>,
    /// Pairing asked by this plugin's events.
    pairing: Option<Id>,
    /// The power the shell was told.
    power: LinkPower,
    /// When something last went over it.
    active: Instant,
    /// What the shell said of it once up.
    mtu: u16,
    security: Security,
}

impl Link {
    /// The events of `plugin`'s use of it.
    fn events_of(&self, plugin: &str) -> Option<Id> {
        self.users
            .iter()
            .find(|u| u.asker.plugin == plugin)
            .map(|u| u.asker.events.clone())
    }
}

/// A characteristic of the device's own GATT server: its handle, service,
/// UUID and the plugin's events.
#[derive(Debug, Clone)]
struct Served {
    handle: u16,
    service: Uuid,
    characteristic: Uuid,
    events: Id,
}

/// Bluetooth, as the kernel keeps it.
#[derive(Debug, Default)]
pub(crate) struct Radio {
    max_links: u8,
    known: Vec<Known>,
    scans: Vec<ScanRequest>,
    adverts: Vec<AdvertRequest>,
    links: Vec<Link>,
    served: Vec<Served>,
    services: Vec<ServedService>,
    /// Reads and writes of the server waiting for their answer: request,
    /// events.
    requests: Vec<(u32, Id)>,
    radio_on: bool,
    /// The scan the shell was told: power, active.
    scanning: Option<(ScanPower, bool)>,
    advertised: Option<Advert>,
    /// Since when nothing uses the radio.
    idle_since: Option<Instant>,
    /// Since when the radio is on, for the time it is on.
    on_since: Option<Instant>,
    /// The time it was on, not yet counted.
    on_ms: u64,
    /// When each plugin's last scan ended.
    scanned: Vec<(String, Instant)>,
    next_device: u32,
    next_link: u32,
    calls: Vec<BtCall>,
    /// Keys of remotes to give the key maps: name, pressed.
    keys_out: Vec<(&'static str, bool)>,
    /// The bonds changed and are to be kept.
    bonds_changed: bool,
    /// A pairing asks the reader: device, question.
    asking: Option<(DeviceId, PairQuestion)>,
    /// A pairing ended: the device, and whether its link was lost.
    pairing_ended: Option<(DeviceId, bool)>,
    /// The bonded devices the shell watches for.
    watching: Vec<String>,
    /// The radio went off by itself: it stays off until then.
    rest_until: Option<Instant>,
    /// The Bluetooth manager's plugin.
    manager: Option<String>,
    /// Held off to make room (ADR 0030): off where no link is up, until
    /// let go.
    held_off: bool,
}

/// The command that tells `events` of `event`.
fn tell(events: &Id, event: BtEvent) -> Command {
    Command::Emit(events.clone(), event.into_params())
}

/// The command that tells `events` that a command failed.
fn fail(events: &Id, failure: BtFailure) -> Command {
    tell(events, BtEvent::Failed(failure))
}

/// The device kind from a GAP appearance and the services.
fn kind_of(appearance: Option<u16>, services: &[Uuid]) -> DeviceKind {
    match appearance.map(|a| a >> 6) {
        Some(0x0F) => match appearance.map(|a| a & 0x3f) {
            Some(1) => DeviceKind::Keyboard,
            _ => DeviceKind::Remote,
        },
        Some(1 | 2) => DeviceKind::Phone,
        Some(0x0D | 0x0E | 0x31 | 0x03) => DeviceKind::Sensor,
        _ if services.contains(&Uuid::sig(0x1812)) => DeviceKind::Remote,
        _ if services.contains(&Uuid::sig(0x180D)) => DeviceKind::Sensor,
        _ => DeviceKind::Other,
    }
}

impl Radio {
    /// A radio keeping up to `max_links` links.
    pub fn new(max_links: u8) -> Self {
        Radio {
            max_links,
            next_device: 1,
            next_link: 1,
            ..Radio::default()
        }
    }

    /// The bonds kept in `secrets` (id, value), as read at start.
    pub fn load<'s>(&mut self, secrets: impl Iterator<Item = (&'s str, &'s str)>) {
        self.known.retain(|k| k.bond.is_none());
        for (id, value) in secrets {
            let Some(number) = id.strip_prefix(BOND_PREFIX).and_then(|n| n.parse().ok()) else {
                continue;
            };
            if let Some(known) = Known::read(number, value) {
                self.next_device = self.next_device.max(number + 1);
                self.known.push(known);
            }
        }
    }

    /// Holds the radio off (to make room), where no link is up; or lets
    /// it come back as what is asked needs it. Whether it is off now.
    pub fn hold_off(&mut self, off: bool, now: Instant) -> bool {
        self.held_off = off;
        self.settle(now);
        !self.radio_on
    }

    /// Whether the radio is on.
    pub fn is_on(&self) -> bool {
        self.radio_on
    }

    /// The Bluetooth manager: the plugin whose screen the slot shows.
    pub fn set_manager(&mut self, manager: Option<String>) {
        self.manager = manager;
    }

    fn manager_is(&self, plugin: &str) -> bool {
        self.manager.as_deref() == Some(plugin)
    }

    /// A service of the device's own GATT server, from a plugin's
    /// registration.
    pub fn add_service(&mut self, spec: &ServiceSpec) {
        let first = self.served.last().map_or(1, |s| s.handle + 1);
        let mut characteristics = Vec::new();
        for (handle, c) in (first..).zip(&spec.characteristics) {
            characteristics.push(ServedCharacteristic {
                uuid: c.uuid,
                properties: c.properties,
                access: c.access,
                max_len: c.max_len,
                handle,
            });
            self.served.push(Served {
                handle,
                service: spec.uuid,
                characteristic: c.uuid,
                events: spec.events.clone(),
            });
        }
        self.services.push(ServedService {
            uuid: spec.uuid,
            characteristics,
        });
    }

    /// The bonds and the GATT server for the shell, at start.
    pub fn start(&mut self) {
        self.calls.push(BtCall::Bonds(self.bonds()));
        if !self.services.is_empty() {
            self.calls.push(BtCall::Server(self.services.clone()));
        }
    }

    fn bonds(&self) -> Vec<Bond> {
        self.known
            .iter()
            .filter_map(|k| {
                Some(Bond {
                    address: k.address.clone(),
                    keys: k.bond.clone()?,
                })
            })
            .collect()
    }

    pub fn take(&mut self) -> Vec<BtCall> {
        core::mem::take(&mut self.calls)
    }

    /// The keys of remotes that came, for the key maps.
    pub fn take_keys(&mut self) -> Vec<(&'static str, bool)> {
        core::mem::take(&mut self.keys_out)
    }

    /// The time the radio was on since this was last asked.
    pub fn take_on_ms(&mut self, now: Instant) -> u64 {
        if let Some(since) = self.on_since {
            self.on_ms = self.on_ms.saturating_add(now.since(since));
            self.on_since = Some(now);
        }
        core::mem::take(&mut self.on_ms)
    }

    /// The bonds as secrets to keep (id, label, value; an empty value to
    /// remove), where they changed.
    pub fn take_bonds(&mut self, all: &[(String, String)]) -> Vec<(String, String, String)> {
        if !core::mem::take(&mut self.bonds_changed) {
            return Vec::new();
        }
        let mut out: Vec<(String, String, String)> = self
            .known
            .iter()
            .filter(|k| k.bond.is_some())
            .map(|k| {
                let id = alloc::format!("{BOND_PREFIX}{}", k.id.number());
                let label = k.name().map_or_else(|| k.address.clone(), String::from);
                (id, label, k.written())
            })
            .collect();
        for (id, _) in all {
            if id.starts_with(BOND_PREFIX) && !out.iter().any(|(o, _, _)| o == id) {
                out.push((id.clone(), String::new(), String::new()));
            }
        }
        out
    }

    /// The pairing question waiting for the reader, once.
    pub fn take_question(&mut self) -> Option<(DeviceId, PairQuestion, String)> {
        let (device, question) = self.asking.take()?;
        let name = self
            .known
            .iter()
            .find(|k| k.id == device)
            .and_then(|k| k.name().map(String::from))
            .unwrap_or_default();
        Some((device, question, name))
    }

    /// What every plugin's view is made of.
    pub fn state(&self) -> Bluetooth {
        let mut state = Bluetooth::new(self.max_links);
        state.state = if self.radio_on {
            RadioState::On
        } else {
            RadioState::Off
        };
        state.busy = self.wanted();
        state.set_manager(self.manager.as_deref().and_then(|m| Id::parse(m).ok()));
        state.set_bonded(
            self.known
                .iter()
                .filter(|k| k.bond.is_some())
                .map(|k| {
                    let name = k.name().map_or_else(|| k.address.clone(), String::from);
                    let mut bonded = Bonded::new(k.id, name, k.kind).given_to(k.given.clone());
                    bonded.connected = self.links.iter().any(|l| l.device == k.id && l.up);
                    bonded.keys = k.keys;
                    bonded
                })
                .collect(),
        );
        state
    }

    /// Something needs the radio.
    fn wanted(&self) -> bool {
        !self.scans.is_empty()
            || !self.adverts.is_empty()
            || !self.links.is_empty()
            || !self.awaited().is_empty()
    }

    /// The addresses of the bonded devices whose keys are wanted and that
    /// have no link: watched for, to connect them again.
    fn awaited(&self) -> Vec<String> {
        self.known
            .iter()
            .filter(|k| k.wanted && k.bond.is_some())
            .filter(|k| !self.links.iter().any(|l| l.device == k.id))
            .map(|k| k.address.clone())
            .collect()
    }

    /// When something it waits for is due: a scan's or advertising's end,
    /// a link idle long enough, the radio's going off.
    pub fn deadline(&self) -> Option<Instant> {
        let ends = self.scans.iter().map(|s| s.until);
        let adverts = self.adverts.iter().map(|a| a.until);
        let idle = self.links.iter().filter_map(|l| {
            let users = l
                .users
                .iter()
                .filter_map(|u| u.idle)
                .min()
                .map(|ms| l.active.add_millis(ms));
            let keys = l
                .keys
                .filter(|_| l.power > LinkPower::Low)
                .map(|last| last.add_millis(KEYS_IDLE_MS));
            users.into_iter().chain(keys).min()
        });
        let off = self
            .idle_since
            .filter(|_| self.radio_on)
            .map(|since| since.add_millis(RADIO_GRACE_MS));
        let rested = self.rest_until.filter(|_| !self.radio_on && self.wanted());
        ends.chain(adverts)
            .chain(idle)
            .chain(off)
            .chain(rested)
            .min()
    }

    /// Time passed: scans and advertising ended, idle links let go or
    /// slowed, the radio off.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<Command>) {
        let (ended, kept): (Vec<_>, Vec<_>) = self.scans.drain(..).partition(|s| s.until <= now);
        self.scans = kept;
        for scan in ended {
            self.scan_ended(&scan.asker, now, out);
        }
        let (ended, kept): (Vec<_>, Vec<_>) = self.adverts.drain(..).partition(|a| a.until <= now);
        self.adverts = kept;
        for advert in ended {
            out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
        }
        self.idle_links(now, out);
        self.settle(now);
    }

    fn scan_ended(&mut self, asker: &Asker, now: Instant, out: &mut Vec<Command>) {
        out.push(tell(&asker.events, BtEvent::ScanEnded));
        self.scanned.retain(|(p, _)| *p != asker.plugin);
        self.scanned.push((asker.plugin.clone(), now));
    }

    /// Users idle longer than they allow let go; a remote's link slowed
    /// once no key came for a while.
    fn idle_links(&mut self, now: Instant, out: &mut Vec<Command>) {
        let mut at = 0;
        while at < self.links.len() {
            let link = &mut self.links[at];
            let quiet = now.since(link.active);
            let device = link.device;
            let (gone, kept): (Vec<_>, Vec<_>) = link
                .users
                .drain(..)
                .partition(|u| u.idle.is_some_and(|ms| quiet >= ms));
            link.users = kept;
            for user in gone {
                let event = BtEvent::disconnected(device, None);
                out.push(tell(&user.asker.events, event));
            }
            if self.keep_or_drop(at, now) {
                at += 1;
            }
        }
    }

    /// Link `at` let go where nothing uses it any more, else at the power
    /// it needs now; whether it is still there.
    fn keep_or_drop(&mut self, at: usize, now: Instant) -> bool {
        let link = &self.links[at];
        if link.users.is_empty() && link.raw.is_empty() && link.keys.is_none() {
            self.calls.push(BtCall::Disconnect { link: link.number });
            self.links.remove(at);
            return false;
        }
        self.power_link(at, now);
        true
    }

    /// The power link `at` needs now: the highest its users ask, a remote
    /// responsive while keys come; the shell told where it changed.
    fn power_link(&mut self, at: usize, now: Instant) {
        let link = &mut self.links[at];
        let keys = link
            .keys
            .filter(|last| now.since(*last) < KEYS_IDLE_MS)
            .map(|_| LinkPower::Responsive);
        let power = link
            .users
            .iter()
            .map(|u| u.power)
            .chain(keys)
            .max()
            .unwrap_or(LinkPower::Low);
        if power != link.power {
            link.power = power;
            if link.up {
                self.calls.push(BtCall::LinkPower {
                    link: link.number,
                    power,
                });
            }
        }
    }

    /// The radio, the scan and advertising as what is asked needs them, at
    /// the cheapest power that serves everyone.
    fn settle(&mut self, now: Instant) {
        if self.held_off {
            if self.radio_on && self.links.is_empty() {
                self.radio_off(now);
            }
            return;
        }
        let resting = !self.radio_on && self.rest_until.is_some_and(|until| now < until);
        if self.wanted() && resting {
            return;
        }
        if self.wanted() {
            self.rest_until = None;
            self.power_up(now);
        } else if self.radio_on {
            let since = *self.idle_since.get_or_insert(now);
            if now.since(since) >= RADIO_GRACE_MS {
                self.radio_off(now);
                return;
            }
        }
        if !self.radio_on {
            return;
        }
        let scan = (!self.scans.is_empty()).then(|| {
            let power = self.scans.iter().map(|s| s.scan.power).max();
            let active = self.scans.iter().any(|s| s.scan.active);
            (power.unwrap_or_default(), active)
        });
        if scan != self.scanning {
            self.scanning = scan;
            let (power, active) = scan.unwrap_or_default();
            self.calls.push(BtCall::Scan {
                on: scan.is_some(),
                power,
                active,
            });
        }
        let advert = self.advertising();
        if advert != self.advertised {
            self.advertised.clone_from(&advert);
            self.calls
                .push(BtCall::Advertise(advert.unwrap_or_default()));
        }
        let awaited = self.awaited();
        if awaited != self.watching {
            self.watching.clone_from(&awaited);
            self.calls.push(BtCall::Watch { addresses: awaited });
        }
    }

    /// The radio on, where it is not: before any call that needs it, so
    /// that the shell never hears of a link before the radio.
    fn power_up(&mut self, now: Instant) {
        self.idle_since = None;
        if !self.radio_on {
            self.radio_on = true;
            self.on_since = Some(now);
            self.calls.push(BtCall::Radio { on: true });
        }
    }

    /// What is advertised for everyone asking: the first name, every
    /// service and datum, the highest power asked.
    fn advertising(&self) -> Option<Advert> {
        if self.adverts.is_empty() {
            return None;
        }
        let all = || self.adverts.iter().map(|a| &a.advertising);
        let mut services: Vec<Uuid> = Vec::new();
        for s in all().flat_map(|a| a.services.iter()) {
            if !services.contains(s) {
                services.push(*s);
            }
        }
        Some(Advert {
            on: true,
            name: all().find_map(|a| a.name.as_deref().map(String::from)),
            services,
            manufacturer_data: all().flat_map(|a| a.manufacturer_data.clone()).collect(),
            service_data: all().flat_map(|a| a.service_data.clone()).collect(),
            appearance: all().find_map(|a| a.appearance),
            connectable: all().any(|a| a.connectable),
            power: all().map(|a| a.power).max().unwrap_or_default(),
        })
    }

    fn radio_off(&mut self, now: Instant) {
        self.radio_on = false;
        self.scanning = None;
        self.advertised = None;
        self.idle_since = None;
        if let Some(since) = self.on_since.take() {
            self.on_ms = self.on_ms.saturating_add(now.since(since));
        }
        self.calls.push(BtCall::Radio { on: false });
        self.watching.clear();
        self.forget_found();
    }

    /// What scans found leaves with the radio, so that Bluetooth off
    /// keeps (almost) nothing: of a device nobody bonded only its
    /// number, address and name stay (a plugin may still connect to it),
    /// and only the newest [`FOUND_KEPT`] of them.
    fn forget_found(&mut self) {
        for known in self.known.iter_mut().filter(|k| k.bond.is_none()) {
            let name = known.found.name.take();
            known.found = Found {
                name,
                ..Found::default()
            };
            known.found.services.shrink_to_fit();
        }
        let linked = |links: &[Link], id: DeviceId| links.iter().any(|l| l.device == id);
        let mut unbonded = self.known.iter().filter(|k| k.bond.is_none()).count();
        let mut at = 0;
        while unbonded > FOUND_KEPT && at < self.known.len() {
            let known = &self.known[at];
            if known.bond.is_none() && !linked(&self.links, known.id) {
                self.known.remove(at);
                unbonded -= 1;
            } else {
                at += 1;
            }
        }
        self.known.shrink_to_fit();
    }

    fn known_mut(&mut self, device: DeviceId) -> Option<&mut Known> {
        self.known.iter_mut().find(|k| k.id == device)
    }

    /// The link to `device`, made where there is none (and a link is
    /// left).
    fn link_to(&mut self, device: DeviceId, now: Instant) -> Result<usize, BtError> {
        if let Some(at) = self.links.iter().position(|l| l.device == device) {
            return Ok(at);
        }
        if self.links.len() >= usize::from(self.max_links) {
            return Err(BtError::Busy);
        }
        let address = self
            .known
            .iter()
            .find(|k| k.id == device)
            .map(|k| k.address.clone())
            .ok_or(BtError::NotFound)?;
        self.power_up(now);
        let number = self.next_link;
        self.next_link += 1;
        self.links.push(Link {
            number,
            device,
            up: false,
            users: Vec::new(),
            keys: None,
            decoder: None,
            raw: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            subscribed: Vec::new(),
            pending: Vec::new(),
            services: Vec::new(),
            pairing: None,
            power: LinkPower::Low,
            active: now,
            mtu: 23,
            security: Security::None,
        });
        self.calls.push(BtCall::Connect {
            link: number,
            address,
            power: LinkPower::Low,
        });
        Ok(self.links.len() - 1)
    }

    /// The keys still down on link `at`, released.
    fn release_keys(&mut self, at: usize) {
        if let Some(decoder) = &mut self.links[at].decoder {
            for change in decoder.reset() {
                self.keys_out.push((change.key.name(), false));
            }
        }
    }

    /// Fewer screens are open than `screens`: what was asked while more
    /// were ends.
    pub fn close_for_closed_screens(
        &mut self,
        screens: usize,
        now: Instant,
        out: &mut Vec<Command>,
    ) {
        let (ended, kept): (Vec<_>, Vec<_>) = self
            .scans
            .drain(..)
            .partition(|s| s.asker.screens > screens);
        self.scans = kept;
        for scan in ended {
            self.scan_ended(&scan.asker, now, out);
        }
        let (ended, kept): (Vec<_>, Vec<_>) = self
            .adverts
            .drain(..)
            .partition(|a| a.asker.screens > screens);
        self.adverts = kept;
        for advert in ended {
            out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
        }
        let mut at = 0;
        while at < self.links.len() {
            let device = self.links[at].device;
            let link = &mut self.links[at];
            let (gone, kept): (Vec<_>, Vec<_>) = link
                .users
                .drain(..)
                .partition(|u| u.asker.screens > screens);
            link.users = kept;
            link.raw.retain(|r| r.screens <= screens);
            for user in gone {
                let event = BtEvent::disconnected(device, None);
                out.push(tell(&user.asker.events, event));
            }
            if self.keep_or_drop(at, now) {
                at += 1;
            }
        }
        self.settle(now);
    }

    /// The device goes to sleep: everything ends, the radio off.
    pub fn suspend(&mut self, now: Instant, out: &mut Vec<Command>) {
        // Never in sleep: the plugins ask again when it wakes.
        for known in &mut self.known {
            known.wanted = false;
        }
        self.rest_until = None;
        self.end_all(now, out);
    }

    /// The radio went off by itself (the shell's host failed): what used
    /// it ends, and after a rest it comes on again for the remotes still
    /// wanted.
    fn went_off(&mut self, now: Instant, out: &mut Vec<Command>) {
        self.end_all(now, out);
        self.rest_until = Some(now.add_millis(RADIO_REST_MS));
    }

    /// Everything ends: scans, advertising, links; the radio off.
    fn end_all(&mut self, now: Instant, out: &mut Vec<Command>) {
        for scan in core::mem::take(&mut self.scans) {
            out.push(tell(&scan.asker.events, BtEvent::ScanEnded));
        }
        for advert in self.adverts.drain(..) {
            out.push(tell(&advert.asker.events, BtEvent::AdvertisingEnded));
        }
        for at in (0..self.links.len()).rev() {
            self.release_keys(at);
            let link = self.links.remove(at);
            for user in link.users {
                let event = BtEvent::disconnected(link.device, None);
                out.push(tell(&user.asker.events, event));
            }
        }
        if self.radio_on {
            self.radio_off(now);
        }
    }

    /// A pairing that ended since this was last asked: the device, whether
    /// its link was lost meanwhile, its name.
    pub fn take_pairing_ended(&mut self) -> Option<(DeviceId, bool, String)> {
        let (device, lost) = self.pairing_ended.take()?;
        let name = self
            .known
            .iter()
            .find(|k| k.id == device)
            .and_then(|k| k.name().map(String::from))
            .unwrap_or_default();
        Some((device, lost, name))
    }

    /// The pairing with `device` ended (`lost`: with its link): its
    /// question, asked or still to ask, is over.
    fn pairing_over(&mut self, device: DeviceId, lost: bool) {
        if self.asking.is_some_and(|(d, _)| d == device) {
            self.asking = None;
        }
        self.pairing_ended = Some((device, lost));
    }

    /// The reader answered the pairing question about `device`.
    pub fn answer(&mut self, device: DeviceId, accept: bool) {
        if let Some(link) = self.links.iter().find(|l| l.device == device) {
            self.calls.push(BtCall::Answer {
                link: link.number,
                accept,
            });
        }
    }

    fn link_at(&self, number: u32) -> Option<usize> {
        self.links.iter().position(|l| l.number == number)
    }

    /// The device numbered for `address`, made known where it is not.
    fn device_at(&mut self, address: &str) -> DeviceId {
        if let Some(known) = self.known.iter().find(|k| k.address == address) {
            return known.id;
        }
        let id = DeviceId::new(self.next_device);
        self.next_device += 1;
        self.known.push(Known {
            id,
            address: address.to_string(),
            found: Found::default(),
            kind: DeviceKind::Other,
            found_by: Vec::new(),
            bond: None,
            given: Vec::new(),
            keys: false,
            wanted: false,
        });
        id
    }
}

#[cfg(test)]
mod tests;
