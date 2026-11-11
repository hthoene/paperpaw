//! Bootstrap prototype under atomic device-block cuts. Torn sectors remain a gate.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "bootstrap regression assertions"
)]
mod common;
use common::{Bytes, Recorded, exfat, exfat_card, read};
use hadris_fat::exfat::sync::{ExFatFs, INTENT_FILE};
use hadris_fs::MountOptions;
use paperpaw_fat::{BlockDisk, Blocks, ExDisk, SharedDisk};
use paperpaw_platform::Storage;
use std::{cell::RefCell, rc::Rc};

fn driver<B: Blocks>(bytes: B) -> ExFatFs<ExDisk<B>> {
    let (disk, _) = BlockDisk::find(bytes).unwrap();
    ExFatFs::mount(ExDisk::new(SharedDisk::new(disk)), MountOptions::new()).unwrap()
}

#[test]
fn bootstrap_publication_and_remount_preserve_existing_files_at_every_block_cut() {
    let mut cuts = 0;
    let mut refused = 0;
    for shift in 0..16 {
        let bytes = exfat_card();
        let card = exfat(&bytes);
        card.write("kept.txt", b"safe content").unwrap();
        for n in 0..shift {
            card.write(&format!("pad{n}"), b"").unwrap();
        }
        card.sync().unwrap();
        drop(card);
        let mut image = bytes.0.borrow().clone();
        let recorded = Recorded {
            data: Rc::new(RefCell::new(image.clone())),
            writes: Rc::new(RefCell::new(Vec::new())),
        };
        let mut fs = driver(recorded.clone());
        let created = fs.bootstrap_intent(123).unwrap();
        let same = fs.bootstrap_intent(456).unwrap();
        assert_eq!(created, same);
        fs.sync().unwrap();
        drop(fs);
        for write in std::iter::once(None).chain(recorded.writes.borrow().iter().map(Some)) {
            if let Some((index, block)) = write {
                let at = *index as usize * 512;
                image[at..at + 512].copy_from_slice(block);
            }
            let cut = Bytes(Rc::new(RefCell::new(image.clone())));
            let card = exfat(&cut);
            assert_eq!(
                read(&card, "kept.txt"),
                b"safe content",
                "shift {shift}, cut {cuts}"
            );
            for n in 0..shift {
                assert_eq!(card.size(&format!("pad{n}")), Ok(0));
            }
            drop(card);
            // Initialization either was not published, or has a complete
            // manifest and allocation; retry must not overwrite user data.
            let before_retry = cut.0.borrow().clone();
            let inherited_dirty = before_retry[106] & 2 != 0;
            let mut fs = driver(cut.clone());
            match fs.bootstrap_intent(789) {
                Ok(_) => {
                    fs.sync().unwrap();
                    drop(fs);
                    assert_eq!(read(&exfat(&cut), "kept.txt"), b"safe content");
                    let mut checker = ExDisk::new(SharedDisk::new(BlockDisk::find(cut).unwrap().0));
                    let mut findings = Vec::new();
                    hadris_fat::exfat::sync::check(&mut checker, &mut [0; 8192], |f| {
                        // The driver deliberately preserves a dirty flag
                        // inherited at mount; sync only clears its own flag.
                        let expected_dirty = inherited_dirty
                            && f.detail().code_in("hadris-fat::exfat") == Some(15)
                            && f.severity() == hadris_fs::Severity::Notice
                            && f.location() == Some(hadris_fs::Location::Byte(106));
                        if !expected_dirty {
                            findings.push(format!("{f:?}"));
                        }
                    })
                    .unwrap();
                    assert!(findings.is_empty(), "{findings:?}");
                }
                Err(error) => {
                    assert!(inherited_dirty, "{error:?}");
                    assert_eq!(
                        error.message(),
                        "journal initialization uncertain; volume check required before retry"
                    );
                    drop(fs);
                    // No second extent, even after repeated fresh mounts.
                    for _ in 0..4 {
                        assert!(driver(cut.clone()).bootstrap_intent(789).is_err());
                        assert_eq!(*cut.0.borrow(), before_retry);
                    }
                    refused += 1;
                }
            }
            cuts += 1;
        }
    }
    eprintln!("bootstrap atomic-block cuts checked: {cuts}");
    assert!(cuts > 100);
    assert!(refused > 0);
    eprintln!("uncertain bootstrap cuts refused without retry allocation: {refused}");
}

