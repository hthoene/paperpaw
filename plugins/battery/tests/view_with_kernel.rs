//! The battery plugin with the real kernel: what it records in its log
//! (a start, a line per measurement, an end on sleep; nothing when
//! switched off; a new log when it grows too large) and what its view
//! shows with no battery, no log, one reading, a hostile log and a log of
//! an hour's reading.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel, WifiEvent};
use paperpaw_platform::{
    Battery as Charge, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat,
    Rotation, SystemStatus,
};
use paperpaw_plugin_battery::Battery;
use paperpaw_sdk::wifi::WifiState;
use paperpaw_sdk::{Env, Id, MemoryStorage};

/// An X4 as the emulator's profile has it: buttons, no touch.
const X4: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const HEADER: &str =
    "kind,ms,millivolts,percent,charging,fast,partial,full,kilopixels,inputs,panel_ms,pages,unix\n";

fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir("battery"))
}

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(storage: MemoryStorage, config: &str) -> Self {
        let kernel = Kernel::builder(X4)
            .wifi(true)
            .storage(storage)
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Battery)
            .build()
            .unwrap();
        let mut device = Self { kernel, now: 0 };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    /// The battery measured at `percent` and `millivolts`, a minute on.
    fn measure(&mut self, percent: u8, millivolts: u16, charging: bool) {
        self.now += 60_000;
        let status =
            SystemStatus::new(None, Some(Charge::new(percent, charging, Some(millivolts))));
        let effects = self.kernel.set_status(Instant(self.now), status);
        self.settle(effects);
    }

    /// The radio goes on or off, `after` milliseconds on.
    fn radio(&mut self, after: u64, on: bool) {
        self.now += after;
        let state = if on { WifiState::On } else { WifiState::Off };
        let effects = self
            .kernel
            .wifi_event(Instant(self.now), WifiEvent::State(state));
        self.settle(effects);
    }

    /// The radio's row in the last note of `network.csv`: its time on and
    /// its starts.
    fn radio_row(&self) -> (u64, u32) {
        let log = self.log("network.csv");
        let row = log
            .lines()
            .rev()
            .find(|l| l.split(',').nth(1) == Some("radio"))
            .unwrap_or_else(|| panic!("no row of the radio in {log}"));
        let cells: Vec<&str> = row.split(',').collect();
        (cells[5].parse().unwrap(), cells[6].parse().unwrap())
    }

    fn log(&self, name: &str) -> String {
        let data = paperpaw_sdk::read_file(self.kernel.storage(), &file(name), 1 << 20)
            .unwrap_or_default();
        String::from_utf8(data).unwrap()
    }

    /// The view's rows, as they say.
    fn view(&mut self) -> Vec<String> {
        self.now += 100;
        let effects = self
            .kernel
            .run_action(Instant(self.now), Id::new_static("battery.open"));
        self.settle(effects);
        assert_eq!(
            self.kernel.active_screen().map(Id::as_str),
            Some("battery.screen")
        );
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }
}

fn has(rows: &[String], text: &str) -> bool {
    rows.iter().any(|r| r.contains(text))
}

#[test]
fn a_time_awake_is_recorded_from_start_to_sleep() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.measure(90, 4100, false);
    device.measure(89, 4090, false);
    device.now += 100;
    let effects = device.kernel.suspend(Instant(device.now));
    device.settle(effects);
    let log = device.log("log.csv");
    let kinds: Vec<&str> = log.lines().map(|l| l.split(',').next().unwrap()).collect();
    assert_eq!(kinds, ["kind", "start", "sample", "sample", "end"], "{log}");
    assert!(log.starts_with(HEADER));
    assert!(log.contains(",4090,89,0,"), "{log}");
    // What each plugin took, at sleep.
    assert!(device.log("plugins.csv").starts_with("ms,plugin,"));
}

#[test]
fn nothing_is_recorded_when_switched_off() {
    let mut device = Device::new(MemoryStorage::new(), "[battery]\nenabled = off\n");
    device.measure(90, 4100, false);
    assert!(!device.kernel.storage().exists(&file("log.csv")));
}

#[test]
fn a_log_grown_too_large_becomes_the_old_one() {
    let mut big = String::from(HEADER);
    while big.len() < 128 * 1024 {
        big.push_str("sample,1000,4000,90,0,0,0,0,0,0,0,0,\n");
    }
    let storage = MemoryStorage::new()
        .with(&file("log.csv"), big.as_str())
        .with(&file("log.old.csv"), "older still");
    let device = Device::new(storage, "");
    assert_eq!(device.log("log.old.csv"), big);
    let log = device.log("log.csv");
    assert!(log.starts_with(HEADER), "{log}");
    assert_eq!(log.lines().count(), 2, "{log}");
}

