//! The checks of data plugins handed in as files, without a file system
//! (as the browser's emulator asks them): a sample theme passes, a
//! damaged one is found wanting, and nothing is written.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;
use std::path::Path;

use paperpaw_dev::data;
use paperpaw_dev::report::Level;
use paperpaw_dev::target::{DataPlugin, Target};
use paperpaw_emulator::profiles;

fn sample(name: &str) -> DataPlugin {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/plugins")
        .join(name);
    let read = DataPlugin::read(&folder).unwrap();
    // Handed in, as by the browser: no folder.
    DataPlugin::new(&read.id, read.files)
}

/// What is looked at: the data plugins `plugins`.
fn of(plugins: Vec<DataPlugin>) -> Target {
    let mut target = Target::default();
    for plugin in plugins {
        target.take_data(plugin);
    }
    target
}

#[test]
fn a_sample_theme_passes_in_memory() {
    let x4 = profiles::find("xteink-x4").unwrap();
    let report = data::check(of(vec![sample("theme-paw")]), vec![x4], &[]).unwrap();
    assert_eq!(report.ran, data::COMMANDS);
    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.level == Level::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(report.findings.iter().all(|f| f.picture.is_none()));
    assert!(report.json().contains("\"package.size\""));
}

#[test]
fn a_damaged_plugin_is_found_wanting() {
    let files = BTreeMap::from([
        (
            "plugin.ini".to_string(),
            b"[plugin]\nid = dusk\nname = Dusk\nversion = 1.0.0\n[broken\n".to_vec(),
        ),
        (
            "settings.ini".to_string(),
            b"[core]\nui-sise = 3\n".to_vec(),
        ),
    ]);
    let boox = profiles::find("boox-go-7").unwrap();
    let plugin = DataPlugin::new("dusk", files);
    let report = data::check(of(vec![plugin]), vec![boox], &["package", "lint"]).unwrap();
    assert_eq!(report.ran, ["package", "lint"]);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.level == Level::Error && f.check == "package.syntax"),
        "{:#?}",
        report.findings
    );
    // Refused at start: lint cannot look at it, and says so.
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "dev.run" && f.place == "lint" && f.message.contains("dusk")),
        "{:#?}",
        report.findings
    );
    // Whole, its unknown setting is found.
    let files = BTreeMap::from([
        (
            "plugin.ini".to_string(),
            b"[plugin]\nid = dusk\nname = Dusk\nversion = 1.0.0\n".to_vec(),
        ),
        (
            "settings.ini".to_string(),
            b"[core]\nui-sise = 3\n".to_vec(),
        ),
    ]);
    let plugin = DataPlugin::new("dusk", files);
    let report = data::check(of(vec![plugin]), vec![boox], &["lint"]).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "lint.unknown-setting" && f.message.contains("core.ui-sise")),
        "{:#?}",
        report.findings
    );
}

#[test]
fn only_checks_of_data_plugins_are_run() {
    let error = data::check(Target::default(), Vec::new(), &["perf"]).unwrap_err();
    assert!(error.contains("perf"));
}

/// A data plugin adds no screens: it is seen on the standard ones.
#[test]
fn a_data_plugin_is_seen_on_the_standard_screens() {
    use paperpaw_dev::cli::Context;
    use paperpaw_dev::session::Session;
    use paperpaw_dev::target::Target;
    use paperpaw_dev::variant::Variant;
    let mut target = Target::default();
    target.take_data(sample("theme-paw"));
    let mut context = Context::new(target, std::path::PathBuf::new());
    context.in_memory = true;
    let session = Session::new(&context, &Variant::x4()).unwrap();
    let screens = paperpaw_dev::cases::screens(&session, &context.target);
    // Which standard screens the dev kernel has depends on the native plugins
    // linked (a workspace run unifies the features): the data plugin adds none.
    let standard = ["files.browser", "home.dashboard", "settings.list"];
    assert!(
        screens.iter().all(|s| standard.contains(&s.as_str())),
        "{screens:?}"
    );
}