#[test]
fn an_unknown_existing_file_is_never_reinitialized() {
    let bytes = exfat_card();
    let card = exfat(&bytes);
    card.write(INTENT_FILE, b"not our journal").unwrap();
    card.sync().unwrap();
    drop(card);
    let before = bytes.0.borrow().clone();
    let mut fs = driver(bytes.clone());
    assert!(fs.bootstrap_intent(123).is_err());
    drop(fs);
    assert_eq!(*bytes.0.borrow(), before);
}

#[test]
fn a_damaged_reserved_name_is_not_replaced_by_a_second_journal() {
    let bytes = exfat_card();
    let mut fs = driver(bytes.clone());
    fs.bootstrap_intent(123).unwrap();
    fs.sync().unwrap();
    drop(fs);
    let name: Vec<u8> = INTENT_FILE
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    {
        let mut data = bytes.0.borrow_mut();
        let text = data
            .windows(name.len())
            .position(|bytes| bytes == name)
            .unwrap();
        let primary = text - 66;
        assert_eq!(data[primary], 0x85);
        data[primary + 2] ^= 1;
    }
    let before = bytes.0.borrow().clone();
    let mut fs = driver(bytes.clone());
    assert!(fs.bootstrap_intent(456).is_err());
    drop(fs);
    assert_eq!(*bytes.0.borrow(), before);
}

#[test]
fn torn_publication_does_not_repair_or_reallocate_from_an_uncertain_manifest() {
    let bytes = exfat_card();
    let card = exfat(&bytes);
    card.write("kept.txt", b"safe content").unwrap();
    card.sync().unwrap();
    drop(card);
    let mut image = bytes.0.borrow().clone();
    let recorded = Recorded {
        data: Rc::new(RefCell::new(image.clone())),
        writes: Rc::new(RefCell::new(Vec::new())),
    };
    let location = driver(recorded.clone()).bootstrap_intent(123).unwrap();
    let publication_block = location.entry / 512;
    let mut tested = 0;
    for (index, block) in recorded.writes.borrow().iter() {
        let at = *index as usize * 512;
        if u64::from(*index) == publication_block {
            // Old/new prefix tears can affect any byte of publication.
            // Bytes outside the intended change must remain unchanged.
            for prefix in 0..=512 {
                let mut torn = image.clone();
                torn[at..at + prefix].copy_from_slice(&block[..prefix]);
                let cut = Bytes(Rc::new(RefCell::new(torn.clone())));
                let mut fs = driver(cut.clone());
                if fs.bootstrap_intent(456).is_err() {
                    drop(fs);
                    assert_eq!(*cut.0.borrow(), torn);
                    for _ in 0..3 {
                        assert!(driver(cut.clone()).bootstrap_intent(456).is_err());
                        assert_eq!(*cut.0.borrow(), torn);
                    }
                }
                assert_eq!(
                    read(&exfat(&cut), "kept.txt"),
                    b"safe content",
                    "prefix {prefix}"
                );
                tested += 1;
            }
        }
        image[at..at + 512].copy_from_slice(block);
    }
    assert!(tested >= 513);
    eprintln!("bootstrap prefix-torn publication cases: {tested}");
}

#[derive(Default)]
struct ReadFault {
    reads: usize,
    fail: Option<usize>,
    hit: bool,
    writes: usize,
    writes_at_failure: usize,
}

struct FallibleReads {
    bytes: Bytes,
    fault: Rc<RefCell<ReadFault>>,
}

impl Blocks for FallibleReads {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        let mut state = self.fault.borrow_mut();
        let read = state.reads;
        state.reads += 1;
        if state.fail == Some(read) {
            state.hit = true;
            state.writes_at_failure = state.writes;
            return Err(paperpaw_fat::DiskError);
        }
        drop(state);
        self.bytes.read_block(index, block)
    }

    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.fault.borrow_mut().writes += 1;
        self.bytes.write_block(index, block)
    }

    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        self.bytes.blocks()
    }
}

