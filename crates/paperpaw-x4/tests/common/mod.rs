//! What the update tests share: the partition tables PaperPaw ships and
//! CrossPoint's, as `espflash` writes them to the flash.

#![allow(dead_code, reason = "each test takes what it needs")]

use paperpaw_x4::partitions::TABLE_LEN;

pub const FLASH: usize = 16 * 1024 * 1024;

/// CrossPoint's `partitions.csv` (github.com/daveallie/crosspoint-reader),
/// the Arduino layout of 16 MB.
const CROSSPOINT: &str = "\
nvs,      data, nvs,     0x9000,  0x5000,
otadata,  data, ota,     0xe000,  0x2000,
app0,     app,  ota_0,   0x10000, 0x640000,
app1,     app,  ota_1,   0x650000,0x640000,
spiffs,   data, spiffs,  0xc90000,0x360000,
coredump, data, coredump,0xFF0000,0x10000,
";

/// The tables: those `shells/xteink` ships (the X4, the X4 Pro and the
/// X3 share one), and CrossPoint's.
pub fn layouts() -> [(&'static str, &'static str); 3] {
    [
        (
            "partitions.csv",
            include_str!("../../../../shells/xteink/partitions.csv"),
        ),
        (
            "partitions-classic.csv",
            include_str!("../../../../shells/xteink/partitions-classic.csv"),
        ),
        ("CrossPoint", CROSSPOINT),
    ]
}

/// A table as `espflash` writes it from a CSV: 32-byte entries, then the
/// MD5 entry, the rest erased.
pub fn table(csv: &str) -> Vec<u8> {
    let number = |text: &str| {
        let text = text.trim();
        let hex = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"));
        let value = match hex {
            Some(hex) => u32::from_str_radix(hex, 16),
            None => text.parse(),
        };
        value.unwrap_or_else(|_| panic!("a number: {text}"))
    };
    let mut bytes = Vec::new();
    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let (kind, subtype) = match (fields[1], fields[2]) {
            ("app", "factory") => (0x00, 0x00),
            ("app", ota) if ota.starts_with("ota_") => (0x00, 0x10 + number(&ota[4..]) as u8),
            ("data", "ota") => (0x01, 0x00),
            ("data", "phy") => (0x01, 0x01),
            ("data", "nvs") => (0x01, 0x02),
            ("data", "coredump") => (0x01, 0x03),
            ("data", "spiffs") => (0x01, 0x82),
            other => panic!("not known: {other:?}"),
        };
        bytes.extend_from_slice(&[0xAA, 0x50, kind, subtype]);
        bytes.extend_from_slice(&number(fields[3]).to_le_bytes());
        bytes.extend_from_slice(&number(fields[4]).to_le_bytes());
        let mut label = [0u8; 16];
        label[..fields[0].len()].copy_from_slice(fields[0].as_bytes());
        bytes.extend_from_slice(&label);
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes.extend_from_slice(&[0xEB, 0xEB]);
    bytes.resize(TABLE_LEN, 0xFF);
    bytes
}
