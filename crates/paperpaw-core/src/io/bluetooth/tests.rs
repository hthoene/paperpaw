use super::*;
use alloc::vec;
use core::time::Duration;

use paperpaw_sdk::Event;
use paperpaw_sdk::bluetooth::{self as bt, BtCommand, CharacteristicSpec, Handle};

const FOUND: Event<BtEvent> = Event::new("a.found");
const PULSE: Event<BtEvent> = Event::new("pulse.found");
const DONE: Event<BtEvent> = Event::new("a.done");

fn radio() -> Radio {
    let mut radio = Radio::new(2);
    radio.start();
    radio.take();
    radio
}

/// The command inside `command`, as the kernel hands it on.
fn inner(command: Command) -> BtCommand {
    match command {
        Command::Bluetooth(request) => request.into_parts().0,
        other => panic!("not a Bluetooth command: {other:?}"),
    }
}

fn asked(now: u64) -> Asked {
    Asked {
        screens: 1,
        reader: false,
        now: Instant(now),
    }
}

fn readers(now: u64) -> Asked {
    Asked {
        reader: true,
        ..asked(now)
    }
}

fn run(
    radio: &mut Radio,
    plugin: &str,
    command: Command,
    asked: Asked,
    out: &mut Vec<Command>,
) -> Result<(), BtFailure> {
    radio.command(plugin, inner(command), asked, out)
}

fn found(radio: &mut Radio, address: &str, services: Vec<Uuid>, out: &mut Vec<Command>) {
    let found = Found {
        address: address.into(),
        name: Some(address.to_uppercase()),
        signal: Some(-50),
        services,
        ..Found::default()
    };
    radio.report(BtReport::Found(found), Instant(10), out);
}

/// The events told on `events`.
fn told(out: &[Command], events: &Event<BtEvent>) -> Vec<BtEvent> {
    out.iter()
        .filter_map(|c| match c {
            Command::Emit(id, params) if id == events.id() => params.carried::<BtEvent>().cloned(),
            _ => None,
        })
        .collect()
}

fn link_of(calls: &[BtCall]) -> u32 {
    calls
        .iter()
        .find_map(|c| match c {
            BtCall::Connect { link, .. } => Some(*link),
            _ => None,
        })
        .expect("a link was made")
}

fn connected(link: u32) -> BtReport {
    BtReport::Connected {
        link,
        mtu: 23,
        security: Security::Encrypted,
    }
}

#[test]
fn a_bond_is_kept_and_read_again() {
    let known = Known {
        id: DeviceId::new(4),
        address: "aa:bb".into(),
        found: Found {
            name: Some("Remote".into()),
            ..Found::default()
        },
        kind: DeviceKind::Remote,
        found_by: Vec::new(),
        bond: Some("1234abcd".into()),
        given: vec![Id::new_static("pulse")],
        keys: true,
        wanted: false,
    };
    let read = Known::read(4, &known.written()).unwrap();
    assert_eq!(read.address, "aa:bb");
    assert_eq!(read.name(), Some("Remote"));
    assert_eq!(read.kind, DeviceKind::Remote);
    assert!(read.keys);
    assert_eq!(read.given, vec![Id::new_static("pulse")]);
    assert_eq!(read.bond.as_deref(), Some("1234abcd"));
}

#[test]
fn kinds_come_from_the_appearance_or_the_services() {
    assert_eq!(kind_of(Some(0x03C1), &[]), DeviceKind::Keyboard);
    assert_eq!(kind_of(Some(0x03C0), &[]), DeviceKind::Remote);
    assert_eq!(kind_of(None, &[Uuid::sig(0x1812)]), DeviceKind::Remote);
    assert_eq!(kind_of(None, &[Uuid::sig(0x180D)]), DeviceKind::Sensor);
    assert_eq!(kind_of(None, &[]), DeviceKind::Other);
}

