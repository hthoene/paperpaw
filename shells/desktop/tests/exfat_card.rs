//! Mounting a big exFAT card (#54): a 64 GB card with 32 KiB clusters has a
//! 16 MiB allocation bitmap and about two million clusters. The driver must
//! not read the bitmap or the FAT at mount, nor hold them in memory
//! (hadris's exFAT needed about 65 KB of heap here). The card is a sparse
//! file, formatted by the driver, read through a counting disk.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::cell::Cell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::rc::Rc;

use paperpaw_emulator::budget;
use paperpaw_fat::fatfs::{VolumeKind, format_exfat};
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError};
use paperpaw_platform::Storage;

const SIZE: u64 = 64 << 30;
const CLUSTER: u32 = 32 << 10;

/// What the card was sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sent {
    reads: u32,
    read_blocks: u32,
    writes: u32,
}

/// A card in a sparse file.
struct Image {
    file: File,
    sent: Rc<Cell<Sent>>,
}

impl Blocks for Image {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.read_blocks(index, block)
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.file
            .seek(SeekFrom::Start(index as u64 * BLOCK as u64))
            .map_err(|_| DiskError)?;
        self.file.write_all(block).map_err(|_| DiskError)?;
        let mut sent = self.sent.get();
        sent.writes += 1;
        self.sent.set(sent);
        Ok(())
    }

    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        self.file
            .seek(SeekFrom::Start(index as u64 * BLOCK as u64))
            .map_err(|_| DiskError)?;
        self.file.read_exact(data).map_err(|_| DiskError)?;
        let mut sent = self.sent.get();
        sent.reads += 1;
        sent.read_blocks += (data.len() / BLOCK) as u32;
        self.sent.set(sent);
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((SIZE / BLOCK as u64) as u32)
    }
}

fn fatfs_flush(disk: &mut BlockDisk<Image>) {
    paperpaw_fat::fatfs::Write::flush(disk).unwrap();
}

fn image(path: &PathBuf, sent: &Rc<Cell<Sent>>) -> Image {
    Image {
        file: File::options().read(true).write(true).open(path).unwrap(),
        sent: sent.clone(),
    }
}

#[test]
fn a_64_gb_card_mounts_in_little_heap_and_few_reads() {
    let path = std::env::temp_dir().join(format!("paperpaw-exfat-card-{}.img", std::process::id()));
    File::create(&path).unwrap().set_len(SIZE).unwrap();
    let sent = Rc::new(Cell::new(Sent::default()));
    let mut disk = BlockDisk::new(image(&path, &sent)).unwrap();
    format_exfat(&mut disk, SIZE, CLUSTER).unwrap();
    fatfs_flush(&mut disk);
    drop(disk);

    sent.set(Sent::default());
    budget::set_limit(None);
    let before = budget::stats().live;
    budget::reset_peak();
    let storage = budget::track(|| paperpaw_fat::mount(image(&path, &sent)).unwrap());
    let mount_peak = budget::stats().peak - before;
    let mount = sent.get();
    assert_eq!(storage.kind(), VolumeKind::ExFat);
    println!("mount: heap peak {mount_peak} B, {mount:?}");

    // The first files: made, listed and read back.
    sent.set(Sent::default());
    budget::reset_peak();
    budget::track(|| {
        storage.write("paperpaw.ini", b"[a]\nb = 1\n").unwrap();
        storage.write("books/one.txt", &[7u8; 100_000]).unwrap();
        storage.sync().unwrap();
        let mut ini = [0u8; 32];
        let got = storage.read_at("paperpaw.ini", 0, &mut ini).unwrap();
        assert_eq!(&ini[..got], b"[a]\nb = 1\n");
        assert_eq!(storage.size("books/one.txt").unwrap(), 100_000);
    });
    let work = sent.get();
    let work_peak = budget::stats().peak - before;
    println!("first files: heap peak {work_peak} B, {work:?}");
    drop(storage);
    std::fs::remove_file(&path).ok();

    assert!(mount_peak <= 16 << 10, "mount heap {mount_peak} B");
    assert!(
        mount.reads <= 8 && mount.read_blocks <= 8 && mount.writes == 0,
        "mount sent {mount:?}"
    );
}
