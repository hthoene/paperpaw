//! The commands for native plugins: making a plugin file, showing what
//! one holds, signing it as its author and checking its signature, and
//! the check that crates keep no mutable statics.

use paperpaw_digest::{hex, sha256::Sha256};
use paperpaw_native::header5::Header5;
use paperpaw_native::{Signature, signature};
use paperpaw_update::{fingerprint, key_hex};

use crate::pack5;
use crate::{secret_key, statics};

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}

/// The format a plugin file says it has, where it is one of a format before
/// 5 (the plugin interfaces before 7): such a file is named for what it is
/// and not read any further.
fn older_format(file: &[u8]) -> Option<u16> {
    (file.len() >= 6 && file[..4] == paperpaw_native::MAGIC)
        .then(|| u16::from_le_bytes([file[4], file[5]]))
        .filter(|format| *format < paperpaw_native::header5::FORMAT)
}

/// What is said of a file of a format before 5.
fn older(path: &str, format: u16) -> String {
    format!(
        "{path}: a plugin file of format {format}, made for a PaperPaw before plugin interface 7: \
         this tool no longer reads it"
    )
}

/// A format-5 file, read as the loader reads it, and its signature if it
/// ends with one.
fn plugin_file5(path: &str) -> Result<(Vec<u8>, Header5, Option<Signature>), String> {
    let file = read(path)?;
    if let Some(format) = older_format(&file) {
        return Err(older(path, format));
    }
    let (header, signature) = pack5::check_file5(&file).map_err(|e| format!("{path}: {e}"))?;
    Ok((file, header, signature))
}

/// `pack`: the file of format 5 of a linked interface-7 plugin.
pub fn pack(elf: &str, out: &str, key_file: Option<&str>) -> Result<(), String> {
    let bytes = read(elf)?;
    pack5::is_interface7(&bytes).map_err(|e| format!("{elf}: {e}"))?;
    let packed = pack5::pack5(&bytes).map_err(|e| format!("{elf}: {e}"))?;
    let header = &packed.header;
    let mut file = packed.file.clone();
    let signed = match key_file {
        Some(key_file) => {
            let seed = secret_key(Some(key_file))?;
            let signed = signature::sign(&file, &seed);
            file.extend_from_slice(&signed.to_bytes());
            Some(signed)
        }
        None => None,
    };
    write(out, &file)?;
    println!(
        "{out}: format 5, {} bytes of data, {} of code, {} words to relocate, interface {}.{}, {}",
        header.code_at,
        header.image_len - header.code_at,
        header.relocs,
        header.abi_major,
        header.needs_minor,
        header.target
    );
    if header.hot_len > 0 {
        println!(
            "{out}: {} bytes of hot code at {}, {} patches",
            header.hot_len, header.hot_at, header.patches
        );
    }
    if let Some(signed) = signed {
        print_signer(&signed);
    }
    Ok(())
}

/// `plugin`: what a plugin file holds. A file of a format before 5 is named
/// as that, in one line, and refused: nothing is said of it on standard
/// output, and the tool fails.
pub fn show(path: &str) -> Result<(), String> {
    let (file, header, signed) = plugin_file5(path)?;
    let mut sha = Sha256::new();
    sha.update(&file);
    println!("format:      5 (interface 7)");
    println!("chip:        {} ({})", header.target, header.target.id());
    println!(
        "interface:   {}, needs minor {}",
        header.abi_major, header.needs_minor
    );
    println!(
        "image:       {} bytes: {} of data, {} of code, linked at {:#010x}, aligned to {}",
        header.image_len,
        header.code_at,
        header.image_len - header.code_at,
        header.base,
        header.align
    );
    println!(
        "entry:       at {}, metadata record of {} bytes at {}",
        header.entry, header.meta_len, header.meta_at
    );
    println!("relocations: {} words", header.relocs);
    if let Some(at) = header.heap_at {
        println!("heap word:   at {at}, filled by the loader");
    }
    println!(
        "hot code:    {} bytes at {}, {} patches",
        header.hot_len, header.hot_at, header.patches
    );
    println!(
        "file:        {} bytes, SHA-256 {}",
        file.len(),
        hex::encode(&sha.finish())
    );
    println!("digest:      {}", hex::encode(&header.sha256));
    match signed {
        Some(signed) => print_signer(&signed),
        None => println!("signer:      none, not signed"),
    }
    Ok(())
}