#[test]
fn lookup_and_manifest_read_failures_never_authorize_creation() {
    let mut checked = 0;
    for existing in 0..3 {
        let bytes = exfat_card();
        let card = exfat(&bytes);
        card.write("kept.txt", b"safe content").unwrap();
        if existing == 1 {
            card.write(INTENT_FILE, b"foreign file").unwrap();
        }
        card.sync().unwrap();
        drop(card);
        if existing == 2 {
            let mut fs = driver(bytes.clone());
            fs.bootstrap_intent(123).unwrap();
            fs.sync().unwrap();
        }
        let initial = bytes.0.borrow().clone();
        for fail in 0..1000 {
            let copy = Bytes(Rc::new(RefCell::new(initial.clone())));
            let fault = Rc::new(RefCell::new(ReadFault::default()));
            let mut fs = driver(FallibleReads {
                bytes: copy.clone(),
                fault: fault.clone(),
            });
            *fault.borrow_mut() = ReadFault {
                fail: Some(fail),
                ..ReadFault::default()
            };
            let result = fs.bootstrap_intent(456);
            drop(fs);
            let state = fault.borrow();
            if !state.hit {
                assert_eq!(result.is_ok(), existing != 1);
                break;
            }
            assert!(result.is_err(), "read failure {fail}, existing {existing}");
            if state.writes_at_failure == 0 || existing != 0 {
                assert_eq!(state.writes, 0, "read failure led to a write");
                assert_eq!(*copy.0.borrow(), initial);
            }
            drop(state);
            assert_eq!(read(&exfat(&copy), "kept.txt"), b"safe content");
            checked += 1;
            assert!(fail < 999, "fault sweep must reach the completed operation");
        }
    }
    assert!(checked > 0);
    eprintln!("bootstrap one-shot read failures checked: {checked}");
}

/// Free clusters as a fresh, read-only use of the driver counts them.
fn free_clusters(bytes: &Bytes) -> u64 {
    driver(bytes.clone()).statfs().unwrap().free_blocks()
}

fn dirty(bytes: &Bytes) -> bool {
    bytes.0.borrow()[106] & 2 != 0
}

#[derive(Default)]
struct WriteFault {
    writes: usize,
    fail: Option<usize>,
    hit: bool,
}

/// Fails exactly one block write, then behaves like the card again.
struct OneShotWrite {
    bytes: Bytes,
    fault: Rc<RefCell<WriteFault>>,
}

impl Blocks for OneShotWrite {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.bytes.read_block(index, block)
    }

    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        let mut state = self.fault.borrow_mut();
        let write = state.writes;
        state.writes += 1;
        if state.fail == Some(write) {
            state.hit = true;
            return Err(paperpaw_fat::DiskError);
        }
        drop(state);
        self.bytes.write_block(index, block)
    }

    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        self.bytes.blocks()
    }
}

/// Senior B1: a one-shot write error during bootstrap, then a successful
/// ordinary `sync`, must not clear the only durable retry guard; neither a
/// retry in the same mount nor after remounting may allocate a second extent.
#[test]
fn a_failed_bootstrap_keeps_its_retry_guard_through_sync_and_remount() {
    let bytes = exfat_card();
    let card = exfat(&bytes);
    card.write("kept.txt", b"safe content").unwrap();
    card.sync().unwrap();
    drop(card);
    assert!(!dirty(&bytes));
    let initial = bytes.0.borrow().clone();
    let free_before = free_clusters(&bytes);
    let extent = free_before - {
        let fresh = Bytes(Rc::new(RefCell::new(initial.clone())));
        driver(fresh.clone()).bootstrap_intent(1).unwrap();
        free_clusters(&fresh)
    };
    assert!(extent > 0);
    let mut checked = 0;
    let mut guarded = 0;
    for fail in 0..1000 {
        let copy = Bytes(Rc::new(RefCell::new(initial.clone())));
        let fault = Rc::new(RefCell::new(WriteFault {
            fail: Some(fail),
            ..WriteFault::default()
        }));
        let mut fs = driver(OneShotWrite {
            bytes: copy.clone(),
            fault: fault.clone(),
        });
        let first = fs.bootstrap_intent(123);
        if !fault.borrow().hit {
            assert!(first.is_ok(), "write {fail}: {first:?}");
            assert!(!fs.intent_uncertain());
            break;
        }
        assert!(first.is_err(), "write failure {fail} reported success");
        // The ordinary sync may now succeed: the failed block is retried.
        let _ = fs.sync();
        let retry = fs.bootstrap_intent(456);
        let verified = retry.is_ok();
        if verified {
            assert!(!fs.intent_uncertain(), "write {fail}");
        } else {
            assert!(fs.intent_uncertain(), "write {fail}");
            let _ = fs.sync();
        }
        drop(fs);
        // Only an unverified attempt keeps the guard on the card.
        // A write failure before anything reached the card leaves it as it
        // was: then nothing needs guarding.
        let untouched = !verified && *copy.0.borrow() == initial;
        if !verified && !untouched {
            assert!(dirty(&copy), "write {fail}: sync cleared the retry guard");
            guarded += 1;
        }
        // At most the one 8 KiB extent was taken, by every path.
        let lost = free_before - free_clusters(&copy);
        assert!(lost <= extent, "write {fail}: {lost} > {extent} clusters");
        for _ in 0..3 {
            let before = copy.0.borrow().clone();
            let mut fs = driver(copy.clone());
            match fs.bootstrap_intent(789) {
                // Only an untouched card may get its first journal now.
                Ok(_) => assert!(
                    verified || untouched,
                    "write {fail}: remount allocated anew"
                ),
                Err(_) => {
                    assert!(!verified && !untouched, "write {fail}");
                    drop(fs);
                    assert_eq!(*copy.0.borrow(), before, "write {fail}");
                }
            }
        }
        assert!(free_before - free_clusters(&copy) <= extent);
        assert_eq!(read(&exfat(&copy), "kept.txt"), b"safe content");
        checked += 1;
        assert!(fail < 999, "fault sweep must reach the completed bootstrap");
    }
    assert!(checked > 0 && guarded > 0);
    eprintln!("bootstrap one-shot write failures checked: {checked}, guarded: {guarded}");
}

