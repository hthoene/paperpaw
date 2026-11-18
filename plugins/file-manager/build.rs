mod declarations;

use std::{fmt::Write, path::PathBuf};

type Emit = fn(&mut dyn FnMut(&[u8]) -> Result<(), i32>) -> Result<(), String>;

/// Emits one family of declarations as statics: `NAME_0`, `NAME_1`, … and the
/// slice `NAME` over them; returns the length-framed bytes for the golden.
fn emit(
    source: &mut String,
    name: &str,
    build: impl FnOnce(&mut dyn FnMut(&[u8]) -> Result<(), i32>) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let mut count = 0;
    let mut framed = Vec::new();
    build(&mut |batch| {
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

/// The batches of length-framed `bytes` (as [`emit`] returns them).
fn unframe(bytes: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut batches = Vec::new();
    let mut rest = bytes;
    while let Some((size, tail)) = rest.split_first_chunk::<4>() {
        let size = usize::try_from(u32::from_le_bytes(*size)).map_err(|_| "batch size")?;
        let (batch, tail) = tail.split_at_checked(size).ok_or("truncated batch")?;
        batches.push(batch);
        rest = tail;
    }
    if !rest.is_empty() {
        return Err("bytes left after the last batch".into());
    }
    Ok(batches)
}

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    println!("cargo:rerun-if-changed=testdata/declarations.bin.v1");
    let mut source = String::new();
    let settings = emit(&mut source, "SETTINGS", |sink| {
        declarations::settings(sink).map_err(|e| format!("invalid File manager settings: {e:?}"))
    })?;
    if settings != include_bytes!("testdata/declarations.bin.v1") {
        // The new bytes are left to be read and blessed on purpose.
        let new = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?)
            .join("declarations.bin.new");
        std::fs::write(&new, &settings).map_err(|e| format!("write {}: {e}", new.display()))?;
        return Err(format!(
            "File manager settings differ from the reviewed golden; the new bytes are in {}",
            new.display()
        ));
    }
    let emits: [(&str, Emit); 1] = [("UI", |sink| {
        declarations::ui(sink).map_err(|e| format!("invalid File manager UI declarations: {e:?}"))
    })];
    // The UI batches name ids too (actions, screens) for the id table.
    let mut ui = Vec::new();
    for (name, build) in emits {
        ui.extend(emit(&mut source, name, build)?);
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    // The settings as a table the kernel keeps in the program (a linked
    // build only), made of the very records declared above (the golden's
    // length-framed batches).
    let batches = unframe(&settings)?;
    let mut named = batches.clone();
    named.extend(unframe(&ui)?);
    let table =
        paperpaw_sdk::native::declare::fixed_settings_source("file-manager", "SETTINGS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_groups_source(
                "file-manager",
                "GROUPS",
                &batches,
            )?
            + &paperpaw_sdk::native::declare::fixed_ids_source("file-manager", "IDS", &named)?
            + &paperpaw_sdk::native::declare::fixed_titles_source(
                "file-manager",
                "TITLES",
                &named,
            )?;
    std::fs::write(out.join("file_manager_settings.rs"), table)
        .map_err(|error| format!("write settings table: {error}"))?;
    std::fs::write(out.join("file_manager_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))?;
    Ok(())
}
