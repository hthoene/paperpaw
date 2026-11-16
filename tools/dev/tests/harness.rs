//! The harness a plugin's own repository uses (see `tools/dev/templates`),
//! run with the tool's UI kit plugin as the plugin: the command line with the plugin
//! added, a snapshot case written, matched, then missed; and the library
//! call a snapshot test makes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::{Path, PathBuf};

use paperpaw_core::KernelBuilder;
use paperpaw_dev::image::Image;
use paperpaw_dev::snapshot::{self, Case};

/// What the template's `examples/paperpaw-dev.rs` gives `paperpaw_dev::main`.
fn add(app: KernelBuilder) -> KernelBuilder {
    paperpaw_dev::ui_kit::add(app)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-dev-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str], out: &Path, snapshots: &Path) -> bool {
    let mut all: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    all.extend([
        "--snapshots".into(),
        snapshots.display().to_string(),
        "--out".into(),
        out.display().to_string(),
    ]);
    paperpaw_dev::cli::run(all.into_iter(), Some(add)).unwrap()
}

#[test]
fn the_command_line_runs_a_plugin_added_by_its_repository() {
    let dir = scratch("harness");
    let snapshots = dir.join("snapshots");
    let out = dir.join("out");
    std::fs::create_dir_all(&snapshots).unwrap();
    std::fs::write(
        snapshots.join(snapshot::CASES_FILE),
        "[x4-ui-kit]\nscreen = ui-kit.text\nprofile = xteink-x4\n",
    )
    .unwrap();
    let args = ["snapshots", "--plugin", "ui-kit"];

    // No reference yet: a failure, with the picture drawn.
    assert!(!run(&args, &out, &snapshots));
    assert!(out.join("snapshots/x4-ui-kit.actual.png").is_file());

    // Written, then matched.
    let mut blessed = args.to_vec();
    blessed.push("--bless");
    assert!(run(&blessed, &out, &snapshots));
    let reference = snapshots.join("x4-ui-kit.png");
    let picture = Image::load_png(&reference).unwrap();
    assert_eq!((picture.width, picture.height), (480, 800));
    assert!(run(&args, &out, &snapshots));

    // Changed: a failure, the differences shown.
    let mut changed = picture.clone();
    changed.outline(
        paperpaw_platform::Rect::new(40, 40, 200, 200),
        20,
        [0, 0, 0],
    );
    changed.save_png(&reference).unwrap();
    assert!(!run(&args, &out, &snapshots));
    for kind in ["expected", "actual", "diff"] {
        assert!(
            out.join(format!("snapshots/x4-ui-kit.{kind}.png"))
                .is_file(),
            "{kind}"
        );
    }
    assert!(out.join("snapshots/index.html").is_file());
    let json = std::fs::read_to_string(out.join("report.json")).unwrap();
    assert!(json.contains("snapshots.differs"), "{json}");
    assert!(json.contains("\"plugin\": \"ui-kit\""), "{json}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_snapshot_test_checks_its_cases() {
    let dir = scratch("snapshot-test");
    let snapshots = dir.join("snapshots");
    let out = dir.join("out");
    let cases = [
        Case::screen("ui-kit.text"),
        Case::screen("ui-kit.text")
            .on("boox-go-7")
            .set("core.night-mode", "true")
            .named("boox-ui-kit-dark"),
    ];
    let written = snapshot::check_into(add, &snapshots, &cases, &out, true).unwrap();
    assert_eq!(written.written.len(), 2);
    assert!(snapshots.join("xteink-x4-ui-kit-text.png").is_file());
    let matched = snapshot::check_into(add, &snapshots, &cases, &out, false).unwrap();
    assert_eq!(matched.matched.len(), 2);
    // Dark is not light.
    let dark = Image::load_png(&snapshots.join("boox-ui-kit-dark.png")).unwrap();
    assert!(dark.luma(5, 5) < 128);
    let missing = [Case::screen("ui-kit.text").named("not-written")];
    let error = snapshot::check_into(add, &snapshots, &missing, &out, false).unwrap_err();
    assert!(error.contains("not-written.png"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
