//! What Battery writes to the card, byte for byte, and what it does with the
//! files it finds there: the log and the per-plugin usage log of a scripted
//! sequence, their rotation at the size cap, a tail cut by a power loss, and
//! the two settings. The golden files in `testdata/` are what the interface-7
//! port has to write again (`PORTING.md` lists them).
//!
//! After a change meant to alter them, write them anew and look at each
//! before committing it:
//!
//! ```sh
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-plugin-battery --test storage_goldens
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::{Path, PathBuf};

use paperpaw_core::Kernel;
use paperpaw_platform::{
    Battery as Charge, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat,
    Rotation, SystemStatus, WallClock,
};
use paperpaw_plugin_battery::Battery;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{
    ActionSpec, Contribution, Id, Manifest, MemoryStorage, Plugin, RegisterError, Registrar,
    SettingKind, SettingValue,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "battery-storage",
    "Battery storage goldens",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);
const LOG: &str = ".paperpaw/plugins/battery/log.csv";
const OLD: &str = ".paperpaw/plugins/battery/log.old.csv";
const PLUGINS: &str = ".paperpaw/plugins/battery/plugins.csv";
const PLUGINS_OLD: &str = ".paperpaw/plugins/battery/plugins.old.csv";
const HEADER: &str =
    "kind,ms,millivolts,percent,charging,fast,partial,full,kilopixels,inputs,panel_ms,pages,unix\n";
const MAX_BYTES: usize = 128 * 1024;
const TURN: Id = Id::new_static("fixture.turn-page");
/// Wednesday, 14 October 2026, 10:41 UTC, in milliseconds.
const UNIX_MS: i64 = 1_791_974_460_000;

/// A plugin that turns a page when its action runs, as a reader does.
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

fn kernel(storage: MemoryStorage, config: &str) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(config.to_string())
        .plugin(Pages)
        .plugin(Battery)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn read(kernel: &Kernel, path: &str) -> Option<String> {
    kernel.storage().exists(path).then(|| {
        String::from_utf8(paperpaw_sdk::read_file(kernel.storage(), path, 1 << 22).unwrap())
            .unwrap()
    })
}

fn measure(kernel: &mut Kernel, at: u64, battery: Option<Charge>) {
    let clock = WallClock::new(Instant(0), UNIX_MS, Some(0));
    kernel.set_status(Instant(at), SystemStatus::new(Some(clock), battery));
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
fn script(kernel: &mut Kernel) {
    measure(kernel, 60_000, Some(Charge::new(80, false, Some(4012))));
    kernel.run_action(Instant(61_000), TURN);
    kernel.run_action(Instant(62_000), TURN);
    measure(kernel, 120_000, Some(Charge::new(79, true, None)));
    kernel.suspend(Instant(121_000));
    kernel.resume(Instant(200_000));
    measure(kernel, 260_000, None);
    kernel.stop(Instant(300_000));
}

#[test]
fn the_log_of_a_scripted_sequence_is_these_bytes() {
    let mut k = kernel(MemoryStorage::new(), "[core]\nlanguage = en\n");
    script(&mut k);
    let log = read(&k, LOG).unwrap();
    golden("log-sequence.csv.v1", &log);
    // Whatever the host measured, the structure is fixed: the header, and
    // 13 fields in each line.
    assert!(log.starts_with(HEADER));
    assert!(log.lines().all(|l| l.split(',').count() == 13));
}

#[test]
fn the_plugin_usage_log_has_these_rows() {
    let mut k = kernel(MemoryStorage::new(), "[core]\nlanguage = en\n");
    script(&mut k);
    let text = read(&k, PLUGINS).unwrap();
    golden("plugins-sequence-shape.csv.v1", &usage_shape(&text));
    // A row for each plugin that has used anything, written at the first
    // measurement, every ten minutes awake and at each end.
    assert!(
        text.starts_with(
            "ms,plugin,busy_ms,calls,shown_ms,fast,partial,full,kilopixels,read_kb,written_kb\n"
        ),
        "{text}"
    );
}

#[test]
fn nothing_is_written_while_battery_is_off_or_unreadable() {
    let mut k = kernel(
        MemoryStorage::new(),
        "[core]\nlanguage = en\n[battery]\nenabled = off\n",
    );
    script(&mut k);
    assert!(read(&k, LOG).is_none() && read(&k, PLUGINS).is_none());
}

/// A line of `n` bytes at most, valid for the log.
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
        let storage = MemoryStorage::new()
            .with(LOG, seeded.clone())
            .with(OLD, "an older log\n");
        let mut k = kernel(storage, "[core]\nlanguage = en\n[battery]\nenabled = on\n");
        // The start-up line is the first to meet the cap.
        let log = read(&k, LOG).unwrap();
        if rotates {
            assert_eq!(read(&k, OLD).unwrap(), seeded, "{size}");
            assert!(
                log.starts_with(HEADER) && log.lines().count() == 2,
                "{size}: {log}"
            );
            assert!(log.lines().nth(1).unwrap().starts_with("start,0,"), "{log}");
        } else {
            assert_eq!(read(&k, OLD).unwrap(), "an older log\n", "{size}");
            assert!(log.starts_with(&seeded), "{size}");
            assert!(log.ends_with(",\n") && log.lines().last().unwrap().starts_with("start,"));
        }
        measure(&mut k, 60_000, Some(Charge::new(80, false, None)));
    }
}

