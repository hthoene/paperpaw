//! Native UI acceptance through the desktop's real loader and input driver.
//! See support/NATIVE-UI.md for inputs and the explicit host activation gates.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "acceptance assertions"
)]

use paperpaw_emulator::{
    device::{self, Device},
    headless, native, native_runtime, profiles,
};
use paperpaw_sdk::{Id, Params, PluginKind};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[path = "support/pixels.rs"]
mod pixels;

#[derive(Clone, Copy, Debug)]
enum MissingHost {
    H1,
    H3,
    H4,
}

// Replace each refusal only with a checked production path and its acceptance
// test. No environment variable may turn these into simulated success.
fn require_host(path: MissingHost) -> Result<(), String> {
    Err(match path {
        MissingHost::H1 => "BLOCKED H1: native screen command activation is not qualified",
        MissingHost::H3 => "BLOCKED H3: native UI composition is not qualified",
        MissingHost::H4 => "BLOCKED H4: native key input dispatch is not qualified",
    }
    .into())
}

struct Switches;
impl Drop for Switches {
    fn drop(&mut self) {
        device::set_code_plugins(false);
        native_runtime::set_enabled(false);
    }
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}; see tests/support/NATIVE-UI.md"))
}

fn compare(device: &Device, reference: &Path, evidence: &Path) -> Result<(), String> {
    let (width, height, expected) = pixels::grays(reference);
    let actual: Vec<u8> = device
        .panel
        .rgb_bytes()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8
        })
        .collect();
    let size = (
        u32::from(device.panel.width()),
        u32::from(device.panel.height()),
    );
    pixels::write_gray(&evidence.join("actual.png"), size.0, size.1, &actual);
    if size != (width, height) {
        return Err(format!(
            "frame size {size:?} differs from golden {width}x{height}"
        ));
    }
    let diff: Vec<u8> = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| a.abs_diff(*b))
        .collect();
    pixels::write_gray(&evidence.join("diff.png"), width, height, &diff);
    let changed = diff.iter().filter(|pixel| **pixel != 0).count();
    if changed != 0 {
        return Err(format!(
            "{changed} changed pixels; evidence {}",
            evidence.display()
        ));
    }
    Ok(())
}

#[test]
fn missing_native_host_paths_fail_explicitly() {
    for (path, name) in [
        (MissingHost::H1, "H1"),
        (MissingHost::H3, "H3"),
        (MissingHost::H4, "H4"),
    ] {
        assert!(
            require_host(path)
                .unwrap_err()
                .starts_with(&format!("BLOCKED {name}:"))
        );
    }
}

#[test]
#[ignore = "requires a real native plugin fixture; H1/H3/H4 currently refuse explicitly"]
#[allow(
    clippy::too_many_lines,
    reason = "one isolated native loader acceptance sequence"
)]
fn loaded_native_plugin_matches_golden_after_keys() -> Result<(), String> {
    let library = PathBuf::from(required("PAPERPAW_NATIVE_UI_LIBRARY"));
    let id = required("PAPERPAW_NATIVE_UI_ID");
    let screen = required("PAPERPAW_NATIVE_UI_SCREEN");
    let reference = PathBuf::from(required("PAPERPAW_NATIVE_UI_GOLDEN"));
    let evidence = PathBuf::from(required("PAPERPAW_NATIVE_UI_EVIDENCE"));
    let script = required("PAPERPAW_NATIVE_UI_KEYS");
    let profile = required("PAPERPAW_NATIVE_UI_PROFILE");
    let config = required("PAPERPAW_NATIVE_UI_SETTINGS");
    // Use a disposable fixture card: trust writes only into that test library.
    assert!(
        library.join(".native-ui-test-card").is_file(),
        "refusing a non-test card"
    );
    assert!(
        reference.is_file(),
        "missing golden; acceptance never blesses"
    );
    assert!(!script.is_empty(), "at least one key press is required");
    for key in script.split(',') {
        assert!(
            matches!(
                key,
                "confirm"
                    | "back"
                    | "left"
                    | "right"
                    | "up"
                    | "down"
                    | "page-forward"
                    | "page-back"
            ),
            "key-only script required"
        );
    }
    let path = library.join("plugins").join(&id).join(native::file_name());
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    assert_eq!(
        native_runtime::classify(&bytes),
        native_runtime::Kind::Interface7
    );
    assert!(
        native::trust(&library)
            .map_err(|e| e.to_string())?
            .contains(&id)
    );
    let _switches = Switches;
    device::set_code_plugins(true);
    native_runtime::set_enabled(true);
    let profile = profiles::find(&profile).ok_or("unknown profile")?;
    let storage = device::library(&library).map_err(|e| e.to_string())?;
    let mut device =
        Device::new(profile, Some(config), storage, true).map_err(|e| e.to_string())?;
    device.set_time(1_700_000_000_000);
    device.start();
    let plugin = device
        .kernel
        .plugins()
        .get(&id)
        .ok_or("native plugin absent from kernel")?;
    assert_eq!(
        plugin.kind,
        PluginKind::Code,
        "built-in fallback is not native acceptance"
    );
    assert_eq!(plugin.problem, None, "native admission refused");
    require_host(MissingHost::H1)?;
    device.open_screen(
        Id::parse(&screen).map_err(|e| format!("{e:?}"))?,
        Params::new(),
    );
    require_host(MissingHost::H3)?;
    require_host(MissingHost::H4)?;
    headless::run_script(&mut device, &script)?;
    compare(&device, &reference, &evidence)
}
