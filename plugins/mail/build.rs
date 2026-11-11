#[allow(
    dead_code,
    reason = "the actions, the entry and the screens are for the declaration tests"
)]
mod declarations;

use std::{fmt::Write, path::PathBuf};

/// Emits one family of declarations as statics: `NAME_0`, `NAME_1`, … and the
/// slice `NAME` over them; returns the length-framed bytes for the golden.
fn emit(
    source: &mut String,
    all: &mut Vec<Vec<u8>>,
    name: &str,
    build: impl FnOnce(&mut dyn FnMut(&[u8]) -> Result<(), i32>) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let mut count = 0;
    let mut framed = Vec::new();
    build(&mut |batch| {
        all.push(batch.to_vec());
        writeln!(
            source,
            "pub static {name}_{count}: [u8; {}] = {batch:?};",
            batch.len()
        )
        .map_err(|_| -2)?;
        framed.extend_from_slice(&u32::try_from(batch.len()).map_err(|_| -2)?.to_le_bytes());
        framed.extend_from_slice(batch);
        count += 1;
        Ok(())
    })?;
    writeln!(source, "pub static {name}: &[&[u8]] = &[").map_err(|e| e.to_string())?;
    for index in 0..count {
        writeln!(source, "&{name}_{index},").map_err(|e| e.to_string())?;
    }
    source.push_str("];\n");
    Ok(framed)
}

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    println!("cargo:rerun-if-changed=testdata/declarations.bin.v1");
    let mut source = String::new();
    let mut all: Vec<Vec<u8>> = Vec::new();
    let settings = emit(&mut source, &mut all, "SETTINGS", |sink| {
        declarations::settings(sink).map_err(|e| format!("invalid Mail settings: {e:?}"))
    })?;
    if settings != include_bytes!("testdata/declarations.bin.v1") {
        // The new bytes are left to be read and blessed on purpose.
        let new = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?)
            .join("declarations.bin.new");
        std::fs::write(&new, &settings).map_err(|e| format!("write {}: {e}", new.display()))?;
        return Err(format!(
            "Mail settings differ from the reviewed golden; the new bytes are in {}",
            new.display()
        ));
    }
    emit(&mut source, &mut all, "UI", |sink| {
        declarations::ui(sink).map_err(|e| format!("invalid Mail actions: {e:?}"))
    })?;
    emit(&mut source, &mut all, "SCREENS", |sink| {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(sink);
        declarations::screens(&mut d).map_err(|e| format!("invalid Mail screens: {e:?}"))
    })?;
    emit(&mut source, &mut all, "EVENTS", |sink| {
        declarations::events(sink).map_err(|e| format!("invalid Mail event declarations: {e:?}"))
    })?;
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    // The settings, setting groups and ids as tables the kernel keeps in the
    // program (a linked build only), made of the very records declared above.
    let batches: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
    let table = paperpaw_sdk::native::declare::fixed_settings_source("mail", "SETTINGS", &batches)?
        + &paperpaw_sdk::native::declare::fixed_groups_source("mail", "GROUPS", &batches)?
        + &paperpaw_sdk::native::declare::fixed_ids_source("mail", "IDS", &batches)?;
    std::fs::write(out.join("mail_settings.rs"), table)
        .map_err(|error| format!("write settings table: {error}"))?;
    std::fs::write(out.join("mail_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))?;
    Ok(())
}
