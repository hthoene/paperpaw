//! The commands for native plugins: making a plugin file, showing what
//! one holds, signing it as its author and checking its signature, and
//! the check that crates keep no mutable statics.

use paperpaw_digest::{hex, sha256::Sha256};
use paperpaw_native::header5::Header5;
use paperpaw_native::{Header, Signature, signature};
use paperpaw_update::{fingerprint, key_hex};

use crate::pack5;
use crate::{secret_key, statics};

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}

/// A plugin file, checked whole, and its author's signature if it ends
/// with one.
fn plugin_file(path: &str) -> Result<(Vec<u8>, Header, Option<Signature>), String> {
    let file = read(path)?;
    let (header, signature) = Header::check_file(&file).map_err(|e| format!("{path}: {e}"))?;
    Ok((file, header, signature))
}

/// Whether `file` is a plugin file of format 5 (interface 7): by its header,
/// not by its name.
fn is_format5(file: &[u8]) -> bool {
    file.len() >= 6
        && file[..4] == paperpaw_native::MAGIC
        && u16::from_le_bytes([file[4], file[5]]) == paperpaw_native::header5::FORMAT
}

/// A format-5 file, read as the loader reads it, and its signature if it
/// ends with one.
fn plugin_file5(path: &str) -> Result<(Vec<u8>, Header5, Option<Signature>), String> {
    let file = read(path)?;
    let (header, signature) = pack5::check_file5(&file).map_err(|e| format!("{path}: {e}"))?;
    Ok((file, header, signature))
}

pub fn pack(elf: &str, out: &str, key_file: Option<&str>) -> Result<(), String> {
    let bytes = read(elf)?;
    if pack5::is_interface7(&bytes).map_err(|e| format!("{elf}: {e}"))? {
        return pack_format5(elf, &bytes, out, key_file);
    }
    let packed = crate::pack::pack(&bytes).map_err(|e| format!("{elf}: {e}"))?;
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
        "{out}: {} bytes of data, {} of code, {} words to relocate, fingerprint {:016x}, {},          built by rustc {}",
        header.code_at,
        header.image_len - header.code_at,
        header.relocs,
        header.fingerprint,
        header.chip,
        packed.compiler
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

/// `pack` for an interface-7 plugin: the file of format 5.
fn pack_format5(elf: &str, bytes: &[u8], out: &str, key_file: Option<&str>) -> Result<(), String> {
    let packed = pack5::pack5(bytes).map_err(|e| format!("{elf}: {e}"))?;
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

/// `show` for a format-5 file.
fn show5(path: &str) -> Result<(), String> {
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

pub fn show(path: &str) -> Result<(), String> {
    if is_format5(&read(path)?) {
        return show5(path);
    }
    let (file, header, signed) = plugin_file(path)?;
    let mut sha = Sha256::new();
    sha.update(&file);
    println!("chip:        {} ({})", header.chip, header.chip.id());
    println!("fingerprint: {:016x}", header.fingerprint);
    // The plugin interface it was built for: the fingerprint's top bits.
    println!("interface:   {}", header.fingerprint >> 48);
    println!("compiler:    {:08x}", header.compiler);
    println!(
        "image:       {} bytes: {} of data, {} of code, linked at {:#010x}, aligned to {}",
        header.image_len,
        header.code_at,
        header.image_len - header.code_at,
        header.base,
        header.align
    );
    println!(
        "entry:       at {}, the table's word at {}",
        header.entry, header.api
    );
    println!("relocations: {} words", header.relocs);
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
    let (mut file, len) = if is_format5(&read(path)?) {
        let (file, header, _) = plugin_file5(path)?;
        (file, header.file_len() as usize)
    } else {
        let (file, header, _) = plugin_file(path)?;
        (file, header.file_len() as usize)
    };
    file.truncate(len);
    let signed = signature::sign(&file, &seed);
    file.extend_from_slice(&signed.to_bytes());
    write(path, &file)?;
    println!("{path}: signed");
    print_signer(&signed);
    Ok(())
}

pub fn check(path: &str) -> Result<(), String> {
    if is_format5(&read(path)?) {
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
        return Ok(());
    }
    let (file, header, signed) = plugin_file(path)?;
    let signed = signed.ok_or_else(|| format!("{path}: not signed"))?;
    println!(
        "plugin:      {} bytes for the {}, fingerprint {:016x}",
        file.len(),
        header.chip,
        header.fingerprint
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
    use crate::elf::EM_RISCV;
    use crate::pack::tests::packed;

    #[test]
    fn a_signed_plugin_checks_and_a_changed_one_does_not() {
        let dir = std::env::temp_dir().join(format!("paperpaw-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let (key, file) = (path("key"), path("plugin.ppn"));
        std::fs::write(&key, format!("{}\n", "07".repeat(32))).unwrap();
        let unsigned = packed(EM_RISCV);
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
        assert!(check(&file).unwrap_err().ends_with("changed or damaged"));
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
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
