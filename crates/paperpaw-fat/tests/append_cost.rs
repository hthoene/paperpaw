//! What a stream of appends costs the card, in commands: the picture cache
//! of a book is written by appending 4 KB pieces to one growing file (#250).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_fat::fatfs::{
    FatType, FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write,
};
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk};
use paperpaw_platform::Storage;

#[derive(Default)]
struct Counts {
    log: Vec<(u32, usize)>,
    /// Every write command's blocks, in order, kept while `keep` is set.
    kept: Vec<(u32, Vec<u8>)>,
    keep: bool,
    write_commands: usize,
    write_blocks: usize,
    read_commands: usize,
}

#[derive(Clone)]
struct Counted {
    bytes: Rc<RefCell<Vec<u8>>>,
    counts: Rc<RefCell<Counts>>,
}

impl Blocks for Counted {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.read_blocks(index, block)
    }
    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        self.counts.borrow_mut().read_commands += 1;
        let at = index as usize * BLOCK;
        data.copy_from_slice(
            self.bytes
                .borrow()
                .get(at..at + data.len())
                .ok_or(DiskError)?,
        );
        Ok(())
    }
    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.write_blocks(index, block)
    }
    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        {
            let mut c = self.counts.borrow_mut();
            c.write_commands += 1;
            c.write_blocks += data.len() / BLOCK;
            c.log.push((index, data.len() / BLOCK));
            if c.keep {
                c.kept.push((index, data.to_vec()));
            }
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

type Fat = FatStorage<SharedDisk<Counted>>;

fn card(cluster: u32, size: usize) -> (&'static Fat, Counted) {
    let bytes = Rc::new(RefCell::new(vec![0u8; size]));
    let counted = Counted {
        bytes,
        counts: Rc::default(),
    };
    let mut disk = BlockDisk::new(counted.clone()).unwrap();
    paperpaw_fat::fatfs::format_volume(
        &mut disk,
        FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .bytes_per_cluster(cluster),
    )
    .unwrap();
    disk.flush().unwrap();
    let mut disk = BlockDisk::new(counted.clone()).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    let shared = SharedDisk::new(disk);
    let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
    let storage = FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared);
    (Box::leak(Box::new(storage)), counted)
}

/// 4 KB pieces appended to one file, as a decoded picture is written: about one
/// write command each, where it took five (the FAT block of the new cluster and its
/// copy, written twice each in order, and the data).
#[test]
fn picture_sized_appends_cost_about_one_command_each() {
    for cluster in [4096u32, 8192, 16384, 32768] {
        let (storage, counted) = card(cluster, cluster as usize * 70_000);
        storage.create_dir("cache").unwrap();
        storage.sync().unwrap();
        let before = counted.counts.borrow().write_commands;
        let data = vec![0xA5u8; 4096];
        for _ in 0..100 {
            storage.append("cache/img.gray", &data).unwrap();
        }
        storage.sync().unwrap();
        let commands = counted.counts.borrow().write_commands - before;
        println!("cluster {cluster} B: {commands} write commands for 100 appends of 4096 B");
        assert!(
            commands <= 135,
            "cluster {cluster} B: {commands} write commands for 100 appends of 4096 B"
        );
    }
}

/// The FAT of a card's bytes: its first copy's entries, for the clusters (index 0 is
/// cluster 2), and the clusters' count.
fn fat_entries(bytes: &[u8]) -> Vec<u32> {
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let sector = u16_at(11);
    let per_cluster = bytes[13] as usize;
    let reserved = u16_at(14);
    let fats = bytes[16] as usize;
    let root = u16_at(17);
    let total = match u16_at(19) {
        0 => u32_at(32),
        n => n,
    };
    let fat32 = u16_at(22) == 0;
    let per_fat = if fat32 { u32_at(36) } else { u16_at(22) };
    let data = total - reserved - fats * per_fat - (root * 32).div_ceil(sector);
    let clusters = data / per_cluster;
    let at = reserved * sector;
    (2..clusters + 2)
        .map(|n| {
            if fat32 {
                (u32_at(at + n * 4) & 0x0FFF_FFFF) as u32
            } else {
                u16_at(at + n * 2) as u32
            }
        })
        .collect()
}

