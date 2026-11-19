//! `.ppn` files, code plugins built for a device: checked as the device
//! checks them before it loads one (`paperpaw_sdk::native::check_file`:
//! header, length, digest, the author's signature), then what they hold,
//! who signed them, what they are made for and their sizes.
//!
//! The format is `paperpaw-native`'s ([ADR 0024]); the device leaves out
//! a plugin built for another PaperPaw build, which the fingerprint tells
//! (`--release <fingerprint>`: the 16 hex digits a release lists in its
//! `paperpaw-x4-<version>-abi.txt`, or `paperpaw-sign plugin` shows for a
//! plugin built with the release's toolchain).
//!
//! [ADR 0024]: ../../../docs/adr/0024-native-code-plugins.md

use std::path::Path;

use paperpaw_native::HEADER_LEN;
use paperpaw_native::header5;
use paperpaw_native::signature::SIGNATURE_LEN;
use paperpaw_sdk::native::{self, Chip, Fingerprint, Header};

use crate::cli::Context;
use crate::report::{Finding, Report};

/// Checks every `.ppn` file of the target.
pub fn check(context: &Context, report: &mut Report) -> Result<(), String> {
    let release = context
        .release
        .as_deref()
        .map(|r| (r, Fingerprint::parse(r)));
    for path in &context.target.ppn {
        for finding in check_file(path, release) {
            report.add(finding);
        }
    }
    Ok(())
}

/// What the file at `path` holds and what is wrong with it; `release` is
/// what `--release` gave, with the fingerprint in it if it is one.
pub fn check_file(path: &Path, release: Option<(&str, Option<Fingerprint>)>) -> Vec<Finding> {
    let place = path.display().to_string();
    let mut findings = Vec::new();
    let (folder, name) = match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) => (folder, name.to_string_lossy().into_owned()),
        _ => {
            return vec![Finding::error("package.ppn", place, "not a file")];
        }
    };
    let folder = if folder.as_os_str().is_empty() {
        Path::new(".")
    } else {
        folder
    };
    // A file of plugin interface 7 (format 5) has a header of its own.
    if let Ok(bytes) = std::fs::read(path)
        && bytes.get(..4) == Some(&paperpaw_native::MAGIC[..])
        && bytes.get(4..6) == Some(&header5::FORMAT.to_le_bytes()[..])
    {
        return check_file5(&bytes, place, release);
    }
    let storage = match paperpaw_fs::DirStorage::new(folder) {
        Ok(storage) => storage,
        Err(e) => return vec![Finding::error("package.ppn", place, e.to_string())],
    };
    let (header, signature) = match native::check_file(&storage, &name) {
        Ok(checked) => checked,
        Err(problem) => {
            findings.push(Finding::error(
                "package.ppn",
                place,
                format!("the device refuses it: {problem}"),
            ));
            return findings;
        }
    };
    let fingerprint = Fingerprint(header.fingerprint);
    findings.push(Finding::info(
        "package.ppn",
        place.clone(),
        format!(
            "made for the {} ({}), fingerprint {fingerprint}, linked at {:#010x}, aligned to {}",
            header.chip,
            devices(header.chip),
            header.base,
            header.align
        ),
    ));
    findings.push(Finding::info(
        "package.ppn-size",
        place.clone(),
        sizes(&header, signature.is_some()),
    ));
    findings.push(signed(&place, signature.as_ref()));
    match release {
        Some((_, Some(release))) if release != fingerprint => findings.push(Finding::error(
            "package.ppn-release",
            place,
            format!(
                "built for another PaperPaw build (fingerprint {fingerprint}, the release's \
                 {release}): the device leaves it out; build it with that release's SDK and \
                 toolchain"
            ),
        )),
        Some((_, Some(_))) => findings.push(Finding::info(
            "package.ppn-release",
            place,
            "fits the release's build (same fingerprint)",
        )),
        Some((asked, None)) => findings.push(Finding::info(
            "package.ppn-release",
            place,
            format!(
                "whether it fits release {asked} shows its fingerprint: give the 16 hex \
                 digits of the release's abi.txt with --release"
            ),
        )),
        None => {}
    }
    findings
}

/// A file of plugin interface 7 (format 5, `abi/ppn5.md`), read as a
/// reader of its chip reads it: no fingerprint, it names the interface's
/// minor it needs instead.
fn check_file5(
    bytes: &[u8],
    place: String,
    release: Option<(&str, Option<Fingerprint>)>,
) -> Vec<Finding> {
    let (header, signature) = match paperpaw_publish::release::build(bytes) {
        Ok(read) => read,
        Err(problem) => return vec![Finding::error("package.ppn", place, problem)],
    };
    let mut findings = vec![Finding::info(
        "package.ppn",
        place.clone(),
        format!(
            "made for the {} ({}), plugin interface {}.{}, linked at {:#010x}, aligned to {}",
            header.target,
            devices(header.target),
            header.abi_major,
            header.needs_minor,
            header.base,
            header.align
        ),
    )];
    let trailer = if signature.is_some() {
        SIGNATURE_LEN as u64
    } else {
        0
    };
    findings.push(Finding::info(
        "package.ppn-size",
        place.clone(),
        format!(
            "{} bytes: header {}, read-only data {}, code {} ({} hot), relocations {} \
             ({} words), patches {}, signature {trailer}",
            header.file_len() + trailer,
            header5::HEADER_LEN,
            header.code_at,
            header.image_len - header.code_at,
            header.hot_len,
            u64::from(header.relocs) * 4,
            header.relocs,
            u64::from(header.patches) * header5::PATCH_BYTES,
        ),
    ));
    findings.push(signed(&place, signature.as_ref()));
    if release.is_some() {
        findings.push(Finding::info(
            "package.ppn-release",
            place,
            format!(
                "runs on every PaperPaw of plugin interface {}.{} or later: a file of \
                 interface 7 has no fingerprint",
                header.abi_major, header.needs_minor
            ),
        ));
    }
    findings
}

