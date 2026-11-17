//! Bluetooth with the real kernel (ADR 0027): a plugin that declares it
//! scans, connects and subscribes; one that does not is refused; a
//! bonded remote's keys run what the key maps bind; a closed screen and
//! sleep end what was asked. (Pairing, which asks the reader in the
//! kernel's own question, is tested with the kernel's unit tests:
//! `src/tests/bluetooth_pairing.rs`.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
use paperpaw_core::{Place, Turn};

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Items;
use paperpaw_core::kit::action::ActionSpec;
use paperpaw_core::sdk::bluetooth::{
    self, BtEvent, DeviceId, Handle, ScanPower, Security, Uuid, ValueKind,
};
use paperpaw_core::sdk::builtin::{actions, slots};
use paperpaw_core::sdk::{
    Canvas, Command, Event, Handled, Id, Manifest, MemoryStorage, Priority, RegisterError, Uses,
    Version,
};
use paperpaw_core::{BtCall, BtReport, Found, Io, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(120, 160, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

const FOUND: Event<BtEvent> = Event::new("pulse.found");
const LINK: Event<BtEvent> = Event::new("pulse.link");
const GO: Id = Id::new_static("pulse.go");
const OTHER_GO: Id = Id::new_static("other.go");
const OTHER_FOUND: Event<BtEvent> = Event::new("other.found");
const HOME: Id = Id::new_static("pulse.home");

type Queue = Rc<RefCell<Vec<Command>>>;
type Log = Rc<RefCell<Vec<String>>>;

/// A screen that logs the actions it gets and the parameters it opened
/// with.
struct Logged {
    name: &'static str,
    log: Log,
}

impl paperpaw_core::testing::Fake for Logged {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, action: &Id, _: &mut Turn<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(format!("{} action {action}", self.name));
        Handled::No
    }
}

std::thread_local! {
    /// Each pulse plugin of the test running: its id, queue and log (its
    /// handlers are plain functions and hold neither).
    static PULSES: RefCell<Vec<(&'static str, Queue, Log)>> = const { RefCell::new(Vec::new()) };
}

/// What the test queued for plugin `id`, taken.
fn queue_of(id: &str) -> Vec<Command> {
    PULSES.with(|pulses| {
        pulses
            .borrow()
            .iter()
            .find(|(of, ..)| *of == id)
            .map(|(_, queue, _)| queue.borrow_mut().drain(..).collect())
            .unwrap_or_default()
    })
}

/// Logs `event` heard, in its plugin's log.
fn note(event: &Event<BtEvent>, params: &paperpaw_sdk::Params) {
    let Some(heard) = paperpaw_core::hear::data::<BtEvent>(params) else {
        return;
    };
    PULSES.with(|pulses| {
        if let Some((_, _, log)) = pulses
            .borrow()
            .iter()
            .find(|(of, ..)| *of == event.id().namespace())
        {
            log.borrow_mut().push(format!("{}: {heard:?}", event.id()));
        }
    });
}

/// A plugin that runs what the test queues, and logs what it hears.
struct Pulse {
    id: &'static str,
    uses: Uses,
    queue: Queue,
    log: Log,
}

impl Pulse {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.id), "Pulse", Version::new(1, 0, 0)).uses(self.uses)
    }
}

