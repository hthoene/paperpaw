//! No cluster is lost to a power cut (#238, #269): cut off after any block
//! written, the card as it is then mounted holds no cluster that is marked
//! taken and belongs to no file or folder, and the free space it tells is
//! what its FAT says.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use paperpaw_fat::fatfs::{
    FatType, FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write,
};
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk};
use paperpaw_platform::Storage;

/// A card that keeps every block written, in order.
#[derive(Clone)]
struct Logged {
    bytes: Rc<RefCell<Vec<u8>>>,
    log: Rc<RefCell<Vec<Written>>>,
}

/// A block written: its index, its bytes, and how many blocks the command
/// that wrote it wrote.
type Written = (u32, [u8; BLOCK], usize);

impl Blocks for Logged {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.bytes.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }
    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.write_blocks(index, block)
    }
    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        let blocks = data.as_chunks::<BLOCK>().0;
        for (i, block) in (index..).zip(blocks) {
            self.log.borrow_mut().push((i, *block, blocks.len()));
        }
        let at = index as usize * BLOCK;
        self.bytes
            .borrow_mut()
            .get_mut(at..at + data.len())
            .ok_or(DiskError)?
            .copy_from_slice(data);
        Ok(())
    }
    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.bytes.borrow().len() / BLOCK) as u32)
    }
}

type Fat = FatStorage<SharedDisk<Logged>>;

/// A FAT32 card of 512-byte clusters (128 of them to a block of the FAT, so
/// a file of some size crosses blocks of it), and the storage on it.
fn card() -> (&'static Fat, Logged) {
    let logged = Logged {
        bytes: Rc::new(RefCell::new(vec![0u8; 40 << 20])),
        log: Rc::default(),
    };
    let mut disk = BlockDisk::new(logged.clone()).unwrap();
    paperpaw_fat::fatfs::format_volume(
        &mut disk,
        FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .bytes_per_cluster(512),
    )
    .unwrap();
    disk.flush().unwrap();
    let mut disk = BlockDisk::new(logged.clone()).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    let shared = SharedDisk::new(disk);
    let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
    let storage = FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared);
    logged.log.borrow_mut().clear();
    (Box::leak(Box::new(storage)), logged)
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The FAT32 volume in `image`, read by hand.
struct Volume<'a> {
    image: &'a [u8],
    fat: usize,
    data: usize,
    cluster: usize,
    clusters: u32,
    root: u32,
}

impl<'a> Volume<'a> {
    fn of(image: &'a [u8]) -> Self {
        let reserved = usize::from(u16::from_le_bytes([image[14], image[15]])) * 512;
        let fat_bytes = word(image, 36) as usize * 512;
        let data = reserved + usize::from(image[16]) * fat_bytes;
        let cluster = usize::from(image[13]) * 512;
        let total = word(image, 32) as usize * 512;
        Self {
            image,
            fat: reserved,
            data,
            cluster,
            clusters: ((total - data) / cluster) as u32,
            root: word(image, 44),
        }
    }

    fn entry(&self, cluster: u32) -> u32 {
        word(self.image, self.fat + cluster as usize * 4) & 0x0FFF_FFFF
    }

    fn free(&self) -> u64 {
        (2..self.clusters + 2)
            .filter(|&c| self.entry(c) == 0)
            .count() as u64
            * self.cluster as u64
    }

    /// The clusters of the chain from `first`, into `held`. A chain that
    /// runs into a free cluster, out of the card or into a cluster held
    /// already is told: no file may point there.
    fn chain(&self, first: u32, held: &mut BTreeSet<u32>, what: &str) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut at = first;
        loop {
            assert!(
                (2..self.clusters + 2).contains(&at),
                "{what}: its chain leaves the card at {at}"
            );
            assert_ne!(self.entry(at), 0, "{what}: its chain runs into free {at}");
            assert!(held.insert(at), "{what}: cluster {at} is held twice");
            chain.push(at);
            match self.entry(at) {
                next if next >= 0x0FFF_FFF8 => return chain,
                next => at = next,
            }
        }
    }

    /// The clusters marked taken that no file and no folder holds. Two
    /// entries may hold one chain from its first cluster on (a file and its
    /// new content, a move cut off between its two entries).
    fn lost(&self, what: &str) -> Vec<u32> {
        let mut held = BTreeSet::new();
        let mut firsts = BTreeSet::from([self.root]);
        let mut folders = vec![self.root];
        while let Some(folder) = folders.pop() {
            'folder: for cluster in self.chain(folder, &mut held, what) {
                let at = self.data + (cluster as usize - 2) * self.cluster;
                for entry in self.image[at..at + self.cluster].as_chunks::<32>().0 {
                    match (entry[0], entry[11]) {
                        (0, _) => break 'folder,
                        // Removed, part of a long name, the volume's label, `.` and `..`.
                        (0xE5, _) | (_, 0x0F) | (b'.', _) => continue,
                        (_, attributes) if attributes & 0x08 != 0 => continue,
                        _ => {}
                    }
                    let first = u32::from(u16::from_le_bytes([entry[26], entry[27]]))
                        | u32::from(u16::from_le_bytes([entry[20], entry[21]])) << 16;
                    if first == 0 || !firsts.insert(first) {
                        continue;
                    }
                    if entry[11] & 0x10 != 0 {
                        folders.push(first);
                    } else {
                        self.chain(first, &mut held, what);
                    }
                }
            }
        }
        (2..self.clusters + 2)
            .filter(|&c| self.entry(c) != 0 && self.entry(c) != 0x0FFF_FFF7 && !held.contains(&c))
            .collect()
    }
}

