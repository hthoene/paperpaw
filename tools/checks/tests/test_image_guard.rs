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
    assert!(
        read("shells/xteink/Cargo.toml")
            .contains("test-image = [\"native-on\", \"paperpaw-core/x4-nested\"]")
    );
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

/// The test image keeps the X4 awake on USB (todo 532: a remote test loses its
/// link to a sleep); no other build changes the setting's default.
#[test]
fn only_the_test_image_stays_awake_on_usb_by_default() {
    let device = read("shells/xteink/src/device.rs");
    assert!(
        device.contains(r#"const STAY_AWAKE_USB_DEFAULT: bool = cfg!(feature = "test-image");"#),
        "the stay-awake default must be the test image's feature alone"
    );
    let setting = device
        .split(r#"STAY_AWAKE_USB.static_str(),"#)
        .nth(1)
        .expect("the stay-awake setting");
    let spec: Vec<&str> = setting.lines().take(4).collect();
    assert!(
        spec.iter()
            .any(|line| line.trim() == "STAY_AWAKE_USB_DEFAULT,"),
        "the setting's default is not the test image's constant: {spec:?}"
    );
}
