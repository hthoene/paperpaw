//! The example flows run on the devices they fit and pass; a reference
//! picture written with `--bless` is compared, and a flow that expects
//! what is not there fails with the line, what was there and a picture.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::PathBuf;

use paperpaw_dev::cli::Context;
use paperpaw_dev::flows;
use paperpaw_dev::report::{Finding, Level, Report};
use paperpaw_dev::target::Target;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/flows/{name}"))
}

fn found<'a>(report: &'a Report, check: &str) -> Vec<&'a Finding> {
    report
        .findings
        .iter()
        .filter(|f| f.check == check)
        .collect()
}

fn ran(report: &Report, flow: &str, profile: &str) -> bool {
    found(report, "flows.passed").iter().any(|f| {
        f.place.ends_with(flow)
            && f.variant
                .as_deref()
                .is_some_and(|v| v.starts_with(&format!("{profile} ")))
    })
}

#[test]
fn flows_pass_and_fail_where_they_should() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-flows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let mut context = Context::new(Target::default(), out.clone());
    context.snapshots = Some(out.join("snapshots"));
    context.flows = vec![fixture("night-mode.flow"), fixture("settings.flow")];

    // The first run writes the settings page's picture.
    context.bless = true;
    let mut report = Report::default();
    flows::run(&context, &mut report).unwrap();
    assert_eq!(
        report.count(Level::Error),
        0,
        "{}",
        report.text(Level::Info)
    );
    let file = out.join("snapshots/boox-go-7/settings.png");
    assert!(file.is_file());
    // Night mode on the devices with the X4's buttons (the X4 alone of
    // those checked by default), the settings page on the Boox only.
    assert_eq!(found(&report, "flows.passed").len(), 2);
    assert!(ran(&report, "night-mode.flow", "xteink-x4"));
    assert!(!ran(&report, "night-mode.flow", "xteink-x4-pro"));
    assert!(!ran(&report, "night-mode.flow", "boox-go-7"));
    assert!(ran(&report, "settings.flow", "boox-go-7"));

    // Then it is compared, and passes.
    context.bless = false;
    let mut report = Report::default();
    flows::run(&context, &mut report).unwrap();
    assert_eq!(
        report.count(Level::Error),
        0,
        "{}",
        report.text(Level::Info)
    );

    // A picture that differs fails, marked where.
    let mut picture = paperpaw_dev::image::Image::load_png(&file).unwrap();
    picture.set_pixel(10, 1000, [0, 0, 0]);
    picture.save_png(&file).unwrap();
    let mut report = Report::default();
    flows::run(&context, &mut report).unwrap();
    let failed = found(&report, "flows.failed");
    assert_eq!(failed.len(), 1, "{}", report.text(Level::Info));
    let failure = failed[0];
    assert!(
        failure.place.ends_with("settings.flow:12"),
        "{}",
        failure.place
    );
    assert!(
        failure.message.contains("1 pixels differing"),
        "{}",
        failure.message
    );
    assert_eq!(failure.area.map(|a| (a.x, a.y)), Some((10, 1000)));
    assert_eq!(
        failure.plugin.as_deref(),
        None,
        "the whole app is looked at"
    );
    assert!(out.join(failure.picture.as_ref().unwrap()).is_file());

    // A flow that expects what is not there names the line and what was;
    // one that cannot be read is reported, the others still run.
    let wrong = out.join("wrong.flow");
    std::fs::write(
        &wrong,
        "profiles: xteink-x4\nlong:confirm\nexpect text \"Tools\"\nexpect focus \"Library\"\n",
    )
    .unwrap();
    context.flows = vec![out.join("missing.flow"), wrong];
    let mut report = Report::default();
    flows::run(&context, &mut report).unwrap();
    assert_eq!(found(&report, "flows.invalid").len(), 1);
    let failed = found(&report, "flows.failed");
    assert_eq!(failed.len(), 1, "{}", report.text(Level::Info));
    let failure = failed[0];
    assert!(failure.place.ends_with("wrong.flow:4"), "{}", failure.place);
    assert!(
        failure.message.contains("\"Quick panel\""),
        "the focused row is named: {}",
        failure.message
    );
    assert!(failure.area.is_some());
    let _ = std::fs::remove_dir_all(&out);
}
