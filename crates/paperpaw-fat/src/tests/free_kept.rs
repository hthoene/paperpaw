//! The free space kept on the card once it is counted (#305): the next
//! mount takes it without reading the FAT, and never where the FAT may
//! have changed since: after a cut at any block written, after a computer
//! changed the card, on a card marked dirty.

use super::*;

/// The free bytes the FAT in `image` says, counted from its blocks (no
/// count kept on the card is looked at).
fn in_fat(image: &[u8]) -> u64 {
    let image = image.to_vec();
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

/// The card of `fat32()` with some files, counted and synced: its free
/// space is kept on it.
fn kept() -> Vec<u8> {
    let card = fat32();
    let s = card.storage();
    count_out(&card, &s);
    s.write("books/a.bin", &vec![7u8; 40 * 1024]).unwrap();
    s.write("books/b.bin", &vec![8u8; 9000]).unwrap();
    s.sync().unwrap();
    card.image()
}

/// Where the FS information sector of `image` starts.
fn fsinfo(image: &[u8]) -> usize {
    usize::from(u16::from_le_bytes([image[48], image[49]])) * 512
}

fn word(image: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([image[at], image[at + 1], image[at + 2], image[at + 3]])
}

/// What the first ask of a new mount of `image` says, and the blocks it
/// read for it.
fn first_ask(image: &[u8]) -> (Option<u64>, usize) {
    let card = Cut::new(image.to_vec());
    let s = card.storage();
    let before = card.reads.get();
    (s.free_space(), card.reads.get() - before)
}

#[test]
fn a_count_kept_is_taken_by_the_next_mount_without_reading_the_fat() {
    let image = kept();
    let (free, read) = first_ask(&image);
    assert_eq!(free, Some(in_fat(&image)));
    // The FAT of this card is 640 blocks.
    assert!(read <= 2, "{read} blocks read");
    // Followed from there, and kept again once written back.
    let card = Cut::new(image);
    let s = card.storage();
    s.write("books/c.bin", &vec![9u8; 30 * 1024]).unwrap();
    s.remove("books/a.bin").unwrap();
    s.sync().unwrap();
    let image = card.image();
    assert_eq!(s.free_space(), Some(in_fat(&image)));
    assert_eq!(first_ask(&image).0, Some(in_fat(&image)));
}

/// The count is taken back on the card before the FAT is written: the
/// first block a change of the FAT writes is the FS information sector,
/// its count unknown and the storage's record of it gone (what stands
/// there instead says which cluster is about to be taken, #238).
#[test]
fn the_count_is_taken_back_before_the_fat_is_written() {
    let image = kept();
    let sector = fsinfo(&image);
    assert_ne!(word(&image, sector + 488), u32::MAX);
    let card = Cut::new(image);
    let s = card.storage();
    card.record_writes();
    s.write("books/c.bin", &vec![9u8; 3000]).unwrap();
    let writes = core::mem::take(&mut *card.writes.borrow_mut());
    // Where the FATs are.
    let reserved = usize::from(u16::from_le_bytes([card.image()[14], card.image()[15]])) * 512;
    let end = card.image()[16] as usize * word(&card.image(), 36) as usize * 512 + reserved;
    let first_of_fat = writes
        .iter()
        .position(|(i, _)| (reserved..end).contains(&(*i as usize * BLOCK)))
        .unwrap();
    let taken_back = writes
        .iter()
        .position(|(i, _)| *i as usize * BLOCK == sector)
        .unwrap();
    assert!(taken_back < first_of_fat, "{taken_back} {first_of_fat}");
    let block = writes[taken_back].1;
    assert_eq!(word(&block, 488), u32::MAX);
    assert_eq!(&block[4..8], b"PPCL");
    let mut record = [0u8; free::NOTE];
    record.copy_from_slice(&block[4..4 + free::NOTE]);
    assert!(free::noted(&record, word(&block, 488), word(&block, 492)).is_none());
    // Once for all the changes until it is kept again.
    let again = writes
        .iter()
        .filter(|(i, _)| *i as usize * BLOCK == sector)
        .count();
    assert_eq!(again, 1);
    // What does not change the FAT leaves the count kept: a file made
    // empty, and removed.
    s.sync().unwrap();
    card.record_writes();
    s.write("mark", b"").unwrap();
    s.remove("mark").unwrap();
    s.sync().unwrap();
    let writes = core::mem::take(&mut *card.writes.borrow_mut());
    assert!(writes.iter().all(|(i, _)| *i as usize * BLOCK != sector));
    let image = card.image();
    assert_eq!(first_ask(&image).0, Some(in_fat(&image)));
}

/// Cut off after any block written (#238): a mount of what is on the card
/// then says the free space the FAT says (as the mount leaves it: what
/// the cut left taken by no file is freed), at once where the count kept
/// still holds, else after counting; never a count from before the cut.
#[test]
fn after_a_cut_at_any_block_the_free_space_is_the_fats() {
    let mut image = kept();
    let card = Cut::new(image.clone());
    let s = card.storage();
    card.record_writes();
    s.write("books/c.bin", &vec![1u8; 20 * 1024]).unwrap();
    s.sync().unwrap();
    for part in 0..6 {
        s.append("books/grown.bin", &vec![part; 5000]).unwrap();
    }
    s.sync().unwrap();
    s.remove("books/a.bin").unwrap();
    s.write("books/b.bin", &vec![2u8; 700]).unwrap();
    s.create_dir("new/folder").unwrap();
    s.rename("books/c.bin", "new/folder/c.bin").unwrap();
    s.sync().unwrap();
    s.remove_dir_all("new").unwrap();
    s.sync().unwrap();
    let writes = core::mem::take(&mut *card.writes.borrow_mut());
    let (mut at_once, mut counted) = (0, 0);
    for cut in 0..=writes.len() {
        if cut > 0 {
            let (index, block) = writes[cut - 1];
            let at = index as usize * BLOCK;
            image[at..at + BLOCK].copy_from_slice(&block);
        }
        let card = Cut::new(image.clone());
        let s = card.storage();
        let exact = in_fat(&card.image());
        match s.free_space() {
            Some(free) => {
                assert_eq!(free, exact, "cut {cut} of {}", writes.len());
                at_once += 1;
            }
            None => {
                count_out(&card, &s);
                assert_eq!(s.free_space(), Some(exact), "cut {cut}");
                counted += 1;
            }
        }
    }
    // Both happen: kept before the first write and after each sync,
    // counted between.
    assert!(at_once >= 4, "{at_once} of {}", writes.len() + 1);
    assert!(counted >= 4, "{counted} of {}", writes.len() + 1);
}

/// A card marked dirty (taken out of a computer that was writing it) is
/// counted, whatever its sector says; once counted and synced it is
/// marked clean, its count kept.
#[test]
fn a_card_marked_dirty_is_counted_and_then_kept() {
    let mut image = kept();
    image[0x41] |= 1;
    assert_eq!(first_ask(&image).0, None);
    let card = Cut::new(image);
    let s = card.storage();
    count_out(&card, &s);
    s.sync().unwrap();
    let image = card.image();
    assert_eq!(image[0x41] & 1, 0);
    assert_eq!(first_ask(&image).0, Some(in_fat(&image)));
}

/// A computer that changed the card wrote its own count or hint into the
/// sector: the FAT is counted. One that took clusters and moved only the
/// hint (Linux, which has not counted) too.
#[test]
fn a_count_or_hint_another_wrote_is_not_taken() {
    let image = kept();
    let sector = fsinfo(&image);
    let (free, hint) = (word(&image, sector + 488), word(&image, sector + 492));
    // Another count.
    let mut other = image.clone();
    other[sector + 488..sector + 492].copy_from_slice(&(free - 3).to_le_bytes());
    assert_eq!(first_ask(&other).0, None);
    // Clusters taken in both FATs, the hint moved, the count left.
    let mut linux = image.clone();
    let reserved = usize::from(u16::from_le_bytes([image[14], image[15]])) * 512;
    let fat_bytes = word(&image, 36) as usize * 512;
    let taken = (hint..hint + 5).collect::<Vec<_>>();
    for cluster in &taken {
        for fat in 0..usize::from(image[16]) {
            let at = reserved + fat * fat_bytes + *cluster as usize * 4;
            assert_eq!(word(&linux, at), 0, "cluster {cluster} was free");
            linux[at..at + 4].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        }
    }
    linux[sector + 492..sector + 496].copy_from_slice(&(hint + 4).to_le_bytes());
    let (said, _) = first_ask(&linux);
    assert_eq!(said, None);
    let card = Cut::new(linux.clone());
    let s = card.storage();
    count_out(&card, &s);
    assert_eq!(s.free_space(), Some(in_fat(&linux)));
    assert_eq!(in_fat(&linux), in_fat(&image) - 5 * 512);
    // The record of another card (its clusters) is not this card's.
    let mut record = [0u8; free::NOTE];
    record.copy_from_slice(&image[sector + 4..sector + 4 + free::NOTE]);
    assert!(free::noted(&record, free, hint).is_some());
    assert!(free::noted(&record, free, hint + 1).is_none());
    assert!(free::noted(&record, free + 1, hint).is_none());
    // One free cluster is never taken from the card (see `free::noted`).
    assert!(free::noted(&free::note(1, hint, 70_000), 1, hint).is_none());
    assert!(free::noted(&free::note(0, hint, 70_000), 0, hint).is_some());
}

/// A count that changes of the FAT ran into may be low: it is told, but
/// not kept on the card, so the next mount counts again.
#[test]
fn a_count_that_changes_ran_into_is_not_kept() {
    let mut image = kept();
    image[0x41] |= 1;
    let card = Cut::new(image);
    let s = card.storage();
    let mut asks = 0;
    while s.free_space().is_none() {
        s.write(&format!("new{asks}.bin"), &vec![2u8; 30 * 1024])
            .unwrap();
        asks += 1;
        assert!(asks < 100, "never counted");
    }
    s.sync().unwrap();
    let image = card.image();
    assert_eq!(first_ask(&image).0, None);
    // Counted in peace, it is.
    let card = Cut::new(image);
    let s = card.storage();
    count_out(&card, &s);
    s.sync().unwrap();
    let image = card.image();
    assert_eq!(first_ask(&image).0, Some(in_fat(&image)));
}

/// FAT12 and FAT16 have no such sector: nothing is kept, they are counted
/// (their FAT is small), and a sync writes nothing for it.
#[test]
fn small_cards_keep_no_count() {
    for fat in [fatfs::FatType::Fat12, fatfs::FatType::Fat16] {
        let card = Cut::formatted_of(8 << 20, FormatVolumeOptions::new().fat_type(fat));
        let s = card.storage();
        count_out(&card, &s);
        card.record_writes();
        s.sync().unwrap();
        assert!(card.writes.borrow().is_empty());
        let fresh = Cut::new(card.image());
        let s = fresh.storage();
        let before = fresh.reads.get();
        assert!(s.free_space().is_some());
        assert!(fresh.reads.get() > before);
    }
}
