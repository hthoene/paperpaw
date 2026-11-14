#![expect(
    clippy::expect_used,
    reason = "a build script that cannot go on fails at once"
)]
//! Writes `own_catalogs.rs`: the catalogs of PaperPaw's own plugins with
//! code (`plugins/<id>/i18n/*.ini`), by plugin, built into this tool. The
//! app has those of the plugins linked into it without their keys
//! (`paperpaw_i18n::BuiltMessages`), and the lint needs the keys; built in,
//! they are there for a `paperpaw-dev` run outside this repository too.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The `.ini` files of `folder`, sorted.
fn catalogs(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "ini"))
        .collect();
    files.sort();
    files
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap_or(root);
    // A plugin added or taken away changes the workspace's lock file.
    println!(
        "cargo::rerun-if-changed={}",
        root.join("Cargo.lock").display()
    );
    let mut plugins: Vec<PathBuf> = std::fs::read_dir(root.join("plugins"))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    plugins.sort();
    let mut out = String::from("&[\n");
    for plugin in plugins {
        let Some(id) = plugin.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // A data plugin's catalogs are files the kernel reads, keys and all.
        if !plugin.join("Cargo.toml").is_file() {
            continue;
        }
        let folder = plugin.join("i18n");
        // The folder: a catalog added to it or taken away.
        println!("cargo::rerun-if-changed={}", folder.display());
        let files = catalogs(&folder);
        if files.is_empty() {
            continue;
        }
        let _ = write!(out, "    ({id:?}, &[");
        for file in files {
            let _ = write!(out, "include_str!({:?}), ", file.display().to_string());
        }
        out.push_str("]),\n");
    }
    out.push(']');
    let dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&dir).join("own_catalogs.rs"), out).expect("own_catalogs.rs written");
}
