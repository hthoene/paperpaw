//! What the plugins of the standard app registered with the kernel, as text
//! lines (`Kernel::registry_lines`), compared byte for byte with
//! `tests/registry-244/<profile>.txt` (#244, safety net P2): the registry must
//! come out the same before and after the interface-6 code leaves the kernel.
//! The lines of a part are sorted by key alone and keep the kernel's own order
//! within a key (see `registry_lines.rs`), so a change in which screen is tried
//! first, or which subscriber hears an event first, shows.
//!
//! The profiles are the emulator's `xteink-x4`, `xteink-x4-pro` and `boox-go-7`.
//! After a change that is meant: `PAPERPAW_BLESS=1` writes the goldens anew.

#![cfg(all(
    feature = "native-linked",
    feature = "native-linked-reading",
    feature = "native-linked-store"
))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use std::path::PathBuf;

use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::MemoryStorage;

/// The emulator's `xteink-x4`.
const X4: DeviceInfo = DeviceInfo::new(
    "xteink-x4",
    "Xteink X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg90, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// The emulator's `xteink-x4-pro`.
const X4_PRO: DeviceInfo = DeviceInfo::new(
    "xteink-x4-pro",
    "Xteink X4 Pro",
    DisplayInfo::new(480, 800, 217, PixelFormat::Mono1, Rotation::Deg90, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

/// The emulator's `boox-go-7`.
const BOOX: DeviceInfo = DeviceInfo::new(
    "boox-go-7",
    "Boox Go 7",
    DisplayInfo::new(1264, 1680, 300, PixelFormat::Gray8, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// A plugin line without its version (`plugin | id 1.0.4 Running ...`): a plugin
/// bump or the release's own version does not redden the goldens.
fn without_version(line: &str) -> String {
    if !line.starts_with("plugin | ") {
        return line.to_string();
    }
    let mut parts: Vec<&str> = line.split(' ').collect();
    if parts.len() > 3 {
        parts.remove(3);
    }
    parts.join(" ")
}

fn golden(device: DeviceInfo) {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    // The plugins that are off in a new setup are on, and the dictionary has its
    // lookup, as on the desktop: their registrations are in the net too.
    let config = "[core]\nlanguage = en\n[plugins]\npebble = on\nmail = on\nnotes-vault = on\npicture-home = on\n";
    native_activation::wire_lookup();
    let (builder, card) = native_activation::quiet_on(device, storage, config);
    let mut kernel = native_activation::build_on(paperpaw_app::install(builder, fonts()), card);
    kernel.start(Instant(0));
    let mut shown = kernel
        .registry_lines()
        .iter()
        .map(|line| without_version(line))
        .collect::<Vec<_>>()
        .join("\n");
    shown.push('\n');
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/registry-244")
        .join(format!("{}.txt", device.id));
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &shown).unwrap();
        return;
    }
    let kept = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing: PAPERPAW_BLESS=1 makes it", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(
        shown,
        kept,
        "{} differs from what the plugins register now (PAPERPAW_BLESS=1 if meant)",
        path.display()
    );
}

#[test]
fn the_registry_of_the_x4_is_as_it_was() {
    golden(X4);
}

#[test]
fn the_registry_of_the_x4_pro_is_as_it_was() {
    golden(X4_PRO);
}

#[test]
fn the_registry_of_the_boox_is_as_it_was() {
    golden(BOOX);
}
