//! No bootloader that PaperPaw ships, or that a plain X4 is known to run,
//! is taken for a dual-boot setup (#224): a wrong detection would refuse
//! every update on a device that has no second firmware. The bootloaders
//! are scanned as the firmware scans them (`scan_marker`, the first 32 KB
//! of the flash); the lowio dualbooter's own is the one that must be found
//! (`tests/dual_boot.rs` has its tag with the bytes around it).
//!
//! `bootloaders/` holds the ones whose licence allows it (builds of ESP-IDF,
//! Apache-2.0), by SHA-256 and where each comes from; `bootloaders/README.md`
//! lists the others by hash. To scan more: `PAPERPAW_BOOTLOADER_DIR=dir
//! cargo test -p paperpaw-x4 --test bootloaders` reads every `*.bin` in
//! `dir` (a bootloader, or a full flash image, whose first 32 KB are
//! scanned); a name with `dual` in it must be found, any other must not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_x4::dual_boot::{Source, scan_marker};
use paperpaw_x4::ota::{Flash, FlashError};

/// A flash with `image` from its start (a bootloader), erased beyond.
struct Chip(Vec<u8>);

impl Chip {
    fn with(image: &[u8]) -> Self {
        let mut flash = vec![0xFF; 0x10000];
        let n = image.len().min(flash.len());
        flash[..n].copy_from_slice(&image[..n]);
        Self(flash)
    }
}

impl Flash for Chip {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.copy_from_slice(&self.0[at as usize..at as usize + buf.len()]);
        Ok(())
    }
    fn program(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
        unreachable!("a scan writes nothing")
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        unreachable!("a scan writes nothing")
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// (name, bytes, SHA-256, what it is).
const SCANNED: [(&str, &[u8], &str, &str); 4] = [
    (
        "paperpaw-esp32c3-x4-x3.bin",
        include_bytes!("bootloaders/paperpaw-esp32c3-x4-x3.bin"),
        "ff342356139b49c5628beff12d718875a8fb34b7d00cadcd4b49b56f63e63a7b",
        "PaperPaw 0.6.0-beta.2 full image of the X4 and the X3 (the espflash it was made with), first 0x8000 bytes",
    ),
    (
        "paperpaw-esp32s3-x4pro-classic.bin",
        include_bytes!("bootloaders/paperpaw-esp32s3-x4pro-classic.bin"),
        "07d68375a509e4722c3bd56aded9ee0be83c4af0eb39b2b7e9bda6f7e6b2290e",
        "PaperPaw 0.6.0-beta.2 full image of the X4 Pro and the X4 Classic, first 0x8000 bytes",
    ),
    (
        "crosspoint-x4.bin",
        include_bytes!("bootloaders/crosspoint-x4.bin"),
        "6e6b0d386095f0783e9f0513ea29ea2ce413790e7f7153a06a2b04e2b5f59fb6",
        "CrossPoint's X4 bootloader (crosspoint-reader/crosspoint-tools public/firmware/bootloader.bin)",
    ),
    (
        "esp-idf-esp32s3.bin",
        include_bytes!("bootloaders/esp-idf-esp32s3.bin"),
        "09c6a477ac9b33721e9ea455fda1e29dc5b2f35c85d0b858b693defcab6f19fb",
        "the ESP32-S3 bootloader CrossPoint's tools host for the X4 Pro (x4pro-bootloader.bin; the same bytes as their lilygo, papermono and sticky ones)",
    ),
];

#[test]
fn the_bootloaders_that_ship_and_the_common_ones_are_not_dual_boot() {
    for (name, bytes, sha256, what) in SCANNED {
        assert_eq!(
            hex(&paperpaw_digest::sha256::digest(bytes)),
            sha256,
            "{name}: not the file recorded ({what})"
        );
        assert_eq!(
            scan_marker(&mut Chip::with(bytes)),
            Ok(None),
            "{name} ({what}) is taken for a dual-boot setup"
        );
        // The chip id of the image header says which chip it is for.
        assert_eq!(bytes[0], 0xE9, "{name}: an ESP image");
    }
}

#[test]
fn an_erased_or_empty_bootloader_window_is_not_dual_boot() {
    assert_eq!(scan_marker(&mut Chip::with(&[])), Ok(None));
    assert_eq!(scan_marker(&mut Chip(vec![0; 0x10000])), Ok(None));
}

#[test]
fn bootloaders_in_a_folder_given_by_the_environment() {
    let Some(dir) = std::env::var_os("PAPERPAW_BOOTLOADER_DIR") else {
        return;
    };
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "bin") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let found = scan_marker(&mut Chip::with(&bytes)).unwrap();
        let sha = hex(&paperpaw_digest::sha256::digest(&bytes));
        println!(
            "{name}: {sha} {}",
            found.map_or("not dual boot", |_| "DUAL BOOT")
        );
        let wanted = name.contains("dual").then_some(Source::X4Boot);
        assert_eq!(found, wanted, "{name} ({sha})");
        seen += 1;
    }
    assert!(seen > 0, "no .bin files in PAPERPAW_BOOTLOADER_DIR");
}
