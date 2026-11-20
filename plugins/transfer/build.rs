//! Turns the declarations into read-only batches built into the plugin image.
#[allow(
    dead_code,
    reason = "the build script uses the builders, not the tokens"
)]
mod declarations;

use paperpaw_sdk::native::declare::Declarations;
use std::{fmt::Write, path::PathBuf};

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

/// The web page (`data/page.html`) as the parts of one gzip stream, which the
/// plugin puts together with the strings where the page names them
/// (`native_gzip.rs`): the text before the placeholder deflated up to a
/// byte boundary (no last block), the text after it deflated to the end, and
/// the checksum and length of each.
fn page(out: &std::path::Path) -> Result<(), String> {
    use flate2::{Compress, Compression, Crc, FlushCompress, Status};
    const PLACEHOLDER: &str = "/*STRINGS*/{}";
    println!("cargo:rerun-if-changed=data/page.html");
    let page = std::fs::read_to_string("data/page.html")
        .map_err(|error| format!("read data/page.html: {error}"))?;
    let at = page
        .find(PLACEHOLDER)
        .ok_or("data/page.html names no place for the strings")?;
    let (head, tail) = (&page[..at], &page[at + PLACEHOLDER.len()..]);
    let mut source = String::new();
    for (name, bytes, flush) in [
        ("HEAD", head.as_bytes(), FlushCompress::Sync),
        ("TAIL", tail.as_bytes(), FlushCompress::Finish),
    ] {
        let mut compress = Compress::new(Compression::best(), false);
        let mut packed = Vec::with_capacity(bytes.len() + 1024);
        let status = compress
            .compress_vec(bytes, &mut packed, flush)
            .map_err(|error| format!("deflate the page: {error}"))?;
        // Sync ends the part on a byte boundary, as a stored block of no bytes.
        if flush == FlushCompress::Finish && status != Status::StreamEnd {
            return Err("the page was not deflated whole".into());
        }
        let mut crc = Crc::new();
        crc.update(bytes);
        std::fs::write(
            out.join(format!("page_{}.deflate", name.to_lowercase())),
            &packed,
        )
        .map_err(|error| format!("write the page's {name}: {error}"))?;
        source.push_str(&format!(
            "pub const {name}_CRC: u32 = {};\npub const {name}_LEN: u32 = {};\n",
            crc.sum(),
            bytes.len()
        ));
    }
    std::fs::write(out.join("page_parts.rs"), source)
        .map_err(|error| format!("write the page's parts: {error}"))
}

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    let mut source = String::new();
    let mut all: Vec<Vec<u8>> = Vec::new();
    emit!(source, all, "SCREENS", declarations::screens);
    emit!(source, all, "CABLE", declarations::cable);
    emit!(source, all, "NAVIGATION", declarations::navigation);
    emit!(source, all, "EVENTS", declarations::events);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    page(&out)?;
    // The settings, setting groups and ids as tables the kernel keeps in the
    // program (a linked build only), made of the very records declared above.
    let batches: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
    let table =
        paperpaw_sdk::native::declare::fixed_settings_source("transfer", "SETTINGS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_groups_source("transfer", "GROUPS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_ids_source("transfer", "IDS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_titles_source("transfer", "TITLES", &batches)?;
    std::fs::write(out.join("transfer_settings.rs"), table)
        .map_err(|error| format!("write settings table: {error}"))?;
    std::fs::write(out.join("transfer_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))
}
