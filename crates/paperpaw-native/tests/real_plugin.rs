//! The preflight on a real plugin: the interface-7 sample
//! (`samples/abi7-tick-draw`), built here as a host library with
//! `-nostartfiles` as the desktop's tests build it, must be accepted. The
//! unit tests show what is refused; this one guards the default-deny rules
//! against refusing a library that is fine. It builds the sample, so it is
//! run with `--ignored` (the "Native plugins" CI job, `tools/ci-local.sh
//! native`).

#![allow(clippy::expect_used, reason = "a build that fails fails the test")]
#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use paperpaw_native::preflight::check_elf64_le;

const MACHINE: u16 = if cfg!(target_arch = "x86_64") {
    62
} else {
    183
};

#[test]
#[ignore = "builds the interface-7 sample as a host library (the Native plugins CI job)"]
fn the_real_native_plugin_is_accepted() {
    let sample =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/abi7-tick-draw");
    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("real-plugin");
    let mut build = std::process::Command::new(env!("CARGO"));
    // The nested build depends on nothing but the toolchain: the test run's
    // own CARGO_* settings (target, profile, flags) must not change it.
    build.env_clear();
    for name in [
        "PATH",
        "HOME",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "RUSTUP_TOOLCHAIN",
    ] {
        if let Some(value) = std::env::var_os(name) {
            build.env(name, value);
        }
    }
    let built = build
        .current_dir(&sample)
        .env("CARGO_TARGET_DIR", &target)
        .args([
            "rustc",
            "--release",
            "--lib",
            "--locked",
            "--",
            "--crate-type=cdylib",
            "-Cpanic=abort",
            "-Clink-arg=-nostartfiles",
        ])
        .output()
        .expect("cargo runs");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    // The override of the crate type leaves the library in `deps/`, hashed.
    let library = std::fs::read_dir(target.join("release/deps"))
        .expect("the sample's build")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("libpaperpaw_sample_abi7_tick_draw-") && name.ends_with(".so")
                })
        })
        .expect("the sample's library");
    let bytes = std::fs::read(&library).expect("the library is read");
    assert_eq!(
        check_elf64_le(&bytes, MACHINE),
        Ok(()),
        "a real -nostartfiles plugin is refused"
    );
}
