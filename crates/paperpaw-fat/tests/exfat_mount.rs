//! Mounting exFAT cards whose volume does not fit them (#5, the owner's
//! card on the X4): a volume a little longer than the card, as a
//! partition said to go past the card's end leaves it, mounts as on a
//! computer; one much longer is refused, and the failure says why.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_fat::{BlockDisk, ExDisk, ExFatStorage, Kind, SharedDisk};
use paperpaw_platform::Storage;

/// The empty exFAT card of the tests, its last `blocks` blocks cut off:
/// its volume is that much longer than the card.
fn cut(blocks: usize) -> Bytes {
    let card = exfat_card();
    let len = card.0.borrow().len();
    card.0.borrow_mut().truncate(len - blocks * 512);
    card
}

#[test]
fn a_volume_a_little_longer_than_the_card_mounts_without_its_missing_end() {
    let whole = exfat(&exfat_card()).free_space().unwrap();
    let card = cut(128);
    let storage = exfat(&card);
    assert_eq!(storage.missing_blocks(), 128);
    assert!(storage.free_space().unwrap() <= whole - 128 * 512);
    storage.create_dir("Books").unwrap();
    storage.write("Books/a.txt", b"a book").unwrap();
    storage.sync().unwrap();
    // As the next start finds it.
    let again = exfat(&card);
    assert_eq!(read(&again, "Books/a.txt"), b"a book");
    assert_eq!(names(&again, "Books"), ["a.txt"]);
}

/// What hadris-fat's checker finds wrong on `card` (nothing), and
/// `fsck.exfat -n` where installed on the volume as a computer sees it
/// (its missing end there, as zeros).
fn assert_clean(card: &Bytes) {
    let (disk, _) = BlockDisk::find(card.clone()).unwrap();
    let mut dev = ExDisk::new(SharedDisk::new(disk));
    let mut scratch = vec![0u8; 8192];
    let mut findings = Vec::new();
    hadris_fat::exfat::sync::check(&mut dev, &mut scratch, |f| {
        findings.push(format!("{f:?}"));
    })
    .unwrap();
    assert_eq!(findings, Vec::<String>::new());
    let Ok(path) = std::process::Command::new("sh")
        .args(["-c", "command -v fsck.exfat"])
        .output()
    else {
        return;
    };
    if !path.status.success() {
        return;
    }
    let mut whole = card.0.borrow().clone();
    whole.resize(exfat_card().0.borrow().len(), 0);
    // One file per call: the tests run side by side.
    static IMAGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = IMAGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file =
        std::env::temp_dir().join(format!("pp-exfat-past-end-{}-{n}.img", std::process::id()));
    std::fs::write(&file, &whole).unwrap();
    let out = std::process::Command::new("fsck.exfat")
        .arg("-n")
        .arg(&file)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&file);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn saves_and_appends_across_the_cards_end_never_fail() {
    let card = cut(128);
    let storage = exfat(&card);
    let past_end = storage.clusters_past_end();
    assert!(past_end > 0);
    // Each save goes through a new file, moving where the next cluster
    // is looked for past every one: round the card, many times.
    for n in 0..6000 {
        let text = format!("save {n}");
        storage
            .write("notes.txt", text.as_bytes())
            .unwrap_or_else(|e| panic!("save {n}: {e:?}"));
    }
    for n in 0..6000u32 {
        let data = [n as u8; 700];
        storage
            .append("log.txt", &data)
            .unwrap_or_else(|e| panic!("append {n}: {e:?}"));
    }
    storage.sync().unwrap();
    let again = exfat(&card);
    assert_eq!(again.clusters_past_end(), past_end);
    assert_eq!(read(&again, "notes.txt"), b"save 5999");
    let log = read(&again, "log.txt");
    assert_eq!(log.len(), 6000 * 700);
    assert!(log[5999 * 700..].iter().all(|&b| b == (5999u32 as u8)));
    drop(again);
    assert_clean(&card);
    assert_tail_untouched(&card, past_end);
}

