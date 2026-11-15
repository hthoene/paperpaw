//! The firmware's commands: signing keys, update files, whole flash
//! images signed apart, and the plugin crates an image has built in.

use std::io::Write;

use paperpaw_update::{AppInfo, Check, PLUGINS_AT, Plugins, TRAILER_LEN, Trailer};

use paperpaw_native::Chip;
use paperpaw_native::header5::Header5;

use crate::elf::Elf;
use crate::secret_key;

/// The symbol under which a firmware's ELF file holds the fingerprint
/// native plugins must have (8 bytes, little-endian), as
/// `shells/xteink/src/native` exports it.
const FINGERPRINT_SYMBOL: &str = "PAPERPAW_NATIVE_FINGERPRINT";

pub fn key(file: &str) -> Result<(), String> {
    let mut out = new_key_file(file).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("{file} exists: a key is never overwritten"),
        _ => format!("{file}: {e}"),
    })?;
    let pair = ed25519_compact::KeyPair::generate();
    let seed: [u8; 32] = *pair.sk.seed();
    writeln!(out, "{}", paperpaw_update::key_hex(&seed)).map_err(|e| format!("{file}: {e}"))?;
    let public = paperpaw_update::public_key(&seed);
    println!("public key:  {}", paperpaw_update::key_hex(&public));
    println!("fingerprint: {}", paperpaw_update::fingerprint(&public));
    Ok(())
}

/// A new file for a secret key: never one that is there (checked and made
/// at once), and readable by its owner alone (on Windows, as the folder
/// it is in lets).
fn new_key_file(file: &str) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(file)
}

pub fn sign_apart(image: &str, signature: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = secret_key(key_file)?;
    let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    let trailer = paperpaw_update::sign(&data, &seed).map_err(|e| format!("{image}: {e}"))?;
    std::fs::write(signature, trailer.to_bytes()).map_err(|e| format!("{signature}: {e}"))?;
    println!(
        "{signature}: {} bytes signed by {}",
        data.len(),
        paperpaw_update::fingerprint(&trailer.key)
    );
    Ok(())
}

pub fn check(image: &str, signature: &str) -> Result<(), String> {
    let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    let sig = std::fs::read(signature).map_err(|e| format!("{signature}: {e}"))?;
    let trailer = paperpaw_update::check_apart(&data, &sig).map_err(|e| format!("{image}: {e}"))?;
    println!("image:       {} bytes", data.len());
    println!(
        "signer:      {} ({})",
        paperpaw_update::fingerprint(&trailer.key),
        paperpaw_update::key_hex(&trailer.key)
    );
    println!("signature:   holds");
    Ok(())
}

pub fn sign(image: &str, update: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = secret_key(key_file)?;
    let mut data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    let info = AppInfo::read(&data).map_err(|e| format!("{image}: {e}"))?;
    let trailer = paperpaw_update::sign(&data, &seed).map_err(|e| format!("{image}: {e}"))?;
    data.extend_from_slice(&trailer.to_bytes());
    std::fs::write(update, &data).map_err(|e| format!("{update}: {e}"))?;
    println!(
        "{update}: {} {}, signed by {}",
        info.project,
        info.version,
        paperpaw_update::fingerprint(&trailer.key)
    );
    Ok(())
}

pub fn show(update: &str) -> Result<(), String> {
    let data = std::fs::read(update).map_err(|e| format!("{update}: {e}"))?;
    let split = data
        .len()
        .checked_sub(TRAILER_LEN)
        .ok_or_else(|| format!("{update}: too short"))?;
    let trailer =
        Trailer::read(&data[split..], data.len() as u64).map_err(|e| format!("{update}: {e}"))?;
    let info = AppInfo::read(&data[..split]).map_err(|e| format!("{update}: {e}"))?;
    let mut check = Check::new(&trailer).map_err(|e| format!("{update}: {e}"))?;
    check.absorb(&data[..split]);
    let holds = check.finish();
    println!("firmware:    {} {}", info.project, info.version);
    println!("chip:        {}", info.chip);
    println!("image:       {} bytes", trailer.image_len);
    match plugins_of(&data[..split]) {
        Some(list) => println!("plugins:     {} built in", list.iter().count()),
        None => println!("plugins:     not listed"),
    }
    println!(
        "signer:      {} ({})",
        paperpaw_update::fingerprint(&trailer.key),
        paperpaw_update::key_hex(&trailer.key)
    );
    match holds {
        Ok(()) => println!("signature:   holds"),
        Err(e) => return Err(format!("{update}: {e}")),
    }
    Ok(())
}

