//! Released flash/installer bytes checked with today's parsers and OTA code.
//! This never executes the old installer, bootloader or a device.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "invalid release evidence must fail the test"
)]

use paperpaw_sdk::firmware::ForeignCheck;
use paperpaw_update::{AppInfo, Check, TRAILER_LEN, Trailer};
use paperpaw_x4::ota::{Flash, FlashError, OtaData, Start, State};
use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN};

mod common;

struct Data {
    bytes: Vec<u8>,
    cut: bool,
}

impl Flash for Data {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.copy_from_slice(&self.bytes[at as usize..at as usize + buf.len()]);
        Ok(())
    }

    fn program(&mut self, at: u32, bytes: &[u8]) -> Result<(), FlashError> {
        let count = if self.cut {
            bytes.len() / 2
        } else {
            bytes.len()
        };
        for (old, new) in self.bytes[at as usize..].iter_mut().zip(&bytes[..count]) {
            *old &= new;
        }
        if self.cut { Err(FlashError) } else { Ok(()) }
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.bytes[from as usize..to as usize].fill(0xff);
        Ok(())
    }
}

fn transitions(original: &[u8]) {
    let mut data = Data {
        bytes: original.to_vec(),
        cut: false,
    };
    let mut ota = OtaData::read(&mut data).unwrap();
    assert_eq!(ota.booted(), 0, "released factory image boots slot zero");
    // Establish the running origin as kept before testing its next update.
    ota.choose(&mut data, 0, State::Valid, 0).unwrap();
    let kept = data.bytes.clone();
    for target in [1, 0] {
        ota.choose(&mut data, target, State::New, 0).unwrap();
        assert_eq!(OtaData::read(&mut data).unwrap().booted(), target);
        assert!(matches!(
            ota.start(&mut data, target),
            Some(Start::Trial { .. })
        ));
        ota.set_state(&mut data, target, State::Valid, 30_000)
            .unwrap();
        assert_eq!(OtaData::read(&mut data).unwrap().booted(), target);
    }
    data.bytes = kept.clone();
    ota = OtaData::read(&mut data).unwrap();
    ota.choose(&mut data, 1, State::New, 0).unwrap();
    assert!(matches!(ota.start(&mut data, 1), Some(Start::Trial { .. })));
    ota = OtaData::read(&mut data).unwrap();
    assert!(matches!(
        ota.start(&mut data, 1),
        Some(Start::Failed { .. })
    ));
    assert_eq!(OtaData::read(&mut data).unwrap().booted(), 0);
    data.bytes = kept;
    ota = OtaData::read(&mut data).unwrap();
    data.cut = true;
    assert!(ota.choose(&mut data, 1, State::New, 0).is_err());
    assert_eq!(OtaData::read(&mut data).unwrap().booted(), 0);
}

fn hash(bytes: &[u8]) -> String {
    paperpaw_digest::sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn candidate_receipt(
    device: &str,
    chip: u16,
    project: &str,
    board: &'static str,
    slots: Slots,
) -> String {
    let mut candidate_receipt = String::new();
    let variable = format!(
        "PAPERPAW_CANDIDATE_{}",
        device.to_uppercase().replace('-', "_")
    );
    if let Some(path) = std::env::var_os(variable) {
        let candidate = std::fs::read(path).unwrap();
        candidate_receipt = format!("candidate\t{device}\t{}\n", hash(&candidate));
        let candidate_info = AppInfo::read(&candidate).unwrap();
        assert_eq!(
            (candidate_info.chip, candidate_info.project.as_str()),
            (chip, project)
        );
        let mut check = ForeignCheck::new(chip, board, candidate.len() as u64);
        check.absorb(&candidate);
        check.finish().unwrap();
        let room = slots
            .apps
            .iter()
            .map(|(_, size)| *size)
            .min()
            .unwrap()
            .min(0x64_0000);
        assert!(
            candidate.len() + 256 * 1024 <= room as usize,
            "candidate slot margin"
        );
    }
    candidate_receipt
}

#[test]
#[ignore = "needs checksum-verified release files; tools/qualify-release-origins.py"]
fn released_origins_match_current_parsers_and_ota_transitions() {
    let manifest = std::env::var("PAPERPAW_RELEASE_ORIGINS").expect("release manifest required");
    let manifest = std::fs::read_to_string(manifest).unwrap();
    let receipt = std::env::var("PAPERPAW_RELEASE_RECEIPT").expect("run-owned receipt required");
    let mut checked = format!("manifest-sha256\t{}\n", hash(manifest.as_bytes()));
    let mut count = 0;
    for line in manifest.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 5);
        let [tag, device, full, install, update] = fields.as_slice() else {
            unreachable!()
        };
        let (chip, project, board) = match *device {
            "x4" => (5, "paperpaw-xteink", "x4"),
            "x4-pro" => (9, "paperpaw-xteink-pro", "x4pro"),
            _ => panic!("unexpected device"),
        };
        let full = std::fs::read(full).unwrap();
        let install = std::fs::read(install).unwrap();
        let update = std::fs::read(update).unwrap();
        let hashes = format!(
            "checked\t{tag}\t{device}\t{}\t{}\t{}\n",
            hash(&full),
            hash(&install),
            hash(&update)
        );
        let info = AppInfo::read(&install).unwrap();
        assert_eq!(
            (info.chip, info.project.as_str(), info.version.as_str()),
            (chip, project, tag.strip_prefix('v').unwrap())
        );
        let mut check = ForeignCheck::new(chip, board, install.len() as u64);
        check.absorb(&install);
        assert_eq!(
            check.finish(),
            Ok(()),
            "image checksum/hash: {tag}/{device}"
        );
        let split = update.len().checked_sub(TRAILER_LEN).unwrap();
        assert_eq!(
            &update[..split],
            install,
            "signed update has identical app bytes"
        );
        let trailer = Trailer::read(&update[split..], update.len() as u64).unwrap();
        assert!(
            paperpaw_x4::updates::KEYS.contains(&trailer.key),
            "trusted release signer"
        );
        let mut signature = Check::new(&trailer).unwrap();
        signature.absorb(&install);
        signature.finish().unwrap();
        let table = &full[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN];
        let slots = Slots::find(table).unwrap();
        let current = Slots::find(&common::table(include_str!(
            "../../../shells/xteink/partitions.csv"
        )))
        .unwrap();
        assert_eq!(slots, current, "origin layout differs from current layout");
        let at = slots.apps[0].0 as usize;
        assert_eq!(&full[at..at + install.len()], install);
        for (_, room) in slots.apps {
            assert!(install.len() <= room as usize);
        }
        let candidate_receipt = candidate_receipt(device, chip, project, board, slots);
        let at = slots.data as usize;
        transitions(&full[at..at + 0x2000]);
        println!(
            "{tag}/{device}: {info:?}; {slots:?}; app_bytes={}",
            install.len()
        );
        checked.push_str(&hashes);
        checked.push_str(&candidate_receipt);
        count += 1;
    }
    assert!(count > 0, "no release origins checked");
    std::fs::write(receipt, checked).unwrap();
}
