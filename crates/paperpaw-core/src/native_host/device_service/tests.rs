#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::native_host::host_services::Caller;
use crate::{
    abi7::{
        context::Frame,
        services_wire::{self, ServiceOp},
    },
    native_host::{
        call_services::validate_reply,
        registrations::{self, Record},
        stage_wire,
    },
};
use alloc::{
    rc::Rc,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_platform::Instant;
use paperpaw_sdk::power::{Power, PowerLevel};

/// A whole fake device. Every part is optional, as on real devices.
#[derive(Default)]
pub(super) struct Fake {
    device: Option<Dev>,
    values: Option<Vec<(String, Option<i64>)>>,
    firmware: bool,
    builtin: Option<Vec<(String, String)>>,
    native: bool,
    memory: Option<MemoryFacts>,
    wifi: bool,
    networks: Vec<(String, i32, bool, bool)>,
    bluetooth: bool,
    bonded: Vec<(u32, String, bool, String)>,
    manager: Option<u64>,
    power: Option<Power>,
    not_installed: Option<Vec<(String, u32)>>,
    keys: Option<Vec<String>>,
    generation: u32,
}

#[derive(Clone)]
struct Dev {
    id: String,
    model: String,
    catalog: String,
    class: u32,
    caps: u64,
    start: u32,
    slept: Option<u64>,
}

impl Dev {
    fn x4(caps: u64) -> Self {
        Self {
            id: "xteink-x4".to_string(),
            model: "Xteink X4".to_string(),
            catalog: "x4".to_string(),
            class: 0,
            caps,
            start: 1,
            slept: None,
        }
    }
}

const ALL: u64 = caps::WIFI | caps::BLUETOOTH | caps::FIRMWARE | caps::NETWORK;

impl Fake {
    /// The generation the shell's lists have.
    pub(super) fn with_generation(mut self, generation: u32) -> Self {
        self.generation = generation;
        self
    }

    /// A device with every part.
    pub(super) fn full() -> Self {
        Self {
            device: Some(Dev::x4(ALL)),
            values: Some(vec![
                ("usb-frames".to_string(), Some(42)),
                ("temperature-c".to_string(), Some(-3)),
            ]),
            firmware: true,
            builtin: Some(
                (0..3)
                    .map(|n| (alloc::format!("core.plugin-{n}"), "1.0.0".to_string()))
                    .collect(),
            ),
            native: true,
            memory: Some(MemoryFacts {
                heap_free: 100,
                heap_size: 200,
                largest: Some(64),
                stack: Some((1000, 2000)),
                external: None,
                cached: false,
            }),
            wifi: true,
            networks: vec![
                ("Weak".to_string(), -80, true, false),
                ("Strong".to_string(), -40, true, true),
            ],
            bluetooth: true,
            bonded: vec![(7, "Page turner".to_string(), true, "remote".to_string())],
            manager: Some(5),
            power: Some(Power::normal().clone()),
            not_installed: Some(vec![("other.author".to_string(), 1)]),
            keys: Some(vec!["back".to_string(), "confirm".to_string()]),
            generation: 3,
        }
    }
    /// A device with only its identity, as Android has no Wi-Fi, Bluetooth
    /// or firmware of its own.
    fn bare() -> Self {
        Self {
            device: Some(Dev::x4(0)),
            ..Self::default()
        }
    }
}

fn walk<T>(
    rows: impl Iterator<Item = T>,
    mut visit: impl FnMut(T) -> ControlFlow<()>,
) -> Result<(), i32> {
    for row in rows {
        if visit(row).is_break() {
            break;
        }
    }
    Ok(())
}

impl DeviceHost for Fake {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        let d = self.device.as_ref().ok_or(ERR_NOT_FOUND)?;
        visit(&DeviceFacts {
            id: &d.id,
            model: &d.model,
            catalog: &d.catalog,
            class: d.class,
            caps: d.caps,
            controls: 0xffff,
            tls_sessions: 1,
            start: d.start,
            slept_ms: d.slept,
        });
        Ok(())
    }
    fn values(&self, visit: &mut dyn FnMut(&Reading<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        let values = self.values.as_ref().ok_or(ERR_NOT_FOUND)?;
        walk(values.iter(), |(name, value)| {
            visit(&Reading {
                name,
                value: *value,
            })
        })
    }
    fn firmware(&self, visit: &mut dyn FnMut(&FirmwareFacts<'_>)) -> Result<(), i32> {
        if !self.firmware {
            return Err(ERR_NOT_FOUND);
        }
        visit(&FirmwareFacts {
            version: "1.2.3",
            project: "paperpaw",
            chip: 5,
            room: 1 << 33,
            undone: true,
            state: 9,
            done: 10,
            total: 20,
            reason: 0,
            request: 4,
        });
        Ok(())
    }
    fn builtin(
        &self,
        visit: &mut dyn FnMut(&BuiltinRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let rows = self.builtin.as_ref().ok_or(ERR_NOT_FOUND)?;
        walk(rows.iter(), |(id, version)| {
            visit(&BuiltinRow { id, version })
        })
    }
    fn native(&self, visit: &mut dyn FnMut(&NativeFacts<'_>)) -> Result<(), i32> {
        if !self.native {
            return Err(ERR_NOT_FOUND);
        }
        visit(&NativeFacts {
            abi_major: 7,
            abi_draft: 19,
            target: 5,
            code_file: "code.ppn",
            safe_mode: 7,
            crashed: "bad.plugin",
            last_loaded: "",
            pending: 1,
            not_installed: 1,
            room_free: 5 << 20,
            room_largest: 1 << 20,
        });
        Ok(())
    }
    fn memory(&self) -> Result<MemoryFacts, i32> {
        self.memory.ok_or(ERR_NOT_FOUND)
    }
    fn wifi(&self, visit: &mut dyn FnMut(&WifiFacts<'_>)) -> Result<(), i32> {
        if !self.wifi {
            return Err(ERR_NOT_FOUND);
        }
        visit(&WifiFacts {
            state: 3,
            ssid: "Home",
            address: "192.168.1.2",
            host_name: "paperpaw",
            guests: 0,
            scanning: true,
            wanted: false,
            busy: true,
        });
        Ok(())
    }
    fn networks(
        &self,
        visit: &mut dyn FnMut(&NetworkRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        if !self.wifi {
            return Err(ERR_NOT_FOUND);
        }
        walk(self.networks.iter(), |(ssid, signal, secured, known)| {
            visit(&NetworkRow {
                ssid,
                signal: *signal,
                secured: *secured,
                known: *known,
            })
        })
    }
    fn bluetooth(&self) -> Result<BluetoothFacts, i32> {
        if self.bluetooth {
            Ok(BluetoothFacts {
                radio: 1,
                max_links: 2,
            })
        } else {
            Err(ERR_NOT_FOUND)
        }
    }
    fn bonded(&self, visit: &mut dyn FnMut(&BondedRow<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        if !self.bluetooth {
            return Err(ERR_NOT_FOUND);
        }
        walk(self.bonded.iter(), |(device, name, connected, given)| {
            visit(&BondedRow {
                device: *device,
                name,
                kind: 2,
                connected: *connected,
                remote_keys: true,
                given,
            })
        })
    }
    fn bluetooth_manager(&self, caller: u64) -> bool {
        self.manager == Some(caller)
    }
    fn power(&self, visit: &mut dyn FnMut(&Power)) -> Result<(), i32> {
        visit(self.power.as_ref().ok_or(ERR_NOT_FOUND)?);
        Ok(())
    }
    fn not_installed(
        &self,
        visit: &mut dyn FnMut(&NotInstalledRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let rows = self.not_installed.as_ref().ok_or(ERR_NOT_FOUND)?;
        walk(rows.iter(), |(plugin, reason)| {
            visit(&NotInstalledRow {
                plugin,
                reason: *reason,
            })
        })
    }
    fn keys(&self, visit: &mut dyn FnMut(&str) -> ControlFlow<()>) -> Result<(), i32> {
        let keys = self.keys.as_ref().ok_or(ERR_NOT_FOUND)?;
        walk(keys.iter(), |name| visit(name))
    }
    fn generation(&self) -> u32 {
        self.generation
    }
}

/// A frame, and the host views the backend reaches through its caller.
struct View<'a> {
    env: &'a paperpaw_sdk::Env<'a>,
    hosts: crate::native_host::host_services::HostServices,
    owner: u64,
}

impl Frame for View<'_> {
    fn settings(&self) -> &paperpaw_sdk::Settings {
        self.env.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.env.i18n()
    }
}

impl View<'_> {
    fn backend(&self, op: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let caller = Caller {
            owner: self.owner,
            plugin: "reader",
            features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        backend(self, &caller, op, input, out)
    }
}

/// The caller `None` is owner 0, which is nobody's Bluetooth manager.
fn with_view<R>(host: Fake, caller: Option<u64>, run: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_device(Rc::new(host)).unwrap();
    run(&View {
        env: &env,
        hosts,
        owner: caller.unwrap_or(0),
    })
}

pub(super) fn schema_op(id: u32) -> &'static ServiceOp {
    services_wire::service_schema(KEY, 1)
        .unwrap()
        .op(id)
        .unwrap()
}

pub(super) fn batch(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = [0; 512];
    let length = registrations::records_encode(records, &mut out).unwrap();
    out[..length].to_vec()
}

pub(super) fn query(cursor: u32, generation: u32) -> Vec<u8> {
    batch(&[Record::ServiceRegQuery(registrations::ServiceRegQuery {
        scope: "",
        cursor,
        generation,
        flags: 0,
    })])
}

pub(super) fn interval(base_ms: u32) -> Vec<u8> {
    batch(&[Record::ServiceDevInterval(
        registrations::ServiceDevInterval { base_ms },
    )])
}

/// The valid input of an op.
pub(super) fn input_of(id: u32) -> Vec<u8> {
    match id {
        4 | 9 | 11 | 14 => query(0, 0),
        12 => interval(1000),
        6 | 13 => batch(&[Record::ServiceDevRequest(
            registrations::ServiceDevRequest { request: 1 },
        )]),
        _ => vec![0],
    }
}

/// Run an op the way the dispatcher does: the backend writes a bare record
/// stream into a scratch of `max_reply` bytes and the generic validator checks
/// it. Returns the reply bytes.
fn run(view: &View<'_>, id: u32, input: &[u8]) -> Result<Vec<u8>, i32> {
    let op = schema_op(id);
    let mut scratch = vec![0_u8; op.max_reply];
    let length = view.backend(op, input, &mut scratch)?;
    let reply = scratch[..length].to_vec();
    assert_eq!(validate_reply(&reply, op), Ok(()), "op {id} reply");
    Ok(reply)
}

pub(super) fn records(reply: &[u8]) -> Vec<Record<'_>> {
    let mut frames = Vec::new();
    stage_wire::visit_stream(reply, |_, framed| {
        let start = framed.as_ptr() as usize - reply.as_ptr() as usize;
        frames.push((start, framed.len()));
        Ok(())
    })
    .unwrap();
    frames
        .into_iter()
        .map(|(start, len)| registrations::record_decode_one(&reply[start..start + len]).unwrap())
        .collect()
}

fn call(host: Fake, id: u32, input: &[u8]) -> Result<Vec<u8>, i32> {
    with_view(host, Some(5), |view| run(view, id, input))
}

fn end_of(rows: &[Record<'_>]) -> registrations::ServiceEnd {
    match rows.last() {
        Some(Record::ServiceEnd(end)) => *end,
        other => panic!("no end record: {other:?}"),
    }
}

#[test]
fn every_op_decodes_validates_and_carries_the_hosts_facts() {
    let host = Fake::full();
    with_view(host, Some(5), |view| {
        let reply = run(view, 1, &[0]).unwrap();
        let [Record::ServiceDevDevice(d)] = records(&reply)[..] else {
            panic!("op 1")
        };
        assert_eq!((d.id, d.model, d.catalog), ("xteink-x4", "Xteink X4", "x4"));
        assert_eq!((d.class, d.start, d.tls_sessions), (0, 1, 1));
        assert_eq!(d.caps, ALL);
        // `controls` is cut to the defined bits, `flags` bit 0 only with a sleep.
        assert_eq!((d.controls, d.flags, d.slept_ms), (0x1f, 0, 0));

        let reply = run(view, 3, &[0]).unwrap();
        let [Record::ServiceDevFirmware(f)] = records(&reply)[..] else {
            panic!("op 3")
        };
        assert_eq!(
            (f.version, f.project, f.chip, f.room),
            ("1.2.3", "paperpaw", 5, 1 << 33)
        );
        // An unknown state reads as failed.
        assert_eq!(
            (f.undone, f.state, f.done, f.total, f.request),
            (1, 4, 10, 20, 4)
        );

        let reply = run(view, 5, &[0]).unwrap();
        let [Record::ServiceDevNative(n)] = records(&reply)[..] else {
            panic!("op 5")
        };
        assert_eq!(
            (n.abi_major, n.abi_draft, n.target, n.code_file),
            (7, 19, 5, "code.ppn")
        );
        // An unknown safe mode reads as 1.
        assert_eq!(
            (n.safe_mode, n.crashed, n.last_loaded),
            (1, "bad.plugin", "")
        );
        assert_eq!(
            (n.pending, n.not_installed, n.room_free, n.room_largest),
            (1, 1, 5 << 20, 1 << 20)
        );

        let reply = run(view, 8, &[0]).unwrap();
        let [Record::ServiceDevWifi(w)] = records(&reply)[..] else {
            panic!("op 8")
        };
        assert_eq!(
            (w.state, w.ssid, w.address, w.host_name),
            (3, "Home", "192.168.1.2", "paperpaw")
        );
        assert_eq!((w.guests, w.flags), (0, 0b101));

        let reply = run(view, 10, &[0]).unwrap();
        assert_eq!(
            records(&reply)[..],
            [Record::ServiceDevBluetooth(
                registrations::ServiceDevBluetooth {
                    radio: 1,
                    max_links: 2,
                    flags: 0,
                }
            )]
        );

        let reply = run(view, 14, &query(0, 0)).unwrap();
        let rows = records(&reply);
        assert_eq!(
            rows[0],
            Record::ServiceDevNotInstalled(registrations::ServiceDevNotInstalled {
                plugin: "other.author",
                reason: 1,
            })
        );
        assert_eq!((end_of(&rows).total, end_of(&rows).flags), (1, 1));

        let reply = run(view, 4, &query(0, 0)).unwrap();
        assert_eq!(records(&reply).len(), 4);
        let reply = run(view, 15, &[0]).unwrap();
        let rows = records(&reply);
        assert_eq!(rows.len(), 3);
        assert_eq!(end_of(&rows).total, 2);
    });
}

#[test]
fn checks_and_installs_answer_unsupported_in_this_slice() {
    with_view(Fake::full(), Some(5), |view| {
        for id in [6, 13] {
            let mut scratch = [0_u8; 4096];
            assert_eq!(
                view.backend(schema_op(id), &input_of(id), &mut scratch),
                Err(ERR_UNSUPPORTED)
            );
        }
    });
}

#[test]
fn a_frame_without_the_view_is_unsupported_and_a_wrong_view_is_not_taken() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let bare = View {
        env: &env,
        hosts: crate::native_host::host_services::HostServices::new(),
        owner: 5,
    };
    let mut scratch = [0_u8; 4096];
    assert_eq!(
        bare.backend(schema_op(1), &[0], &mut scratch),
        Err(ERR_UNSUPPORTED)
    );
}

#[test]
fn parts_the_device_lacks_are_not_found_and_never_zeros() {
    for id in [3, 4, 5, 7, 8, 9, 10, 11, 12, 14] {
        assert_eq!(
            call(Fake::bare(), id, &input_of(id)),
            Err(ERR_NOT_FOUND),
            "op {id}"
        );
    }
    // No identity at all is not a record of zeros either.
    assert_eq!(call(Fake::default(), 1, &[0]), Err(ERR_NOT_FOUND));
    // Values and keys that are not known are empty lists (device.md 2).
    for id in [2, 15] {
        let reply = call(Fake::bare(), id, &[0]).unwrap();
        let rows = records(&reply);
        assert_eq!(rows.len(), 1, "op {id}");
        assert_eq!(end_of(&rows).total, 0);
    }
}

#[test]
fn a_host_that_says_ok_without_a_visit_is_not_here_either() {
    struct Silent;
    impl DeviceHost for Silent {
        fn device(&self, _: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
            Ok(())
        }
        fn power(&self, _: &mut dyn FnMut(&Power)) -> Result<(), i32> {
            Ok(())
        }
    }
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_device(Rc::new(Silent)).unwrap();
    let view = View {
        env: &env,
        hosts,
        owner: 0,
    };
    assert_eq!(run(&view, 1, &[0]), Err(ERR_NOT_FOUND));
    assert_eq!(run(&view, 12, &interval(5)), Err(ERR_NOT_FOUND));
}

#[test]
fn caps_and_the_ops_agree_for_every_combination_of_the_three_parts() {
    for wifi in [false, true] {
        for bluetooth in [false, true] {
            for firmware in [false, true] {
                let mut host = Fake::full();
                let mut bits = 0;
                bits |= if wifi { caps::WIFI } else { 0 };
                bits |= if bluetooth { caps::BLUETOOTH } else { 0 };
                bits |= if firmware { caps::FIRMWARE } else { 0 };
                host.device = Some(Dev::x4(bits));
                host.wifi = wifi;
                host.bluetooth = bluetooth;
                host.firmware = firmware;
                assert_eq!(check_caps_agree(&host), Ok(()));
                // The wire agrees with the check.
                with_view(host, Some(5), |view| {
                    let missing = |id| run(view, id, &input_of(id)) == Err(ERR_NOT_FOUND);
                    assert_eq!((missing(8), missing(9)), (!wifi, !wifi));
                    assert_eq!((missing(10), missing(11)), (!bluetooth, !bluetooth));
                    assert_eq!(missing(3), !firmware);
                });
            }
        }
    }
}

#[test]
fn a_cap_without_its_part_or_a_part_without_its_cap_is_caught() {
    let mut host = Fake::full();
    host.wifi = false;
    assert!(check_caps_agree(&host).is_err());
    let mut host = Fake::bare();
    host.bluetooth = true;
    assert!(check_caps_agree(&host).is_err());
    let mut host = Fake::bare();
    host.firmware = true;
    assert!(check_caps_agree(&host).is_err());
    let mut host = Fake::full();
    host.device = Some(Dev::x4(caps::WIFI | caps::BLUETOOTH));
    assert!(check_caps_agree(&host).is_err());
    assert!(check_caps_agree(&Fake::default()).is_err());
}

#[test]
fn values_are_named_in_range_and_at_most_eight() {
    let mut host = Fake::full();
    let big = i64::from(i32::MAX) + 1;
    let mut values: Vec<(String, Option<i64>)> = vec![
        ("ok-min".to_string(), Some(i64::from(i32::MIN))),
        ("ok-max".to_string(), Some(i64::from(i32::MAX))),
        ("too-big".to_string(), Some(big)),
        ("too-small".to_string(), Some(i64::from(i32::MIN) - 1)),
        ("unreadable".to_string(), None),
        ("Upper".to_string(), Some(1)),
        (String::new(), Some(1)),
        ("a".repeat(33), Some(1)),
        ("under_score".to_string(), Some(1)),
        ("zero".to_string(), Some(0)),
    ];
    values.extend((0..10).map(|n| (alloc::format!("v{n}"), Some(n))));
    host.values = Some(values);
    let reply = call(host, 2, &[0]).unwrap();
    let rows = records(&reply);
    let names: Vec<&str> = rows
        .iter()
        .filter_map(|r| match r {
            Record::ServiceDevValue(v) => Some(v.name),
            _ => None,
        })
        .collect();
    // A reading that cannot be read or does not fit is left out, never 0.
    assert_eq!(
        names,
        ["ok-min", "ok-max", "zero", "v0", "v1", "v2", "v3", "v4"]
    );
    assert_eq!(end_of(&rows).total, 8);
    let Record::ServiceDevValue(min) = rows[0] else {
        panic!()
    };
    assert_eq!(min.value, i32::MIN);
}

#[test]
fn the_device_record_cuts_every_string_at_its_cap_on_a_char_boundary() {
    let mut host = Fake::full();
    let mut dev = Dev::x4(u64::MAX);
    dev.id = "é".repeat(40);
    dev.model = "m".repeat(100);
    dev.catalog = "c".repeat(33);
    dev.class = 9;
    dev.start = 9;
    dev.slept = Some(1234);
    host.device = Some(dev);
    let reply = call(host, 1, &[0]).unwrap();
    let [Record::ServiceDevDevice(d)] = records(&reply)[..] else {
        panic!()
    };
    assert_eq!(d.id.len(), 64);
    assert!(d.id.chars().all(|c| c == 'é'));
    assert_eq!((d.model.len(), d.catalog.len()), (64, 32));
    // Unknown bits are not sent; unknown enumerations read as unknown.
    assert_eq!(d.caps, caps::CAPS_MASK);
    assert_eq!((d.class, d.start), (0, 0));
    assert_eq!((d.slept_ms, d.flags), (1234, 1));
    assert!(
        reply.len() <= 211 + 8,
        "device.md 6.1: 211 bytes at the caps"
    );
}

#[test]
fn every_text_field_of_every_record_is_cut_to_its_cap() {
    let mut host = Fake::full();
    host.builtin = Some(vec![("i".repeat(80), "v".repeat(80))]);
    host.not_installed = Some(vec![("p".repeat(80), 3)]);
    host.keys = Some(vec!["k".repeat(80)]);
    host.bonded = vec![(1, "n".repeat(80), false, "g".repeat(80))];
    host.networks = vec![("s".repeat(80), -50, false, false)];
    let sizes = |id: u32, input: Vec<u8>, host: Fake| {
        let reply = call(host, id, &input).unwrap();
        records(&reply)
            .iter()
            .map(|r| match r {
                Record::ServiceDevBuiltin(b) => b.id.len() * 1000 + b.version.len(),
                Record::ServiceDevNotInstalled(n) => n.plugin.len(),
                Record::ServiceDevKey(k) => k.name.len(),
                Record::ServiceDevBonded(b) => b.name.len() * 1000 + b.given.len(),
                Record::ServiceDevNetwork(n) => n.ssid.len(),
                _ => 0,
            })
            .next()
            .unwrap()
    };
    let fake = |f: &Fake| Fake {
        builtin: f.builtin.clone(),
        not_installed: f.not_installed.clone(),
        keys: f.keys.clone(),
        bonded: f.bonded.clone(),
        networks: f.networks.clone(),
        ..Fake::full()
    };
    assert_eq!(sizes(4, query(0, 0), fake(&host)), 64 * 1000 + 32);
    assert_eq!(sizes(14, query(0, 0), fake(&host)), 64);
    assert_eq!(sizes(15, vec![0], fake(&host)), 32);
    // The caller is the Bluetooth manager here, so `given` is shown (and cut).
    assert_eq!(sizes(11, query(0, 0), fake(&host)), 64 * 1000 + 64);
    assert_eq!(sizes(9, query(0, 0), fake(&host)), 32);
}

#[test]
fn networks_come_strongest_first_with_ties_in_hosts_order_and_at_most_32() {
    let mut host = Fake::full();
    host.networks = vec![
        ("a-weak".to_string(), -90, false, false),
        ("b-mid".to_string(), -60, true, false),
        (String::new(), -10, true, true),
        ("c-mid".to_string(), -60, false, true),
        ("d-strong".to_string(), -30, true, true),
    ];
    let reply = call(host, 9, &query(0, 0)).unwrap();
    let rows = records(&reply);
    let seen: Vec<(&str, i32, u32)> = rows
        .iter()
        .filter_map(|r| match r {
            Record::ServiceDevNetwork(n) => Some((n.ssid, n.signal, n.flags)),
            _ => None,
        })
        .collect();
    // The unnamed network is left out; `secured` is bit 0, `known` bit 1.
    assert_eq!(
        seen,
        [
            ("d-strong", -30, 3),
            ("b-mid", -60, 1),
            ("c-mid", -60, 2),
            ("a-weak", -90, 0)
        ]
    );
    assert_eq!(end_of(&rows).total, 4);

    // 40 networks: the strongest 32, in order, whatever order the host has.
    let mut host = Fake::full();
    host.networks = (0..40)
        .map(|n| (alloc::format!("n{n}"), -((n * 7) % 40) - 20, false, false))
        .collect();
    let reply = call(host, 9, &query(0, 0)).unwrap();
    let rows = records(&reply);
    let signals: Vec<i32> = rows
        .iter()
        .filter_map(|r| match r {
            Record::ServiceDevNetwork(n) => Some(n.signal),
            _ => None,
        })
        .collect();
    assert_eq!(signals.len(), 32);
    assert!(signals.windows(2).all(|w| w[0] >= w[1]));
    // The 32 strongest of -20..=-59 (each value once): -20 down to -51.
    assert_eq!((signals[0], signals[31]), (-20, -51));
    let end = end_of(&rows);
    assert_eq!(
        (end.total, end.next, end.flags, end.generation),
        (32, 32, 1, 3)
    );
    assert!(reply.len() <= 1566 + 64, "device.md 6.1");
}

#[test]
fn pages_follow_the_cursor_the_generation_and_the_registry_rules() {
    let mut host = Fake::full();
    host.builtin = Some(
        (0..70)
            .map(|n| (alloc::format!("b.{n:02}"), "1".to_string()))
            .collect(),
    );
    with_view(host, Some(5), |view| {
        let mut cursor = 0;
        let mut generation = 0;
        let mut pages = Vec::new();
        loop {
            let reply = run(view, 4, &query(cursor, generation)).unwrap();
            let rows = records(&reply);
            let end = end_of(&rows);
            pages.push(rows.len() - 1);
            assert_eq!((end.total, end.generation), (70, 3));
            assert_eq!(end.next, cursor + (rows.len() as u32 - 1));
            if end.flags & 1 != 0 {
                assert_eq!(end.next, 70);
                break;
            }
            (cursor, generation) = (end.next, end.generation);
        }
        assert_eq!(pages, [32, 32, 6]);
        // A stale generation, or a cursor without one, starts again.
        assert_eq!(run(view, 4, &query(32, 9)), Err(ERR_STATE));
        assert_eq!(run(view, 4, &query(32, 0)), Err(ERR_STATE));
        // A cursor past the end is an empty last page.
        let reply = run(view, 4, &query(500, 3)).unwrap();
        let rows = records(&reply);
        let end = end_of(&rows);
        assert_eq!((rows.len(), end.total, end.next, end.flags), (1, 70, 70, 1));
        // A scope that names nothing is an empty page; reserved flags are invalid.
        let scoped = batch(&[Record::ServiceRegQuery(registrations::ServiceRegQuery {
            scope: "nothing",
            cursor: 0,
            generation: 0,
            flags: 0,
        })]);
        let rows = records(&run(view, 4, &scoped).unwrap()).len();
        assert_eq!(rows, 1);
        let flagged = batch(&[Record::ServiceRegQuery(registrations::ServiceRegQuery {
            scope: "",
            cursor: 0,
            generation: 0,
            flags: 1,
        })]);
        assert_eq!(run(view, 4, &flagged), Err(ERR_INVALID));
    });
}

#[test]
fn the_generation_is_never_zero() {
    let mut host = Fake::full();
    host.generation = 0;
    let reply = call(host, 14, &query(0, 0)).unwrap();
    assert_eq!(end_of(&records(&reply)).generation, 1);
}

#[test]
fn row_limits_hold_at_their_boundaries() {
    let count = |id: u32, host: Fake, input: Vec<u8>| {
        let reply = call(host, id, &input).unwrap();
        let rows = records(&reply);
        (rows.len() - 1, end_of(&rows))
    };
    for (rows, shown) in [(31, 31), (32, 32), (33, 32), (200, 32)] {
        let mut host = Fake::full();
        host.networks = (0..rows)
            .map(|n| (alloc::format!("n{n}"), -n, false, false))
            .collect();
        assert_eq!(count(9, host, query(0, 0)).0, shown, "networks {rows}");
        let mut host = Fake::full();
        host.not_installed = Some((0..rows).map(|n| (alloc::format!("p{n}"), 1)).collect());
        assert_eq!(
            count(14, host, query(0, 0)).0,
            shown,
            "not installed {rows}"
        );
        let mut host = Fake::full();
        host.keys = Some((0..rows).map(|n| alloc::format!("k{n}")).collect());
        assert_eq!(count(15, host, vec![0]).0, shown, "keys {rows}");
    }
    for (rows, shown) in [(15, 15), (16, 16), (17, 16), (99, 16)] {
        let mut host = Fake::full();
        host.bonded = (0..rows)
            .map(|n| (n, alloc::format!("d{n}"), false, String::new()))
            .collect();
        let (n, end) = count(11, host, query(0, 0));
        assert_eq!(
            (n, end.total, end.flags),
            (shown, shown as u32, 1),
            "bonded {rows}"
        );
    }
    for (rows, shown) in [(7, 7), (8, 8), (9, 8), (50, 8)] {
        let mut host = Fake::full();
        host.values = Some(
            (0..rows)
                .map(|n| (alloc::format!("v{n}"), Some(i64::from(n))))
                .collect(),
        );
        let (n, end) = count(2, host, vec![0]);
        assert_eq!((n, end.total), (shown, shown as u32), "values {rows}");
    }
    // Largest replies stay inside 4,096 bytes, as device.md 6.1 computes.
    let mut host = Fake::full();
    host.bonded = (0..16)
        .map(|n| (n, "n".repeat(64), true, "g".repeat(64)))
        .collect();
    let reply = call(host, 11, &query(0, 0)).unwrap();
    assert!(reply.len() <= 2430 + 64);
}

#[test]
fn builtin_pages_never_cut_a_row_and_a_full_page_says_more_follows() {
    let mut host = Fake::full();
    host.builtin = Some(
        (0..32)
            .map(|n| (alloc::format!("{n:02}{}", "i".repeat(62)), "v".repeat(32)))
            .collect(),
    );
    let reply = call(host, 4, &query(0, 0)).unwrap();
    let rows = records(&reply);
    let end = end_of(&rows);
    assert_eq!((rows.len(), end.total, end.flags), (33, 32, 1));
    assert!(reply.len() <= 3422 + 32, "device.md 6.1");
}

#[test]
fn bonded_given_is_shown_only_to_the_bluetooth_manager() {
    let given = |caller: Option<u64>| {
        with_view(Fake::full(), caller, |view| {
            let reply = run(view, 11, &query(0, 0)).unwrap();
            let rows = records(&reply);
            let Record::ServiceDevBonded(b) = rows[0] else {
                panic!()
            };
            assert_eq!(
                (b.device, b.name, b.kind, b.flags),
                (7, "Page turner", 2, 0b11)
            );
            b.given.to_string()
        })
    };
    assert_eq!(given(Some(5)), "remote");
    // Another plugin, and a frame that cannot name its caller, see nothing.
    assert_eq!(given(Some(6)), "");
    assert_eq!(given(None), "");
}

#[test]
fn power_stretches_the_base_by_the_level_as_interface_6_does() {
    let level = |level, stretch, base| {
        let mut host = Fake::full();
        host.power = Some(Power::new(
            level,
            paperpaw_sdk::Id::parse("core.saving").unwrap(),
            true,
            stretch,
        ));
        let reply = call(host, 12, &interval(base)).unwrap();
        let [Record::ServiceDevPower(p)] = records(&reply)[..] else {
            panic!()
        };
        (p.level, p.mode.to_string(), p.flags, p.stretch, p.every_ms)
    };
    assert_eq!(level(PowerLevel::Normal, 4, 1000).4, 1000);
    assert_eq!(
        level(PowerLevel::Saving, 4, 1000),
        (1, "core.saving".to_string(), 0b01 | 0b10, 4, 4000)
    );
    // Offline means never: 0, flags without `online`.
    assert_eq!(
        level(PowerLevel::Offline, 4, 1000),
        (2, "core.saving".to_string(), 0b01, 4, 0)
    );
    // No base is no interval; an overflowing one is cut at u32::MAX.
    assert_eq!(level(PowerLevel::Saving, 4, 0).4, 0);
    assert_eq!(level(PowerLevel::Saving, 4, u32::MAX).4, u32::MAX);
    // The rule is the SDK's own.
    for l in [PowerLevel::Normal, PowerLevel::Saving, PowerLevel::Offline] {
        let power = Power::new(l, paperpaw_sdk::Id::parse("x.y").unwrap(), false, 3);
        let expect = power.every(750).map_or(0, |ms| u32::try_from(ms).unwrap());
        assert_eq!(level(l, 3, 750).4, expect);
    }
}

#[test]
fn memory_flags_say_what_is_known_and_unknown_fields_are_zero() {
    let flags = |facts: MemoryFacts| {
        let mut host = Fake::full();
        host.memory = Some(facts);
        let reply = call(host, 7, &[0]).unwrap();
        let [Record::ServiceDevMemory(m)] = records(&reply)[..] else {
            panic!()
        };
        m
    };
    let base = MemoryFacts {
        heap_free: 1,
        heap_size: 2,
        largest: None,
        stack: None,
        external: None,
        cached: false,
    };
    let m = flags(base);
    assert_eq!((m.heap_free, m.heap_size, m.flags), (1, 2, 0));
    assert_eq!(
        (m.largest, m.stack_free, m.stack_size, m.external_free),
        (0, 0, 0, 0)
    );
    let m = flags(MemoryFacts {
        largest: Some(9),
        stack: Some((3, 4)),
        external: Some((5, 6)),
        cached: true,
        ..base
    });
    assert_eq!(m.flags, 0b1111);
    assert_eq!((m.largest, m.stack_free, m.stack_size), (9, 3, 4));
    assert_eq!((m.external_free, m.external_size), (5, 6));
    let mut host = Fake::full();
    host.memory = None;
    assert_eq!(call(host, 7, &[0]), Err(ERR_NOT_FOUND));
}

#[test]
fn the_probe_runs_once_per_250_ms_and_answers_from_the_cache_inside() {
    let probes = Cell::new(0_u32);
    let cache = ProbeCache::<u32>::new();
    let read = |now: u64, value: Option<u32>| {
        cache.get(now, || {
            probes.set(probes.get() + 1);
            value
        })
    };
    assert_eq!(read(1000, Some(1)), (Some(1), false));
    assert_eq!(read(1000, Some(2)), (Some(1), true));
    assert_eq!(read(1249, Some(3)), (Some(1), true));
    assert_eq!(probes.get(), 1);
    // The window is [at, at + 250): the boundary probes.
    assert_eq!(read(1250, Some(4)), (Some(4), false));
    assert_eq!(read(1499, Some(5)), (Some(4), true));
    assert_eq!(read(1500, Some(6)), (Some(6), false));
    assert_eq!(probes.get(), 3);
    // A probe that read nothing is cached for the window too, and then retried.
    assert_eq!(read(1600, None), (Some(6), true));
    assert_eq!(read(1750, None), (None, false));
    assert_eq!(read(1800, Some(7)), (None, true));
    assert_eq!(read(2000, Some(7)), (Some(7), false));
    // A clock that went backwards is outside the window.
    assert_eq!(read(10, Some(8)), (Some(8), false));
    assert_eq!(probes.get(), 6);
    assert_eq!(PROBE_WINDOW_MS, 250);
}

#[test]
fn malformed_input_is_refused_and_never_panics() {
    let valid = |id: u32| input_of(id);
    let mut hostile: Vec<Vec<u8>> = vec![
        vec![],
        vec![1],
        vec![2],
        vec![0, 0],
        vec![0xff],
        vec![0xff; 40],
        vec![1, 0xff, 0xff, 0xff, 0xff, 0x0f],
        vec![1, 1, 0],
        vec![1, 0x81, 0x08, 0xff],
        (0..=255).collect(),
        vec![0; 300],
    ];
    // Right records for another op, truncations and trailing bytes of valid ones.
    for id in [4, 12, 6] {
        let good = valid(id);
        hostile.push(good.clone());
        for cut in 0..good.len() {
            hostile.push(good[..cut].to_vec());
        }
        let mut longer = good.clone();
        longer.push(0);
        hostile.push(longer);
        let mut twice = good.clone();
        twice[0] = 2;
        twice.extend_from_slice(&good[1..]);
        hostile.push(twice);
    }
    with_view(Fake::full(), Some(5), |view| {
        for id in 1..=16 {
            for input in &hostile {
                let mut scratch = vec![0_u8; schema_op(id).max_reply];
                let result = view.backend(schema_op(id), input, &mut scratch);
                if *input == valid(id) && ![6, 13, 16].contains(&id) {
                    assert!(result.is_ok(), "op {id}");
                } else {
                    assert!(result.is_err(), "op {id} input {input:?}");
                }
            }
        }
        // Undefined ops and a short scratch are errors, not panics.
        let mut scratch = [0_u8; 4096];
        for id in [0, 17, 99, u32::MAX] {
            let op = ServiceOp {
                id,
                ..*schema_op(1)
            };
            assert_eq!(view.backend(&op, &[0], &mut scratch), Err(ERR_UNSUPPORTED));
        }
        for size in [0, 1, 20, 100] {
            let mut small = vec![0_u8; size];
            for id in [1, 2, 4, 7, 9, 12, 15] {
                let result = view.backend(schema_op(id), &input_of(id), &mut small);
                // Whatever fits is whole: it still validates.
                if let Ok(n) = result {
                    assert_eq!(validate_reply(&small[..n], schema_op(id)), Ok(()));
                }
                assert!(size > 1 || result.is_err());
            }
        }
    });
}

#[test]
fn the_dispatcher_opens_the_service_and_an_unwired_host_refuses_its_calls() {
    use crate::native_host::{
        HostResources, call_services, environment::EnvironmentScope, shell::DeclaredServices,
    };
    use core::pin::pin;
    use paperpaw_native::{
        host_glue::{OK, PpBuffer, PpBytes},
        metadata::Declared,
    };
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    let declared = DeclaredServices::try_from_iter(core::iter::once(Declared {
        key: KEY,
        major: 1,
        semantic_revision: 0,
    }))
    .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let key = PpBytes {
        ptr: KEY.as_ptr(),
        len: KEY.len(),
    };
    let mut handle = 0;
    scope
        .as_ref()
        .scope(&env, || {
            // SAFETY: all pairs are live and disjoint for these calls.
            unsafe {
                assert_eq!(
                    call_services::open(&scope, &owner, &declared, key, 1, 0, &mut handle),
                    OK
                );
                let input = [0_u8];
                let mut out = [0_u8; 4096];
                let status = call_services::call(
                    &scope,
                    &owner,
                    "reader",
                    handle,
                    1,
                    PpBytes {
                        ptr: input.as_ptr(),
                        len: 1,
                    },
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: out.len(),
                    },
                );
                // No shell view is wired behind the key.
                assert_eq!(status, ERR_UNSUPPORTED);
            }
        })
        .unwrap();
}

#[test]
fn the_service_catalog_offers_only_core_device() {
    assert_eq!(DEVICE_SERVICES.len(), 1);
    assert_eq!(DEVICE_SERVICES[0].key, "core.device");
    assert_eq!(
        (
            DEVICE_SERVICES[0].major,
            DEVICE_SERVICES[0].semantic_revision
        ),
        (1, 0)
    );
    assert!(
        !crate::native_host::shell::DATA_SERVICES
            .iter()
            .any(|s| s.key == KEY)
    );
}

#[path = "firmware_tests.rs"]
mod firmware;
