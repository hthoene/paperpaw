//! The firmware's `FirmwareHost` (`paperpaw_x4::updates`) as the kernel drives
//! it: a check that streams the plugins an image has built in, a write bound
//! to the digest the user confirmed, the read-back and the commit, on a flash
//! in memory with the partition table PaperPaw ships. Every way it must end
//! other than well: a changed image, a card or flash that fails, a key the
//! user did not accept, a device shared with another firmware.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::Cell;

use common::{FLASH, table};
use paperpaw_platform::firmware::{FirmwareHost, Reason, Signer, Step, WRITE_BLOCK};
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::firmware::{
    CHIP_ESP32_C3, CHIP_ESP32_S3, Firmware, InstallFailure, PLUGINS_AT, PluginCrate, Plugins,
};
use paperpaw_x4::dual_boot::Source;
use paperpaw_x4::ota::{Flash, FlashError, OtaData, State};
use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN, entries};
use paperpaw_x4::updates::Updates;

const SEED: [u8; 32] = [5; 32];
const PROJECT: &str = "paperpaw-xteink";
const UPDATE: &str = "update.ppu";
const DATA_LEN: u32 = 0x2000;

/// A flash in memory that fails a write after so many.
struct Chip {
    bytes: Vec<u8>,
    programs_left: Cell<usize>,
    /// A program at this address is lost without a word (the flash says it
    /// is done).
    lose_at: Cell<Option<u32>>,
}

impl Flash for Chip {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.copy_from_slice(&self.bytes[at as usize..at as usize + buf.len()]);
        Ok(())
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        let left = self.programs_left.get();
        if left == 0 {
            return Err(FlashError);
        }
        self.programs_left.set(left - 1);
        if self.lose_at.get() == Some(at) {
            return Ok(());
        }
        for (byte, new) in self.bytes[at as usize..].iter_mut().zip(data) {
            *byte &= new;
        }
        Ok(())
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.bytes[from as usize..to as usize].fill(0xFF);
        Ok(())
    }
}

/// A card that fails a read after so many.
struct Card {
    files: MemoryStorage,
    reads_left: Cell<usize>,
}