fn pulse(kept: Pulse) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let go = Id::parse(&format!("{}.go", kept.id)).unwrap();
        // A kernel made again (as after a restart) is the one driven.
        PULSES.with(|pulses| {
            let mut pulses = pulses.borrow_mut();
            pulses.retain(|(id, ..)| *id != kept.id);
            pulses.push((kept.id, kept.queue.clone(), kept.log.clone()));
        });
        let run: paperpaw_core::kit::action::FixedHandler = match kept.id {
            "pulse" => |_, _, out| out.extend(queue_of("pulse")),
            _ => |_, _, out| out.extend(queue_of("other")),
        };
        r.add_action(ActionSpec::new(go, "go").handler(run))?;
        let hears: [(&Event<BtEvent>, paperpaw_core::Hear); 3] = [
            (&FOUND, |_, p| note(&FOUND, p)),
            (&LINK, |_, p| note(&LINK, p)),
            (&OTHER_FOUND, |_, p| note(&OTHER_FOUND, p)),
        ];
        for (event, run) in hears {
            if event.id().namespace() == kept.id {
                r.subscribe(event.id().clone(), run)?;
            }
        }
        if kept.id == "pulse" {
            let log = kept.log.clone();
            r.add_fake(paperpaw_core::testing::FakeSpec::new(
                HOME,
                "home",
                Box::new(move |_, _| {
                    Box::new(Logged {
                        name: "home",
                        log: log.clone(),
                    })
                }),
            ))?;
            r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)?;
        }
        Ok(())
    }
}

struct Test {
    kernel: Kernel,
    queue: Queue,
    log: Log,
    now: u64,
}

impl Test {
    fn new() -> Self {
        let queue: Queue = Rc::default();
        let log: Log = Rc::default();
        let plugin = |id, uses| Pulse {
            id,
            uses,
            queue: queue.clone(),
            log: log.clone(),
        };
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .user_config(String::from(
                "[core]
language = en
",
            ))
            .bluetooth(2)
            .pulse(plugin("pulse", Uses::BLUETOOTH))
            .pulse(plugin("other", Uses::NOTHING))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.render(&mut |_, _| {});
        let mut test = Test {
            kernel,
            queue,
            log,
            now: 0,
        };
        test.calls();
        test.heard();
        test
    }

    fn at(&mut self) -> Instant {
        self.now += 10;
        Instant(self.now)
    }

    fn run(&mut self, action: Id, commands: impl IntoIterator<Item = Command>) {
        self.queue.borrow_mut().extend(commands);
        let at = self.at();
        self.kernel.run_action(at, action);
    }

    fn report(&mut self, report: BtReport) {
        let at = self.at();
        self.kernel.bluetooth_event(at, report);
    }