/// The plugin list of an app image (or an update file, which begins with
/// one).
fn plugins_of(image: &[u8]) -> Option<Plugins> {
    Plugins::read(image.get(PLUGINS_AT..)?)
}

pub fn plugins(image: &str) -> Result<(), String> {
    let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
    AppInfo::read(&data).map_err(|e| format!("{image}: {e}"))?;
    let list = plugins_of(&data).ok_or_else(|| format!("{image}: no list of plugins"))?;
    if list.is_empty() {
        return Err(format!("{image}: no plugins listed"));
    }
    print!("{}", list.text());
    Ok(())
}

pub fn fingerprint(elf: &str) -> Result<(), String> {
    let data = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let fingerprint = fingerprint_of(&data).map_err(|e| format!("{elf}: {e}"))?;
    println!("{fingerprint:016x}");
    Ok(())
}

/// The fingerprint a firmware's ELF file names.
fn fingerprint_of(file: &[u8]) -> Result<u64, String> {
    let bytes = symbol_bytes(file, FINGERPRINT_SYMBOL, 8)
        .map_err(|e| format!("{e}: a firmware without code plugins"))?;
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| "not 8 bytes")?;
    Ok(u64::from_le_bytes(bytes))
}

/// The `len` bytes of the object `name` in a firmware's ELF file.
fn symbol_bytes<'a>(file: &'a [u8], name: &str, len: usize) -> Result<&'a [u8], String> {
    let elf = Elf::parse(file)?;
    let (_, symbols) = elf.symbols()?.ok_or("no symbol table")?;
    let symbol = symbols
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("no {name}"))?;
    elf.sections
        .get(usize::from(symbol.shndx))
        .filter(|_| usize::try_from(symbol.size).is_ok_and(|size| size == len))
        .and_then(|section| {
            let at = usize::try_from(symbol.value.checked_sub(section.addr)?).ok()?;
            elf.data(section).ok()?.get(at..at.checked_add(len)?)
        })
        .ok_or_else(|| format!("{name} is not {len} bytes of the file"))
}

/// What a firmware's ELF file says of the plugins it runs, as its
/// `PAPERPAW_NATIVE_ABI` keeps it (version 2): the plugin interface and its
/// minor, the newest a plugin file may need to run on it (`abi/ppn5.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Abi {
    interface: u16,
    minor: u16,
}

const ABI_SYMBOL: &str = "PAPERPAW_NATIVE_ABI";
const ABI_WORDS: usize = 3;
/// The record of the firmwares before (version 1: the size of the table for
/// plugins and the shared functions' revisions): told apart, not read.
const ABI_WORDS_BEFORE: usize = 19;

fn abi_of(file: &[u8]) -> Result<Abi, String> {
    let bytes = symbol_bytes(file, ABI_SYMBOL, 4 * ABI_WORDS).map_err(|e| {
        match symbol_bytes(file, ABI_SYMBOL, 4 * ABI_WORDS_BEFORE) {
            Ok(_) => format!("{ABI_SYMBOL} of version 1: a firmware before interface 7"),
            Err(_) => e,
        }
    })?;
    let word = |i: usize| {
        let at = 4 * i;
        bytes
            .get(at..at + 4)
            .and_then(|b| b.try_into().ok())
            .map_or(0, u32::from_le_bytes)
    };
    if word(0) != 2 {
        return Err(format!("{ABI_SYMBOL} of version {}, not 2", word(0)));
    }
    let small = |i: usize| {
        u16::try_from(word(i)).map_err(|_| format!("{ABI_SYMBOL}: word {i} is too large"))
    };
    Ok(Abi {
        interface: small(1)?,
        minor: small(2)?,
    })
}

