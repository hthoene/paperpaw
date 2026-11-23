//! `BlockDisk` against plain bytes: random reads, writes and seeks of
//! any length and alignment (whole blocks passing by, blocks read ahead,
//! more blocks than are kept) read what was written, and a flush puts
//! it all on the disk. Finding the volume on cards with broken partition
//! tables.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_fat::fatfs::{Read, Seek, SeekFrom, Write};
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, VolumeError};
use proptest::prelude::*;

/// A disk of blocks in memory, its bytes shared to look at, that fails
/// outside its blocks and takes whole blocks only.
#[derive(Clone)]
struct Disk(Rc<RefCell<Vec<u8>>>);

impl Disk {
    fn new(data: Vec<u8>) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }
}

impl Blocks for Disk {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.read_blocks(index, block)
    }

    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        assert_eq!(data.len() % BLOCK, 0, "whole blocks only");
        let at = index as usize * BLOCK;
        data.copy_from_slice(self.0.borrow().get(at..at + data.len()).ok_or(DiskError)?);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.write_blocks(index, block)
    }

    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        assert_eq!(data.len() % BLOCK, 0, "whole blocks only");
        let at = index as usize * BLOCK;
        self.0
            .borrow_mut()
            .get_mut(at..at + data.len())
            .ok_or(DiskError)?
            .copy_from_slice(data);
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.0.borrow().len() / BLOCK) as u32)
    }
}

const BLOCKS: usize = 64;

#[derive(Debug, Clone)]
enum Op {
    Read(u64, usize),
    Write(u64, Vec<u8>),
    Flush,
}