#[test]
fn the_radio_comes_on_for_a_scan_goes_off_a_while_after_and_its_time_is_counted() {
    let mut radio = radio();
    let mut out = Vec::new();
    let scan = bt::scan(&FOUND).lasting(Duration::from_secs(1)).command();
    run(&mut radio, "a", scan, asked(0), &mut out).unwrap();
    assert_eq!(
        radio.take(),
        [
            BtCall::Radio { on: true },
            BtCall::Scan {
                on: true,
                power: ScanPower::Low,
                active: false,
            }
        ]
    );
    // The scan ends at its time; the radio stays on a little.
    radio.tick(Instant(1_000), &mut out);
    assert_eq!(
        radio.take(),
        [BtCall::Scan {
            on: false,
            power: ScanPower::Low,
            active: false,
        }]
    );
    assert_eq!(told(&out, &FOUND), [BtEvent::ScanEnded]);
    assert_eq!(radio.deadline(), Some(Instant(1_000 + RADIO_GRACE_MS)));
    radio.tick(Instant(1_000 + RADIO_GRACE_MS), &mut out);
    assert_eq!(radio.take(), [BtCall::Radio { on: false }]);
    assert_eq!(radio.take_on_ms(Instant(10_000)), 1_000 + RADIO_GRACE_MS);
    assert_eq!(radio.take_on_ms(Instant(20_000)), 0);
}

#[test]
fn the_radio_comes_on_before_a_link_is_asked_for() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa	Remote	remote	0		keys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    let calls = radio.take();
    assert_eq!(calls[0], BtCall::Radio { on: true }, "{calls:?}");
    assert!(matches!(calls[1], BtCall::Connect { .. }), "{calls:?}");
    assert_eq!(calls.len(), 2, "{calls:?}");
}

#[test]
fn what_scans_found_leaves_with_the_radio() {
    let mut radio = radio();
    let mut out = Vec::new();
    let scan = bt::scan(&FOUND).lasting(Duration::from_secs(1)).command();
    run(&mut radio, "a", scan, asked(0), &mut out).unwrap();
    for n in 0..20 {
        found(
            &mut radio,
            &alloc::format!("d{n:02}"),
            vec![Uuid::sig(0x180D)],
            &mut out,
        );
    }
    radio.tick(Instant(1_000), &mut out);
    radio.tick(Instant(1_000 + RADIO_GRACE_MS), &mut out);
    assert!(radio.take().contains(&BtCall::Radio { on: false }));
    // The newest stay, with their names, without what they advertised.
    assert_eq!(radio.known.len(), FOUND_KEPT);
    assert_eq!(radio.known[0].name(), Some("D04"));
    assert!(radio.known.iter().all(|k| k.found.services.is_empty()));
    // A plugin may still connect to one it found.
    let device = radio.known[FOUND_KEPT - 1].id;
    run(
        &mut radio,
        "a",
        bt::connect(device, &FOUND).command(),
        asked(2_000),
        &mut out,
    )
    .unwrap();
    assert!(
        radio
            .take()
            .iter()
            .any(|c| matches!(c, BtCall::Connect { .. }))
    );
}

#[test]
fn a_scan_is_never_longer_than_a_minute() {
    let mut radio = radio();
    let mut out = Vec::new();
    let scan = bt::scan(&FOUND)
        .lasting(Duration::from_secs(3_600))
        .command();
    run(&mut radio, "a", scan, asked(0), &mut out).unwrap();
    assert_eq!(radio.deadline(), Some(Instant(60_000)));
}

#[test]
fn scans_run_at_the_cheapest_power_that_serves_all() {
    let mut radio = radio();
    let mut out = Vec::new();
    let low = bt::scan(&FOUND).lasting(Duration::from_secs(30)).command();
    let balanced = bt::scan(&PULSE)
        .power(ScanPower::Balanced)
        .lasting(Duration::from_secs(10))
        .command();
    run(&mut radio, "a", low, asked(0), &mut out).unwrap();
    run(&mut radio, "pulse", balanced, asked(0), &mut out).unwrap();
    let scans = |calls: Vec<BtCall>| -> Vec<ScanPower> {
        calls
            .into_iter()
            .filter_map(|c| match c {
                BtCall::Scan {
                    on: true, power, ..
                } => Some(power),
                _ => None,
            })
            .collect()
    };
    assert_eq!(scans(radio.take()), [ScanPower::Low, ScanPower::Balanced]);
    // The balanced one ends: back to low.
    radio.tick(Instant(10_000), &mut out);
    assert_eq!(scans(radio.take()), [ScanPower::Low]);
}