/// The card `image` mounted as it is and written back: no cluster is lost on
/// it, and the free space told is what its FAT says. Whether the mount had
/// clusters to give back (the cut left them taken by no file).
fn nothing_is_lost(image: &[u8], what: &str) -> bool {
    // Before the mount too, no file points at a free cluster or at another's.
    let lost_before = Volume::of(image).lost(what).len();
    let bytes = Rc::new(RefCell::new(image.to_vec()));
    let card = common::Card::mount(common::Bytes(bytes.clone()));
    card.storage.sync().unwrap();
    let free = loop {
        if let Some(free) = card.storage.free_space() {
            break free;
        }
    };
    card.storage.sync().unwrap();
    {
        let now = bytes.borrow();
        let volume = Volume::of(&now);
        let lost = volume.lost(what);
        assert!(
            lost.is_empty(),
            "{what}: {} clusters taken by no file: {lost:?}",
            lost.len()
        );
        assert_eq!(free, volume.free(), "{what}: the free space told");
    }
    // What the card keeps of the count is what the next start takes.
    let again = card.remounted();
    let free = loop {
        if let Some(free) = again.storage.free_space() {
            break free;
        }
    };
    assert_eq!(
        free,
        Volume::of(&again.bytes.0.borrow()).free(),
        "{what}: the free space after one more start"
    );
    lost_before > 0
}

/// `work` done on a fresh card (after `before`, written back), and `look`
/// at the card as a cut after each block written leaves it (where a command
/// wrote many blocks of a file's data: after its first and its last).
fn at_every_cut(
    name: &str,
    before: impl FnOnce(&Fat),
    work: impl FnOnce(&Fat),
    mut look: impl FnMut(&[u8], &str),
) {
    let (storage, logged) = card();
    before(storage);
    storage.sync().unwrap();
    let mut image = logged.bytes.borrow().clone();
    logged.log.borrow_mut().clear();
    look(&image, &format!("{name}: before"));
    work(storage);
    storage.sync().unwrap();
    let log = std::mem::take(&mut *logged.log.borrow_mut());
    assert!(log.len() > 20, "{name}: {} blocks written", log.len());
    let mut in_command = 0;
    for (n, (index, block, of)) in log.iter().enumerate() {
        let at = *index as usize * BLOCK;
        image[at..at + BLOCK].copy_from_slice(block);
        in_command = if in_command + 1 == *of {
            0
        } else {
            in_command + 1
        };
        if *of > 8 && in_command > 1 {
            continue;
        }
        look(&image, &format!("{name}: after block {n} of {}", log.len()));
    }
}

/// At every cut of `work`, nothing is lost; and the cuts are such that
/// the mount has clusters to give back after some of them.
fn cut_at_every_block(name: &str, before: impl FnOnce(&Fat), work: impl FnOnce(&Fat)) {
    let mut given_back = 0;
    at_every_cut(name, before, work, |image, what| {
        given_back += usize::from(nothing_is_lost(image, what));
    });
    assert!(given_back > 0, "{name}: no cut left a cluster to give back");
}

fn pattern(len: usize, seed: usize) -> Vec<u8> {
    (0..len).map(|k| (k * 7 + seed * 13) as u8).collect()
}

/// #238: a file made by appends that were not written back yet, and an
/// older file that grows by them.
#[test]
fn an_append_cut_off_before_it_is_written_back_loses_no_cluster() {
    cut_at_every_block(
        "appends",
        |s| {
            s.create_dir("cache").unwrap();
            s.append("cache/old.bin", &pattern(3000, 1)).unwrap();
        },
        |s| {
            for part in 0..4 {
                s.append("cache/new.bin", &pattern(5000, part)).unwrap();
            }
            for part in 0..4 {
                s.append("cache/old.bin", &pattern(5000, part)).unwrap();
            }
            s.sync().unwrap();
            s.append("cache/new.bin", &pattern(700, 9)).unwrap();
        },
    );
}

/// #269: a file that grows across blocks of the FAT (each holds 128
/// clusters here) and around other files, written back between.
#[test]
fn a_file_grown_across_blocks_of_the_fat_loses_no_cluster() {
    cut_at_every_block(
        "growth",
        |s| {
            s.create_dir("cache").unwrap();
            // Holes for the growing file to go around.
            for n in 0..6 {
                s.write(&format!("cache/small{n}.bin"), &pattern(900, n))
                    .unwrap();
            }
            s.remove("cache/small1.bin").unwrap();
            s.remove("cache/small4.bin").unwrap();
        },
        |s| {
            for (i, size) in [
                700usize, 3000, 4096, 900, 12_000, 40_000, 5000, 70_000, 9000,
            ]
            .iter()
            .enumerate()
            {
                s.append("cache/img.gray", &pattern(*size, i)).unwrap();
                if i % 2 == 1 {
                    s.sync().unwrap();
                }
            }
        },
    );
}

