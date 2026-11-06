//! Observable event/settings/storage fixtures for comparing the flat-ABI port.
//!
//! Keep the assertions at the kernel boundary: no Battery-private state or
//! guessed wire records. Usage timing fields are deliberately not golden data.
#![allow(
    clippy::unwrap_used,
    reason = "unexpected fixture failures must fail tests"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{Battery as Charge, Rotation, SystemStatus};
use paperpaw_plugin_battery::Battery;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{
    ActionSpec, DeviceInfo, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar,
};

const LOG: &str = ".paperpaw/plugins/battery/log.csv";
const PLUGINS_LOG: &str = ".paperpaw/plugins/battery/plugins.csv";
const TURN: Id = Id::new_static("fixture.turn-page");
const DEVICE: DeviceInfo = DeviceInfo::new(
    "battery-contract",
    "Battery contract fixture",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// A real typed page event emitted by an action, as a reader would emit it.
struct PageEvents;
impl Plugin for PageEvents {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("fixture"),
            "Fixture",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(TURN, "fixture.turn").handler(|_, _, out| {
            out.push(events::PAGE_TURNED.emit(events::Reading::new(
                "books/fixture.txt".into(),
                "Fixture".into(),
                50.0,
                0,
                12,
                String::new(),
            )));
        }))
    }
}

fn kernel(after_sleep: bool) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .user_config("[core]\nlanguage = en\n")
        .after_sleep(after_sleep)
        .plugin(PageEvents)
        .plugin(Battery)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn text(kernel: &Kernel, path: &str) -> String {
    String::from_utf8(paperpaw_sdk::read_file(kernel.storage(), path, 1 << 20).unwrap()).unwrap()
}

/// Column identities come from the public CSV header, not internal structs.
fn column(kernel: &Kernel, name: &str) -> Vec<String> {
    let log = text(kernel, LOG);
    let mut lines = log.lines();
    let index = lines
        .next()
        .unwrap()
        .split(',')
        .position(|field| field == name)
        .unwrap();
    lines
        .map(|line| line.split(',').nth(index).unwrap().to_owned())
        .collect()
}

fn measure(kernel: &mut Kernel, at: u64) {
    kernel.set_status(
        Instant(at),
        SystemStatus::new(None, Some(Charge::new(80, false, None))),
    );
}

#[test]
fn startup_after_sleep_and_duplicate_resume_write_only_one_start() {
    let mut kernel = kernel(true);
    // after_sleep emits STARTED and RESUME during the same start call.
    assert_eq!(column(&kernel, "kind"), ["start"]);
    kernel.resume(Instant(100));
    assert_eq!(column(&kernel, "kind"), ["start"]);
    measure(&mut kernel, 60_000);
    assert_eq!(column(&kernel, "kind"), ["start", "sample"]);
    assert_eq!(column(&kernel, "millivolts"), ["", ""]);
    assert_eq!(column(&kernel, "percent"), ["", "80"]);
}

#[test]
fn sleep_and_restart_end_the_session_and_resume_starts_the_next() {
    let mut kernel = kernel(false);
    measure(&mut kernel, 60_000);
    kernel.suspend(Instant(61_000));
    kernel.resume(Instant(120_000));
    measure(&mut kernel, 180_000);
    kernel.stop(Instant(181_000));
    assert_eq!(
        column(&kernel, "kind"),
        ["start", "sample", "end", "start", "sample", "end"]
    );
    assert_eq!(
        column(&kernel, "ms"),
        ["0", "60000", "61000", "120000", "180000", "181000"]
    );
}

#[test]
fn runtime_setting_disables_writes_without_losing_the_awake_session() {
    let mut kernel = kernel(false);
    let before = text(&kernel, LOG);
    kernel
        .set_setting(Instant(100), "battery.enabled", "off")
        .unwrap();
    measure(&mut kernel, 60_000);
    kernel.suspend(Instant(61_000));
    kernel.resume(Instant(120_000));
    assert_eq!(text(&kernel, LOG), before);
    assert!(!kernel.storage().exists(PLUGINS_LOG));
    kernel
        .set_setting(Instant(120_100), "battery.enabled", "on")
        .unwrap();
    measure(&mut kernel, 180_000);
    // Disabled suspend did not end Battery's own awake state.
    assert_eq!(column(&kernel, "kind"), ["start", "sample"]);
}

#[test]
fn typed_page_events_are_counted_across_sleep_and_recorded_on_measurement() {
    let mut kernel = kernel(false);
    for at in [100, 200] {
        kernel.run_action(Instant(at), TURN);
    }
    measure(&mut kernel, 60_000);
    kernel.suspend(Instant(61_000));
    kernel.resume(Instant(120_000));
    kernel.run_action(Instant(120_100), TURN);
    measure(&mut kernel, 180_000);
    assert_eq!(column(&kernel, "pages"), ["0", "2", "2", "2", "3"]);
}

#[test]
fn plugin_usage_log_is_rate_limited_but_always_written_at_session_end() {
    let mut kernel = kernel(false);
    measure(&mut kernel, 60_000);
    let first = text(&kernel, PLUGINS_LOG);
    assert!(first.starts_with("ms,plugin,busy_ms,calls,shown_ms,"));
    assert!(first.lines().any(|line| line.starts_with("60000,battery,")));
    measure(&mut kernel, 120_000);
    assert_eq!(text(&kernel, PLUGINS_LOG), first);
    measure(&mut kernel, 660_000);
    let due = text(&kernel, PLUGINS_LOG);
    assert!(due.starts_with(&first));
    assert!(due.lines().any(|line| line.starts_with("660000,battery,")));
    kernel.suspend(Instant(661_000));
    let ended = text(&kernel, PLUGINS_LOG);
    assert!(ended.starts_with(&due));
    assert!(
        ended
            .lines()
            .any(|line| line.starts_with("661000,battery,"))
    );
}
