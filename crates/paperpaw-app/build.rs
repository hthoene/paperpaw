#![expect(
    clippy::expect_used,
    reason = "a build script that cannot go on fails at once"
)]

//! Tells the shells which plugin crates this build of the app has
//! (`DEP_PAPERPAW_APP_CRATES` in their build scripts, by `links`): every
//! `paperpaw-plugin-*` dependency that is not optional, and the optional
//! ones of the features switched on. The X4's firmware writes that list
//! into its image (ADR 0022); its `Cargo.lock` cannot say it, as a lock
//! file names optional dependencies whether they are built or not.
//!
//! It also makes the indexes of the translation catalogs of those plugins,
//! the kernel's and the app's, kept in the program
//! (`paperpaw_i18n::FixedIndexes`): their catalogs then keep none on the
//! heap; and indexes their English catalogs together
//! (`paperpaw_i18n::BuiltCatalogs`): one catalog for all of them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=Cargo.toml");
    let manifest = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml");
    // As checked out on Windows, too.
    let manifest = manifest.replace("\r\n", "\n");
    let section = |name: &str| {
        manifest
            .split(&format!("\n[{name}]\n"))
            .nth(1)
            .map_or("", |rest| rest.split("\n[").next().unwrap_or(rest))
    };
    let mut crates: Vec<String> = section("dependencies")
        .lines()
        .filter(|line| !line.contains("optional = true"))
        .filter_map(|line| line.split(['.', ' ', '=']).next())
        .filter(|name| name.starts_with("paperpaw-plugin-"))
        .map(String::from)
        .collect();
    // `feature = ["dep:…", …]`, perhaps over several lines.
    let mut on = false;
    // The plugins a feature links (`"paperpaw-plugin-…/linked"`): the ones whose
    // catalogs the program carries for them (`native_catalogs`).
    let mut linked: Vec<String> = Vec::new();
    for line in section("features").lines() {
        if let Some((name, _)) = line.split_once(" = [") {
            let variable = format!(
                "CARGO_FEATURE_{}",
                name.trim().to_uppercase().replace('-', "_")
            );
            on = std::env::var_os(variable).is_some();
        }
        if on {
            for part in line.split('"') {
                if let Some(name) = part.strip_suffix("/linked") {
                    let name = name.trim_end_matches('?');
                    if name.starts_with("paperpaw-plugin-") {
                        linked.push(String::from(name));
                    }
                }
            }
            for part in line.split("\"dep:").skip(1) {
                if let Some(name) = part.split('"').next()
                    && name.starts_with("paperpaw-plugin-")
                {
                    crates.push(String::from(name));
                }
            }
        }
    }
    crates.sort_unstable();
    crates.dedup();
    let list = crates.join(",");
    println!("cargo::metadata=crates={list}");
    println!("cargo::rustc-env=PAPERPAW_APP_CRATES={list}");
    plugin_versions(&crates);
    catalog_indexes(&crates);
    linked.sort_unstable();
    linked.dedup();
    native_catalogs(&linked);
}

/// Writes `plugin_versions.rs`: `(id, version)` of every plugin crate built,
/// from the crate's own `Cargo.toml` (the version the plugin's file is named
/// by), for the plugin manager's list of the linked native plugins.
fn plugin_versions(crates: &[String]) {
    let root = Path::new("../..");
    let mut out = String::from("&[\n");
    // The same versions as manifests kept in the program (#248, B part b).
    let mut manifests = String::from("&[\n");
    for id in crates
        .iter()
        .filter_map(|name| name.strip_prefix("paperpaw-plugin-"))
    {
        let file = root.join("plugins").join(id).join("Cargo.toml");
        println!("cargo::rerun-if-changed={}", file.display());
        let Ok(source) = std::fs::read_to_string(&file) else {
            println!(
                "cargo::warning=no {}: `{id}` has no version to list",
                file.display()
            );
            continue;
        };
        let source = format!("\n{}", source.replace("\r\n", "\n"));
        let package = source
            .split("\n[package]\n")
            .nth(1)
            .and_then(|rest| rest.split("\n[").next())
            .unwrap_or("");
        let version = package.lines().find_map(|line| {
            let value = line.strip_prefix("version = \"")?;
            Some(value.strip_suffix('"')?.to_string())
        });
        match version {
            Some(version) => {
                let _ = writeln!(out, "    ({id:?}, {version:?}),");
                let (numbers, pre) = version.split_once('-').unwrap_or((&version, ""));
                let parts: Vec<&str> = numbers.split('.').collect();
                if parts.len() == 3 && !version.contains('+') {
                    let _ = writeln!(
                        manifests,
                        "    ({id:?}, paperpaw_core::sdk::Manifest::fixed(paperpaw_core::sdk::Id::new_static({id:?}), {id:?}, \"\", paperpaw_core::sdk::Version::of_package({:?}, {:?}, {:?}, {pre:?}))),",
                        parts[0], parts[1], parts[2]
                    );
                }
            }
            None => println!("cargo::warning=`{id}` has no version of its own to list"),
        }
    }
    out.push(']');
    let dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&dir).join("plugin_versions.rs"), out)
        .expect("plugin_versions.rs written");
    manifests.push(']');
    std::fs::write(Path::new(&dir).join("plugin_manifests.rs"), manifests)
        .expect("plugin_manifests.rs written");
}