#[test]
fn the_usage_log_rotates_the_same_way() {
    let usage = filler(MAX_BYTES);
    let storage = MemoryStorage::new().with(PLUGINS, usage.clone());
    let mut k = kernel(storage, "[core]\nlanguage = en\n[battery]\nenabled = on\n");
    measure(&mut k, 60_000, Some(Charge::new(80, false, None)));
    assert_eq!(read(&k, PLUGINS_OLD).unwrap(), usage);
    let fresh = read(&k, PLUGINS).unwrap();
    assert!(fresh.starts_with("ms,plugin,busy_ms,"), "{fresh}");
    assert!(fresh.len() < 4096);
}

#[test]
fn a_tail_cut_by_a_power_loss_loses_only_itself_in_the_view() {
    use paperpaw_sdk::Env;
    let whole = format!(
        "{HEADER}start,0,4100,90,0,0,0,0,0,0,0,0,\n\
         end,3600000,4000,85,0,0,0,0,0,0,0,1,\n"
    );
    // The next hour was cut in the middle of its first line, and at every
    // other place: none of them may stop the view from being built.
    let next = "start,4000000,4000,85,0,0,0,0,0,0,0,0,\nend,7600000,3900,80,0,0,0,0,0,0,0,2,\n";
    for cut in 0..=next.len() {
        let log = format!("{whole}{}", &next[..cut]);
        let storage =
            MemoryStorage::new().with(&format!("{}/log.csv", Env::plugin_dir("battery")), log);
        let mut k = kernel(storage, "[core]\nlanguage = en\n[battery]\nenabled = off\n");
        k.run_action(Instant(100), paperpaw_plugin_battery::OPEN);
        let labels: Vec<String> = k.describe().into_iter().map(|p| p.label).collect();
        assert!(!labels.is_empty(), "cut at {cut}");
    }
}

/// The log of a run that starts on a log whose last line was cut by a power
/// loss (no newline), then measures once.
fn after_a_cut_tail() -> (String, String) {
    let cut = format!("{HEADER}start,0,4100,90,0,0,0,0,0,0,0,0,\nsample,60000,40");
    let storage = MemoryStorage::new().with(LOG, cut.clone());
    let mut k = kernel(storage, "[core]\nlanguage = en\n[battery]\nenabled = on\n");
    measure(&mut k, 120_000, Some(Charge::new(70, false, None)));
    (cut, read(&k, LOG).unwrap())
}

