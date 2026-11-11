//! What the journal bootstrap costs on the card: blocks read and written
//! from mount to the first clean sync, on the test card and (ignored, as
//! in `exfat_cost.rs`) on a big sparse `mkfs.exfat` image:
//!
//! ```sh
//! truncate -s 64G /tmp/big64.img && mkfs.exfat -c 32K /tmp/big64.img
//! PAPERPAW_EXFAT_BIG=/tmp/big64.img cargo test -p paperpaw-fat \
//!     --test exfat_bootstrap_cost -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The big-card test runs twice on that image: fresh, then with the
//! allocation bitmap marked used except its last 4 KiB (a full card whose
//! only free run lies at the end: the longest search). It changes the
//! image; make a new one for each run.
//!
//! Device time is estimated from these counts in the #5 report; the X4
//! measurement is part of device qualification.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "measurement assertions"
)]
mod common;
use common::exfat_card;
use hadris_fat::exfat::sync::ExFatFs;
use hadris_fs::MountOptions;
use paperpaw_fat::{BlockDisk, Blocks, ExDisk, SharedDisk};
use std::io::{Read, Seek, SeekFrom, Write};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct Counts {
    reads: Cell<u64>,
    writes: Cell<u64>,
}

struct Counted<B> {
    inner: B,
    counts: Rc<Counts>,
}

impl<B: Blocks> Blocks for Counted<B> {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.counts.reads.set(self.counts.reads.get() + 1);
        self.inner.read_block(index, block)
    }

    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.counts.writes.set(self.counts.writes.get() + 1);
        self.inner.write_block(index, block)
    }

    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        self.inner.blocks()
    }
}

/// (reads, writes) of mount, bootstrap and sync on `card`.
fn measure<B: Blocks>(card: B) -> ((u64, u64), (u64, u64)) {
    let counts = Rc::new(Counts::default());
    let counted = Counted {
        inner: card,
        counts: counts.clone(),
    };
    let (disk, _) = BlockDisk::find(counted).unwrap();
    let mut fs = ExFatFs::mount(ExDisk::new(SharedDisk::new(disk)), MountOptions::new()).unwrap();
    let mounted = (counts.reads.get(), counts.writes.get());
    fs.bootstrap_intent(123).unwrap();
    fs.sync().unwrap();
    let total = (
        counts.reads.get() - mounted.0,
        counts.writes.get() - mounted.1,
    );
    // The second mount only verifies.
    drop(fs);
    (mounted, total)
}

#[test]
fn bootstrap_reads_and_writes_few_blocks_on_the_test_card() {
    let ((_, mount_writes), (reads, writes)) = measure(exfat_card());
    eprintln!("test card bootstrap+sync: {reads} blocks read, {writes} written");
    assert_eq!(mount_writes, 0);
    // 8 KiB journal (16 blocks), bitmap, entries, flags: bounded, not
    // growing with the card.
    assert!(writes <= 40, "{writes}");
    assert!(reads <= 64, "{reads}");
}

/// A card in a file (a sparse image of any size).
struct FileCard(std::fs::File);

impl Blocks for FileCard {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.0
            .seek(SeekFrom::Start(u64::from(index) * 512))
            .map_err(|_| paperpaw_fat::DiskError)?;
        self.0
            .read_exact(block)
            .map_err(|_| paperpaw_fat::DiskError)
    }

    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.0
            .seek(SeekFrom::Start(u64::from(index) * 512))
            .map_err(|_| paperpaw_fat::DiskError)?;
        self.0.write_all(block).map_err(|_| paperpaw_fat::DiskError)
    }

    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        let bytes = self
            .0
            .metadata()
            .map_err(|_| paperpaw_fat::DiskError)?
            .len();
        u32::try_from(bytes / 512).map_err(|_| paperpaw_fat::DiskError)
    }
}

fn open(path: &str) -> FileCard {
    FileCard(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap(),
    )
}

/// Marks the allocation bitmap of the whole-disk exFAT image at `path`
/// used, except its last 4 KiB. Returns the bitmap's length in bytes.
fn fill_bitmap_but_the_end(path: &str) -> u64 {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let mut boot = [0u8; 512];
    file.read_exact(&mut boot).unwrap();
    let le32 =
        |b: &[u8], at: usize| u64::from(u32::from_le_bytes(b[at..at + 4].try_into().unwrap()));
    let sector = 1u64 << boot[108];
    let cluster = sector << boot[109];
    let heap = le32(&boot, 88) * sector;
    let at = |c: u64| heap + (c - 2) * cluster;
    let mut root = vec![0u8; cluster as usize];
    file.seek(SeekFrom::Start(at(le32(&boot, 96)))).unwrap();
    file.read_exact(&mut root).unwrap();
    let entry = root
        .chunks(32)
        .find(|e| e[0] == 0x81)
        .expect("bitmap entry");
    let first = le32(entry, 20);
    let len = u64::from_le_bytes(entry[24..32].try_into().unwrap());
    let used = len.saturating_sub(4096);
    file.seek(SeekFrom::Start(at(first))).unwrap();
    file.write_all(&vec![0xff; used as usize]).unwrap();
    len
}

#[test]
#[ignore = "needs a big sparse mkfs.exfat image in PAPERPAW_EXFAT_BIG"]
fn bootstrap_on_a_big_card() {
    let path = std::env::var("PAPERPAW_EXFAT_BIG").expect("PAPERPAW_EXFAT_BIG");
    let ((mount_reads, _), (reads, writes)) = measure(open(&path));
    eprintln!(
        "fresh {path}: mount {mount_reads} blocks read; bootstrap+sync {reads} read, {writes} written"
    );
    // A second, freshly made card of the same size, full but its end.
    let copy = format!("{path}.full");
    std::process::Command::new("cp")
        .args(["--sparse=always", &path, &copy])
        .status()
        .unwrap();
    let fresh = std::process::Command::new("mkfs.exfat")
        .args(["-c", "32K", &copy])
        .status()
        .unwrap();
    assert!(fresh.success());
    let bitmap = fill_bitmap_but_the_end(&copy);
    let ((mount_reads, _), (reads, writes)) = measure(open(&copy));
    eprintln!(
        "full but the end ({bitmap} B bitmap): mount {mount_reads} blocks read; bootstrap+sync {reads} read, {writes} written"
    );
    std::fs::remove_file(&copy).unwrap();
}
