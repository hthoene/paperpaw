//! Turns the declarations into read-only batches built into the plugin image.
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

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    let mut source = String::new();
    let mut all: Vec<Vec<u8>> = Vec::new();
    emit!(source, all, "SETTINGS", declarations::settings);
    emit!(source, all, "SCREENS", declarations::screens);
    emit!(source, all, "NAVIGATION", declarations::navigation);
    emit!(source, all, "EVENTS", declarations::events);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    // The settings, setting groups and ids as tables the kernel keeps in the
    // program (a linked build only), made of the very records declared above.
    let batches: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
    let table = paperpaw_sdk::native::declare::fixed_settings_source("wifi", "SETTINGS", &batches)?
        + &paperpaw_sdk::native::declare::fixed_groups_source("wifi", "GROUPS", &batches)?
        + &paperpaw_sdk::native::declare::fixed_ids_source("wifi", "IDS", &batches)?;
    std::fs::write(out.join("wifi_settings.rs"), table)
        .map_err(|error| format!("write settings table: {error}"))?;
    std::fs::write(out.join("wifi_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))
}