/// Writes `catalog_indexes.rs`: the index of every catalog in the
/// `i18n` folders of the kernel, the app and the plugin crates built.
fn catalog_indexes(crates: &[String]) {
    let root = Path::new("../..");
    let mut folders = vec![
        root.join("crates/paperpaw-core/i18n"),
        PathBuf::from("i18n"),
    ];
    folders.extend(
        crates
            .iter()
            .filter_map(|name| name.strip_prefix("paperpaw-plugin-"))
            .map(|dir| root.join("plugins").join(dir).join("i18n")),
    );
    let mut files = Vec::new();
    for folder in &folders {
        ini_files(folder, &mut files);
    }
    files.sort();
    let mut out = String::from("&[\n");
    let mut seen = Vec::new();
    // One table for each language the program has built in.
    let mut tables = [
        paperpaw_i18n::BuiltLanguageTable::new("en"),
        paperpaw_i18n::BuiltLanguageTable::new("de"),
    ];
    for file in files {
        println!("cargo::rerun-if-changed={}", file.display());
        let source = std::fs::read_to_string(&file).expect("a catalog");
        let print = paperpaw_i18n::fingerprint(&source);
        // Catalogs of a language together; one that cannot be (a key another
        // has) stays a catalog of its own, with its index here.
        let mut built = false;
        for table in &mut tables {
            built |= table.add(&source).unwrap_or(false);
        }
        // A source a table holds is read through the table: it needs no index
        // of its own.
        if built {
            continue;
        }
        let Ok(catalog) = paperpaw_i18n::Catalog::parse(source) else {
            continue;
        };
        let Some(index) = catalog.narrow_index() else {
            continue;
        };
        if seen.contains(&print) {
            continue;
        }
        seen.push(print);
        let _ = writeln!(out, "    ({print:#x}, &{index:?}),");
    }
    out.push(']');
    let dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&dir).join("catalog_indexes.rs"), out)
        .expect("catalog_indexes.rs written");
    let mut built = String::from("&[");
    for table in &tables {
        built.push_str(&table.to_rust());
        built.push(',');
    }
    built.push(']');
    std::fs::write(Path::new(&dir).join("built_catalogs.rs"), built)
        .expect("built_catalogs.rs written");
}

/// The `.ini` files in `folder` and the folders in it.
fn ini_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            ini_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "ini") {
            files.push(path);
        }
    }
}

/// Writes `native_catalogs.rs`: for each plugin crate a feature links that has catalogs
/// (`plugins/<id>/i18n/<language>.ini`), its id and its catalogs as texts
/// built into the program, for a linked native plugin that brings them built
/// in (`screens-input.md` [D] Translations).
fn native_catalogs(crates: &[String]) {
    let root = Path::new("../..");
    let mut out = String::from("&[\n");
    for id in crates
        .iter()
        .filter_map(|name| name.strip_prefix("paperpaw-plugin-"))
    {
        let folder = root.join("plugins").join(id).join("i18n");
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "ini"))
            .collect();
        files.sort();
        if files.is_empty() {
            continue;
        }
        let _ = write!(out, "    ({id:?}, &[");
        for file in files {
            let file = std::fs::canonicalize(&file).expect("a catalog");
            println!("cargo::rerun-if-changed={}", file.display());
            let _ = write!(out, "include_str!({:?}), ", file.display().to_string());
        }
        out.push_str("]),\n");
    }
    out.push(']');
    let dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&dir).join("native_catalogs.rs"), out)
        .expect("native_catalogs.rs written");
}
