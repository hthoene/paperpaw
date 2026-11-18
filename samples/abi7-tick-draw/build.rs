use std::{env, fs, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::var("TARGET")? != "x86_64-unknown-linux-gnu" {
        return Err("this inactive sample currently qualifies Linux x86_64 only".into());
    }
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../..");
    let out = PathBuf::from(env::var("OUT_DIR")?);
    let status = Command::new("python3")
        .arg(root.join("tools/abi-metadata.py"))
        .arg("--schema")
        .arg(root.join("abi/abi.def"))
        .arg("--input")
        .arg("metadata.json")
        .arg("--bytes")
        .arg(out.join("metadata.bin"))
        .arg("--json")
        .arg(out.join("metadata.json"))
        .status()?;
    if !status.success() {
        return Err("canonical metadata generation failed".into());
    }
    let bytes = fs::read(out.join("metadata.bin"))?;
    fs::write(
        out.join("metadata.rs"),
        format!(
            "#[unsafe(no_mangle)]\npub static PAPERPAW_METADATA: [u8; {}] = {:?};\n",
            bytes.len(),
            bytes
        ),
    )?;
    for file in [
        "metadata.json",
        "../../abi/abi.def",
        "../../abi/generated/paperpaw_abi.rs",
        "../../abi/generated/records_wire.rs",
        "../../tools/abi-metadata.py",
        "../../tools/abi_metadata.py",
        "../../tools/abi-gen.py",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    Ok(())
}
