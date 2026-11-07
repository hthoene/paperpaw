//! What Battery records, byte for byte, and what it does with each event: the
//! acceptance table for events, the golden files of the log and the usage log,
//! their rotation at the size cap, and a tail cut by a power loss. They run the
//! interface-7 recorder through the real native entry glue and a test-only
//! host (`support/native_host.rs`); they are the interface-6 plugin's tests
//! (`PORTING.md`) carried over, with their goldens unchanged except where
//! `PORTING.md` says otherwise.
//!
//! After a change meant to alter a golden, write it anew and look at each
//! before committing it:
//!
//! ```sh
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-plugin-battery --test native_goldens
//! ```
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "invalid fixture setup fails tests"
)]
#[path = "support/native_host.rs"]
mod host;
use std::path::{Path, PathBuf};

use paperpaw_core::native_host::{
    events::{
        PageTurn, encode_battery_measured, encode_page_turned, encode_resume, encode_started,
        encode_stopping, encode_suspend,
    },
    registrations::{Record, ServiceEnd, ServicePluginUsage},
};
use paperpaw_platform::Battery as Charge;
use paperpaw_plugin_battery::recording::Recorder;
use paperpaw_sdk::native::{Error, Host, Plugin, events::Event, reply::Reply};

#[derive(Default)]
struct Probe(Recorder);
impl Plugin for Probe {
    fn make(_: &Host<'_>) -> Result<Self, Error> {
        Ok(Self::default())
    }
    fn event(
        &mut self,
        host: &Host<'_>,
        _: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.0.event(host, event)
    }
}
// Existing generated SDK-entry metadata is adequate for this in-process probe.
paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../testdata/native-test-metadata.bin"),
    heap = none
);

const HEADER: &str =
    "kind,ms,millivolts,percent,charging,fast,partial,full,kilopixels,inputs,panel_ms,pages,unix\n";
const PLUGINS_HEADER: &str =
    "ms,plugin,busy_ms,calls,shown_ms,fast,partial,full,kilopixels,read_kb,written_kb\n";
const PLUGINS: &str = "plugins.csv";
const PLUGINS_OLD: &str = "plugins.old.csv";
const MAX_BYTES: usize = 128 * 1024;
/// Wednesday, 14 October 2026, 10:41 UTC, in milliseconds.
const UNIX_MS: i64 = 1_791_974_460_000;

/// A device in the test host: what the next event will find when it reads.
struct Device {
    h: host::Harness,
    battery: Option<Charge>,
    clock: bool,
    enabled: bool,
    /// The plugins that have used something, as the host lists them.
    plugins: &'static [&'static str],
    /// Whether the first full refresh (384 kilopixels) has been drawn.
    drawn: bool,
}

type Encoder = fn(u64, &mut [u8]) -> Result<usize, i32>;

impl Device {
    fn new(enabled: bool, clock: bool) -> Self {
        let entry = unsafe {
            // SAFETY: the SDK export is repr(transparent) over PpEntry; this is
            // the entry pointer supplied to native loaders.
            &*std::ptr::from_ref(&PAPERPAW_ENTRY).cast::<paperpaw_native::host_glue::PpEntry>()
        };
        Self {
            h: host::Harness::new(entry),
            battery: None,
            clock,
            enabled,
            plugins: &["core", "battery"],
            drawn: false,
        }
    }

    /// What the host reports at `at` milliseconds.
    fn at(&mut self, at: u64) {
        let unix = self
            .clock
            .then(|| (UNIX_MS + i64::try_from(at).unwrap()) / 1000);
        *self.h.context.environment.borrow_mut() =
            host::environment(at, self.battery, unix, self.enabled);
        let usage = if self.drawn {
            host::usage(1, 384_000)
        } else {
            host::usage(0, 0)
        };
        let mut services = self.h.context.services.borrow_mut();
        services.insert(
            "core.usage".into(),
            host::record(Record::ServiceUsage(usage)),
        );
        let mut rows = Vec::new();
        for plugin in self.plugins {
            rows.extend(host::record(Record::ServicePluginUsage(
                ServicePluginUsage {
                    plugin,
                    busy_us: 1_000,
                    calls: 1,
                    shown_ms: 0,
                    fast: 0,
                    partial: 0,
                    full: 0,
                    pixels: 0,
                    read: 0,
                    written: 0,
                    syncs: 0,
                    requests: 0,
                    refused: 0,
                    awake_ms: 0,
                    awake_reason: "",
                },
            )));
        }
        rows.extend(host::record(Record::ServiceEnd(ServiceEnd {
            flags: 0,
            total: u32::try_from(self.plugins.len()).unwrap(),
            generation: 1,
            next: u32::try_from(self.plugins.len()).unwrap(),
        })));
        services.insert("core.plugin-usage/from.0".into(), rows);
    }