fn op() -> impl Strategy<Value = Op> {
    let size = (BLOCKS * BLOCK) as u64;
    prop_oneof![
        (0..size + 10, 0usize..5 * BLOCK).prop_map(|(at, n)| Op::Read(at, n)),
        (0..size + 10, 0usize..5 * BLOCK, any::<u8>()).prop_map(|(at, n, seed)| {
            Op::Write(at, (0..n).map(|i| (i as u8) ^ seed).collect())
        }),
        // Aligned whole blocks, which pass by the kept ones.
        (0..BLOCKS as u64, 1usize..12, any::<u8>())
            .prop_map(|(block, n, seed)| Op::Write(block * BLOCK as u64, vec![seed; n * BLOCK])),
        Just(Op::Flush),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    #[test]
    fn reads_see_what_was_written(ops in prop::collection::vec(op(), 1..60)) {
        let initial: Vec<u8> = (0..BLOCKS * BLOCK).map(|i| (i * 7 / 3) as u8).collect();
        let mut model = initial.clone();
        let bytes = Disk::new(initial);
        let mut disk = BlockDisk::new(bytes.clone()).unwrap();
        for op in ops {
            match op {
                Op::Read(at, n) => {
                    let result = disk.seek(SeekFrom::Start(at));
                    if at as usize > model.len() {
                        prop_assert!(result.is_err());
                        continue;
                    }
                    prop_assert_eq!(result, Ok(at));
                    let mut buf = vec![0u8; n];
                    let read = disk.read(&mut buf).unwrap();
                    let end = (at as usize + n).min(model.len());
                    prop_assert_eq!(read, end - at as usize);
                    prop_assert!(buf[..read] == model[at as usize..end]);
                }
                Op::Write(at, data) => {
                    if disk.seek(SeekFrom::Start(at)).is_err() {
                        prop_assert!(at as usize > model.len());
                        continue;
                    }
                    let written = disk.write(&data).unwrap();
                    let end = (at as usize + data.len()).min(model.len());
                    prop_assert_eq!(written, end - at as usize);
                    model[at as usize..end].copy_from_slice(&data[..written]);
                }
                Op::Flush => disk.flush().unwrap(),
            }
        }
        disk.seek(SeekFrom::Start(0)).unwrap();
        let mut all = vec![0u8; model.len()];
        disk.read_exact(&mut all).unwrap();
        prop_assert!(all == model);
        // Flushed: the disk itself holds it all.
        disk.flush().unwrap();
        prop_assert!(*bytes.0.borrow() == model);
    }
}

/// Small writes into more blocks than are kept, by turns: after a flush
/// every one is on the disk.
#[test]
fn flushed_writes_are_on_the_disk() {
    let bytes = Disk::new(vec![0; BLOCKS * BLOCK]);
    let mut disk = BlockDisk::new(bytes.clone()).unwrap();
    for round in 0..3u8 {
        for block in 0..40u64 {
            disk.seek(SeekFrom::Start(
                block * BLOCK as u64 + 100 + u64::from(round),
            ))
            .unwrap();
            disk.write_all(&[round + 1; 10]).unwrap();
        }
    }
    disk.flush().unwrap();
    let data = bytes.0.borrow();
    for block in 0..40 {
        let at = block * BLOCK + 100;
        assert_eq!(
            &data[at..at + 12],
            &[1, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3],
            "{block}"
        );
    }
}

#[test]
fn seeks_stay_within_the_disk() {
    let mut disk = BlockDisk::new(Disk::new(vec![0; 4 * BLOCK])).unwrap();
    let size = 4 * BLOCK as u64;
    assert_eq!(disk.seek(SeekFrom::End(0)), Ok(size));
    assert_eq!(disk.seek(SeekFrom::End(-1)), Ok(size - 1));
    assert_eq!(disk.seek(SeekFrom::End(1)), Err(DiskError));
    assert_eq!(disk.seek(SeekFrom::Start(size + 1)), Err(DiskError));
    assert_eq!(disk.seek(SeekFrom::Start(u64::MAX)), Err(DiskError));
    assert_eq!(disk.seek(SeekFrom::Current(i64::MIN)), Err(DiskError));
    assert_eq!(disk.seek(SeekFrom::Start(10)), Ok(10));
    assert_eq!(disk.seek(SeekFrom::Current(-10)), Ok(0));
    // At the end: nothing read, nothing written.
    disk.seek(SeekFrom::End(0)).unwrap();
    assert_eq!(disk.read(&mut [0u8; 10]), Ok(0));
    assert_eq!(disk.write(&[1u8; 10]), Ok(0));
    // A disk without blocks reads nothing.
    let mut empty = BlockDisk::new(Disk::new(Vec::new())).unwrap();
    assert_eq!(empty.read(&mut [0u8; 4]), Ok(0));
}

/// A card whose first block is a partition table with `entries` (kind,
/// first block, count) and nothing else.
fn card(blocks: usize, entries: &[(u8, u32, u32)]) -> Disk {
    let mut data = vec![0u8; blocks * BLOCK];
    for (i, (kind, first, count)) in entries.iter().enumerate() {
        let at = 446 + 16 * i;
        data[at + 4] = *kind;
        data[at + 8..at + 12].copy_from_slice(&first.to_le_bytes());
        data[at + 12..at + 16].copy_from_slice(&count.to_le_bytes());
    }
    data[510] = 0x55;
    data[511] = 0xAA;
    Disk::new(data)
}

#[test]
fn broken_partition_tables_find_no_volume() {
    // Partitions outside the card, empty or at block 0.
    let disk = card(
        64,
        &[
            (0x0c, 1000, 10),
            (0x0c, 5, 0),
            (0x0c, 0, 64),
            (0x0c, u32::MAX, u32::MAX),
        ],
    );
    assert_eq!(BlockDisk::volume(disk).err(), Some(VolumeError::NoFat));
    // No signature: no table.
    let disk = card(64, &[(0x0c, 8, 56)]);
    disk.0.borrow_mut()[510] = 0;
    assert_eq!(BlockDisk::volume(disk).err(), Some(VolumeError::NoFat));
    // A disk without blocks.
    assert_eq!(
        BlockDisk::volume(Disk::new(Vec::new())).err(),
        Some(VolumeError::Disk)
    );
}

/// A GPT whose header says odd things: entries of a size that does not
/// divide a block, of no size, a table past the card's end, too many
/// entries.
#[test]
fn broken_gpt_headers_find_no_volume() {
    let gpt = |first_entry: u64, count: u32, size: u32| {
        let disk = card(64, &[(0xEE, 1, 63)]);
        {
            let mut data = disk.0.borrow_mut();
            let header = &mut data[BLOCK..2 * BLOCK];
            header[..8].copy_from_slice(b"EFI PART");
            header[72..80].copy_from_slice(&first_entry.to_le_bytes());
            header[80..84].copy_from_slice(&count.to_le_bytes());
            header[84..88].copy_from_slice(&size.to_le_bytes());
        }
        disk
    };
    for (first, count, size) in [
        (2, 4, 0),
        (2, 4, 100),
        (2, 4, 384),
        (2, 4, 1024),
        (2, 0, 128),
    ] {
        assert_eq!(
            BlockDisk::volume(gpt(first, count, size)).err(),
            Some(VolumeError::NoFat),
            "{first} {count} {size}"
        );
    }
    // A table past the card: reading it fails.
    assert_eq!(
        BlockDisk::volume(gpt(1000, 4, 128)).err(),
        Some(VolumeError::Disk)
    );
    assert_eq!(
        BlockDisk::volume(gpt(u64::MAX, 4, 128)).err(),
        Some(VolumeError::Disk)
    );
    // As many entries as a header may say (128 are read): all empty.
    assert_eq!(
        BlockDisk::volume(gpt(2, u32::MAX, 128)).err(),
        Some(VolumeError::NoFat)
    );
    // Not a GPT at all.
    let disk = gpt(2, 4, 128);
    disk.0.borrow_mut()[BLOCK] = b'X';
    assert_eq!(BlockDisk::volume(disk).err(), Some(VolumeError::NoFat));
}

/// A card that answers `reads` block reads, then none (a glitch on its
/// bus).
struct Glitching {
    disk: Disk,
    reads: u32,
}

impl Blocks for Glitching {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.reads = self.reads.checked_sub(1).ok_or(DiskError)?;
        self.disk.read_block(index, block)
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.disk.write_block(index, block)
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        self.disk.blocks()
    }
}

#[test]
fn a_card_that_stops_answering_is_unreadable_not_without_fat() {
    // A first block that says FAT where the volume is looked for, and
    // holds nothing else FAT needs.
    let disk = Disk::new(vec![0u8; 64 * BLOCK]);
    {
        let mut data = disk.0.borrow_mut();
        data[0] = 0xEB;
        data[0x36..0x39].copy_from_slice(b"FAT");
        data[11..13].copy_from_slice(&512u16.to_le_bytes());
    }
    // Read whole, it holds no FAT file system.
    assert_eq!(
        paperpaw_fat::mount(disk.clone()).err(),
        Some(VolumeError::NoFat)
    );
    // Not read past the block that found the volume: the card could not
    // be read, which formatting it would not mend.
    assert_eq!(
        paperpaw_fat::mount(Glitching { disk, reads: 1 }).err(),
        Some(VolumeError::Disk)
    );
}