/// A release's `-abi.txt`: two lines, the interface and its minor, which the
/// store and the web flasher read. No fingerprint: an interface-7 plugin file
/// has none.
fn abi_text(abi: &Abi) -> String {
    format!("interface: {}\nminor: {}\n", abi.interface, abi.minor)
}

pub fn abi(elf: &str) -> Result<(), String> {
    let data = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let abi = abi_of(&data).map_err(|e| format!("{elf}: {e}"))?;
    print!("{}", abi_text(&abi));
    Ok(())
}

/// Why a plugin file of format 5 does not run on a firmware with this record,
/// built for `machine` (the ELF file's): another chip, another interface, or
/// a minor the firmware does not have yet.
fn misfit(abi: &Abi, machine: u16, header: &Header5) -> Option<String> {
    let chip = match machine {
        crate::elf::EM_RISCV => Chip::Esp32C3,
        _ => Chip::Esp32S3,
    };
    if header.target != chip {
        return Some(format!(
            "built for the {}, the firmware is the {chip}'s",
            header.target
        ));
    }
    if header.abi_major != abi.interface {
        return Some(format!(
            "of interface {}, the firmware has {}",
            header.abi_major, abi.interface
        ));
    }
    (header.needs_minor > abi.minor).then(|| {
        format!(
            "needs minor {} of the interface, the firmware has {}",
            header.needs_minor, abi.minor
        )
    })
}