    fn send(&mut self, at: u64, encode: Encoder) {
        self.at(at);
        let mut bytes = [0; 64];
        let len = encode(at, &mut bytes).unwrap();
        assert_eq!(self.h.dispatch(&bytes[..len]), 0);
    }

    fn start(&mut self, at: u64) {
        self.send(at, encode_started);
        self.drawn = true;
    }
    fn resume(&mut self, at: u64) {
        self.send(at, encode_resume);
    }
    fn suspend(&mut self, at: u64) {
        self.send(at, encode_suspend);
    }
    fn stop(&mut self, at: u64) {
        self.send(at, encode_stopping);
    }
    /// The status changes; where the device has a battery the host says so.
    fn status(&mut self, at: u64, battery: Option<Charge>) {
        self.battery = battery;
        self.at(at);
        if battery.is_some() {
            self.send(at, encode_battery_measured);
        }
    }
    fn turn(&mut self, at: u64) {
        self.at(at);
        let mut bytes = [0; 512];
        let len = encode_page_turned(
            &PageTurn {
                now_ms: at,
                section: 0,
                offset: 0,
                progress_ppm: 500_000,
                path: "books/fixture.txt",
                title: "Fixture",
                chapter: "",
                page: Some(12),
                pages: None,
            },
            &mut bytes,
        )
        .unwrap();
        assert_eq!(self.h.dispatch(&bytes[..len]), 0);
    }
    fn file(&self, name: &str) -> Option<String> {
        self.h
            .context
            .files
            .borrow()
            .get(name)
            .map(|bytes| String::from_utf8(bytes.clone()).unwrap())
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
    /// `battery.enabled` at start-up.
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
    let mut device = Device::new(row.enabled, false);
    device.start(0);
    if row.after_sleep {
        device.resume(0);
    }
    for step in row.steps {
        match *step {
            Measure(at) => device.status(at, Some(Charge::new(80, false, None))),
            Suspend(at) => device.suspend(at),
            Resume(at) => device.resume(at),
            Stop(at) => device.stop(at),
            Turn(at) => device.turn(at),
            Enabled(on) => device.enabled = on,
        }
    }
    device.file("log.csv")
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
    let mut device = Device::new(true, false);
    device.start(0);
    for at in [60_000, 120_000] {
        device.status(at, None);
    }
    let log = device.file("log.csv").unwrap();
    assert_eq!(column(&log, "kind"), ["start"]);
    assert_eq!(column(&log, "millivolts"), [""]);
    assert_eq!(column(&log, "percent"), [""]);
    assert_eq!(column(&log, "charging"), ["0"]);
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join(name)
}

/// Compares `text` with the golden `name`, or writes it where
/// `PAPERPAW_BLESS` is set.
fn golden(name: &str, text: &str) {
    let path = golden_path(name);
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(&path, text).unwrap();
    }
    let golden = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no golden {}: PAPERPAW_BLESS=1 writes it", path.display()));
    assert_eq!(
        golden, text,
        "{name} differs: PAPERPAW_BLESS=1 writes it anew"
    );
}

/// The usage log without the columns that depend on how long the host took
/// (`busy_ms`) and on drawing (`shown_ms` and the refreshes): what is golden
/// is the rows (when, which plugins) and the header.
fn usage_shape(text: &str) -> String {
    let mut out = String::new();
    for (n, line) in text.lines().enumerate() {
        let fields: Vec<&str> = line.split(',').collect();
        if n == 0 {
            out.push_str(line);
        } else {
            out.push_str(&format!("{},{},…", fields[0], fields[1]));
        }
        out.push('\n');
    }
    out
}

/// The sequence every golden below is made of: start, a measure with and
/// without voltage, two pages, charging, sleep, wake, a reading without a
/// battery, and a restart.
fn script(device: &mut Device) {
    // Start-up has no wall clock yet, as before the clock chip is read.
    device.clock = false;
    device.start(0);
    device.clock = true;
    device.status(60_000, Some(Charge::new(80, false, Some(4012))));
    // Pages are turned by the reader, which has used something by then.
    device.plugins = &["core", "battery", "fixture"];
    device.turn(61_000);
    device.turn(62_000);
    device.status(120_000, Some(Charge::new(79, true, None)));
    device.suspend(121_000);
    device.resume(200_000);
    device.status(260_000, None);
    device.stop(300_000);
}

#[test]
fn the_log_of_a_scripted_sequence_is_these_bytes() {
    let mut device = Device::new(true, true);
    script(&mut device);
    let log = device.file("log.csv").unwrap();
    golden("log-sequence.csv.v1", &log);
    // Whatever the host measured, the structure is fixed: the header, and
    // 13 fields in each line.
    assert!(log.starts_with(HEADER));
    assert!(log.lines().all(|l| l.split(',').count() == 13));
}