/// Senior B2: a free run early in the root must not hide a later
/// checksum-damaged reserved-name set.
#[test]
fn a_damaged_reserved_name_after_a_free_run_refuses_without_writes() {
    let bytes = exfat_card();
    let card = exfat(&bytes);
    card.write("gap.txt", b"to be removed").unwrap();
    card.write("kept.txt", b"safe content").unwrap();
    card.write(INTENT_FILE, b"foreign").unwrap();
    card.remove("gap.txt").unwrap();
    card.sync().unwrap();
    drop(card);
    let name: Vec<u8> = INTENT_FILE
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    {
        let mut data = bytes.0.borrow_mut();
        let text = data
            .windows(name.len())
            .position(|bytes| bytes == name)
            .unwrap();
        let primary = text - 66;
        assert_eq!(
            &data[primary..primary + 65]
                .iter()
                .step_by(32)
                .copied()
                .collect::<Vec<_>>(),
            &[0x85, 0xc0, 0xc1]
        );
        // Move the set into a later deleted three-entry run of the same
        // block and delete it where it was, so a free run comes first.
        let block_end = primary / 512 * 512 + 512;
        let later = (primary + 96..=block_end - 96)
            .step_by(32)
            .find(|&at| (0..3).all(|i| matches!(data[at + i * 32], 0x05 | 0x40 | 0x41)))
            .expect("fixture needs a later deleted run in the block");
        let set: Vec<u8> = data[primary..primary + 96].to_vec();
        data[later..later + 96].copy_from_slice(&set);
        for i in 0..3 {
            data[primary + i * 32] &= 0x7f;
        }
        // Damage only the moved set's checksum.
        data[later + 2] ^= 1;
    }
    assert!(!dirty(&bytes));
    let before = bytes.0.borrow().clone();
    let mut fs = driver(bytes.clone());
    let result = fs.bootstrap_intent(456);
    assert_eq!(
        result.map(|_| ()).map_err(|e| e.kind()),
        Err(hadris_fs::ErrorKind::Corrupt)
    );
    drop(fs);
    assert_eq!(*bytes.0.borrow(), before);
}

/// Senior nit: a user file whose name only starts with the reserved name is
/// another file, not a damaged journal; bootstrap proceeds and keeps it.
#[test]
fn a_longer_name_starting_with_the_reserved_name_is_not_a_journal() {
    let bytes = exfat_card();
    let card = exfat(&bytes);
    card.write("PP$INTENTX.txt", b"user file").unwrap();
    card.sync().unwrap();
    drop(card);
    let mut fs = driver(bytes.clone());
    fs.bootstrap_intent(123).unwrap();
    fs.sync().unwrap();
    drop(fs);
    assert_eq!(read(&exfat(&bytes), "PP$INTENTX.txt"), b"user file");
}