/// The FAT of `bytes` must hold no link into a free cluster and no two links to one.
fn check_chains(bytes: &[u8], when: &str) {
    let entries = fat_entries(bytes);
    let end = |v: u32| v >= 0xFFF8 && v != 0x0FFF_FFF7 && v != 0xFFF7 || v >= 0x0FFF_FFF8;
    let mut pointed = vec![0u8; entries.len() + 2];
    for (i, &v) in entries.iter().enumerate() {
        if v == 0 || end(v) || v == 0xFFF7 || v == 0x0FFF_FFF7 {
            continue;
        }
        let n = v as usize;
        assert!(
            n >= 2 && n < entries.len() + 2,
            "{when}: cluster {} links to {n}, outside",
            i + 2
        );
        assert!(
            entries[n - 2] != 0,
            "{when}: cluster {} links into free cluster {n}",
            i + 2
        );
        pointed[n] += 1;
        assert!(pointed[n] == 1, "{when}: two links to cluster {n}");
    }
}

/// How many clusters the FAT of `bytes` holds taken.
fn used_clusters(bytes: &[u8]) -> usize {
    fat_entries(bytes).iter().filter(|&&v| v != 0).count()
}

/// The card before a stream, its write commands, what was appended, the clusters taken before.
type Stream = (Vec<u8>, Vec<(u32, Vec<u8>)>, Vec<u8>, usize);

/// The write commands of a stream of appends (sizes that cross FAT blocks, on clusters of
/// 512 B so that they do), each written back after the piece (the entry holds a size while
/// the next piece takes a run: a cut leaves a file with a size and a chain longer than it, a
/// run part used). The card as it was before, the commands, what was appended and the
/// clusters taken before.
fn a_stream(fat_type: FatType, card_bytes: usize) -> Stream {
    let bytes = Rc::new(RefCell::new(vec![0u8; card_bytes]));
    let counted = Counted {
        bytes: bytes.clone(),
        counts: Rc::default(),
    };
    {
        let mut disk = BlockDisk::new(counted.clone()).unwrap();
        paperpaw_fat::fatfs::format_volume(
            &mut disk,
            FormatVolumeOptions::new()
                .fat_type(fat_type)
                .bytes_per_cluster(512),
        )
        .unwrap();
        disk.flush().unwrap();
    }
    let before_stream = bytes.borrow().clone();
    let used_before = used_clusters(&before_stream);
    counted.counts.borrow_mut().keep = true;
    let storage = {
        let mut disk = BlockDisk::new(counted.clone()).unwrap();
        disk.seek(SeekFrom::Start(0)).unwrap();
        let shared = SharedDisk::new(disk);
        let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
        FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared)
    };
    storage.create_dir("cache").unwrap();
    let mut whole = Vec::new();
    for (i, size) in [
        700usize, 3000, 4096, 900, 8192, 12_000, 2500, 40_000, 5000, 70_000, 3000, 9000, 20_000,
    ]
    .iter()
    .enumerate()
    {
        let piece: Vec<u8> = (0..*size).map(|k| (k * 7 + i * 13) as u8).collect();
        storage.append("cache/img.gray", &piece).unwrap();
        whole.extend_from_slice(&piece);
        storage.sync().unwrap();
    }
    storage.sync().unwrap();
    let log = std::mem::take(&mut counted.counts.borrow_mut().kept);
    assert!(log.len() > 150, "{} commands", log.len());
    // Clean end: the file is whole and takes the clusters it needs, no more.
    let end_bytes = bytes.borrow().clone();
    check_chains(&end_bytes, "after the stream");
    assert_eq!(
        used_clusters(&end_bytes) - used_before,
        whole.len().div_ceil(512) + 1,
        "clusters taken (and the folder's)"
    );
    (before_stream, log, whole, used_before)
}

