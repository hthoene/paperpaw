// Legacy conversion is confined to this fixture generator. A ported plugin
// shares its borrowed declaration function with build.rs instead.
mod fixture_conversion;

use paperpaw_sdk::{Id, Plugin, Registrar, native::declare::Declarations};
use std::{fmt::Write, path::PathBuf};

fn emit(id: &'static str, plugin: &impl Plugin) {
    let mut registrar = Registrar::new(Id::new_static(id));
    plugin.register(&mut registrar).unwrap();
    let contributions = registrar.into_contributions();
    let mut source = String::new();
    let mut batch_count = 0;
    let mut framed = Vec::new();
    let mut builder = Declarations::new(|batch: &[u8]| {
        // These are the canonical encoder's bytes, not a second wire codec.
        writeln!(
            source,
            "pub static BATCH_{batch_count}: [u8; {}] = {batch:?};",
            batch.len()
        )
        .unwrap();
        batch_count += 1;
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(batch);
        Ok(())
    });
    fixture_conversion::submit(id, &contributions, &mut builder).unwrap();
    drop(builder);
    source.push_str("pub static DECLARATIONS: &[&[u8]] = &[\n");
    for index in 0..batch_count {
        writeln!(source, "&BATCH_{index},").unwrap();
    }
    source.push_str("];\n");
    let golden = format!("../../plugins/{id}/testdata/declarations.bin.v1");
    assert_eq!(
        framed,
        std::fs::read(&golden).unwrap(),
        "{id} golden changed"
    );
    println!("cargo:rerun-if-changed={golden}");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join(format!("{id}.rs")), source).unwrap();
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=fixture_conversion.rs");
    println!("cargo:rerun-if-changed=../../plugins/widgets/src");
    println!("cargo:rerun-if-changed=../../plugins/battery/src");
    emit("battery", &paperpaw_plugin_battery::Battery);
    emit("widgets", &paperpaw_plugin_widgets::Widgets);
}
