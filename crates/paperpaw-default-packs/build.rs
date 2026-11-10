//! Builds the English hyphenation package from the sources in
//! `plugins/hyphenation/` (the same code that builds the flasher's package,
//! `tools/hyphenation-packs`) and writes `english.rs`: the files of the
//! package, for the crate to embed.

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// The package that is installed by default.
const ID: &str = "hyphenation";

/// The files under `dir`, as paths relative to it, sorted.
fn files(dir: &Path, prefix: &str, found: &mut Vec<String>) -> Result<(), Box<dyn Error>> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}{name}");
        if entry.file_type()?.is_dir() {
            files(&entry.path(), &format!("{path}/"), found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let root = manifest.join("../..");
    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    let packs = out.join("packs");
    let _ = fs::remove_dir_all(&packs);
    let package = paperpaw_hyphenation_packs::PACKAGES
        .iter()
        .find(|p| p.id == ID)
        .ok_or("the English package is not one of the hyphenation packages")?;
    let dir = paperpaw_hyphenation_packs::build(&root, &packs, package)?;
    // The same check the store and the flasher's packages go through: a
    // package with a problem is not embedded (it would be put on every
    // device's card).
    let problems = paperpaw_hyphenation_packs::check(&dir);
    if !problems.is_empty() {
        let told: Vec<String> = problems.iter().map(ToString::to_string).collect();
        return Err(format!("the English package has problems: {}", told.join("; ")).into());
    }
    let mut found = Vec::new();
    files(&dir, "", &mut found)?;
    let mut source = String::from("&[\n");
    for path in &found {
        let file = dir.join(path);
        writeln!(
            source,
            "    ({path:?}, include_bytes!({:?})),",
            file.display().to_string()
        )?;
    }
    source.push_str("]\n");
    fs::write(out.join("english.rs"), source)?;
    for source in [
        "plugins/hyphenation",
        "tools/hyphenation-packs",
        "crates/paperpaw-hyphen",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(source).display());
    }
    Ok(())
}