impl Card {
    fn with(path: &str, bytes: &[u8]) -> Self {
        let files = MemoryStorage::new();
        files.write(path, bytes).unwrap();
        Self {
            files,
            reads_left: Cell::new(usize::MAX),
        }
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let left = self.reads_left.get();
        if left == 0 {
            return Err(StorageError::Io);
        }
        self.reads_left.set(left - 1);
        self.files.read_at(path, at, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }
}

/// Where an image's checksum is: the last byte of the 16-byte block after
/// its segments.
fn checksum_at(image: &[u8]) -> usize {
    let mut at = 24;
    for _ in 0..image[1] {
        let len = u32::from_le_bytes(image[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8 + len;
    }
    (at + 1).next_multiple_of(16) - 1
}

/// Ends `image` as espflash does: the checksum, the SHA-256 of all before it.
fn seal(image: &mut Vec<u8>) {
    let at = checksum_at(image);
    let mut xor = 0xEFu8;
    let mut segment = 24;
    for _ in 0..image[1] {
        let len = u32::from_le_bytes(image[segment + 4..segment + 8].try_into().unwrap()) as usize;
        xor = image[segment + 8..segment + 8 + len]
            .iter()
            .fold(xor, |x, b| x ^ b);
        segment += 8 + len;
    }
    image.truncate(segment);
    image.resize(at, 0);
    image.push(xor);
    let hash = paperpaw_digest_sha256(image);
    image.extend_from_slice(&hash);
}

fn paperpaw_digest_sha256(bytes: &[u8]) -> [u8; 32] {
    paperpaw_sdk::digest::sha256::digest(bytes)
}

/// An app image as espflash makes it, of about `len` bytes, for `chip` and
/// `project`, with `plugins` listed after the descriptor (`None`: no list, as
/// an older image) and `tag` somewhere in it.
fn app_image(
    chip: u16,
    project: &str,
    version: &str,
    plugins: Option<&[&str]>,
    tag: &[u8],
    len: usize,
) -> Vec<u8> {
    let mut data: Vec<u8> = (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ version.len() as u8)
        .collect();
    data[..4].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
    data[4..PLUGINS_AT - 32].fill(0);
    data[16..16 + version.len()].copy_from_slice(version.as_bytes());
    data[48..48 + project.len()].copy_from_slice(project.as_bytes());
    if let Some(plugins) = plugins {
        let list = Plugins::record(plugins.iter().map(|name| PluginCrate {
            name,
            version: "1.0.0",
        }))
        .unwrap();
        data[PLUGINS_AT - 32..PLUGINS_AT - 32 + list.len()].copy_from_slice(&list);
    }
    let at = len - 1000;
    data[at..at + tag.len()].copy_from_slice(tag);
    let mut image = vec![0u8; 24];
    image[0] = 0xE9;
    image[1] = 1;
    image[12..14].copy_from_slice(&chip.to_le_bytes());
    image[23] = 1;
    image.extend_from_slice(&0x4200_0020u32.to_le_bytes());
    image.extend_from_slice(&(data.len() as u32).to_le_bytes());
    image.extend_from_slice(&data);
    seal(&mut image);
    image
}

/// An update file: `image` and its trailer, signed with `seed`.
fn signed(image: &[u8], seed: &[u8; 32]) -> Vec<u8> {
    let trailer = paperpaw_update::sign(image, seed).unwrap();
    let mut file = image.to_vec();
    file.extend_from_slice(&trailer.to_bytes());
    file
}

/// A device with PaperPaw `0.5.0` in the first slot, started.
struct Device {
    updates: Updates<Chip>,
    slots: Slots,
}

impl Device {
    fn started(keys: Vec<[u8; 32]>) -> Self {
        let mut flash = vec![0xFF; FLASH];
        let table = table(include_str!("../../../shells/xteink/partitions.csv"));
        flash[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN].copy_from_slice(&table);
        for (n, partition) in entries(&table).enumerate() {
            let from = partition.offset as usize;
            flash[from..from + 64].fill(n as u8);
        }
        let slots = Slots::find(&flash[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN]).unwrap();
        let (first, _) = slots.apps[0];
        let running = app_image(CHIP_ESP32_C3, PROJECT, "0.5.0", None, b"", 200_000);
        flash[first as usize..first as usize + running.len()].copy_from_slice(&running);
        let chip = Chip {
            bytes: flash,
            programs_left: Cell::new(usize::MAX),
            lose_at: Cell::new(None),
        };
        let this = Firmware::new("0.5.0", PROJECT, CHIP_ESP32_C3).keys(keys);
        let (updates, _) = Updates::start(chip, &slots, 0, this, "x4").unwrap();
        Self { updates, slots }
    }

    fn data(&self) -> OtaData {
        let mut bytes =
            self.updates.flash().bytes[self.slots.data as usize..][..DATA_LEN as usize].to_vec();
        struct Part<'a>(&'a mut Vec<u8>);
        impl Flash for Part<'_> {
            fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
                buf.copy_from_slice(&self.0[at as usize..at as usize + buf.len()]);
                Ok(())
            }
            fn program(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
                Err(FlashError)
            }
            fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
                Err(FlashError)
            }
        }
        OtaData::read(&mut Part(&mut bytes)).unwrap()
    }

    /// The first bytes of the slot an update goes to.
    fn target_head(&self) -> [u8; 4] {
        let (at, _) = self.slots.apps[1];
        self.updates.flash().bytes[at as usize..at as usize + 4]
            .try_into()
            .unwrap()
    }

    /// What is in the slot an update goes to, `len` bytes.
    fn target(&self, len: usize) -> &[u8] {
        let (at, _) = self.slots.apps[1];
        &self.updates.flash().bytes[at as usize..at as usize + len]
    }
}

/// A whole check; what it streamed and how it ended.
fn check(
    host: &mut impl FirmwareHost,
    card: &Card,
    path: &str,
) -> (
    Vec<String>,
    Result<paperpaw_platform::CheckFacts, Reason>,
    u32,
) {
    let mut seen = Vec::new();
    let mut steps = 0;
    if let Err(reason) = host.begin_check(path) {
        return (seen, Err(reason), steps);
    }
    loop {
        steps += 1;
        match host.check_step(card, &mut |id| seen.push(id.to_string())) {
            Step::Progress { .. } => {}
            Step::Done(facts) => return (seen, Ok(facts), steps),
            Step::Failed(reason) => return (seen, Err(reason), steps),
        }
    }
}