/// The card's own FAT entries and bitmap bits of the `past_end` clusters
/// past its end are as they were (free): nothing was written for them.
fn assert_tail_untouched(card: &Bytes, past_end: u32) {
    let bytes = card.0.borrow().clone();
    let boot = paperpaw_fat::BootSizes::read(bytes[..512].try_into().unwrap()).unwrap();
    let fat = (boot.fat.0 as usize) << boot.sector_shift;
    let end = boot.clusters as usize + 2;
    let first = end - past_end as usize;
    assert!(
        bytes[fat + 4 * first..fat + 4 * end]
            .iter()
            .all(|&b| b == 0)
    );
    // As a computer sees the card (its missing end there, as zeros): the
    // clusters past the end are free there, nothing else.
    let mut whole = bytes;
    whole.resize(exfat_card().0.borrow().len(), 0);
    let computer = exfat(&Bytes(std::rc::Rc::new(std::cell::RefCell::new(whole))));
    let storage = exfat(card);
    let tail = computer.total_space().unwrap() - storage.total_space().unwrap();
    assert!(tail > 0);
    assert_eq!(
        computer.free_space().unwrap() - storage.free_space().unwrap(),
        tail
    );
}

#[test]
fn what_does_not_fit_on_the_card_fails_and_leaves_the_rest() {
    let card = cut(128);
    let storage = exfat(&card);
    storage.write("kept.txt", b"kept").unwrap();
    // More than the card holds, though the volume would hold it.
    let whole = exfat(&exfat_card()).free_space().unwrap();
    let big = vec![7u8; whole as usize - 64 * 1024];
    assert!(storage.write("big.bin", &big).is_err());
    storage.sync().unwrap();
    let again = exfat(&card);
    assert_eq!(read(&again, "kept.txt"), b"kept");
    assert_eq!(names(&again, ""), ["kept.txt"]);
}

#[test]
fn a_volume_much_longer_than_the_card_is_refused_saying_why() {
    // 1 MB of 8: more than a 32nd.
    let card = cut(2048);
    let (disk, kind) = BlockDisk::find(card).unwrap();
    assert_eq!(kind, Kind::ExFat);
    let failure = ExFatStorage::mount(SharedDisk::new(disk)).unwrap_err();
    assert!(failure.longer_than_device(), "{failure}");
    let said = failure.to_string();
    assert!(said.contains("longer than the card"), "{said}");
    assert!(
        said.contains(&format!("device {} blocks", failure.device_blocks)),
        "{said}"
    );
    let boot = failure.boot.unwrap();
    assert_eq!(boot.volume_sectors, failure.device_blocks + 2048);
}

#[test]
fn a_card_that_fits_misses_nothing() {
    assert_eq!(exfat(&exfat_card()).missing_blocks(), 0);
}

/// The cluster size of the volume on `card`.
fn cluster_size(card: &Bytes) -> u64 {
    let bytes = card.0.borrow();
    let boot = paperpaw_fat::BootSizes::read(bytes[..512].try_into().unwrap()).unwrap();
    1 << (boot.sector_shift + boot.cluster_shift)
}

/// A card cut so that its end falls inside a byte of the bitmap (the
/// first cluster past it is not the first of a byte), and how many
/// clusters lie past the end.
fn cut_mid_byte() -> (Bytes, u32) {
    for blocks in (8..=400).step_by(4) {
        let card = cut(blocks);
        let clusters = {
            let bytes = card.0.borrow();
            paperpaw_fat::BootSizes::read(bytes[..512].try_into().unwrap())
                .unwrap()
                .clusters
        };
        let past_end = exfat(&card).clusters_past_end();
        if past_end > 0 && !(clusters - past_end).is_multiple_of(8) {
            return (card, past_end);
        }
    }
    panic!("no cut ends inside a bitmap byte");
}

/// Fills `storage` to its last cluster: a large file, then files of a
/// cluster each (named `f0`, `f1`, …) until none fits; how many.
fn fill(storage: &ExFatStorage<impl paperpaw_fat::Blocks + 'static>, cluster: u64) -> usize {
    let free = storage.free_space().unwrap();
    storage
        .write("big.bin", &vec![1u8; (free - 24 * cluster) as usize])
        .unwrap();
    let mut n = 0;
    while storage
        .write(&format!("f{n}"), &vec![n as u8; cluster as usize])
        .is_ok()
    {
        n += 1;
    }
    assert!(n > 8, "{n}");
    assert!(storage.free_space().unwrap() < 2 * cluster);
    n
}