/// A few bytes appended and written back to the card as it is (cut off before command
/// `k`), ending inside the last cluster in use: what a cut left taken after it is given
/// back all the same. What removing frees is the file's chain: as long as its size needs
/// (clusters a cut left taken and linked from nowhere are not part of it).
fn a_short_append_after_the_cut(k: usize, image: &[u8], size: usize) {
    let copy = Rc::new(RefCell::new(image.to_vec()));
    let card = common::Card::mount(common::Bytes(copy.clone()));
    card.storage.append("cache/img.gray", &[7u8; 100]).unwrap();
    card.storage.sync().unwrap();
    let held = used_clusters(&copy.borrow());
    card.storage.remove("cache/img.gray").unwrap();
    card.storage.sync().unwrap();
    assert_eq!(
        held - used_clusters(&copy.borrow()),
        (size + 100).div_ceil(512),
        "before command {k}: the chain after a short append and a sync"
    );
}

/// The next append goes on from the card as it is (cut off before command `k`), and
/// removing the file frees every cluster it holds.
fn the_next_append_and_a_remove_after_the_cut(
    k: usize,
    card: &common::Card,
    size: usize,
    whole: &[u8],
    used_before: usize,
    lost_ok: usize,
) {
    let rest = &whole[size..];
    if !rest.is_empty() {
        card.storage
            .append("cache/img.gray", &rest[..rest.len().min(5000)])
            .unwrap();
    }
    card.storage.sync().unwrap();
    let got = common::read(card.storage, "cache/img.gray");
    assert_eq!(
        got,
        whole[..got.len()],
        "before command {k}: after one append more"
    );
    card.storage.remove("cache/img.gray").unwrap();
    card.storage.sync().unwrap();
    let after = card.bytes.0.borrow().clone();
    check_chains(&after, &format!("after removing, cut at {k}"));
    // A file whose entry holds a size has its whole chain freed, the clusters taken ahead
    // of its end too: nothing stays taken. (A run in two FAT blocks left up to 32 clusters
    // taken and linked from nowhere by a cut between their writes; a run is now in one.)
    let leaked = used_clusters(&after) - (used_before + 1);
    if size > 0 {
        assert!(
            leaked <= lost_ok,
            "removing the file after a cut at command {k} leaves {leaked} clusters taken"
        );
    } else {
        assert!(
            leaked <= whole.len().div_ceil(512) + 128,
            "removing the file after a cut at command {k}: {leaked} clusters lost"
        );
    }
}

/// Cut off before any write command of a stream of appends, and after it: the FAT never
/// links into a free cluster or twice to one, the file reads as a prefix of what was
/// appended, a short or longer append goes on from there, and removing the file gives all
/// its clusters back.
fn cut_at_every_command(fat_type: FatType, card_bytes: usize, lost_ok: usize) {
    let (before_stream, log, whole, used_before) = a_stream(fat_type, card_bytes);
    let mut image = before_stream;
    for (k, (index, data)) in log.iter().enumerate() {
        // Before command k.
        let copy = Rc::new(RefCell::new(image.clone()));
        check_chains(&copy.borrow(), &format!("before command {k}"));
        let card = common::Card::mount(common::Bytes(copy));
        let size = card.storage.size("cache/img.gray").unwrap_or(0) as usize;
        assert!(size <= whole.len(), "before command {k}: size {size}");
        if size > 0 {
            assert_eq!(
                common::read(card.storage, "cache/img.gray"),
                whole[..size],
                "before command {k}: what is read is not what was appended"
            );
        }
        if size > 0 && !size.is_multiple_of(512) {
            a_short_append_after_the_cut(k, &image, size);
        }
        the_next_append_and_a_remove_after_the_cut(k, &card, size, &whole, used_before, lost_ok);
        let at = *index as usize * BLOCK;
        image[at..at + data.len()].copy_from_slice(data);
    }
    check_chains(&image, "the end");
}

/// FAT16: a run crosses blocks of the FAT every 256 clusters. One cluster may stay taken
/// after a cut and a remove here too (the old one-at-a-time order loses it between marking
/// the new cluster and linking it: ticket #269), checked at every command.
#[test]
fn a_cut_in_a_stream_of_appends_leaves_a_sound_volume() {
    cut_at_every_command(FatType::Fat16, 24 << 20, 1);
}

