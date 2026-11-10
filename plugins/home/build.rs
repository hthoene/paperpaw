//! Turns the declarations into read-only batches built into the plugin image.
mod declarations;

use paperpaw_sdk::native::declare::Declarations;
use std::{fmt::Write, path::PathBuf};

/// Runs `$declare` against a sink that writes each batch as a static.
macro_rules! emit {
    ($source:expr, $name:literal, $declare:path) => {{
        let mut count = 0;
        let mut batches = String::new();
        {
            let mut d = Declarations::new(|batch: &[u8]| -> Result<(), i32> {
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
    emit!(source, "SETTINGS", declarations::settings);
    emit!(source, "WIDGETS", declarations::widgets);
    emit!(source, "SCREENS", declarations::screens);
    emit!(source, "NAVIGATION", declarations::navigation);
    emit!(source, "EVENTS", declarations::events);
    emit!(source, "HELP", declarations::help);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    std::fs::write(out.join("home_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))
}