fn print_signer(signed: &Signature) {
    println!(
        "signer:      {} ({})",
        fingerprint(&signed.key),
        key_hex(&signed.key)
    );
    println!("signature:   holds ({})", hex::encode(&signed.signature));
}

/// Signs a plugin file as its author, its signature put at its end (a
/// signature it had before replaced).
pub fn sign(path: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = secret_key(key_file)?;
    let (mut file, header, _) = plugin_file5(path)?;
    let len = header.file_len() as usize;
    file.truncate(len);
    let signed = signature::sign(&file, &seed);
    file.extend_from_slice(&signed.to_bytes());
    write(path, &file)?;
    println!("{path}: signed");
    print_signer(&signed);
    Ok(())
}

pub fn check(path: &str) -> Result<(), String> {
    let (file, header, signed) = plugin_file5(path)?;
    let signed = signed.ok_or_else(|| format!("{path}: not signed"))?;
    println!(
        "plugin:      {} bytes of format 5 for the {}, interface {}.{}",
        file.len(),
        header.target,
        header.abi_major,
        header.needs_minor
    );
    print_signer(&signed);
    Ok(())
}

pub fn statics(archives: &[&str]) -> Result<(), String> {
    let mut count = 0;
    for path in archives {
        let found = statics::statics(&read(path)?).map_err(|e| format!("{path}: {e}"))?;
        let name = statics::crate_name(path);
        for s in &found {
            println!(
                "{name}: {}: {} ({}, {} bytes)",
                s.object, s.symbol, s.section, s.size
            );
        }
        count += found.len();
    }
    if count > 0 {
        return Err(format!(
            "{count} mutable statics: a native plugin with these crates could not load"
        ));
    }
    println!("no mutable statics in {} archives", archives.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real sample's file (`samples/abi7-x4-min`), not signed.
    const SAMPLE: &[u8] =
        include_bytes!("../../../crates/paperpaw-native/tests/data/abi7-x4-min.ppn");

    #[test]
    fn a_signed_plugin_checks_and_a_changed_one_does_not() {
        let dir = std::env::temp_dir().join(format!("paperpaw-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let (key, file) = (path("key"), path("plugin.ppn"));
        std::fs::write(&key, format!("{}\n", "07".repeat(32))).unwrap();
        let unsigned = SAMPLE.to_vec();
        std::fs::write(&file, &unsigned).unwrap();
        assert!(check(&file).unwrap_err().ends_with("not signed"));

        sign(&file, Some(&key)).unwrap();
        let signed = std::fs::read(&file).unwrap();
        assert_eq!(signed.len(), unsigned.len() + signature::SIGNATURE_LEN);
        assert_eq!(signed[..unsigned.len()], unsigned[..], "the file is kept");
        check(&file).unwrap();
        show(&file).unwrap();
        // Signed again: one signature, the new one.
        sign(&file, Some(&key)).unwrap();
        assert_eq!(
            std::fs::read(&file).unwrap(),
            signed,
            "Ed25519 signs deterministically"
        );

        // A changed byte: the file no longer holds, before its signature.
        let mut changed = signed.clone();
        changed[100] ^= 1;
        std::fs::write(&file, &changed).unwrap();
        assert!(check(&file).is_err());
        // A changed signature.
        let mut changed = signed;
        let last = changed.len() - 20;
        changed[last] ^= 1;
        std::fs::write(&file, &changed).unwrap();
        assert!(
            check(&file)
                .unwrap_err()
                .ends_with("its author's signature does not hold")
        );
        // A file of the format before: named as that, not read, not signed.
        let mut old = SAMPLE.to_vec();
        old[4..6].copy_from_slice(&4u16.to_le_bytes());
        std::fs::write(&file, &old).unwrap();
        assert!(show(&file).unwrap_err().contains("format 4"));
        assert!(check(&file).unwrap_err().contains("format 4"));
        assert!(sign(&file, Some(&key)).unwrap_err().contains("format 4"));
        assert_eq!(std::fs::read(&file).unwrap(), old);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
