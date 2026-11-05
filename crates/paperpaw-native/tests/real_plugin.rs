//! The preflight on a real plugin: the native test plugin, built apart from
//! the kernel with `-nostartfiles` (its `build.rs`), must be accepted. The
//! unit tests show what is refused; this one guards the default-deny rules
//! against refusing a library that is fine. CI builds the plugin first (the
//! "Native plugins" job, `tools/ci-local.sh native`), so it is run with
//! `--ignored` there and fails, not skips, when the file is missing.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use paperpaw_native::preflight::check_elf64_le;

const MACHINE: u16 = if cfg!(target_arch = "x86_64") { 62 } else { 183 };

#[test]
#[ignore = "needs the native test plugin built first (the Native plugins CI job)"]
fn the_real_native_plugin_is_accepted() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../paperpaw-core/tests/native-plugin/target/release/libpaperpaw_test_native_plugin.so",
    );
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("build the native test plugin first ({}): {e}", path.display()));
    assert_eq!(
        check_elf64_le(&bytes, MACHINE),
        Ok(()),
        "a real -nostartfiles plugin is refused"
    );
}
