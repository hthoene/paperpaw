//! The free space counted once and then followed by the clusters the file
//! system marks taken and free: exact also after a write that failed part
//! of the way and when a folder grows (Senior's remark 2 on PR #188),
//! never more than is free.

use super::*;

/// The free bytes the FAT on `card` says, counted from its blocks with a
/// count of its own (what the storage wrote back first).
fn on_card(s: &FatStorage<SharedDisk<Cut>>, card: &Cut) -> u64 {
    s.sync().unwrap();
    let image = card.image();
    let read = move |at: u64, buf: &mut [u8]| {
        let at = at as usize;
        buf.copy_from_slice(&image[at..at + buf.len()]);
        true
    };
    let mut free = free::Free::default();
    for _ in 0..100 {
        if let Some(bytes) = free.ask(&read) {
            return bytes;
        }
    }
    panic!("never counted");
}

/// The card of `fat32()` with less than `left` bytes free, counted.
fn nearly_full(left: u64) -> (Cut, FatStorage<SharedDisk<Cut>>) {
    let card = fat32();
    let s = card.storage();
    count_out(&card, &s);
    let mut n = 0;
    while s.free_space().unwrap() > left {
        s.write(&format!("fill/{n}.bin"), &vec![7u8; 1 << 20])
            .unwrap();
        n += 1;
    }
    (card, s)
}

/// A write too large for what is left fails part of the way: what it took
/// and gave back, and the folders it made, are followed as they were.
#[test]
fn a_write_that_fails_part_of_the_way_is_followed() {
    let (card, s) = nearly_full(3 << 20);
    assert!(s.write("new/big.bin", &vec![1u8; 4 << 20]).is_err());
    assert_eq!(s.free_space(), Some(on_card(&s, &card)));
    // Over a file that is there: it stays, the new content goes.
    s.write("kept.bin", &vec![2u8; 100 * 1024]).unwrap();
    assert!(s.write("kept.bin", &vec![3u8; 4 << 20]).is_err());
    assert_eq!(s.free_space(), Some(on_card(&s, &card)));
}

/// An append the card has no room for keeps what it wrote: the clusters
/// it took are followed, not the ones it asked for.
#[test]
fn an_append_that_fails_part_of_the_way_is_followed() {
    let (card, s) = nearly_full(2 << 20);
    let chunk = vec![4u8; 300 * 1024];
    let mut appended = 0;
    while s.append("log.bin", &chunk).is_ok() {
        appended += 1;
        assert!(appended < 100, "never full");
    }
    let free = s.free_space();
    assert_eq!(free, Some(on_card(&s, &card)));
    assert!(free.unwrap() < 300 * 1024);
}

/// A folder takes a cluster more where its names fill the ones it has:
/// followed, also for names that take no cluster of their own (empty
/// files, long names in several entries each).
#[test]
fn a_folder_that_grows_is_followed() {
    let card = fat32();
    let s = card.storage();
    count_out(&card, &s);
    s.create_dir("many").unwrap();
    let before = s.free_space().unwrap();
    for n in 0..120 {
        s.write(&format!("many/a long name for an empty file {n}.txt"), b"")
            .unwrap();
    }
    let after = s.free_space().unwrap();
    assert_eq!(after, on_card(&s, &card));
    // 120 names of 3 entries and more, in clusters of 512 bytes (16
    // entries): the folder grew by more than 20 clusters.
    assert!(before - after > 20 * 512, "{before} {after}");
    s.remove_dir_all("many").unwrap();
    assert_eq!(s.free_space(), Some(on_card(&s, &card)));
}