#[test]
fn without_a_battery_the_view_says_so() {
    let mut device = Device::new(MemoryStorage::new(), "");
    let rows = device.view();
    assert!(has(
        &rows,
        "This device does not say how full its battery is."
    ));
    assert!(has(&rows, "Too little recorded yet"));
    assert!(!has(&rows, "Recent times awake"));
}

#[test]
fn one_reading_shows_the_charge_and_too_little_for_the_rest() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.measure(87, 4012, true);
    let rows = device.view();
    assert!(has(&rows, "87 % · 4.01 V"), "{rows:?}");
    assert!(has(&rows, "Charging"), "{rows:?}");
    assert!(has(&rows, "87 % to 87 %"), "{rows:?}");
    assert!(has(&rows, "Too little recorded yet"), "{rows:?}");
}

/// The voltage is shown with two decimals, rounded; from x.995 V on the
/// rounding carries into the hundredths and the volts are not raised.
#[test]
fn the_voltage_rounds_up_into_the_next_volt() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.measure(87, 3999, false);
    let rows = device.view();
    assert!(has(&rows, "87 % · 4.00 V"), "{rows:?}");
}

/// A log of an hour on battery: 90 % to 85 %, 100 pages.
fn an_hour() -> MemoryStorage {
    let mut log = String::from(HEADER);
    log.push_str("start,0,4100,90,0,0,0,0,0,0,0,0,\n");
    for minute in 1..=60u32 {
        let percent = 90 - minute / 12;
        log.push_str(&format!(
            "sample,{},4000,{percent},0,0,0,0,0,0,0,{},\n",
            minute * 60_000,
            minute * 5 / 3
        ));
    }
    log.push_str("end,3600000,4000,85,0,0,0,0,0,0,0,100,\n");
    MemoryStorage::new().with(&file("log.csv"), log)
}

/// The texts after `title` in the view (a value is drawn before its
/// title).
fn value(rows: &[String], title: &str) -> String {
    let at = rows.iter().position(|r| r == title).unwrap();
    rows[at - 1].clone()
}

#[test]
fn an_hour_of_reading_on_battery_gives_the_pace() {
    let mut device = Device::new(an_hour(), "");
    device.measure(80, 3950, false);
    // The lines are on the pages the screen's size makes of the view.
    let rows = every_page(&mut device);
    assert_eq!(value(&rows, "An hour awake"), "5.0 %", "{rows:?}");
    assert_eq!(value(&rows, "100 pages"), "5.0 %", "{rows:?}");
}

/// The view and what the buttons show of it (turning its pages) on the
/// X4's screen, with a time awake going on.
fn every_page(device: &mut Device) -> Vec<String> {
    let mut seen = device.view();
    for action in ["core.next-page", "core.down", "core.next"] {
        device.now += 100;
        let effects = device
            .kernel
            .run_action(Instant(device.now), Id::new_static(action));
        device.settle(effects);
        device.kernel.render(&mut |_, _| {});
        seen.extend(device.kernel.describe().into_iter().map(|p| p.label));
    }
    seen
}

/// Every line of the view can be reached: where they do not fit, the view
/// is turned page by page. (It used to draw its lines until the screen
/// ended, so the hours left and the recent times awake could not be seen
/// on the X4.)
#[test]
fn every_line_of_the_view_can_be_seen() {
    let mut device = Device::new(an_hour(), "");
    device.measure(80, 3950, false);
    let seen = every_page(&mut device);
    assert!(has(&seen, "Left at this pace"), "{seen:?}");
    assert!(has(&seen, "Recent times awake"), "{seen:?}");
    assert!(has(&seen, "1 h 0 min · 100 pages"), "{seen:?}");
}

/// The hours left at this pace show whole beside their title on the X4's
/// screen (they were cut to "about 16 h aw..." while they said "awake").
#[test]
fn the_hours_left_are_shown_whole() {
    let mut device = Device::new(an_hour(), "");
    device.measure(80, 3950, false);
    let seen = every_page(&mut device);
    assert!(seen.iter().any(|r| r.ends_with("about 16 h")), "{seen:?}");
}