    fn calls(&mut self) -> Vec<BtCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Bluetooth(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn heard(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    /// A belt in reach, found by pulse's scan: its device.
    fn belt(&mut self) -> DeviceId {
        self.run(
            GO,
            [bluetooth::scan(&FOUND).service(Uuid::sig(0x180D)).command()],
        );
        assert!(self.calls().contains(&BtCall::Scan {
            on: true,
            power: ScanPower::Low,
            active: false,
        }));
        self.report(BtReport::Found(Found {
            address: "11:22".into(),
            name: Some("Belt".into()),
            signal: Some(-60),
            services: vec![Uuid::sig(0x180D)],
            ..Found::default()
        }));
        let heard = self.heard();
        assert!(
            heard
                .iter()
                .any(|h| h.contains("Found") && h.contains("Belt")),
            "{heard:?}"
        );
        DeviceId::new(1)
    }

    /// pulse's link to `device`, connected.
    fn link(&mut self, device: DeviceId) -> u32 {
        self.run(GO, [bluetooth::connect(device, &LINK).command()]);
        let link = self
            .calls()
            .into_iter()
            .find_map(|c| match c {
                BtCall::Connect { link, .. } => Some(link),
                _ => None,
            })
            .expect("a connection asked");
        self.report(BtReport::Connected {
            link,
            mtu: 23,
            security: Security::None,
        });
        assert!(self.heard().iter().any(|h| h.contains("Connected")));
        link
    }
}

#[test]
fn a_plugin_without_bluetooth_is_refused() {
    let mut test = Test::new();
    test.run(OTHER_GO, [bluetooth::scan(&OTHER_FOUND).command()]);
    assert!(test.calls().is_empty(), "no radio for it");
    let heard = test.heard();
    assert!(heard.iter().any(|h| h.contains("NotAllowed")), "{heard:?}");
}

#[test]
fn a_plugin_scans_connects_and_subscribes() {
    let mut test = Test::new();
    let belt = test.belt();
    let link = test.link(belt);
    test.run(GO, [bluetooth::subscribe(belt, Handle::new(0x2a))]);
    assert!(test.calls().contains(&BtCall::Subscribe {
        link,
        handle: 0x2a,
        on: true
    }));
    test.report(BtReport::Value {
        link,
        handle: 0x2a,
        kind: ValueKind::Notified,
        bytes: vec![0, 72],
    });
    let heard = test.heard();
    assert!(
        heard
            .iter()
            .any(|h| h.contains("Value") && h.contains("[0, 72]")),
        "{heard:?}"
    );
    // Sleep ends it all, and the radio goes off.
    let at = test.at();
    test.kernel.suspend(at);
    let calls = test.calls();
    assert!(calls.contains(&BtCall::Radio { on: false }), "{calls:?}");
    assert!(test.heard().iter().any(|h| h.contains("Disconnected")));
    // The time it was on is counted, for the battery's statistics.
    assert_eq!(test.kernel.usage().bluetooth_ms, at.0 - 10);
}

#[test]
fn another_plugin_cannot_use_a_device_it_did_not_find() {
    let mut test = Test::new();
    let belt = test.belt();
    test.run(OTHER_GO, [bluetooth::connect(belt, &OTHER_FOUND).command()]);
    assert!(
        !test
            .calls()
            .iter()
            .any(|c| matches!(c, BtCall::Connect { .. }))
    );
}

#[test]
fn a_bonded_remotes_keys_run_what_the_key_maps_bind() {
    let mut test = Test::new();
    test.kernel
        .set_secret(paperpaw_core::sdk::secrets::SecretChange::keep(
            "core.bluetooth.5",
            "Remote",
            "aa:bb\tRemote\tremote\t0\tpulse\tkeys",
        ));
    // Read at start, as after a restart.
    let mut test = {
        let storage_secrets = test.kernel.secrets().encode();
        let queue: Queue = Rc::default();
        let log: Log = Rc::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new().with(paperpaw_core::sdk::secrets::FILE, storage_secrets))
            .bluetooth(2)
            .pulse(Pulse {
                id: "pulse",
                uses: Uses::BLUETOOTH,
                queue: queue.clone(),
                log: log.clone(),
            })
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.render(&mut |_, _| {});
        test = Test {
            kernel,
            queue,
            log,
            now: 0,
        };
        assert!(
            test.calls()
                .iter()
                .any(|c| matches!(c, BtCall::Bonds(b) if b.len() == 1))
        );
        test
    };
    test.run(GO, [bluetooth::remote_keys(DeviceId::new(5), &LINK)]);
    let link = test
        .calls()
        .into_iter()
        .find_map(|c| match c {
            BtCall::Connect { link, address, .. } if address == "aa:bb" => Some(link),
            _ => None,
        })
        .expect("the remote connected");
    test.report(BtReport::Connected {
        link,
        mtu: 23,
        security: Security::Encrypted,
    });
    assert!(test.calls().contains(&BtCall::Hid { link, on: true }));
    test.heard();
    // Page down, pressed and let go: a page turned.
    for keys in [[0x4e, 0], [0, 0]] {
        test.report(BtReport::HidReport {
            link,
            report: None,
            bytes: vec![0, 0, keys[0], keys[1], 0, 0, 0, 0],
        });
    }
    let heard = test.heard();
    assert!(
        heard
            .iter()
            .any(|h| *h == format!("home action {}", actions::NEXT_PAGE)),
        "{heard:?}"
    );
}

/// Adds the fixtures that keep their fields (`TestPlugin`, `ServicePlugin`) to a builder.
trait AddFixture {
    fn pulse(self, plugin: Pulse) -> Self;
}

impl AddFixture for paperpaw_core::KernelBuilder {
    fn pulse(self, plugin: Pulse) -> Self {
        self.items(plugin.manifest(), pulse(plugin))
    }
}
