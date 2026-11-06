mod declarations;

use std::{fmt::Write, path::PathBuf};

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    println!("cargo:rerun-if-changed=testdata/declarations.bin.v1");
    let mut source = String::new();
    let mut framed = Vec::new();
    let mut count = 0;
    declarations::settings(|batch| {
        writeln!(
            source,
            "pub static SETTINGS_{count}: [u8; {}] = {batch:?};",
            batch.len()
        )
        .map_err(|_| -2)?;
        framed.extend_from_slice(&u32::try_from(batch.len()).map_err(|_| -2)?.to_le_bytes());
        framed.extend_from_slice(batch);
        count += 1;
        Ok(())
    })
    .map_err(|error| format!("invalid Battery settings: {error:?}"))?;
    if framed != include_bytes!("testdata/declarations.bin.v1") {
        return Err("Battery settings differ from the reviewed golden".into());
    }
    source.push_str("pub static SETTINGS: &[&[u8]] = &[\n");
    for index in 0..count {
        writeln!(source, "&SETTINGS_{index},")
            .map_err(|error| format!("write static reference: {error}"))?;
    }
    source.push_str("];\n");
    let mut ui_count = 0;
    declarations::ui(|batch| {
        writeln!(
            source,
            "pub static UI_{ui_count}: [u8; {}] = {batch:?};",
            batch.len()
        )
        .map_err(|_| -2)?;
        ui_count += 1;
        Ok(())
    })
    .map_err(|error| format!("invalid Battery UI declarations: {error:?}"))?;
    source.push_str("pub static UI: &[&[u8]] = &[\n");
    for index in 0..ui_count {
        writeln!(source, "&UI_{index},").map_err(|error| format!("write UI reference: {error}"))?;
    }
    source.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    std::fs::write(out.join("battery_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))?;
    Ok(())
}
