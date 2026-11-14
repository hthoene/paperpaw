//! The hyphenation of the plugins the emulator links (todo 613): `text.hyphenate`
//! is wired for them, and the data plugins' tables reach it. The German names of
//! the tools drawer break at a hyphen only where the German pack is installed.

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).unwrap().to_path_buf()
}

/// The German drawer of an X4 with a fresh library, the German pack on the
/// card or not: the picture the emulator draws, as bytes.
fn drawer(name: &str, german_pack: bool) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("paperpaw-hyphen-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".paperpaw/plugins/setup")).unwrap();
    fs::write(dir.join(".paperpaw/plugins/setup/done"), "").unwrap();
    if german_pack {
        let german = paperpaw_hyphenation_packs::PACKAGES
            .iter()
            .find(|package| package.id == "hyphenation-de")
            .unwrap();
        let plugins = dir.join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        paperpaw_hyphenation_packs::build(&root(), &plugins, german).unwrap();
    }
    let shot = dir.join("shot.png");
    let output = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .args(["--profile", "xteink-x4", "--headless", "--library"])
        .arg(&dir)
        .args(["--set", "core.language=de", "--set", "help.show=false"])
        .args(["--script", "open:home.drawer,wait:300", "--screenshot"])
        .arg(&shot)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = fs::read(&shot).unwrap();
    let _ = fs::remove_dir_all(&dir);
    bytes
}

#[test]
fn the_german_pack_changes_how_the_drawer_breaks_its_names() {
    let without = drawer("without", false);
    let with = drawer("with", true);
    assert_ne!(
        with, without,
        "the German pack is on the card and the linked drawer draws no differently"
    );
}
