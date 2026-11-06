//! `.so` libraries, code plugins of interface 7: checked as the loader
//! checks them before it maps one (`paperpaw_native::preflight`): a
//! well-formed ELF64 shared object for this machine that is freestanding,
//! i.e. names no other library, imports no symbol and exports nothing but
//! the plugin's two entry symbols ([`PLUGIN_EXPORTS`]). The same rule, the
//! same list, as the loader's: a library that passes here is not refused
//! for its imports or exports there.

use std::path::Path;

use paperpaw_native::preflight::{
    FreestandingRefusal, PLUGIN_EXPORTS, Refusal, check_freestanding,
};

use crate::cli::Context;
use crate::report::{Finding, Report};

/// The most a library may be for this check to read it.
const MAX_BYTES: u64 = 64 << 20;

/// Checks every `.so` library of the target.
pub fn check(context: &Context, report: &mut Report) -> Result<(), String> {
    for path in &context.target.libraries {
        for finding in check_file(path) {
            report.add(finding);
        }
    }
    Ok(())
}

/// The machines the check reads (`e_machine`): x86-64 and AArch64, the
/// desktop and Android loaders'. Anything else is an error of its own.
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;

/// What is wrong with the library at `path` (one error), or that it is
/// freestanding (one info).
pub fn check_file(path: &Path) -> Vec<Finding> {
    let place = path.display().to_string();
    let bytes = match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_BYTES => {
            return vec![Finding::error(
                "package.library",
                place,
                format!("{} bytes: more than {MAX_BYTES}, not read", meta.len()),
            )];
        }
        Ok(_) => match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return vec![Finding::error("package.library", place, e.to_string())],
        },
        Err(e) => return vec![Finding::error("package.library", place, e.to_string())],
    };
    let mut findings = Vec::new();
    check_library(&bytes, &place, &mut findings);
    findings
}

/// The freestanding rule of the loader on the bytes of a library, for
/// `place` (a file's path or a name in a package): one error saying what to
/// change, or one info.
pub(crate) fn check_library(bytes: &[u8], place: &str, out: &mut Vec<Finding>) {
    // The machine the file says it is for; only the two the loaders read.
    let Some(machine) = bytes.get(18..20).map(|m| u16::from_le_bytes([m[0], m[1]])) else {
        out.push(Finding::error(
            "package.library",
            place,
            "too short to be a library (no ELF header)",
        ));
        return;
    };
    if bytes.get(..4) == Some(b"\x7fELF") && machine != EM_X86_64 && machine != EM_AARCH64 {
        out.push(Finding::error(
            "package.library",
            place,
            format!(
                "built for machine {machine}: the loaders read x86-64 ({EM_X86_64}) and AArch64 \
                 ({EM_AARCH64}) only"
            ),
        ));
        return;
    }
    match check_freestanding(bytes, machine, PLUGIN_EXPORTS) {
        Ok(()) => out.push(Finding::info(
            "package.library",
            place,
            format!(
                "freestanding: no other library, no import, exports only {}",
                PLUGIN_EXPORTS.join(" and ")
            ),
        )),
        Err(refusal) => out.push(Finding::error(
            "package.library",
            place,
            format!("the loader refuses it: {}", describe(&refusal)),
        )),
    }
}

/// Why a library is refused, and how to see and change it.
fn describe(refusal: &FreestandingRefusal) -> String {
    let [entry, metadata] = [PLUGIN_EXPORTS[0], PLUGIN_EXPORTS[1]];
    match refusal {
        FreestandingRefusal::Preflight(why) => format!(
            "{}; build a 64-bit little-endian shared object (`cdylib`) for x86-64 or AArch64",
            match why {
                Refusal::NotSupported => {
                    "not an ELF64 little-endian shared object this check reads"
                }
                Refusal::Truncated => "a header or table lies outside the file (cut short)",
                Refusal::OtherMachine => "built for another machine",
                Refusal::UnsupportedPlatform => "no check exists for this platform's libraries",
                Refusal::Segments => {
                    "its loadable segments overlap, are too many or are laid out ambiguously, \
                     or ask for an executable stack"
                }
                _ => "refused by the ELF preflight",
            }
        ),
        FreestandingRefusal::NeededLibrary => String::from(
            "it needs another library: `readelf -d` must show no NEEDED entry. A plugin links to \
             none and reaches the host only through the table it is given; build it freestanding \
             (`-nostartfiles`, no C library)",
        ),
        FreestandingRefusal::UndefinedImport => String::from(
            "it imports a symbol it does not define: `nm -D --undefined-only` must list nothing. \
             A plugin reaches the host only through the table it is given",
        ),
        FreestandingRefusal::UnexpectedExport => format!(
            "it exports a symbol besides the two entry symbols: `nm -D --defined-only` must list \
             exactly {entry} and {metadata}. Hide the rest (version script, `--exclude-libs`, \
             hidden visibility; see the link recipe in the docs of `check_freestanding`)"
        ),
        _ => String::from("refused by the freestanding check"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(bytes: &[u8]) -> Finding {
        let mut out = Vec::new();
        check_library(bytes, "lib.so", &mut out);
        assert_eq!(out.len(), 1);
        out.remove(0)
    }

    #[test]
    fn every_refusal_says_what_to_see_and_change() {
        for (refusal, hint) in [
            (FreestandingRefusal::NeededLibrary, "readelf -d"),
            (
                FreestandingRefusal::UndefinedImport,
                "nm -D --undefined-only",
            ),
            (
                FreestandingRefusal::UnexpectedExport,
                "nm -D --defined-only",
            ),
            (
                FreestandingRefusal::Preflight(Refusal::Truncated),
                "cut short",
            ),
        ] {
            let text = describe(&refusal);
            assert!(text.contains(hint), "{refusal:?}: {text}");
        }
        let exports = describe(&FreestandingRefusal::UnexpectedExport);
        assert!(exports.contains("PAPERPAW_ENTRY") && exports.contains("PAPERPAW_METADATA"));
    }

    #[test]
    fn short_files_and_other_machines_are_errors_of_their_own() {
        let short = one(b"\x7fELF");
        assert_eq!(short.level, crate::report::Level::Error);
        assert!(short.message.contains("too short"), "{}", short.message);
        // A well-formed header start for machine 3 (x86), not read.
        let mut header = [0u8; 64];
        header[..4].copy_from_slice(b"\x7fELF");
        header[18] = 3;
        let other = one(&header);
        assert!(other.message.contains("machine 3"), "{}", other.message);
        // Not an ELF at all: the loader's refusal, not a machine.
        let junk = one(&[0u8; 64]);
        assert!(junk.message.contains("loader refuses"), "{}", junk.message);
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-lib-{}", std::process::id()));
        let findings = check_file(&dir.join("missing.so"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].level, crate::report::Level::Error);
    }
}
