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
use paperpaw_fat::{BlockDisk, ExFatStorage, Kind, SharedDisk};
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