/// Write and read back; how it ended.
fn write(
    host: &mut impl FirmwareHost,
    card: &Card,
    path: &str,
    digest: [u8; 32],
    key: Option<[u8; 32]>,
    foreign: bool,
) -> Result<(), Reason> {
    host.begin_write(path, digest, key, foreign)?;
    loop {
        match host.write_step(card) {
            Step::Progress { .. } => {}
            Step::Done(()) => break,
            Step::Failed(reason) => return Err(reason),
        }
    }
    loop {
        match host.readback_step() {
            Step::Progress { .. } => {}
            Step::Done(()) => return Ok(()),
            Step::Failed(reason) => return Err(reason),
        }
    }
}

fn image_0_6(plugins: &[&str]) -> Vec<u8> {
    app_image(CHIP_ESP32_C3, PROJECT, "0.6.0", Some(plugins), b"", 400_000)
}

/// The check streams the plugins the image lists, in its order, then hashes
/// and verifies all of it in bounded steps, and says what it found.
#[test]
fn a_check_streams_the_built_ins_and_reports_what_it_found() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = image_0_6(&["paperpaw-plugin-stats", "paperpaw-plugin-weather"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (seen, facts, steps) = check(&mut device.updates, &card, UPDATE);
    assert_eq!(seen, ["stats", "weather"]);
    let facts = facts.unwrap();
    assert!(facts.chip_ok && !facts.foreign);
    assert_eq!(facts.size as usize, image.len());
    assert_eq!(facts.version.as_str(), "0.6.0");
    assert_eq!(facts.project.as_str(), PROJECT);
    assert_eq!(facts.sha256, paperpaw_digest_sha256(&image));
    match facts.signer {
        Signer::PaperPaw { key: k, name } => {
            assert_eq!(k, key);
            assert_eq!(name.as_str(), "PaperPaw");
        }
        other => panic!("{other:?}"),
    }
    // 400 KB at 16 KiB a step: many steps, none of them the whole image.
    assert!(steps > 20, "{steps}");
    assert!(!device.updates.installing());
}

/// An image without the list (an older one) streams nothing.
#[test]
fn an_older_image_without_a_list_streams_nothing() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = app_image(CHIP_ESP32_C3, PROJECT, "0.4.0", None, b"", 100_000);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (seen, facts, _) = check(&mut device.updates, &card, UPDATE);
    assert!(seen.is_empty());
    assert!(facts.is_ok());
}

/// A whole install: written block by block, read back, committed: the slot
/// holds the image, the OTA data chooses it on trial, the running slot is
/// untouched, and the device then starts it.
#[test]
fn a_whole_install_writes_the_slot_and_chooses_it() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    let digest = facts.unwrap().sha256;

    device
        .updates
        .begin_write(UPDATE, digest, None, false)
        .unwrap();
    assert!(device.updates.installing());
    let mut blocks = 0;
    loop {
        match device.updates.write_step(&card) {
            Step::Progress { done, total } => {
                assert!(done <= total);
                blocks += 1;
            }
            Step::Done(()) => break,
            Step::Failed(reason) => panic!("{reason:?}"),
        }
        // Nothing the boot loader would start until the commit.
        assert_ne!(
            device.target_head(),
            image[..4],
            "the first word is not written"
        );
    }
    // The start (head and descriptor) is one step; then a block each.
    assert_eq!(blocks, 1 + image.len().div_ceil(WRITE_BLOCK as usize) - 1);
    while !matches!(device.updates.readback_step(), Step::Done(())) {}
    assert_ne!(
        device.target_head(),
        image[..4],
        "the first word is not written"
    );
    device.updates.commit().unwrap();
    assert!(!device.updates.installing());
    assert_eq!(device.target(image.len()), &image[..]);
    let data = device.data();
    assert_eq!(data.booted(), 1);
    assert_eq!(data.state_of(1).map(|s| s.0), Some(State::New));
    // The running firmware's slot is as it was.
    assert_eq!(device.updates.running(), 0);
}

/// Restart is the firmware's own; without one it says it cannot.
#[test]
fn restart_is_the_firmwares_and_says_so_where_there_is_none() {
    let mut device = Device::started(Vec::new());
    assert_eq!(device.updates.restart(), Err(Reason::NOT_HERE));
    device.updates.set_restarter(|| Ok(()));
    assert_eq!(device.updates.restart(), Ok(()));
}

