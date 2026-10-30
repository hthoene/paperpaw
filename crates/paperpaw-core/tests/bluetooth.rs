//! Bluetooth with the real kernel (ADR 0027): a plugin that declares it
//! scans, connects and subscribes; one that does not is refused; pairing
//! asks the reader in the kernel's own question, whose answer only counts
//! with the question's number, and the bond is kept with the secrets; a
//! bonded remote's keys run what the key maps bind; a closed screen and
//! sleep end what was asked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::sdk::bluetooth::{
    self, BtEvent, DeviceId, Handle, ScanPower, Security, Uuid, ValueKind,
};
use paperpaw_core::sdk::builtin::{actions, slots};
use paperpaw_core::sdk::replies::Confirmed;
use paperpaw_core::sdk::{
    ActionSpec, Canvas, Command, Cx, Env, Event, EventData, Handled, Id, Manifest, MemoryStorage,
    Params, Plugin, Priority, RegisterError, Registrar, Screen, ScreenSpec, Uses, Version,
};
use paperpaw_core::{BtCall, BtReport, Found, Io, Kernel, PairQuestion};
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
const ASK: Id = Id::new_static("pulse.ask");
const ANSWERED: &str = "core.bluetooth-pair-answered";

type Queue = Rc<RefCell<Vec<Command>>>;
type Log = Rc<RefCell<Vec<String>>>;

/// A screen that logs the actions it gets and the parameters it opened
/// with.
struct Logged {
    name: &'static str,
    log: Log,
}

impl Screen for Logged {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(format!("{} action {action}", self.name));
        Handled::No
    }
}

/// A plugin that runs what the test queues, and logs what it hears.
struct Pulse {
    id: &'static str,
    uses: Uses,
    queue: Queue,
    log: Log,
}

impl Plugin for Pulse {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.id), "Pulse", Version::new(1, 0, 0)).uses(self.uses)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let go = Id::parse(&format!("{}.go", self.id)).unwrap();
        let queue = self.queue.clone();
        r.add_action(ActionSpec::new(go, "go").handler(move |_, _, out| {
            out.extend(queue.borrow_mut().drain(..));
        }))?;
        for event in [FOUND, LINK, OTHER_FOUND] {
            if event.id().namespace() != self.id {
                continue;
            }
            let log = self.log.clone();
            r.on(event.clone(), move |_, heard: &BtEvent, _| {
                log.borrow_mut().push(format!("{}: {heard:?}", event.id()));
            })?;
        }
        if self.id == "pulse" {
            let log = self.log.clone();
            r.add_screen(ScreenSpec::new(
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
            let log = self.log.clone();
            r.add_screen(ScreenSpec::new(
                ASK,
                "ask",
                Box::new(move |_, params| {
                    let told: Vec<String> =
                        params.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    log.borrow_mut().push(format!("asked {}", told.join(" ")));
                    Box::new(Logged {
                        name: "ask",
                        log: log.clone(),
                    })
                }),
            ))?;
            r.bind_slot(slots::CONFIRM, ASK, Priority::DEFAULT)?;
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
            .plugin(plugin("pulse", Uses::BLUETOOTH))
            .plugin(plugin("other", Uses::NOTHING))
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
fn the_pairing_question_closes_when_the_link_drops() {
    let mut test = Test::new();
    let belt = test.belt();
    let link = test.link(belt);
    test.run(GO, [bluetooth::pair(belt)]);
    test.report(BtReport::Ask {
        link,
        question: PairQuestion::Show(123_456),
    });
    assert_eq!(test.kernel.active_screen(), Some(&ASK));
    test.report(BtReport::Disconnected { link, reason: None });
    assert_ne!(
        test.kernel.active_screen(),
        Some(&ASK),
        "the question closed"
    );
}

#[test]
fn pairing_asks_the_reader_and_keeps_the_bond() {
    let mut test = Test::new();
    let belt = test.belt();
    let link = test.link(belt);
    test.run(GO, [bluetooth::pair(belt)]);
    assert!(test.calls().contains(&BtCall::Pair { link }));
    test.report(BtReport::Ask {
        link,
        question: PairQuestion::Compare(123_456),
    });
    // The kernel asks, in its own words.
    let heard = test.heard();
    let asked = heard
        .iter()
        .find(|h| h.starts_with("asked"))
        .expect("asked");
    assert!(
        asked.contains("Pair with Belt?") && asked.contains("123456"),
        "{asked}"
    );
    let context = |nonce: &str| {
        let nonce = nonce.to_string();
        let mut context = Params::new().with("device", "1");
        context.set("nonce", nonce);
        Confirmed::new(true, context).to_params()
    };
    // An answer without the question's number does nothing.
    let at = test.at();
    test.kernel
        .send_event(at, Id::new_static(ANSWERED), context("1"));
    assert!(
        !test
            .calls()
            .iter()
            .any(|c| matches!(c, BtCall::Answer { .. }))
    );
    let nonce = asked
        .split(' ')
        .find_map(|p| p.strip_prefix("nonce="))
        .expect("its number");
    let at = test.at();
    test.kernel
        .send_event(at, Id::new_static(ANSWERED), context(nonce));
    assert!(
        test.calls()
            .contains(&BtCall::Answer { link, accept: true })
    );
    test.report(BtReport::Paired {
        link,
        keys: "ab12".into(),
    });
    assert!(test.heard().iter().any(|h| h.contains("Paired")));
    let kept = test
        .kernel
        .secrets()
        .get("core.bluetooth.1")
        .unwrap_or_default();
    assert!(
        kept.starts_with("11:22\tBelt\t") && kept.ends_with("\tpulse\tab12"),
        "{kept}"
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
            .plugin(Pulse {
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