#[test]
fn a_fast_scan_is_the_readers_own_doing() {
    let mut radio = radio();
    let mut out = Vec::new();
    let fast = || bt::scan(&FOUND).power(ScanPower::Fast).command();
    run(&mut radio, "a", fast(), asked(0), &mut out).unwrap();
    assert!(radio.take().contains(&BtCall::Scan {
        on: true,
        power: ScanPower::Balanced,
        active: false,
    }));
    run(&mut radio, "a", fast(), readers(0), &mut out).unwrap();
    assert!(radio.take().contains(&BtCall::Scan {
        on: true,
        power: ScanPower::Fast,
        active: false,
    }));
    // And short.
    assert_eq!(radio.deadline(), Some(Instant(5_000)));
}

#[test]
fn a_plugin_may_not_scan_again_so_soon() {
    let mut radio = radio();
    let mut out = Vec::new();
    let scan = || bt::scan(&FOUND).lasting(Duration::from_secs(1)).command();
    run(&mut radio, "a", scan(), asked(0), &mut out).unwrap();
    radio.tick(Instant(1_000), &mut out);
    let held = run(&mut radio, "a", scan(), asked(2_000), &mut out).unwrap_err();
    assert_eq!(held.op, BtOp::Scan);
    assert!(matches!(held.error, BtError::Held(_)));
    // Unless the reader asks, or the minute is over.
    run(&mut radio, "a", scan(), readers(2_000), &mut out).unwrap();
    radio.tick(Instant(3_000), &mut out);
    run(&mut radio, "a", scan(), asked(63_000), &mut out).unwrap();
}

