mod declarations;

use std::{fmt::Write, path::PathBuf};

fn batches(
    source: &mut String,
    name: &str,
    build: impl FnOnce(&mut dyn FnMut(&[u8]) -> Result<(), i32>) -> Result<(), String>,
) -> Result<(), String> {
    let mut count = 0;
    build(&mut |batch: &[u8]| {
        writeln!(
            source,
            "pub static {name}_{count}: [u8; {}] = {batch:?};",
            batch.len()
        )
        .map_err(|_| -2)?;
        count += 1;
        Ok(())
    })?;
    writeln!(source, "pub static {name}: &[&[u8]] = &[").map_err(|e| e.to_string())?;
    for index in 0..count {
        writeln!(source, "&{name}_{index},").map_err(|e| e.to_string())?;
    }
    source.push_str("];\n");
    Ok(())
}

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    let mut source = String::new();
    batches(&mut source, "UI", |sink| {
        declarations::ui(sink).map_err(|error| format!("invalid Setup UI declarations: {error:?}"))
    })?;
    batches(&mut source, "EVENTS", |sink| {
        declarations::events(sink)
            .map_err(|error| format!("invalid Setup event declarations: {error:?}"))
    })?;
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    std::fs::write(out.join("setup_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))?;
    Ok(())
}