/// FAT32 (128 entries to a block of the FAT). One cluster may stay taken after a cut and a
/// remove (ticket #269): the base's one-cluster order (mark the new cluster taken in its FAT
/// block, then link it from the block before) has that window at every cluster; a run takes
/// it where it starts in a new block. (`set_append_reservation(false)` still uses the joined
/// write, so it does not show the base.) At most one:
/// the cut test's other checks (no link into a free cluster, none twice, a prefix is read)
/// stay as strict as on FAT16.
#[test]
fn a_cut_in_a_stream_of_appends_leaves_a_sound_volume_on_fat32() {
    cut_at_every_command(FatType::Fat32, 40 << 20, 1);
}

/// The free space the storage reports (it follows the clusters the file system marks taken
/// and free) is what the FAT says, after a stream of appends written back and after the
/// file is removed: the clusters taken ahead and given back are all counted.
#[test]
fn free_space_follows_the_clusters_taken_ahead_and_given_back() {
    let cluster = 4096u32;
    let (storage, counted) = card(cluster, cluster as usize * 70_000);
    let free = |storage: &Fat| loop {
        if let Some(n) = storage.free_space() {
            return n;
        }
    };
    let from_fat = || {
        let bytes = counted.bytes.borrow();
        let entries = fat_entries(&bytes);
        (entries.iter().filter(|&&v| v == 0).count() as u64) * u64::from(cluster)
    };
    storage.create_dir("cache").unwrap();
    storage.sync().unwrap();
    assert_eq!(free(storage), from_fat());
    let data = vec![0x5Au8; 3000];
    for _ in 0..90 {
        storage.append("cache/img.gray", &data).unwrap();
    }
    storage.sync().unwrap();
    assert_eq!(free(storage), from_fat(), "after the appends");
    storage.append("cache/img.gray", &data).unwrap();
    storage.remove("cache/img.gray").unwrap();
    storage.sync().unwrap();
    assert_eq!(free(storage), from_fat(), "after removing the file");
}

/// Every way an appended file leaves the open files gives back the clusters it took ahead:
/// evicted by the seventeenth open file, renamed, removed. After each, the clusters taken are
/// those the files need (and the folder's), and nothing links into a free cluster.
#[test]
fn every_way_out_of_the_open_files_gives_back_the_clusters_taken_ahead() {
    // The same steps with the clusters taken one at a time are the measure: what the files
    // and the folder need, whatever a rename does to the folder.
    assert_eq!(leaving_the_open_files(false), leaving_the_open_files(true));
}

/// The clusters taken (beyond the first state) after eviction, after the rename, after the
/// removal.
fn leaving_the_open_files(ahead: bool) -> [usize; 3] {
    let cluster = 4096usize;
    let (storage, counted) = card(cluster as u32, cluster * 70_000);
    storage.set_append_reservation(ahead);
    storage.create_dir("cache").unwrap();
    storage.sync().unwrap();
    let base = used_clusters(&counted.bytes.borrow());
    let piece = vec![0x3Cu8; 4096];
    // 20 files (more than `OPEN_FILES`) of 24 pieces each, appended in turn: the first are
    // evicted while the later ones grow.
    for round in 0..24 {
        for f in 0..20 {
            storage
                .append(&format!("cache/f{f}.gray"), &piece)
                .unwrap_or_else(|e| panic!("round {round} file {f}: {e:?}"));
        }
    }
    storage.sync().unwrap();
    let bytes = counted.bytes.borrow().clone();
    check_chains(&bytes, "after eviction");
    let mut taken = [used_clusters(&bytes) - base, 0, 0];
    assert_eq!(taken[0], 20 * 24, "evicted files");
    for f in 0..20 {
        assert_eq!(
            storage.size(&format!("cache/f{f}.gray")).unwrap(),
            24 * 4096
        );
    }
    // Renamed while open and not written back.
    storage.append("cache/f19.gray", &piece).unwrap();
    storage
        .rename("cache/f19.gray", "cache/moved.gray")
        .unwrap();
    storage.sync().unwrap();
    let bytes = counted.bytes.borrow().clone();
    check_chains(&bytes, "after rename");
    taken[1] = used_clusters(&bytes) - base;
    assert!(taken[1] <= 20 * 24 + 2, "renamed file: {}", taken[1]);
    assert_eq!(storage.size("cache/moved.gray").unwrap(), 25 * 4096);
    // Removed while open: every cluster is free again, those taken ahead too.
    storage.append("cache/f3.gray", &piece).unwrap();
    for f in 0..19 {
        storage.remove(&format!("cache/f{f}.gray")).unwrap();
    }
    storage.remove("cache/moved.gray").unwrap();
    storage.sync().unwrap();
    let bytes = counted.bytes.borrow().clone();
    check_chains(&bytes, "after removing");
    taken[2] = used_clusters(&bytes) - base;
    // The folder keeps the cluster its entries grew into, as it did.
    assert!(taken[2] <= 1, "after removing everything: {}", taken[2]);
    taken
}

