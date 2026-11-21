//! Turns the declarations into read-only batches built into the plugin image,
//! and packs the licences for the build's product: the firmware's
//! (`data/licences-firmware.md`, for the readers, `target_os = "none"`) or the
//! apps' (`data/licences-app.md`), both from `tools/licences/build.py`. They
//! are split into parts of about [`PART`] bytes at paragraphs, each deflated
//! on its own, so that the page writes them out one part at a time, with
//! little memory, and they take little flash (as Beta 3 packed them).
mod declarations;

use paperpaw_sdk::native::declare::Declarations;
use std::{fmt::Write, path::PathBuf};

/// The largest part of the licences, before it is deflated.
const PART: usize = 8 * 1024;

/// Runs `$declare` against a sink that writes each batch as a static.
macro_rules! emit {
    ($source:expr, $all:expr, $name:literal, $declare:path) => {{
        let mut count = 0;
        let mut batches = String::new();
        {
            let mut d = Declarations::new(|batch: &[u8]| -> Result<(), i32> {
                $all.push(batch.to_vec());
                writeln!(
                    batches,
                    "pub static {}_{count}: [u8; {}] = {batch:?};",
                    $name,
                    batch.len()
                )
                .map_err(|_| -2)?;
                count += 1;
                Ok(())
            });
            $declare(&mut d)
                .map_err(|error| format!("invalid {} declarations: {error:?}", $name))?;
        }
        $source.push_str(&batches);
        let _ = writeln!($source, "pub static {}: &[&[u8]] = &[", $name);
        for index in 0..count {
            let _ = writeln!($source, "&{}_{index},", $name);
        }
        $source.push_str("];\n");
    }};
}

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    let mut source = String::new();
    let mut all: Vec<Vec<u8>> = Vec::new();
    emit!(source, all, "SCREENS", declarations::screens);
    emit!(source, all, "NAVIGATION", declarations::navigation);
    emit!(source, all, "STAGE_SPECS", declarations::stages);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    // The settings, setting groups and ids as tables the kernel keeps in the
    // program (a linked build only), made of the very records declared above.
    let batches: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
    let table =
        paperpaw_sdk::native::declare::fixed_settings_source("about", "SETTINGS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_groups_source("about", "GROUPS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_ids_source("about", "IDS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_titles_source("about", "TITLES", &batches)?;
    std::fs::write(out.join("about_settings.rs"), table)
        .map_err(|error| format!("write settings table: {error}"))?;
    std::fs::write(out.join("about_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))?;
    pack_licences(&out)
}

/// Writes `licences.bin`: each part as its deflated length and its length
/// (little-endian `u32`s), then the deflated bytes.
fn pack_licences(out: &std::path::Path) -> Result<(), String> {
    let product = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("none") => "firmware",
        _ => "app",
    };
    let source = format!("data/licences-{product}.md");
    println!("cargo:rerun-if-changed={source}");
    let text = std::fs::read_to_string(&source).map_err(|error| format!("{source}: {error}"))?;
    let mut packed = Vec::new();
    for part in parts(&text) {
        let deflated = miniz_oxide::deflate::compress_to_vec(part.as_bytes(), 10);
        let size = |n: usize| u32::try_from(n).map_err(|_| format!("{source}: part too large"));
        packed.extend_from_slice(&size(deflated.len())?.to_le_bytes());
        packed.extend_from_slice(&size(part.len())?.to_le_bytes());
        packed.extend_from_slice(&deflated);
    }
    std::fs::write(out.join("licences.bin"), packed)
        .map_err(|error| format!("write licences: {error}"))
}

/// `text` in parts of at most [`PART`] bytes, cut after an empty line where
/// there is one (else at a line, else at a character).
fn parts(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = text;
    while rest.len() > PART {
        let mut end = PART;
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let window = &rest[..end];
        let cut = window
            .rfind("\n\n")
            .map(|at| at + 2)
            .or_else(|| window.rfind('\n').map(|at| at + 1))
            .filter(|at| *at > 0)
            .unwrap_or(end);
        parts.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        parts.push(rest);
    }
    parts
}
