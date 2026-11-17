//! Pairing asks the reader in the kernel's own yes or no question (#293: on
//! the dialog plugin's slot), whose answer only counts with the question's
//! number; the bond is kept with the secrets; the question closes when the
//! link drops. (Moved from `tests/bluetooth.rs`: the question is the ui
//! store's, which a unit test wires.)
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::bluetooth::{self, BtEvent, DeviceId, Security, Uuid};
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::{
    ActionSpec, Command, Event, EventData, Id, Manifest, MemoryStorage, Params, RegisterError,
    Uses, Version,
};

use super::questions;
use crate::native_host::shared::Shared;
use crate::native_host::ui_store::UiStore;
use crate::{BtCall, BtReport, Found, Io, Items, Kernel, PairQuestion};

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
const ANSWERED: &str = "core.bluetooth-pair-answered";

extern crate std;

std::thread_local! {
    /// What the test queued for the plugin's action to run.
    static QUEUE: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
}

fn pulse(r: &mut Items) -> Result<(), RegisterError> {
    r.add_action(ActionSpec::new(GO, "go").handler(|_, _, out| {
        QUEUE.with(|queue| out.extend(queue.borrow_mut().drain(..)));
    }))?;
    r.subscribe(FOUND.id().clone(), |_, _| {})?;
    r.subscribe(LINK.id().clone(), |_, _| {})
}

struct Test {
    kernel: Kernel,
    store: Shared<UiStore>,
    now: u64,
}

impl Test {
    fn new() -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .user_config(String::from("[core]\nlanguage = en\n"))
            .bluetooth(2)
            .items(
                Manifest::new(Id::new_static("pulse"), "Pulse", Version::new(1, 0, 0))
                    .uses(Uses::BLUETOOTH),
                pulse,
            )
            .items(questions::serves_manifest(), questions::serves())
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let store = questions::wire(&mut kernel);
        kernel.render(&mut |_, _| {});
        let mut test = Self {
            kernel,
            store,
            now: 0,
        };
        test.calls();
        test
    }

    fn at(&mut self) -> Instant {
        self.now += 10;
        Instant(self.now)
    }

    fn run(&mut self, commands: impl IntoIterator<Item = Command>) {
        QUEUE.with(|queue| queue.borrow_mut().extend(commands));
        let at = self.at();
        self.kernel.run_action(at, GO);
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

    /// A belt in reach, found by the plugin's scan, linked: its device and link.
    fn linked_belt(&mut self) -> (DeviceId, u32) {
        self.run([bluetooth::scan(&FOUND).service(Uuid::sig(0x180D)).command()]);
        self.report(BtReport::Found(Found {
            address: "11:22".into(),
            name: Some("Belt".into()),
            signal: Some(-60),
            services: alloc::vec![Uuid::sig(0x180D)],
            ..Found::default()
        }));
        let belt = DeviceId::new(1);
        self.run([bluetooth::connect(belt, &LINK).command()]);
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
        (belt, link)
    }

    /// The number the open pairing question was asked with.
    fn nonce(&self) -> String {
        self.kernel
            .asks
            .context_of_first()
            .and_then(|context| context.get("nonce").map(ToString::to_string))
            .expect("a question with its number")
    }
}

#[test]
fn pairing_asks_the_reader_and_keeps_the_bond() {
    let mut test = Test::new();
    let (belt, link) = test.linked_belt();
    test.run([bluetooth::pair(belt)]);
    assert!(test.calls().contains(&BtCall::Pair { link }));
    test.report(BtReport::Ask {
        link,
        question: PairQuestion::Compare(123_456),
    });
    // The kernel asks, in its own words.
    let (kind, title, text, words) = questions::asked(&test.kernel, &test.store).expect("asked");
    assert_eq!(kind, 2);
    assert_eq!(title, "Pair with Belt?");
    assert!(text.contains("123456"), "{text}");
    assert!(!words.is_empty(), "the buttons' own words");
    let context = |nonce: &str| {
        let mut context = Params::new().with("device", "1");
        context.set("nonce", String::from(nonce));
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
    // Yes on the dialog: the answer with its number.
    let nonce = test.nonce();
    questions::answer(&mut test.kernel, &test.store, 1);
    assert!(
        test.calls()
            .contains(&BtCall::Answer { link, accept: true }),
        "answered with {nonce}"
    );
    test.report(BtReport::Paired {
        link,
        keys: "ab12".into(),
    });
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
fn the_pairing_question_closes_when_the_link_drops() {
    let mut test = Test::new();
    let (belt, link) = test.linked_belt();
    test.run([bluetooth::pair(belt)]);
    test.report(BtReport::Ask {
        link,
        question: PairQuestion::Show(123_456),
    });
    assert_eq!(test.kernel.active_screen(), Some(&questions::DIALOG));
    test.report(BtReport::Disconnected { link, reason: None });
    assert_ne!(
        test.kernel.active_screen(),
        Some(&questions::DIALOG),
        "the question closed"
    );
}
