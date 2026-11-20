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

/// `bytes` raw-deflated with a history of `WINDOW_BITS` bits, which the
/// program's inflating (`lib.rs`) holds.
fn deflate(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    const WINDOW_BITS: u8 = 12;
    let mut compress =
        flate2::Compress::new_with_window_bits(flate2::Compression::best(), false, WINDOW_BITS);
    // Room for what does not compress, too.
    let mut out = Vec::with_capacity(bytes.len() + 1024);
    let status = compress.compress_vec(bytes, &mut out, flate2::FlushCompress::Finish)?;
    if status != flate2::Status::StreamEnd {
        return Err("a file of the English package was not deflated whole".into());
    }
    Ok(out)
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
    // Each file deflated (the table is half its size, #428): the program
    // carries them so and inflates them as they are written to the card.
    let deflated = out.join("deflated");
    let _ = fs::remove_dir_all(&deflated);
    fs::create_dir_all(&deflated)?;
    let mut source = String::from("&[\n");
    for (at, path) in found.iter().enumerate() {
        let bytes = fs::read(dir.join(path))?;
        let file = deflated.join(format!("{at}.deflate"));
        fs::write(&file, deflate(&bytes)?)?;
        writeln!(
            source,
            "    ({path:?}, {}, include_bytes!({:?})),",
            bytes.len(),
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
