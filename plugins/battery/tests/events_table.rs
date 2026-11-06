//! What Battery does with each event it reacts to, as a table: the events of
//! the host (start-up, battery measured, suspend, resume, stopping, a page
//! turned) and the setting that switches it off, in each order that matters,
//! and the lines of the log that result. The rows are the acceptance table of
//! the interface-7 port for events (`PORTING.md`); they pin today's behaviour,
//! including what looks odd (rows marked `odd`), so that a port changes it only
//! on purpose.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{
    Battery as Charge, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat,
    Rotation, SystemStatus,
};
use paperpaw_plugin_battery::Battery;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{ActionSpec, Id, Manifest, MemoryStorage, Plugin, RegisterError, Registrar};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "battery-events",
    "Battery events table",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);
const LOG: &str = ".paperpaw/plugins/battery/log.csv";
const TURN: Id = Id::new_static("fixture.turn-page");

struct Pages;
impl Plugin for Pages {
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

/// What happens to the device, at a time in milliseconds since it started.
#[derive(Debug, Clone, Copy)]
enum Step {
    Measure(u64),
    Suspend(u64),
    Resume(u64),
    Stop(u64),
    Turn(u64),
    Enabled(bool),
}
use Step::{Enabled, Measure, Resume, Stop, Suspend, Turn};

struct Row {
    name: &'static str,
    /// `[battery] enabled` at start-up.
    enabled: bool,
    /// The start that ends a sleep: STARTED and RESUME in one call.
    after_sleep: bool,
    steps: &'static [Step],
    /// The `kind` column of the log, in order (empty: no log at all).
    kinds: &'static [&'static str],
    /// The `pages` column, in order.
    pages: &'static [&'static str],
}

const ROWS: &[Row] = &[
    Row {
        name: "start-up alone writes a start",
        enabled: true,
        after_sleep: false,
        steps: &[],
        kinds: &["start"],
        pages: &["0"],
    },
    Row {
        name: "a start that ends a sleep is told twice and written once",
        enabled: true,
        after_sleep: true,
        steps: &[],
        kinds: &["start"],
        pages: &["0"],
    },
    Row {
        name: "resume while awake writes nothing",
        enabled: true,
        after_sleep: false,
        steps: &[Resume(100)],
        kinds: &["start"],
        pages: &["0"],
    },
    Row {
        name: "a measurement writes a sample",
        enabled: true,
        after_sleep: false,
        steps: &[Measure(60_000)],
        kinds: &["start", "sample"],
        pages: &["0", "0"],
    },
    Row {
        name: "suspend ends the time awake, resume starts the next",
        enabled: true,
        after_sleep: false,
        steps: &[Suspend(1_000), Resume(5_000)],
        kinds: &["start", "end", "start"],
        pages: &["0", "0", "0"],
    },
    Row {
        name: "stopping ends the time awake",
        enabled: true,
        after_sleep: false,
        steps: &[Stop(1_000)],
        kinds: &["start", "end"],
        pages: &["0", "0"],
    },
    Row {
        name: "a measurement after sleep starts a time awake first",
        enabled: true,
        after_sleep: false,
        steps: &[Suspend(1_000), Measure(60_000)],
        kinds: &["start", "end", "start", "sample"],
        pages: &["0", "0", "0", "0"],
    },
    Row {
        name: "odd: suspend twice writes two ends",
        enabled: true,
        after_sleep: false,
        steps: &[Suspend(1_000), Suspend(2_000)],
        kinds: &["start", "end", "end"],
        pages: &["0", "0", "0"],
    },
    Row {
        name: "pages turned are counted until the next line, across sleep",
        enabled: true,
        after_sleep: false,
        steps: &[
            Turn(100),
            Turn(200),
            Measure(60_000),
            Suspend(61_000),
            Turn(62_000),
            Resume(63_000),
            Measure(120_000),
        ],
        kinds: &["start", "sample", "end", "start", "sample"],
        pages: &["0", "2", "2", "3", "3"],
    },
    Row {
        name: "off in the settings: no log at all",
        enabled: false,
        after_sleep: false,
        steps: &[
            Measure(60_000),
            Suspend(61_000),
            Resume(62_000),
            Stop(63_000),
        ],
        kinds: &[],
        pages: &[],
    },
    Row {
        name: "switched off while awake: nothing, then on: the awake state is kept",
        enabled: true,
        after_sleep: false,
        steps: &[
            Enabled(false),
            Measure(60_000),
            Suspend(61_000),
            Resume(62_000),
            Enabled(true),
            Measure(120_000),
        ],
        kinds: &["start", "sample"],
        pages: &["0", "0"],
    },
    Row {
        name: "odd: off at start-up, then on: the first measurement writes start and sample",
        enabled: false,
        after_sleep: false,
        steps: &[Enabled(true), Measure(60_000)],
        kinds: &["start", "sample"],
        pages: &["0", "0"],
    },
];

fn column(log: &str, name: &str) -> Vec<String> {
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

fn play(row: &Row) -> Option<String> {
    let config = format!(
        "[core]\nlanguage = en\n[battery]\nenabled = {}\n",
        if row.enabled { "on" } else { "off" }
    );
    let mut kernel: Kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .user_config(config)
        .after_sleep(row.after_sleep)
        .plugin(Pages)
        .plugin(Battery)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    for step in row.steps {
        match *step {
            Measure(at) => {
                kernel.set_status(
                    Instant(at),
                    SystemStatus::new(None, Some(Charge::new(80, false, None))),
                );
            }
            Suspend(at) => {
                kernel.suspend(Instant(at));
            }
            Resume(at) => {
                kernel.resume(Instant(at));
            }
            Stop(at) => {
                kernel.stop(Instant(at));
            }
            Turn(at) => {
                kernel.run_action(Instant(at), TURN);
            }
            Enabled(on) => {
                kernel
                    .set_setting(Instant(1), "battery.enabled", if on { "on" } else { "off" })
                    .unwrap();
            }
        }
    }
    kernel.storage().exists(LOG).then(|| {
        String::from_utf8(paperpaw_sdk::read_file(kernel.storage(), LOG, 1 << 20).unwrap()).unwrap()
    })
}

#[test]
fn every_row_writes_these_lines() {
    for row in ROWS {
        let log = play(row);
        let (kinds, pages) = match &log {
            Some(log) => (column(log, "kind"), column(log, "pages")),
            None => (Vec::new(), Vec::new()),
        };
        assert_eq!(kinds, row.kinds, "{}: kinds", row.name);
        assert_eq!(pages, row.pages, "{}: pages", row.name);
    }
}

/// Where the device does not say how full its battery is, no reading event
/// comes: the log has the start line and nothing else (its battery fields
/// empty), however often the status is set.
#[test]
fn without_a_battery_there_is_a_start_line_and_no_samples() {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .user_config("[core]\nlanguage = en\n".to_string())
        .plugin(Battery)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    for at in [60_000, 120_000] {
        kernel.set_status(Instant(at), SystemStatus::new(None, None));
    }
    let log = String::from_utf8(paperpaw_sdk::read_file(kernel.storage(), LOG, 1 << 20).unwrap())
        .unwrap();
    assert_eq!(column(&log, "kind"), ["start"]);
    assert_eq!(column(&log, "millivolts"), [""]);
    assert_eq!(column(&log, "percent"), [""]);
    assert_eq!(column(&log, "charging"), ["0"]);
}