#[test]
fn the_clusters_beside_the_end_are_taken_and_freed_as_the_card_says() {
    // The clusters just before the end share a bitmap byte, and a block
    // of FAT entries, with those past it: the driver's writes of them
    // must reach the card, those of the tail's must not.
    let (card, past_end) = cut_mid_byte();
    let cluster = cluster_size(&card);
    let storage = exfat(&card);
    let n = fill(&storage, cluster);
    storage.sync().unwrap();
    assert_clean(&card);
    assert_tail_untouched(&card, past_end);
    // Every other one freed, then the card filled again.
    for i in (0..n).step_by(2) {
        storage.remove(&format!("f{i}")).unwrap();
    }
    storage.sync().unwrap();
    assert_clean(&card);
    assert_tail_untouched(&card, past_end);
    let mut again = 0;
    while storage
        .write(&format!("g{again}"), &vec![0xA5; cluster as usize])
        .is_ok()
    {
        again += 1;
    }
    assert!(again >= n / 2, "{again} of {n}");
    storage.sync().unwrap();
    drop(storage);
    assert_clean(&card);
    assert_tail_untouched(&card, past_end);
    let remounted = exfat(&card);
    for i in (1..n).step_by(2) {
        assert_eq!(
            read(&remounted, &format!("f{i}")),
            vec![i as u8; cluster as usize]
        );
    }
    for i in 0..again {
        assert_eq!(
            read(&remounted, &format!("g{i}")),
            vec![0xA5; cluster as usize]
        );
    }
}

/// What hadris-fat's checker finds on the card `image` (cut short), but
/// clusters a power cut left taken by no file and the dirty flag (as in
/// tests/exfat.rs).
fn findings(image: &[u8]) -> Vec<String> {
    let card = Bytes(std::rc::Rc::new(std::cell::RefCell::new(image.to_vec())));
    let (disk, _) = BlockDisk::find(card).unwrap();
    let mut dev = ExDisk::new(SharedDisk::new(disk));
    let mut scratch = vec![0u8; 8192];
    let mut found = Vec::new();
    hadris_fat::exfat::sync::check(&mut dev, &mut scratch, |f| found.push(format!("{f:?}")))
        .unwrap();
    found.retain(|f| {
        let lost = f.contains("no allocation reaches") || f.contains("longer than its data length");
        !lost && !f.contains("marked dirty")
    });
    found
}

#[test]
fn a_save_beside_the_end_is_whole_or_not_there_after_any_cut() {
    let (card, past_end) = cut_mid_byte();
    let cluster = cluster_size(&card);
    let storage = exfat(&card);
    let n = fill(&storage, cluster);
    // The last ones written lie just before the end.
    for i in n - 3..n {
        storage.remove(&format!("f{i}")).unwrap();
    }
    storage.sync().unwrap();
    drop(storage);
    let image = card.0.borrow().clone();
    let new = vec![0x5Au8; 2 * cluster as usize];
    // The save, every block it writes recorded.
    let recorded = Recorded {
        data: std::rc::Rc::new(std::cell::RefCell::new(image.clone())),
        writes: std::rc::Rc::default(),
    };
    let (disk, _) = BlockDisk::find(recorded.clone()).unwrap();
    let saving = ExFatStorage::mount(SharedDisk::new(disk)).unwrap();
    saving.write("near.bin", &new).unwrap();
    saving.sync().unwrap();
    drop(saving);
    let writes = recorded.writes.borrow().clone();
    assert!(writes.len() > 2, "{} blocks written", writes.len());
    let mut now = image;
    for cut in 0..=writes.len() {
        if cut > 0 {
            let (index, block) = &writes[cut - 1];
            let at = *index as usize * 512;
            now[at..at + 512].copy_from_slice(block);
        }
        let card = Bytes(std::rc::Rc::new(std::cell::RefCell::new(now.clone())));
        let storage = exfat(&card);
        let size = storage.size("near.bin");
        if size.is_ok() {
            assert_eq!(read(&storage, "near.bin"), new, "cut {cut}");
        } else {
            assert_eq!(
                size,
                Err(paperpaw_platform::StorageError::NotFound),
                "cut {cut}"
            );
        }
        assert_eq!(
            read(&storage, "f0"),
            vec![0u8; cluster as usize],
            "cut {cut}"
        );
        drop(storage);
        assert_tail_untouched(&card, past_end);
        // The next save heals what the cut left (a save's file and its
        // temporary one may share their clusters until then, as on any
        // card: tests/exfat.rs).
        let storage = exfat(&card);
        storage.write("near.bin", b"again").unwrap();
        storage.sync().unwrap();
        drop(storage);
        assert_eq!(
            findings(&card.0.borrow()),
            Vec::<String>::new(),
            "cut {cut}"
        );
    }
}