#[test]
fn the_plugin_usage_log_has_these_rows() {
    let mut device = Device::new(true, true);
    script(&mut device);
    let text = device.file(PLUGINS).unwrap();
    golden("plugins-sequence-shape.csv.v1", &usage_shape(&text));
    // A row for each plugin that has used anything, written at the first
    // measurement, every ten minutes awake and at each end.
    assert!(text.starts_with(PLUGINS_HEADER), "{text}");
}

#[test]
fn nothing_is_written_while_battery_is_off() {
    let mut device = Device::new(false, true);
    script(&mut device);
    assert!(device.h.context.files.borrow().is_empty());
}

/// A log of `total` bytes, valid for the log.
fn filler(total: usize) -> String {
    let mut log = String::from(HEADER);
    let line = "sample,60000,4012,80,0,0,0,0,0,0,0,0,\n";
    while log.len() + line.len() <= total {
        log.push_str(line);
    }
    // Pad the last line so that the file is exactly `total` bytes.
    let missing = total - log.len();
    if missing > 0 {
        log.push_str(&"#".repeat(missing - 1));
        log.push('\n');
    }
    assert_eq!(log.len(), total);
    log
}

#[test]
fn a_full_log_becomes_the_old_one_and_a_new_one_starts() {
    for (size, rotates) in [
        (MAX_BYTES - 1, false),
        (MAX_BYTES, true),
        (MAX_BYTES + 1, true),
    ] {
        let seeded = filler(size);
        let mut device = Device::new(true, false);
        device.h.context.seed("log.csv", &seeded);
        device.h.context.seed("log.old.csv", "an older log\n");
        // The start-up line is the first to meet the cap.
        device.start(0);
        let log = device.file("log.csv").unwrap();
        if rotates {
            assert_eq!(device.file("log.old.csv").unwrap(), seeded, "{size}");
            assert!(
                log.starts_with(HEADER) && log.lines().count() == 2,
                "{size}: {log}"
            );
            assert!(log.lines().nth(1).unwrap().starts_with("start,0,"), "{log}");
        } else {
            assert_eq!(
                device.file("log.old.csv").unwrap(),
                "an older log\n",
                "{size}"
            );
            assert!(log.starts_with(&seeded), "{size}");
            assert!(log.ends_with(",\n") && log.lines().last().unwrap().starts_with("start,"));
        }
    }
}

#[test]
fn the_usage_log_rotates_the_same_way() {
    let usage = filler(MAX_BYTES);
    let mut device = Device::new(true, false);
    device.h.context.seed(PLUGINS, &usage);
    device.start(0);
    device.status(60_000, Some(Charge::new(80, false, None)));
    assert_eq!(device.file(PLUGINS_OLD).unwrap(), usage);
    let fresh = device.file(PLUGINS).unwrap();
    assert!(fresh.starts_with(PLUGINS_HEADER), "{fresh}");
    assert!(fresh.len() < 4096);
}

/// The log of a run that starts on a log whose last line was cut by a power
/// loss (no newline), then measures once.
fn after_a_cut_tail(seeded: &str) -> String {
    let mut device = Device::new(true, false);
    device.h.context.seed("log.csv", seeded);
    device.start(0);
    device.clock = true;
    device.status(120_000, Some(Charge::new(70, false, None)));
    device.file("log.csv").unwrap()
}

/// The ONE intended deviation from the interface-6 plugin (`PORTING.md`):
/// when the log does not end in a newline, the partial line is terminated
/// before anything is appended, so the new `start` is a record of its own.
/// Nothing is rewritten or deleted; the partial line stays as a short,
/// unreadable record that readers skip.
#[test]
fn a_line_appended_after_a_cut_tail_is_a_record_of_its_own() {
    let cut = format!("{HEADER}start,0,4100,90,0,0,0,0,0,0,0,0,\nsample,60000,40");
    let log = after_a_cut_tail(&cut);
    golden("log-after-cut-tail.interface7.csv.v1", &log);
    assert!(log.starts_with(&format!("{cut}\n")));
    assert_eq!(log.lines().filter(|l| l.starts_with("start,")).count(), 2);
}

/// A log with a cut tail never breaks recording, wherever it is cut.
#[test]
fn recording_survives_a_tail_cut_at_every_byte() {
    let whole =
        format!("{HEADER}start,0,4100,90,0,0,0,0,0,0,0,0,\nend,3600000,4000,85,0,0,0,0,0,0,0,1,\n");
    let next = "start,4000000,4000,85,0,0,0,0,0,0,0,0,\nend,7600000,3900,80,0,0,0,0,0,0,0,2,\n";
    for cut in 0..=next.len() {
        let log = format!("{whole}{}", &next[..cut]);
        let after = after_a_cut_tail(&log);
        assert!(after.starts_with(&log), "cut at {cut}");
        assert!(
            after.lines().any(|l| l.starts_with("start,0,")),
            "cut at {cut}"
        );
    }
}
