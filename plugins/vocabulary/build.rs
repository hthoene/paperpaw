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

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=declarations.rs");
    println!("cargo:rerun-if-changed=testdata/declarations.bin.v1");
    let mut source = String::new();
    let settings = emit(&mut source, "SETTINGS", |sink| {
        declarations::settings(sink).map_err(|e| format!("invalid Vocabulary settings: {e:?}"))
    })?;
    if settings != include_bytes!("testdata/declarations.bin.v1") {
        // The new bytes are left to be read and blessed on purpose.
        let new = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?)
            .join("declarations.bin.new");
        std::fs::write(&new, &settings).map_err(|e| format!("write {}: {e}", new.display()))?;
        return Err(format!(
            "Vocabulary settings differ from the reviewed golden; the new bytes are in {}",
            new.display()
        ));
    }
    let emits: [(&str, Emit); 6] = [
        ("HELP", |sink| {
            declarations::help(sink)
                .map_err(|e| format!("invalid Vocabulary help declarations: {e:?}"))
        }),
        ("NAVIGATION", |sink| {
            declarations::navigation(sink)
                .map_err(|e| format!("invalid Vocabulary navigation declarations: {e:?}"))
        }),
        ("WIDGETS", |sink| {
            declarations::widgets(sink)
                .map_err(|e| format!("invalid Vocabulary widget declarations: {e:?}"))
        }),
        ("STAGES", |sink| {
            declarations::stages(sink)
                .map_err(|e| format!("invalid Vocabulary stage declarations: {e:?}"))
        }),
        ("EVENTS", |sink| {
            declarations::events(sink)
                .map_err(|e| format!("invalid Vocabulary event declarations: {e:?}"))
        }),
        ("UI", |sink| {
            declarations::ui(sink).map_err(|e| format!("invalid Vocabulary UI declarations: {e:?}"))
        }),
    ];
    for (name, build) in emits {
        emit(&mut source, name, build)?;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?);
    std::fs::write(out.join("vocabulary_declarations.rs"), source)
        .map_err(|error| format!("write declarations: {error}"))?;
    Ok(())
}
