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
    .map_err(|error| format!("invalid Power settings: {error:?}"))?;
    if framed != include_bytes!("testdata/declarations.bin.v1") {
        return Err("Power settings differ from the reviewed golden".into());
    }
    source.push_str("pub static SETTINGS: &[&[u8]] = &[\n");
    for index in 0..count {
        writeln!(source, "&SETTINGS_{index},")
            .map_err(|error| format!("write static reference: {error}"))?;
    }
    source.push_str("];\n");
    let mut ui_count = 0;
    // The UI batches too name ids (actions, screens) for the id table.
    let mut ui_batches: Vec<Vec<u8>> = Vec::new();
    declarations::ui(|batch| {
        ui_batches.push(batch.to_vec());
        writeln!(
            source,
            "pub static UI_{ui_count}: [u8; {}] = {batch:?};",
            batch.len()
        )
        .map_err(|_| -2)?;
        ui_count += 1;
        Ok(())
    })
    .map_err(|error| format!("invalid Power UI declarations: {error:?}"))?;
    source.push_str("pub static UI: &[&[u8]] = &[\n");
    for index in 0..ui_count {
        writeln!(source, "&UI_{index},").map_err(|error| format!("write UI reference: {error}"))?;
    }
    source.push_str("];\n");
    let mut events_count = 0;
    declarations::events(|batch| {
        writeln!(
            source,
            "pub static EVENTS_{events_count}: [u8; {}] = {batch:?};",
            batch.len()
        )
        .map_err(|_| -2)?;
        events_count += 1;
        Ok(())
    })
    .map_err(|error| format!("invalid Power event declarations: {error:?}"))?;
    source.push_str("pub static EVENTS: &[&[u8]] = &[\n");
    for index in 0..events_count {
        writeln!(source, "&EVENTS_{index},")
            .map_err(|error| format!("write events reference: {error}"))?;
    }
    source.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    // The settings as a table the kernel keeps in the program (a linked
    // build only), made of the very records declared above (the golden's
    // length-framed batches).
    let mut batches: Vec<&[u8]> = Vec::new();
    let mut rest = framed.as_slice();
    while let Some((size, tail)) = rest.split_first_chunk::<4>() {
        let size = usize::try_from(u32::from_le_bytes(*size)).map_err(|_| "batch size")?;
        let (batch, tail) = tail.split_at_checked(size).ok_or("truncated batch")?;
        batches.push(batch);
        rest = tail;
    }
    if !rest.is_empty() {
        return Err("bytes left after the last batch of the golden".into());
    }
    let mut named = batches.clone();
    named.extend(ui_batches.iter().map(Vec::as_slice));
    let table =
        paperpaw_sdk::native::declare::fixed_settings_source("power", "SETTINGS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_groups_source("power", "GROUPS", &batches)?
            + &paperpaw_sdk::native::declare::fixed_ids_source("power", "IDS", &named)?;
    std::fs::write(out.join("power_settings.rs"), table)
        .map_err(|error| format!("write settings table: {error}"))?;
    std::fs::write(out.join("power_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))?;
    Ok(())
}