/// Who signed the file, or that nobody did.
fn signed(place: &str, signature: Option<&paperpaw_native::Signature>) -> Finding {
    match signature {
        Some(signature) => Finding::info(
            "package.ppn-signature",
            place.to_string(),
            format!(
                "signed by the key {}: the store remembers it at the first install and \
                 asks before code by another key replaces it",
                paperpaw_digest::hex::encode(&signature.key)
            ),
        ),
        None => Finding::warning(
            "package.ppn-signature",
            place.to_string(),
            "not signed: nobody can tell who made it; sign it with \
             `paperpaw-sign sign-plugin <file> --key <key>` (or `pack ... --key`)",
        ),
    }
}

/// The devices with `chip`.
fn devices(chip: Chip) -> &'static str {
    match chip {
        Chip::Esp32C3 => "Xteink X4, X3",
        Chip::Esp32S3 => "Xteink X4 Pro, X4 Classic",
        _ => "an unknown chip",
    }
}

/// The file's parts and their sizes.
fn sizes(header: &Header, signed: bool) -> String {
    let data = header.code_at;
    let code = header.image_len - header.code_at;
    let relocations = u64::from(header.relocs) * 4;
    let patches = u64::from(header.patches) * 8;
    let trailer = if signed { SIGNATURE_LEN as u64 } else { 0 };
    let total = header.file_len() + trailer;
    format!(
        "{total} bytes: header {HEADER_LEN}, read-only data {data}, code {code} \
         ({} hot), relocations {relocations} ({} words), patches {patches}, \
         signature {trailer}",
        header.hot_len, header.relocs
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Level;
    use paperpaw_native::signature;

    /// A small plugin file: no code to run, only to check (as the SDK's
    /// own tests make one).
    fn file() -> Vec<u8> {
        let mut header = Header {
            chip: Chip::Esp32C3,
            fingerprint: 0x0085_073a_6132_3717,
            base: paperpaw_native::LINK_BASE,
            image_len: 128,
            code_at: 96,
            entry: 0,
            api: 32,
            relocs: 0,
            align: 16,
            compiler: 1,
            hot_at: 0,
            hot_len: 0,
            patches: 0,
            needs_api: 0,
            families: [0; 8],
            interface: 5,
            sha256: [0; 32],
        };
        let image = [9u8; 128];
        let mut sha = header.digest();
        sha.update(&image);
        header.sha256 = sha.finish();
        let mut file = header.to_bytes().to_vec();
        file.extend_from_slice(&image);
        file
    }

    fn checked(name: &str, bytes: &[u8], release: Option<&str>) -> Vec<Finding> {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-ppn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let findings = check_file(&path, release.map(|r| (r, Fingerprint::parse(r))));
        let _ = std::fs::remove_file(path);
        findings
    }

    fn has(findings: &[Finding], check: &str, level: Level) -> bool {
        findings
            .iter()
            .any(|f| f.check == check && f.level == level)
    }

    #[test]
    fn a_signed_file_is_shown() {
        let mut bytes = file();
        let author = signature::sign(&bytes, &[2; 32]);
        bytes.extend_from_slice(&author.to_bytes());
        let findings = checked("signed.ppn", &bytes, Some("0085073a61323717"));
        assert!(!has(&findings, "package.ppn", Level::Error), "{findings:?}");
        assert!(has(&findings, "package.ppn-signature", Level::Info));
        assert!(has(&findings, "package.ppn-release", Level::Info));
        let size = findings
            .iter()
            .find(|f| f.check == "package.ppn-size")
            .unwrap();
        assert!(
            size.message.starts_with("356 bytes: header 116"),
            "{}",
            size.message
        );
    }

    #[test]
    fn unsigned_damaged_and_other_builds_are_found() {
        let bytes = file();
        let findings = checked("unsigned.ppn", &bytes, Some("0000000000000001"));
        assert!(has(&findings, "package.ppn-signature", Level::Warning));
        assert!(has(&findings, "package.ppn-release", Level::Error));
        let mut damaged = bytes.clone();
        damaged[100] ^= 1;
        let findings = checked("damaged.ppn", &damaged, None);
        assert!(has(&findings, "package.ppn", Level::Error), "{findings:?}");
        let findings = checked("text.ppn", b"hello", None);
        assert!(has(&findings, "package.ppn", Level::Error));
    }
}