/// Files written, replaced and removed (one of them across blocks of the
/// FAT), folders made, filled until they grow, moved and removed.
#[test]
fn writes_removals_and_folders_cut_off_lose_no_cluster() {
    cut_at_every_block(
        "files",
        |s| {
            s.create_dir("books").unwrap();
            s.write("books/a.bin", &pattern(2000, 1)).unwrap();
            s.write("books/big.bin", &pattern(90_000, 2)).unwrap();
        },
        |s| {
            s.write("books/a.bin", &pattern(700, 3)).unwrap();
            s.write("books/b.bin", &pattern(5000, 4)).unwrap();
            s.remove("books/big.bin").unwrap();
            s.sync().unwrap();
            s.create_dir("new/folder").unwrap();
            for n in 0..14 {
                s.write(
                    &format!("new/folder/a file with a long name {n}.bin"),
                    &pattern(300, n),
                )
                .unwrap();
            }
            s.rename("books/b.bin", "new/folder/b.bin").unwrap();
            s.append("new/folder/b.bin", &pattern(20_000, 5)).unwrap();
            s.remove("new/folder/b.bin").unwrap();
            // A file made by an append, moved at once, and one made in its place.
            s.append("books/c.bin", &pattern(300, 6)).unwrap();
            s.rename("books/c.bin", "new/c.bin").unwrap();
            s.append("books/c.bin", &pattern(300, 7)).unwrap();
            s.write("books/d.bin", b"").unwrap();
            s.append("books/d.bin", &pattern(900, 8)).unwrap();
            s.remove_dir_all("new").unwrap();
        },
    );
}

/// The FS information sector of `image` and the block of the first FAT that
/// holds the entry of `cluster`.
fn sector_and_fat_block(image: &[u8], cluster: u32) -> (usize, usize) {
    let sector = usize::from(u16::from_le_bytes([image[48], image[49]])) * 512;
    let fat = usize::from(u16::from_le_bytes([image[14], image[15]])) * 512;
    (sector, fat + cluster as usize * 4 / 512 * 512)
}

/// What the cut left taken by no file is freed only where the card is as
/// the cut left it: a count or a hint another device wrote into the FS
/// information sector, or a cluster taken or freed in the same block of the
/// FAT, and the mount frees nothing. (A device that changed the card may
/// have given the clusters a file.)
#[test]
fn what_another_device_changed_is_not_freed() {
    let mut tried = 0;
    at_every_cut(
        "changed",
        |s| {
            s.create_dir("cache").unwrap();
            for n in 0..4 {
                s.write(&format!("cache/small{n}.bin"), &pattern(900, n))
                    .unwrap();
            }
            s.remove("cache/small1.bin").unwrap();
        },
        |s| {
            s.append("cache/img.gray", &pattern(3000, 1)).unwrap();
            s.sync().unwrap();
            s.append("cache/img.gray", &pattern(70_000, 2)).unwrap();
            s.remove("cache/img.gray").unwrap();
            s.create_dir("cache/more").unwrap();
        },
        |image, what| {
            let lost = Volume::of(image).lost(what);
            let Some(&first) = lost.first() else {
                return;
            };
            tried += 1;
            let (sector, fat_block) = sector_and_fat_block(image, first);
            let mounted = |change: &dyn Fn(&mut Vec<u8>)| {
                let mut changed = image.to_vec();
                change(&mut changed);
                let bytes = Rc::new(RefCell::new(changed));
                let card = common::Card::mount(common::Bytes(bytes.clone()));
                card.storage.sync().unwrap();
                let now = bytes.borrow();
                Volume::of(&now).lost(what)
            };
            assert!(mounted(&|_| {}).is_empty(), "{what}: as the cut left it");
            // Another count; another hint.
            let kept = mounted(&|bytes| bytes[sector + 488..sector + 492].fill(0));
            assert_eq!(kept, lost, "{what}: a count was written");
            let kept = mounted(&|bytes| bytes[sector + 492..sector + 496].fill(0));
            assert_eq!(kept, lost, "{what}: a hint was written");
            // A cluster taken in the same block of the FAT (the first free one
            // there): what the cut left in that block stays. (What it left in a
            // block nobody touched is still no file's, and may go.)
            let free = (0..128).find(|i| word(image, fat_block + i * 4) & 0x0FFF_FFFF == 0);
            if let Some(free) = free {
                let kept = mounted(&|bytes| {
                    let at = fat_block + free * 4;
                    bytes[at..at + 4].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
                });
                let block = first / 128;
                assert!(
                    lost.iter()
                        .filter(|&&cluster| cluster / 128 == block)
                        .all(|cluster| kept.contains(cluster)),
                    "{what}: a cluster was taken in the block: {kept:?} of {lost:?}"
                );
            }
        },
    );
    assert!(tried > 5, "{tried} cuts left clusters taken by no file");
}