/// The X4 has two app partitions, so it is dual-boot capable; it is declared
/// only once `settle` says so, and `restart_running` is the same restart.
#[test]
fn dual_boot_facts_are_the_devices() {
    let mut device = Device::started(Vec::new());
    assert!(device.updates.dual_boot_capable());
    assert!(!device.updates.dual_boot_declared());
    assert_eq!(device.updates.restart_running(), Err(Reason::NOT_HERE));
    device.updates.set_restarter(|| Ok(()));
    assert_eq!(device.updates.restart_running(), Ok(()));
    device.updates.settle(Some(Source::Ini));
    assert!(device.updates.dual_boot_declared());
    device.updates.settle(None);
    assert!(!device.updates.dual_boot_declared());
}

/// `restart_running` starts the image running and nothing else: where an
/// image waits (committed, chosen) it is refused and the restarter is not
/// called; the waiting image's restart is the reader's to confirm.
#[test]
fn restart_running_never_starts_a_waiting_image() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static RESTARTED: AtomicBool = AtomicBool::new(false);
    let mut device = Device::started(Vec::new());
    device.updates.set_restarter(|| {
        RESTARTED.store(true, Ordering::SeqCst);
        Ok(())
    });
    // Nothing waits: the running image restarts.
    assert_eq!(device.updates.restart_running(), Ok(()));
    assert!(RESTARTED.swap(false, Ordering::SeqCst));
    // Another firmware's image committed: the OTA data now chooses it.
    let image = app_image(
        CHIP_ESP32_C3,
        "crosspoint",
        "1.2.3",
        Some(&["paperpaw-plugin-stats"]),
        b"CROSSPOINT-BOARD-V1:x4;",
        300_000,
    );
    let card = Card::with(UPDATE, &image);
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    let facts = facts.unwrap();
    write(&mut device.updates, &card, UPDATE, facts.sha256, None, true).unwrap();
    // Open (written, not committed): refused too.
    assert_eq!(device.updates.restart_running(), Err(Reason::STOPPED));
    device.updates.commit().unwrap();
    assert_eq!(device.data().booted(), 1);
    assert_eq!(device.updates.restart_running(), Err(Reason::STOPPED));
    assert!(
        !RESTARTED.load(Ordering::SeqCst),
        "the restarter is not called"
    );
}

/// A key that is not PaperPaw's installs only where the user accepted that
/// very key; the check says whose it is and decides nothing.
#[test]
fn an_unknown_key_installs_only_when_accepted() {
    let mut device = Device::started(Vec::new());
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    let facts = facts.unwrap();
    let key = paperpaw_update::public_key(&SEED);
    assert_eq!(facts.signer, Signer::Other { key });
    // Not accepted, or another key accepted.
    for accepted in [None, Some([1; 32])] {
        let ended = write(
            &mut device.updates,
            &card,
            UPDATE,
            facts.sha256,
            accepted,
            false,
        );
        assert_eq!(ended, Err(Reason::UNKNOWN_KEY));
        assert!(!device.updates.installing());
        assert_ne!(
            device.target_head(),
            image[..4],
            "the first word is not written"
        );
    }
    write(
        &mut device.updates,
        &card,
        UPDATE,
        facts.sha256,
        Some(key),
        false,
    )
    .unwrap();
    device.updates.commit().unwrap();
    assert_eq!(device.data().booted(), 1);
}

/// An image that is not the one confirmed (changed on the card after the
/// check) is reason 18; the slot keeps no first word, so nothing would start
/// it, and nothing is chosen.
#[test]
fn a_changed_image_is_reason_18_and_nothing_is_chosen() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    let digest = facts.unwrap().sha256;
    // The same image, changed after the check, signed again (a valid file).
    let mut other = image.clone();
    other[300_000] ^= 1;
    card.write(UPDATE, &signed(&other, &SEED)).unwrap();
    let ended = write(&mut device.updates, &card, UPDATE, digest, None, false);
    assert_eq!(ended, Err(Reason::CHANGED));
    assert_ne!(
        device.target_head(),
        image[..4],
        "the first word is not written"
    );
    assert_eq!(device.data().booted(), 0);
    assert!(!device.updates.installing());
    // The job is over: a commit has nothing to commit.
    assert!(device.updates.commit().is_err());
}

