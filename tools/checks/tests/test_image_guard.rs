//! The test image (decision 156, #2462: the high-water log of `src/high_water.rs`)
//! never ships: its feature is in no default list and no release build, and the
//! firmware refuses `test-image` together with `release` at compile time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_checks::root;

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The `default = [...]` line of a manifest.
fn default_features(manifest: &str) -> String {
    manifest
        .lines()
        .find(|line| line.trim_start().starts_with("default"))
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_test_image_is_in_no_default_feature_list() {
    for path in ["shells/xteink/Cargo.toml", "crates/paperpaw-x4/Cargo.toml"] {
        let default = default_features(&read(path));
        assert!(!default.contains("test-image"), "{path}: {default}");
    }
}

#[test]
fn no_release_build_asks_for_the_test_image() {
    for path in [".github/workflows/release.yml", "RELEASING.md"] {
        assert!(
            !read(path).contains("test-image"),
            "{path} mentions the test image"
        );
    }
}

#[test]
fn the_firmware_refuses_a_test_image_that_is_a_release() {
    let main = read("shells/xteink/src/main.rs");
    assert!(main.contains(r#"all(feature = "test-image", feature = "release")"#));
    assert!(read("shells/xteink/Cargo.toml").contains("test-image = [\"native-on\"]"));
}

/// The release build sets `release`, which refuses `test-image` at compile
/// time: without it the compile error could never fire in a real release.
#[test]
fn the_release_build_sets_the_release_feature() {
    let workflow = read(".github/workflows/release.yml");
    assert!(
        workflow.contains(r#"--features "$DEVICE,release""#),
        "release.yml must build the firmware with the `release` feature"
    );
    assert!(read("shells/xteink/Cargo.toml").contains("\nrelease = []"));
}

/// The device tests' never-sleep defaults (no idle sleep, awake on the cable, no
/// switch-off) exist only in a statement behind `cfg(feature = "test-image")`:
/// sleeping is the normal default of every other build.
#[test]
fn the_never_sleep_defaults_are_behind_the_test_image() {
    let main = read("shells/xteink/src/main.rs");
    let lines: Vec<&str> = main.lines().collect();
    for (at, line) in lines.iter().enumerate() {
        let sets_sleep = line.contains("settings::SLEEP_AFTER")
            || line.contains("STAY_AWAKE_USB")
            || line.contains("x4.power-off-after");
        if sets_sleep && line.contains("setting_default") {
            let behind = lines[at.saturating_sub(6)..at]
                .iter()
                .any(|before| before.contains(r#"#[cfg(feature = "test-image")]"#));
            assert!(behind, "main.rs:{}: a sleep default outside the test image: {line}", at + 1);
        }
    }
}

/// The nested-gate test feature brings the test image (which `release` refuses).
#[test]
fn a_test_feature_that_opens_gates_brings_the_test_image() {
    let manifest = read("shells/xteink/Cargo.toml");
    for line in manifest.lines().filter(|l| l.starts_with("test-nested")) {
        assert!(line.contains("\"test-image\""), "{line}");
    }
}

/// A release is checked on the image itself: no test-image build gets through.
#[test]
fn the_release_checks_its_images_for_the_test_build() {
    let workflow = read(".github/workflows/release.yml");
    assert!(workflow.contains("check-owner-image.py --release"));
}
