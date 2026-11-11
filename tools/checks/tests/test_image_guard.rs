//! The test image's veiling sealer (decision 156, `paperpaw_x4::native_offer::VeilSeal`)
//! never ships: its features are in no default list and no release build, and
//! the firmware refuses `test-image` together with `release` at compile time.

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
fn the_veiling_sealer_is_in_no_default_feature_list() {
    for path in ["shells/xteink/Cargo.toml", "crates/paperpaw-x4/Cargo.toml"] {
        let default = default_features(&read(path));
        assert!(
            !default.contains("test-image") && !default.contains("test-veil"),
            "{path}: {default}"
        );
    }
}

#[test]
fn no_release_build_asks_for_the_test_image() {
    for path in [".github/workflows/release.yml", "RELEASING.md"] {
        let text = read(path);
        assert!(
            !text.contains("test-image") && !text.contains("test-veil"),
            "{path} mentions the test image"
        );
    }
}

#[test]
fn the_firmware_refuses_a_test_image_that_is_a_release() {
    let main = read("shells/xteink/src/main.rs");
    assert!(main.contains(r#"all(feature = "test-image", feature = "release")"#));
    let manifest = read("shells/xteink/Cargo.toml");
    assert!(manifest.contains("test-image = [\"native-on\", \"paperpaw-x4/test-veil\"]"));
}