/// A damaged signature is reason 4, in the check.
#[test]
fn a_damaged_signature_fails_the_check() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let mut file = signed(&image, &SEED);
    file[1000] ^= 1;
    let card = Card::with(UPDATE, &file);
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    assert_eq!(facts.unwrap_err(), Reason::DAMAGED);
}

/// An app image that PaperPaw did not sign, read as an update: unsigned
/// (reason 8), not damaged.
#[test]
fn an_unsigned_app_image_is_reason_8() {
    let mut device = Device::started(Vec::new());
    let card = Card::with(UPDATE, &image_0_6(&[]));
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    assert_eq!(facts.unwrap_err(), Reason::UNSIGNED);
    let card = Card::with(UPDATE, &[1, 2, 3]);
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    assert_eq!(facts.unwrap_err(), Reason::DAMAGED);
}

/// An update for another chip is checked and said so; installing it is
/// reason 12.
#[test]
fn an_image_for_another_chip_is_said_so_and_not_installed() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = app_image(CHIP_ESP32_S3, PROJECT, "0.6.0", Some(&[]), b"", 100_000);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    let facts = facts.unwrap();
    assert!(!facts.chip_ok);
    let ended = write(
        &mut device.updates,
        &card,
        UPDATE,
        facts.sha256,
        None,
        false,
    );
    assert_eq!(ended, Err(Reason::OTHER_CHIP));
}

/// A card that fails during a check or a write is reason 16, and the job ends.
#[test]
fn a_failing_card_is_reason_16() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    let digest = facts.unwrap().sha256;
    for reads in [0, 3, 12, 40] {
        card.reads_left.set(reads);
        let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
        assert_eq!(
            facts.unwrap_err(),
            Reason::CARD,
            "check after {reads} reads"
        );
        card.reads_left.set(reads);
        let ended = write(&mut device.updates, &card, UPDATE, digest, None, false);
        assert_eq!(ended, Err(Reason::CARD), "write after {reads} reads");
        assert_ne!(
            device.target_head(),
            image[..4],
            "the first word is not written"
        );
    }
}

/// A flash that stops writing in the middle (a cut) is a failure, the slot
/// holds no first word, and nothing is chosen.
#[test]
fn a_cut_mid_write_leaves_nothing_chosen_and_nothing_bootable() {
    let key = paperpaw_update::public_key(&SEED);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let digest = paperpaw_digest_sha256(&image);
    for programs in [0, 3, 50, 90] {
        let mut device = Device::started(vec![key]);
        device
            .updates
            .begin_write(UPDATE, digest, None, false)
            .unwrap();
        // The flash fails after `programs` more writes.
        let mut cut = None;
        let chip = device.updates.flash();
        chip.programs_left.set(programs);
        loop {
            match device.updates.write_step(&card) {
                Step::Progress { .. } => {}
                Step::Done(()) => break,
                Step::Failed(reason) => {
                    cut = Some(reason);
                    break;
                }
            }
        }
        assert_eq!(cut, Some(Reason::OTHER), "after {programs} writes");
        assert_ne!(
            device.target_head(),
            image[..4],
            "the first word is not written"
        );
        assert_eq!(device.data().booted(), 0);
        assert!(!device.updates.installing());
    }
}

/// Another firmware's image: checked as the boot loader would, unsigned,
/// installed only where it is called that; it is final at the switch (valid,
/// no trial), and no list of built-ins is read.
#[test]
fn another_firmwares_image_installs_final() {
    let mut device = Device::started(Vec::new());
    let image = app_image(
        CHIP_ESP32_C3,
        "crosspoint",
        "1.2.3",
        Some(&["paperpaw-plugin-stats"]),
        b"CROSSPOINT-BOARD-V1:x4;",
        300_000,
    );
    let card = Card::with(UPDATE, &image);
    let (seen, facts, _) = check(&mut device.updates, &card, UPDATE);
    let facts = facts.unwrap();
    assert!(seen.is_empty() && facts.foreign && facts.chip_ok);
    assert_eq!(facts.signer, Signer::Unsigned);
    assert_eq!(facts.project.as_str(), "crosspoint");
    assert_eq!(facts.version.as_str(), "1.2.3");
    assert_eq!(facts.sha256, paperpaw_digest_sha256(&image));
    write(&mut device.updates, &card, UPDATE, facts.sha256, None, true).unwrap();
    device.updates.commit().unwrap();
    assert_eq!(device.target(image.len()), &image[..]);
    let data = device.data();
    assert_eq!(data.booted(), 1);
    assert_eq!(data.state_of(1).map(|s| s.0), Some(State::Valid));
}