/// INTERFACE 6 ONLY. What happens today: the log is appended to as it is, so
/// the first new line goes onto the end of a line that lost its newline. Two
/// records become one unreadable line: the cut sample and the `start` that the
/// new run wrote. That `start` is lost as well, and the time awake it opens has
/// no beginning in the file. This is a bug; Manager decision (#771): the
/// interface-6 plugin is not patched, the interface-7 port fixes it, and this
/// test and its golden are deleted with the interface-6 plugin.
#[test]
fn interface_6_only_a_line_appended_after_a_cut_tail_joins_it() {
    let (cut, log) = after_a_cut_tail();
    golden("log-after-cut-tail.interface6.csv.v1", &log);
    assert!(log.starts_with(&cut));
    let joined = log
        .lines()
        .find(|l| l.starts_with("sample,60000,40"))
        .unwrap();
    assert!(joined.len() > "sample,60000,40".len(), "{log}");
}

/// What the interface-7 port has to write instead (the ONE intended deviation
/// from today's behaviour, `PORTING.md`): when the log does not end in a
/// newline, the partial line is terminated before anything is appended, so the
/// new `start` is a record of its own. Nothing is rewritten or deleted; the
/// partial line stays as a short, unreadable record that readers skip.
///
/// Ignored here because the interface-6 plugin does not do this; the port
/// removes the `ignore` and the interface-6 test above.
#[test]
#[ignore = "interface 7 only: the port's acceptance test, fails on the interface-6 plugin"]
fn interface_7_a_line_appended_after_a_cut_tail_is_a_record_of_its_own() {
    let (cut, log) = after_a_cut_tail();
    golden("log-after-cut-tail.interface7.csv.v1", &log);
    assert!(log.starts_with(&format!("{cut}\n")));
    assert_eq!(log.lines().filter(|l| l.starts_with("start,")).count(), 2);
}

/// The two goldens differ by exactly that newline, so the one for the port is
/// not a different story.
#[test]
fn the_interface_7_golden_is_the_interface_6_one_with_the_tail_terminated() {
    let old = std::fs::read_to_string(golden_path("log-after-cut-tail.interface6.csv.v1")).unwrap();
    let new = std::fs::read_to_string(golden_path("log-after-cut-tail.interface7.csv.v1")).unwrap();
    assert_eq!(old.replacen("40start,", "40\nstart,", 1), new);
}

#[test]
fn the_settings_are_two_switches_on_by_default() {
    let mut r = Registrar::new(Id::new_static("battery"));
    Battery.register(&mut r).unwrap();
    let specs: Vec<_> = r
        .into_contributions()
        .into_iter()
        .filter_map(|c| match c {
            Contribution::FixedSettings(list) => Some(list.to_vec()),
            Contribution::Setting(spec) => Some(vec![*spec]),
            _ => None,
        })
        .flatten()
        .collect();
    let seen: Vec<_> = specs
        .iter()
        .map(|s| {
            (
                s.id.as_str().to_string(),
                s.title.as_str().to_string(),
                s.description.as_ref().map(|d| d.as_str().to_string()),
                s.group.as_str().to_string(),
                s.default.clone(),
                matches!(s.kind, SettingKind::Bool),
                (s.scoped, s.guarded, s.kept_on_reset),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (
                "battery.enabled".into(),
                "battery.setting.enabled".into(),
                Some("battery.setting.enabled.help".into()),
                "power.group".into(),
                SettingValue::Bool(true),
                true,
                (false, false, false)
            ),
            (
                "battery.by-plugin".into(),
                "battery.setting.by-plugin".into(),
                Some("battery.setting.by-plugin.help".into()),
                specs[1].group.as_str().to_string(),
                SettingValue::Bool(true),
                true,
                (false, false, false)
            ),
        ]
    );
    // The second one is shown with the developer settings.
    assert_eq!(
        specs[1].group.as_str(),
        paperpaw_sdk::builtin::groups::DEVELOPER.as_str()
    );
}
