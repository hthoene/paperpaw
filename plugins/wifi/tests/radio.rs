//! When the radio goes on and off, with the real kernel: a plugin's
//! request wants the network, no network joined before is in reach, and
//! the radio does not stay on for work that has already failed. And the
//! plugin's translations.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Io, Kernel, WifiEvent};
use paperpaw_platform::Rotation;
use paperpaw_plugin_wifi::Wifi;
use paperpaw_sdk::builtin::slots;
use paperpaw_sdk::secrets::{FILE, Secrets};
use paperpaw_sdk::wifi::{self, JoinFailure, WifiCommand, WifiNetwork, WifiState};
use paperpaw_sdk::{
    Command, DeviceInfo, DisplayInfo, HttpRequest, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, Params, PixelFormat, Plugin, Priority, RegisterError, Registrar, ScreenSpec,
    Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(200, 300, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// Asked for, the probe makes a request, which wants the network.
const FETCH: Id = Id::new_static("probe.fetch");
const REPLY: Id = Id::new_static("probe.reply");
/// Asked for, the probe opens its tool, or goes home.
const OPEN_TOOL: Id = Id::new_static("probe.open-tool");
const GO_HOME: Id = Id::new_static("probe.go-home");
const TOOL: Id = Id::new_static("probe.tool");
const HOME: Id = Id::new_static("probe.home");

/// A screen that shows nothing.
struct Blank;

impl paperpaw_sdk::Screen for Blank {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            HOME,
            "home",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.add_screen(ScreenSpec::new(
            TOOL,
            "tool",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)?;
        r.subscribe(
            OPEN_TOOL,
            Box::new(|_, _, out| out.push(Command::OpenScreen(TOOL, Params::new()))),
        )?;
        r.subscribe(
            GO_HOME,
            Box::new(|_, _, out| out.push(Command::open_slot(slots::HOME))),
        )?;
        r.subscribe(
            FETCH,
            Box::new(|_, _, out| {
                let request = HttpRequest::new("GET", "http://example.org/", REPLY);
                out.push(Command::http(request));
            }),
        )
    }
}

struct Test {
    kernel: Kernel,
    at: u64,
}

impl Test {
    /// A device that joined `Home` before, its radio off.
    fn new() -> Self {
        let mut secrets = Secrets::default();
        secrets.apply(wifi::keep("Home", "pw"));
        let storage = MemoryStorage::new().with(FILE, secrets.encode().into_bytes());
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .network(true)
            .wifi(true)
            .plugin(Wifi)
            .plugin(Probe)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut test = Test { kernel, at: 0 };
        test.radio();
        test
    }

    /// `seconds` later.
    fn later(&mut self, seconds: u64) -> Instant {
        self.at += seconds * 1000;
        Instant(self.at)
    }

    /// What the radio is told to do.
    fn radio(&mut self) -> Vec<WifiCommand> {
        let io = self.kernel.take_io().into_iter();
        io.filter_map(|io| match io {
            Io::Wifi(command) => Some(command),
            _ => None,
        })
        .collect()
    }

    fn event(&mut self, event: WifiEvent) -> Vec<WifiCommand> {
        let now = self.later(1);
        self.kernel.wifi_event(now, event);
        self.radio()
    }

    /// What the radio is told in the next `seconds`, with the kernel's
    /// timers run each second and the radio doing as told (see
    /// [`Test::answer`]).
    fn wait(&mut self, seconds: u64, in_reach: &[WifiNetwork]) -> Vec<WifiCommand> {
        let mut told = Vec::new();
        for _ in 0..seconds {
            let now = self.later(1);
            self.kernel.tick(now);
            let commands = self.radio();
            told.extend(self.answer(commands, in_reach));
        }
        told
    }

    /// The radio does as told: `in_reach` is what a scan finds, and
    /// joining fails. Returns `commands` and what the radio is told then.
    fn answer(
        &mut self,
        mut commands: Vec<WifiCommand>,
        in_reach: &[WifiNetwork],
    ) -> Vec<WifiCommand> {
        let mut told = Vec::new();
        commands.reverse();
        while let Some(command) = commands.pop() {
            let answer = match &command {
                WifiCommand::Scan => WifiEvent::Scanned(in_reach.to_vec()),
                WifiCommand::Join { ssid, .. } => WifiEvent::Failed {
                    ssid: ssid.clone(),
                    reason: JoinFailure::Timeout,
                },
                WifiCommand::Off => WifiEvent::State(WifiState::Off),
                WifiCommand::On => WifiEvent::State(WifiState::On),
                _ => continue,
            };
            told.push(command);
            let mut next = self.event(answer);
            next.reverse();
            commands.extend(next);
        }
        told
    }

    /// The probe is asked for `what`; what the radio is told then.
    fn ask(&mut self, what: Id) -> Vec<WifiCommand> {
        let now = self.later(1);
        self.kernel.send_event(now, what, Params::new());
        self.radio()
    }

    /// A plugin wants the network: the radio comes on and looks for
    /// networks.
    fn want(&mut self) {
        let now = self.later(1);
        self.kernel.send_event(now, FETCH, Params::new());
        assert_eq!(self.radio(), [WifiCommand::On]);
        assert_eq!(
            self.event(WifiEvent::State(WifiState::On)),
            [WifiCommand::Scan]
        );
    }
}

#[test]
fn translations_are_complete() {
    use paperpaw_i18n::{Catalog, check};
    let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn the_radio_goes_off_when_no_known_network_is_in_reach() {
    let mut test = Test::new();
    test.want();
    // None in reach: looked for again a few times, then the request
    // fails.
    let mut scans = 1;
    while test.event(WifiEvent::Scanned(Vec::new())) == [WifiCommand::Scan] {
        scans += 1;
        assert!(scans < 10, "looks for ever");
    }
    assert_eq!(scans, 3);
    // Nothing waits any more: no more rounds, and the radio goes off.
    let told = test.wait(5 * 60, &[]);
    assert_eq!(told, [WifiCommand::Off]);
}

/// The first scan after the radio came on often finds nothing (an X4's
/// radio, just started): it is made again, and the network found then
/// joined, without the request failing meanwhile.
#[test]
fn a_scan_that_finds_nothing_is_made_again() {
    let home = [WifiNetwork::new(String::from("Home"), -50, true)];
    let mut test = Test::new();
    test.want();
    assert_eq!(
        test.event(WifiEvent::Scanned(Vec::new())),
        [WifiCommand::Scan]
    );
    let joins = test.event(WifiEvent::Scanned(home.to_vec()));
    assert!(matches!(joins[..], [WifiCommand::Join { .. }]), "{joins:?}");
}

#[test]
fn the_radio_goes_off_once_joining_failed_for_as_long_as_work_waits() {
    let home = [WifiNetwork::new(String::from("Home"), -50, true)];
    let mut test = Test::new();
    test.want();
    let joins = test.event(WifiEvent::Scanned(home.to_vec()));
    assert!(matches!(joins[..], [WifiCommand::Join { .. }]), "{joins:?}");
    // Tried again a while, as long as the request waits; then no more,
    // and the radio goes off.
    let mut told = test.answer(joins, &home);
    told.extend(test.wait(5 * 60, &home));
    let rounds = told.iter().filter(|c| **c == WifiCommand::Scan).count();
    assert!((1..=3).contains(&rounds), "{told:?}");
    assert_eq!(told.last(), Some(&WifiCommand::Off), "{told:?}");
    assert_eq!(told.iter().filter(|c| **c == WifiCommand::Off).count(), 1);
}

/// Back home from a tool that used the network, the radio goes off at
/// once, not after its idle time: its memory is free before a book opens.
#[test]
fn the_radio_goes_off_at_once_back_home() {
    let mut test = Test::new();
    test.ask(OPEN_TOOL);
    test.want();
    // None in reach: the request fails, nothing waits; the radio waits
    // for its idle time while the tool shows.
    while test.event(WifiEvent::Scanned(Vec::new())) == [WifiCommand::Scan] {}
    assert_eq!(test.wait(10, &[]), []);
    assert_eq!(test.ask(GO_HOME), [WifiCommand::Off]);
}

/// Home while the network is still wanted: the radio stays on.
#[test]
fn the_radio_stays_on_at_home_while_work_waits() {
    let mut test = Test::new();
    test.ask(OPEN_TOOL);
    test.want();
    assert_eq!(test.ask(GO_HOME), []);
}