/// Where the device is shared with another firmware nothing is installed
/// (the slot reports no room, a write is reason 19), but a check works.
#[test]
fn a_shared_device_checks_but_does_not_install() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    device.updates.settle(Some(Source::Ini));
    assert_eq!(FirmwareHost::running(&device.updates).room, 0);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let (_, facts, _) = check(&mut device.updates, &card, UPDATE);
    let facts = facts.unwrap();
    let ended = write(
        &mut device.updates,
        &card,
        UPDATE,
        facts.sha256,
        None,
        false,
    );
    assert_eq!(ended, Err(Reason::NOT_HERE));
}

/// One job at a time: a second begin is a failure until the first ends or is
/// abandoned.
#[test]
fn a_second_job_waits_for_the_first_to_end() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    device.updates.begin_check(UPDATE).unwrap();
    assert_eq!(device.updates.begin_check(UPDATE), Err(Reason::OTHER));
    assert_eq!(
        device.updates.begin_write(UPDATE, [0; 32], None, false),
        Err(Reason::OTHER)
    );
    device.updates.abandon();
    assert!(matches!(
        device.updates.check_step(&card, &mut |_| {}),
        Step::Failed(Reason::OTHER)
    ));
    device
        .updates
        .begin_write(UPDATE, [0; 32], None, false)
        .unwrap();
    assert_eq!(device.updates.begin_check(UPDATE), Err(Reason::OTHER));
}

/// The version and room the kernel is told are the running firmware's.
#[test]
fn the_running_firmware_is_reported() {
    let device = Device::started(Vec::new());
    let running = FirmwareHost::running(&device.updates);
    assert_eq!(running.version, "0.5.0");
    assert_eq!(running.project, PROJECT);
    assert_eq!(running.chip, CHIP_ESP32_C3);
    assert_eq!(u64::from(device.slots.room(0)), running.room);
    assert!(!running.undone);
}

/// The first word is read back after it is written: one the flash lost
/// ends the commit with an error, chooses nothing, and closes the job.
#[test]
fn a_first_word_that_did_not_stay_chooses_nothing() {
    let key = paperpaw_update::public_key(&SEED);
    let mut device = Device::started(vec![key]);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let card = Card::with(UPDATE, &signed(&image, &SEED));
    let digest = paperpaw_digest_sha256(&image);
    write(&mut device.updates, &card, UPDATE, digest, None, false).unwrap();
    let target = device.slots.apps[1].0;
    device.updates.flash().lose_at.set(Some(target));
    assert_eq!(device.updates.commit(), Err(Reason::OTHER));
    assert_eq!(device.data().booted(), 0);
    assert!(!device.updates.installing());
    // The job is over: another can begin.
    device.updates.flash().lose_at.set(None);
    device.updates.begin_check(UPDATE).unwrap();
}

/// Interface 6's install and the kernel's check or write never run
/// together: while one is open the other is refused and writes nothing.
#[test]
fn the_interface_6_install_refuses_while_a_job_is_open() {
    let key = paperpaw_update::public_key(&SEED);
    let image = image_0_6(&["paperpaw-plugin-stats"]);
    let file = signed(&image, &SEED);
    let card = Card::with(UPDATE, &file);
    let busy = Err(InstallFailure::Other(paperpaw_x4::updates::BUSY));
    for write_job in [false, true] {
        let mut device = Device::started(vec![key]);
        if write_job {
            let digest = paperpaw_digest_sha256(&image);
            device
                .updates
                .begin_write(UPDATE, digest, None, false)
                .unwrap();
        } else {
            device.updates.begin_check(UPDATE).unwrap();
        }
        let before = device.updates.flash().bytes.clone();
        assert_eq!(
            device.updates.begin(&card, UPDATE, None, false).map(|_| ()),
            busy
        );
        assert_eq!(
            device.updates.begin_foreign(&card, UPDATE).map(|_| ()),
            busy
        );
        assert_eq!(device.updates.flash().bytes, before, "nothing was written");
        // Given up, interface 6 may go on.
        device.updates.abandon();
        assert!(device.updates.begin(&card, UPDATE, None, false).is_ok());
    }
}