/// A card nearly full: a file that took clusters ahead on a roomy card must not fail for want
/// of space while a cluster is free. Appends go on until the very last cluster is used, then
/// fail; what was written reads back and the free space is what the FAT says.
#[test]
fn a_nearly_full_card_is_filled_to_the_last_cluster() {
    let cluster = 4096u32;
    // FAT16, as many clusters as the free space we want plus the ones taken before.
    let bytes = Rc::new(RefCell::new(vec![0u8; 24 << 20]));
    let counted = Counted {
        bytes: bytes.clone(),
        counts: Rc::default(),
    };
    {
        let mut disk = BlockDisk::new(counted.clone()).unwrap();
        paperpaw_fat::fatfs::format_volume(
            &mut disk,
            FormatVolumeOptions::new()
                .fat_type(FatType::Fat16)
                .bytes_per_cluster(cluster),
        )
        .unwrap();
        disk.flush().unwrap();
    }
    let storage = {
        let mut disk = BlockDisk::new(counted.clone()).unwrap();
        disk.seek(SeekFrom::Start(0)).unwrap();
        let shared = SharedDisk::new(disk);
        let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
        FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared)
    };
    storage.create_dir("cache").unwrap();
    let piece = vec![0xC3u8; cluster as usize];
    // A file that has grown (takes runs), and a filler that leaves 37 clusters free, in
    // pieces so that the free clusters are not one run but the filler's own runs hold them.
    for _ in 0..8 {
        storage.append("cache/a.gray", &piece).unwrap();
    }
    storage.sync().unwrap();
    let free_now = (fat_entries(&bytes.borrow())
        .iter()
        .filter(|&&v| v == 0)
        .count()) as u64;
    let leave = 37u64;
    let filler = vec![0x11u8; cluster as usize];
    let mut left = free_now;
    while left > leave {
        storage.append("cache/filler.bin", &filler).unwrap();
        left -= 1;
        if left.is_multiple_of(500) {
            storage.sync().unwrap();
        }
    }
    storage.sync().unwrap();
    let free_start = (fat_entries(&bytes.borrow())
        .iter()
        .filter(|&&v| v == 0)
        .count()) as u64;
    assert!(free_start <= leave + 2 * 16, "{free_start} free");
    let mut written = 0u64;
    while storage.append("cache/a.gray", &piece).is_ok() {
        written += 1;
        assert!(written <= free_start, "appends past the free space");
    }
    storage.sync().unwrap();
    let after = bytes.borrow().clone();
    check_chains(&after, "after filling");
    let free_after = fat_entries(&after).iter().filter(|&&v| v == 0).count() as u64;
    assert!(
        free_after <= 1,
        "{free_after} clusters free after the append failed with {written} of {free_start} written"
    );
    assert_eq!(
        storage.size("cache/a.gray").unwrap(),
        (8 + written) * u64::from(cluster)
    );
    // Whatever was left (a cluster the append failed on at most) is what the storage says.
    let free = loop {
        if let Some(n) = storage.free_space() {
            break n;
        }
    };
    assert_eq!(free, free_after * u64::from(cluster));
}