#[test]
fn a_plugin_hears_only_what_its_filters_pass() {
    let mut radio = radio();
    let mut out = Vec::new();
    run(
        &mut radio,
        "a",
        bt::scan(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    let hr = bt::scan(&PULSE).service(Uuid::sig(0x180D)).command();
    run(&mut radio, "pulse", hr, asked(0), &mut out).unwrap();
    found(&mut radio, "remote", vec![Uuid::sig(0x1812)], &mut out);
    found(&mut radio, "belt", vec![Uuid::sig(0x180D)], &mut out);
    assert_eq!(told(&out, &FOUND).len(), 2);
    assert_eq!(told(&out, &PULSE).len(), 1);
    // The belt is pulse's; the remote is not.
    let remote = radio.known[0].id;
    let refused = run(
        &mut radio,
        "pulse",
        bt::connect(remote, &PULSE).command(),
        asked(20),
        &mut out,
    )
    .unwrap_err();
    assert_eq!(
        refused,
        BtFailure::new(BtOp::Connect, BtError::NotFound).device(remote)
    );
}

#[test]
fn scans_do_not_see_someone_elses_bond() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "remote\tRemote\tremote\t0\tpulse\tkeys")].into_iter());
    let mut out = Vec::new();
    run(
        &mut radio,
        "a",
        bt::scan(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    run(
        &mut radio,
        "pulse",
        bt::scan(&PULSE).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    found(&mut radio, "remote", vec![Uuid::sig(0x1812)], &mut out);
    assert!(told(&out, &FOUND).is_empty());
    let pulse = told(&out, &PULSE);
    assert!(matches!(&pulse[..], [BtEvent::Found(d)] if d.bonded));
}

#[test]
fn a_device_found_before_is_told_to_a_new_scan_and_first_ends_it() {
    let mut radio = radio();
    let mut out = Vec::new();
    run(
        &mut radio,
        "pulse",
        bt::scan(&PULSE).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    found(&mut radio, "belt", vec![Uuid::sig(0x180D)], &mut out);
    out.clear();
    let first = bt::scan(&FOUND).first().command();
    run(&mut radio, "a", first, asked(20), &mut out).unwrap();
    let events = told(&out, &FOUND);
    assert!(matches!(
        &events[..],
        [BtEvent::Found(_), BtEvent::ScanEnded]
    ));
}

#[test]
fn forgetting_and_giving_are_for_the_reader_and_the_manager() {
    let mut radio = radio();
    radio.load([("core.bluetooth.3", "aa\tRemote\tremote\t0\t\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(3);
    let refused = run(
        &mut radio,
        "a",
        bt::forget(device, &DONE),
        asked(0),
        &mut out,
    );
    assert_eq!(
        refused.unwrap_err(),
        BtFailure::new(BtOp::Forget, BtError::NotAllowed).device(device)
    );
    radio.set_manager(Some("a".into()));
    run(
        &mut radio,
        "a",
        bt::give(device, Id::new_static("pulse"), &DONE),
        asked(0),
        &mut out,
    )
    .unwrap();
    assert_eq!(radio.known[0].given, [Id::new_static("pulse")]);
    radio.set_manager(None);
    run(
        &mut radio,
        "a",
        bt::forget(device, &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    assert_eq!(
        told(&out, &DONE),
        [
            BtEvent::done(BtOp::Give, device),
            BtEvent::done(BtOp::Forget, device)
        ]
    );
    assert!(radio.known.iter().all(|k| k.bond.is_none()));
    let kept = radio.take_bonds(&[("core.bluetooth.3".into(), "x".into())]);
    assert_eq!(
        kept,
        [("core.bluetooth.3".into(), String::new(), String::new())]
    );
}

#[test]
fn raw_reports_only_for_a_plugin_the_remote_is_given_to() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\tpulse\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    let refused = run(
        &mut radio,
        "a",
        bt::hid_reports(device, &FOUND),
        asked(0),
        &mut out,
    );
    assert_eq!(refused.unwrap_err().error, BtError::NotFound);
    run(
        &mut radio,
        "pulse",
        bt::hid_reports(device, &PULSE),
        asked(0),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(10), &mut out);
    assert!(radio.take().contains(&BtCall::Hid { link, on: true }));
    let report = BtReport::HidReport {
        link,
        report: None,
        bytes: vec![0, 0, 0x4f, 0, 0, 0, 0, 0],
    };
    radio.report(report, Instant(20), &mut out);
    assert_eq!(told(&out, &PULSE).len(), 1);
    // Not keys: they were not switched on.
    assert_eq!(radio.take_keys(), []);
}

#[test]
fn only_the_reader_the_manager_or_a_given_plugin_switches_keys() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\tpulse\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    let refused = run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        asked(0),
        &mut out,
    );
    assert_eq!(refused.unwrap_err().error, BtError::NotAllowed);
    run(
        &mut radio,
        "pulse",
        bt::remote_keys(device, &PULSE),
        asked(0),
        &mut out,
    )
    .unwrap();
    run(
        &mut radio,
        "a",
        bt::remote_keys_off(device, &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    assert!(!radio.known[0].keys);
}

#[test]
fn a_remotes_reports_become_keys_and_its_link_slows_when_quiet() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\t\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(10), &mut out);
    let calls = radio.take();
    // The shell serves calls in order: slow HID discovery must not hold
    // the requested connection parameters behind it.
    assert_eq!(
        calls,
        [
            BtCall::LinkPower {
                link,
                power: LinkPower::Responsive
            },
            BtCall::Hid { link, on: true },
        ]
    );
    // A boot keyboard's report: right arrow down, then up.
    let report = |keys: [u8; 6]| BtReport::HidReport {
        link,
        report: None,
        bytes: [0, 0].into_iter().chain(keys).collect(),
    };
    radio.report(report([0x4f, 0, 0, 0, 0, 0]), Instant(20), &mut out);
    assert_eq!(radio.take_keys(), [("hid.right", true)]);
    radio.report(report([0, 0, 0, 0, 0, 0]), Instant(30), &mut out);
    assert_eq!(radio.take_keys(), [("hid.right", false)]);
    // No key for a minute: the link slows down, and stays.
    assert_eq!(radio.deadline(), Some(Instant(30 + KEYS_IDLE_MS)));
    radio.tick(Instant(30 + KEYS_IDLE_MS), &mut out);
    assert_eq!(
        radio.take(),
        [BtCall::LinkPower {
            link,
            power: LinkPower::Low,
        }]
    );
    assert_eq!(radio.deadline(), None);
    // A key: responsive again.
    radio.report(report([0x50, 0, 0, 0, 0, 0]), Instant(70_000), &mut out);
    assert_eq!(radio.take_keys(), [("hid.left", true)]);
    assert!(radio.take().contains(&BtCall::LinkPower {
        link,
        power: LinkPower::Responsive,
    }));
    // Dropped: the key still down is let go.
    radio.report(
        BtReport::Disconnected { link, reason: None },
        Instant(70_010),
        &mut out,
    );
    assert_eq!(radio.take_keys(), [("hid.left", false)]);
}

#[test]
fn a_link_is_let_go_when_its_user_is_idle_as_long_as_it_allows() {
    let mut radio = radio();
    let mut out = Vec::new();
    run(
        &mut radio,
        "a",
        bt::scan(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    found(&mut radio, "belt", vec![Uuid::sig(0x180D)], &mut out);
    let belt = radio.known[0].id;
    let connect = bt::connect(belt, &FOUND)
        .idle(Duration::from_secs(5))
        .command();
    run(&mut radio, "a", connect, asked(100), &mut out).unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(200), &mut out);
    radio.take();
    assert_eq!(radio.deadline(), Some(Instant(5_200)));
    out.clear();
    radio.tick(Instant(5_200), &mut out);
    assert!(radio.take().contains(&BtCall::Disconnect { link }));
    assert_eq!(told(&out, &FOUND), [BtEvent::disconnected(belt, None)]);
}

#[test]
fn a_link_runs_at_the_highest_power_its_users_ask() {
    let mut radio = radio();
    let mut out = Vec::new();
    run(
        &mut radio,
        "a",
        bt::scan(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    found(&mut radio, "belt", vec![Uuid::sig(0x180D)], &mut out);
    let belt = radio.known[0].id;
    let link = {
        run(
            &mut radio,
            "a",
            bt::connect(belt, &FOUND).command(),
            asked(0),
            &mut out,
        )
        .unwrap();
        link_of(&radio.take())
    };
    radio.report(connected(link), Instant(10), &mut out);
    assert!(
        !radio
            .take()
            .iter()
            .any(|c| matches!(c, BtCall::LinkPower { .. }))
    );
    let burst = bt::connect(belt, &FOUND).power(LinkPower::Burst).command();
    run(&mut radio, "a", burst, asked(20), &mut out).unwrap();
    assert!(radio.take().contains(&BtCall::LinkPower {
        link,
        power: LinkPower::Burst,
    }));
}

#[test]
fn a_failed_read_is_told_to_who_asked_with_what_failed() {
    let mut radio = radio();
    let mut out = Vec::new();
    run(
        &mut radio,
        "a",
        bt::scan(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    found(&mut radio, "belt", vec![Uuid::sig(0x180D)], &mut out);
    let belt = radio.known[0].id;
    run(
        &mut radio,
        "a",
        bt::connect(belt, &FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(10), &mut out);
    run(
        &mut radio,
        "a",
        bt::read(belt, Handle::new(9)),
        asked(20),
        &mut out,
    )
    .unwrap();
    assert!(radio.take().contains(&BtCall::Read { link, handle: 9 }));
    out.clear();
    let error = BtError::Att(paperpaw_sdk::bluetooth::AttError::INVALID_HANDLE);
    radio.report(
        BtReport::Failed {
            link,
            op: BtOp::Read,
            handle: Some(9),
            error,
        },
        Instant(30),
        &mut out,
    );
    assert_eq!(
        told(&out, &FOUND),
        [BtEvent::Failed(
            BtFailure::new(BtOp::Read, error)
                .device(belt)
                .handle(Handle::new(9))
        )]
    );
}

#[test]
fn the_gatt_server_numbers_its_characteristics_and_tells_the_shell() {
    let mut radio = Radio::new(2);
    let a = Uuid::sig(0x180F);
    let b = Uuid::sig(0x181C);
    radio.add_service(
        &ServiceSpec::new(a, &FOUND)
            .characteristic(CharacteristicSpec::new(Uuid::sig(0x2A19)).read().notify()),
    );
    radio.add_service(
        &ServiceSpec::new(b, &PULSE)
            .characteristic(CharacteristicSpec::new(Uuid::sig(0x2A8A)).write())
            .characteristic(CharacteristicSpec::new(Uuid::sig(0x2A90)).read()),
    );
    radio.start();
    let calls = radio.take();
    let Some(BtCall::Server(services)) = calls.get(1) else {
        panic!("no server: {calls:?}");
    };
    let handles: Vec<u16> = services
        .iter()
        .flat_map(|s| s.characteristics.iter().map(|c| c.handle))
        .collect();
    assert_eq!(handles, [1, 2, 3]);
    assert_eq!(services[0].characteristics[0].access, Access::Encrypted);
    // Only the service's plugin notifies on it.
    let mut out = Vec::new();
    let notify = bt::notify_centrals(a, Uuid::sig(0x2A19), [80]);
    assert!(run(&mut radio, "pulse", notify.clone(), asked(0), &mut out).is_err());
    run(&mut radio, "a", notify, asked(0), &mut out).unwrap();
    assert!(radio.take().contains(&BtCall::Notify {
        handle: 1,
        bytes: vec![80],
    }));
}

#[test]
fn advertising_is_time_bound_and_merged() {
    let mut radio = radio();
    let mut out = Vec::new();
    let long = bt::advertise(&FOUND)
        .name("PaperPaw")
        .lasting(Duration::from_secs(3_600))
        .command();
    run(&mut radio, "a", long, asked(0), &mut out).unwrap();
    let other = bt::advertise(&PULSE).service(Uuid::sig(0x180D)).command();
    run(&mut radio, "pulse", other, asked(0), &mut out).unwrap();
    let last = radio
        .take()
        .into_iter()
        .rev()
        .find_map(|c| match c {
            BtCall::Advertise(advert) => Some(advert),
            _ => None,
        })
        .unwrap();
    assert_eq!(last.name.as_deref(), Some("PaperPaw"));
    assert_eq!(last.services, [Uuid::sig(0x180D)]);
    assert_eq!(last.power, AdvertisePower::Low);
    // Ten minutes at most.
    radio.tick(Instant(600_000), &mut out);
    assert!(told(&out, &FOUND).contains(&BtEvent::AdvertisingEnded));
}

/// The watch the shell was last asked for, in `calls`.
fn watched(calls: &[BtCall]) -> Option<Vec<String>> {
    calls.iter().rev().find_map(|c| match c {
        BtCall::Watch { addresses } => Some(addresses.clone()),
        _ => None,
    })
}

#[test]
fn a_remote_whose_keys_are_wanted_is_connected_again_when_it_advertises() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\t\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(10), &mut out);
    radio.take();
    // It falls asleep: its link drops, and it is watched for.
    let lost = BtReport::Disconnected {
        link,
        reason: Some(BtError::Lost),
    };
    radio.report(lost, Instant(20), &mut out);
    let calls = radio.take();
    assert_eq!(watched(&calls), Some(vec![String::from("aa")]), "{calls:?}");
    assert!(!calls.contains(&BtCall::Radio { on: false }));
    // A key wakes it: it advertises, and is connected at once.
    found(&mut radio, "aa", Vec::new(), &mut out);
    let calls = radio.take();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, BtCall::Connect { address, .. } if address == "aa")),
        "{calls:?}"
    );
    assert_eq!(watched(&calls), Some(Vec::new()));
}

#[test]
fn nothing_is_watched_for_once_the_keys_are_not_wanted_or_in_sleep() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\t\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    let failed = BtReport::Failed {
        link,
        op: BtOp::Connect,
        handle: None,
        error: BtError::Timeout,
    };
    radio.report(failed, Instant(15_000), &mut out);
    assert_eq!(watched(&radio.take()), Some(vec![String::from("aa")]));
    // Keys off: no watch, and the radio goes off after its grace.
    run(
        &mut radio,
        "a",
        bt::remote_keys_off(device, &DONE),
        readers(16_000),
        &mut out,
    )
    .unwrap();
    assert_eq!(watched(&radio.take()), Some(Vec::new()));
    radio.tick(Instant(16_000 + RADIO_GRACE_MS), &mut out);
    assert!(radio.take().contains(&BtCall::Radio { on: false }));
    // Wanted again, then sleep: nothing wanted after it.
    run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        readers(20_000),
        &mut out,
    )
    .unwrap();
    radio.suspend(Instant(21_000), &mut out);
    assert!(radio.known.iter().all(|k| !k.wanted));
    assert_eq!(radio.deadline(), None);
}

#[test]
fn after_the_radio_went_off_by_itself_a_wanted_remote_comes_back() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\t\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(10), &mut out);
    radio.take();
    radio.report(BtReport::Radio { on: false }, Instant(20), &mut out);
    assert!(!radio.take().contains(&BtCall::Radio { on: true }));
    // It rests, then comes on again and watches for the remote.
    assert_eq!(radio.deadline(), Some(Instant(20 + RADIO_REST_MS)));
    radio.tick(Instant(20 + RADIO_REST_MS), &mut out);
    let calls = radio.take();
    assert_eq!(
        calls.first(),
        Some(&BtCall::Radio { on: true }),
        "{calls:?}"
    );
    assert_eq!(watched(&calls), Some(vec![String::from("aa")]));
}

#[test]
fn a_pairing_that_loses_its_link_ends_its_question() {
    let mut radio = radio();
    let mut out = Vec::new();
    run(
        &mut radio,
        "a",
        bt::scan(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    found(&mut radio, "kb", vec![Uuid::sig(0x1812)], &mut out);
    let device = radio.known[0].id;
    run(
        &mut radio,
        "a",
        bt::connect(device, &FOUND).command(),
        asked(10),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(20), &mut out);
    run(&mut radio, "a", bt::pair(device), asked(30), &mut out).unwrap();
    let ask = BtReport::Ask {
        link,
        question: PairQuestion::Show(123_456),
    };
    radio.report(ask, Instant(40), &mut out);
    assert!(radio.take_question().is_some());
    radio.report(
        BtReport::Disconnected { link, reason: None },
        Instant(50),
        &mut out,
    );
    let (ended, lost, name) = radio.take_pairing_ended().expect("ended");
    assert_eq!((ended, lost, name.as_str()), (device, true, "KB"));
    assert!(radio.take_pairing_ended().is_none());
}

#[test]
fn everything_ends_and_the_radio_goes_off_before_sleep() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t1\t\tkeys")].into_iter());
    let mut out = Vec::new();
    run(
        &mut radio,
        "a",
        bt::scan(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    run(
        &mut radio,
        "a",
        bt::advertise(&FOUND).command(),
        asked(0),
        &mut out,
    )
    .unwrap();
    run(
        &mut radio,
        "a",
        bt::remote_keys(DeviceId::new(1), &DONE),
        readers(0),
        &mut out,
    )
    .unwrap();
    radio.take();
    out.clear();
    radio.suspend(Instant(500), &mut out);
    assert_eq!(radio.take(), [BtCall::Radio { on: false }]);
    let events = told(&out, &FOUND);
    assert!(events.contains(&BtEvent::ScanEnded));
    assert!(events.contains(&BtEvent::AdvertisingEnded));
    assert_eq!(radio.deadline(), None);
    assert_eq!(radio.take_on_ms(Instant(9_000)), 500);
}

#[test]
fn enabling_keys_on_an_existing_link_sets_power_before_hid_discovery() {
    let mut radio = radio();
    radio.load([("core.bluetooth.1", "aa\tRemote\tremote\t0\tpulse\tkeys")].into_iter());
    let mut out = Vec::new();
    let device = DeviceId::new(1);
    run(
        &mut radio,
        "pulse",
        bt::hid_reports(device, &PULSE),
        asked(0),
        &mut out,
    )
    .unwrap();
    let link = link_of(&radio.take());
    radio.report(connected(link), Instant(10), &mut out);
    // Raw HID alone retains the existing low-power policy.
    assert_eq!(radio.take(), [BtCall::Hid { link, on: true }]);
    run(
        &mut radio,
        "a",
        bt::remote_keys(device, &DONE),
        readers(20),
        &mut out,
    )
    .unwrap();
    assert_eq!(
        radio.take(),
        [
            BtCall::LinkPower {
                link,
                power: LinkPower::Responsive
            },
            BtCall::Hid { link, on: true },
        ]
    );
    radio.tick(Instant(20 + KEYS_IDLE_MS), &mut out);
    assert_eq!(
        radio.take(),
        [BtCall::LinkPower {
            link,
            power: LinkPower::Low
        }]
    );
}

#[test]
fn radio_and_global_usage_times_saturate() {
    let mut radio = radio();
    radio.on_ms = u64::MAX;
    radio.on_since = Some(Instant(0));
    assert_eq!(radio.take_on_ms(Instant(1)), u64::MAX);
    radio.on_ms = u64::MAX;
    radio.radio_off(Instant(2));
    assert_eq!(radio.take_on_ms(Instant(2)), u64::MAX);
    radio.on_ms = 1;
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.shared.usage.bluetooth_ms = u64::MAX;
    kernel.io.bluetooth = Some(radio);
    kernel.sync_bluetooth();
    assert_eq!(kernel.shared.usage.bluetooth_ms, u64::MAX);
}