#[test]
fn a_hostile_log_shows_a_view_without_panicking() {
    let mut log = String::from("garbage\n\u{0}\u{1}\n");
    log.push_str("start,0,,,0,0,0,0,0,0,0,0,\n");
    log.push_str(
        "sample,99999999999999,65535,255,0,4294967295,4294967295,4294967295,0,0,0,4294967295,\n",
    );
    log.push_str("end,5,1,0,1,0,0,0,0,0,0,0,-9223372036854775808\n");
    log.push_str("sample,,,,,,,,,,,,\n");
    let storage = MemoryStorage::new()
        .with(&file("log.csv"), log)
        .with(&file("log.old.csv"), b"\xff\xfe\x00binary".to_vec());
    let mut device = Device::new(storage, "");
    device.measure(0, 0, false);
    let rows = device.view();
    assert!(has(&rows, "0 % · 0.00 V"), "{rows:?}");
}

/// Makes a request each time its action runs.
struct Fetcher;

impl paperpaw_sdk::Plugin for Fetcher {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("net"),
            "Fetcher",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
        .description("")
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_action(
            paperpaw_sdk::ActionSpec::new(Id::new_static("net.go"), "go").handler(|_, _, out| {
                out.push(paperpaw_sdk::Command::http(paperpaw_sdk::HttpRequest::get(
                    "https://example.org/",
                    Id::new_static("net.answered"),
                )));
            }),
        )
    }
}

#[test]
fn the_view_shows_what_each_plugin_did_with_the_network() {
    let kernel = Kernel::builder(X4)
        .storage(MemoryStorage::new())
        .user_config("[core]\nlanguage = en\n")
        .network(true)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Battery)
        .plugin(Fetcher)
        .build()
        .unwrap();
    let mut device = Device { kernel, now: 0 };
    let effects = device.kernel.start(Instant(0));
    device.settle(effects);
    for _ in 0..2 {
        device.now += 100;
        let effects = device
            .kernel
            .run_action(Instant(device.now), Id::new_static("net.go"));
        device.settle(effects);
    }
    // The reader switches its network off: the next request is held back.
    device.now += 100;
    device
        .kernel
        .set_setting(Instant(device.now), "core.network.net", "off")
        .unwrap();
    device.now += 100;
    let effects = device
        .kernel
        .run_action(Instant(device.now), Id::new_static("net.go"));
    device.settle(effects);
    let rows = every_page(&mut device);
    assert!(has(&rows, "Network this time awake"), "{rows:?}");
    assert!(has(&rows, "2 requests"), "{rows:?}");
    assert!(has(&rows, "1 held back · network off"), "{rows:?}");
}

#[test]
fn the_radios_time_on_and_its_starts_are_noted_with_the_plugins_network_use() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.measure(90, 4100, false);
    device.radio(1_000, true);
    device.radio(30_000, false);
    device.radio(10_000, true);
    device.radio(5_000, false);
    device.now += 100;
    let effects = device.kernel.suspend(Instant(device.now));
    device.settle(effects);
    assert!(
        device
            .log("network.csv")
            .starts_with("ms,plugin,requests,refused,awake_ms,radio_ms,radio_starts\n")
    );
    // 30 s and 5 s on, in two starts.
    assert_eq!(
        device.radio_row(),
        (35_000, 2),
        "{}",
        device.log("network.csv")
    );
}

#[test]
fn a_radio_on_at_sleep_stops_counting_there_and_starts_again_on_waking() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.measure(90, 4100, false);
    device.radio(0, true);
    device.now += 20_000;
    let effects = device.kernel.suspend(Instant(device.now));
    device.settle(effects);
    assert_eq!(device.radio_row(), (20_000, 1));
    // Asleep for an hour; the radio's state is still "on" when it wakes,
    // as the shell has not said otherwise.
    device.now += 3_600_000;
    let effects = device.kernel.resume(Instant(device.now));
    device.settle(effects);
    device.measure(89, 4090, false);
    device.now += 5_000;
    let effects = device.kernel.suspend(Instant(device.now));
    device.settle(effects);
    let (on, starts) = device.radio_row();
    assert_eq!(starts, 2, "{}", device.log("network.csv"));
    assert!((20_000 + 60_000..20_000 + 70_000).contains(&on), "{on}");
}

#[test]
fn a_radio_never_on_is_noted_as_zero() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.measure(90, 4100, false);
    device.now += 100;
    let effects = device.kernel.suspend(Instant(device.now));
    device.settle(effects);
    assert_eq!(device.radio_row(), (0, 0));
}