/// Checks that each of the plugin files runs on the firmware: its chip, its
/// interface, and no newer minor than it has. The release runs it on the
/// plugins it builds for the store.
pub fn fits(elf: &str, files: &[&str]) -> Result<(), String> {
    let data = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let abi = abi_of(&data).map_err(|e| format!("{elf}: {e}"))?;
    let machine = Elf::parse(&data)
        .map_err(|e| format!("{elf}: {e}"))?
        .machine;
    for file in files {
        let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        let (header, _) = crate::pack5::check_file5(&bytes).map_err(|e| format!("{file}: {e}"))?;
        if let Some(why) = misfit(&abi, machine, &header) {
            return Err(format!("{file}: {why}"));
        }
    }
    println!(
        "{} plugin files run on interface {}.{}",
        files.len(),
        abi.interface,
        abi.minor
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::build::Builder;
    use crate::elf::{EM_XTENSA, ET_EXEC, SHF_ALLOC};

    #[test]
    fn the_firmware_names_its_fingerprint() {
        let fingerprint = 0x86fe_a46c_f2a7_188a_u64;
        let mut data = vec![0xAA; 4];
        data.extend_from_slice(&fingerprint.to_le_bytes());
        let mut b = Builder::new(EM_XTENSA, ET_EXEC);
        let rodata = b.section(".rodata", SHF_ALLOC, 0x3c00_0020, &data, 4);
        b.symbol(FINGERPRINT_SYMBOL, 0x3c00_0024, 8, rodata as u16, false);
        assert_eq!(fingerprint_of(&b.build()), Ok(fingerprint));

        let mut b = Builder::new(EM_XTENSA, ET_EXEC);
        let rodata = b.section(".rodata", SHF_ALLOC, 0x3c00_0020, &data, 4);
        b.symbol("main", 0x3c00_0020, 4, rodata as u16, false);
        assert!(fingerprint_of(&b.build()).unwrap_err().starts_with("no "));
        // Not in the file.
        let mut b = Builder::new(EM_XTENSA, ET_EXEC);
        let rodata = b.section(".rodata", SHF_ALLOC, 0x3c00_0020, &data, 4);
        b.symbol(FINGERPRINT_SYMBOL, 0x3c00_0030, 8, rodata as u16, false);
        assert!(fingerprint_of(&b.build()).is_err());
    }

    /// A firmware's ELF file with `words` under the record's name.
    fn firmware_with(words: &[u32]) -> Vec<u8> {
        let data: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let mut b = Builder::new(EM_XTENSA, ET_EXEC);
        let rodata = b.section(".rodata", SHF_ALLOC, 0x3c00_0000, &data, 4);
        b.symbol(
            ABI_SYMBOL,
            0x3c00_0000,
            data.len() as u32,
            rodata as u16,
            false,
        );
        b.build()
    }

    #[test]
    fn the_abi_file_is_the_interface_and_its_minor() {
        let abi = abi_of(&firmware_with(&[2, 7, 1])).unwrap();
        assert_eq!(
            abi,
            Abi {
                interface: 7,
                minor: 1
            }
        );
        // Two lines, `key: value` in decimal, nothing before them: no
        // fingerprint, no table of shared functions.
        assert_eq!(abi_text(&abi), "interface: 7\nminor: 1\n");
        assert_eq!(
            abi_text(&abi_of(&firmware_with(&[2, 7, 12])).unwrap()),
            "interface: 7\nminor: 12\n"
        );
        // The record of a firmware before (version 1, 19 words) is named, not
        // read as this one; another version, another length or a number that
        // is no `u16` is refused.
        let mut before = vec![1u32, 6, 72];
        before.extend([3, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0]);
        let problem = abi_of(&firmware_with(&before)).unwrap_err();
        assert!(problem.contains("version 1"), "{problem}");
        let problem = abi_of(&firmware_with(&[3, 7, 1])).unwrap_err();
        assert!(problem.contains("version 3"), "{problem}");
        assert!(abi_of(&firmware_with(&[2, 7])).is_err());
        assert!(abi_of(&firmware_with(&[2, 7, 1, 0])).is_err());
        assert!(abi_of(&firmware_with(&[2, 7, 70_000])).is_err());
    }

    #[test]
    fn a_plugin_file_fits_its_chip_its_interface_and_a_minor_the_firmware_has() {
        // The sample's file as the packer made it: for the X4's chip,
        // interface 7.
        let file = include_bytes!("../../../crates/paperpaw-native/tests/data/abi7-x4-min.ppn");
        let (header, _) = crate::pack5::check_file5(file).unwrap();
        assert_eq!((header.target, header.abi_major), (Chip::Esp32C3, 7));
        let has = |interface, minor| Abi { interface, minor };
        let needs = header.needs_minor;
        assert_eq!(misfit(&has(7, needs), crate::elf::EM_RISCV, &header), None);
        assert_eq!(
            misfit(&has(7, needs + 3), crate::elf::EM_RISCV, &header),
            None
        );
        let other_chip = misfit(&has(7, needs), EM_XTENSA, &header).unwrap();
        assert!(other_chip.contains("built for"), "{other_chip}");
        let other = misfit(&has(8, needs), crate::elf::EM_RISCV, &header).unwrap();
        assert!(other.contains("of interface 7"), "{other}");
        let mut newer = header;
        newer.needs_minor = needs + 1;
        let too_new = misfit(&has(7, needs), crate::elf::EM_RISCV, &newer).unwrap();
        assert!(too_new.contains("needs minor"), "{too_new}");
    }

    #[test]
    fn a_key_is_made_once_for_its_owner() {
        let dir = std::env::temp_dir().join(format!("paperpaw-sign-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("secret.key");
        let file = file.to_str().unwrap();
        let _ = std::fs::remove_file(file);
        key(file).unwrap();
        let first = std::fs::read_to_string(file).unwrap();
        assert!(secret_key(Some(file)).is_ok());
        // A second key never replaces the first.
        assert!(key(file).unwrap_err().contains("never overwritten"));
        assert_eq!(std::fs::read_to_string(file).unwrap(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
