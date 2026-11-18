//! The card's free space: counted a part of the FAT at each ask, never
//! the count the card keeps of itself, and followed as files change.

use super::*;

/// The card's own word serves at once, the FAT is counted a part at a
/// time, and what is written and removed is followed.
#[test]
fn free_space_is_counted_a_part_at_a_time() {
    let card = fat32();
    let s = card.storage();
    // Not known until counted: the card's own count is not told.
    assert_eq!(s.free_space(), None);
    assert!(count_out(&card, &s) > 1);
    assert_eq!(s.free_space(), Some(counted(&s)));
    // Written, appended, made, replaced, moved and removed: followed
    // cluster by cluster, without counting again.
    s.write("books/a.bin", &vec![7u8; 64 * 1024]).unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.append("books/a.bin", &[1u8; 700]).unwrap();
    s.sync().unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.write("books/a.bin", b"short").unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.write("b.bin", &vec![1u8; 3000]).unwrap();
    s.rename("b.bin", "books/a.bin").unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.create_dir("x/y/z").unwrap();
    s.write("x/y/z/c.bin", &vec![2u8; 5000]).unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.remove_dir_all("x").unwrap();
    s.remove("books/a.bin").unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    // Asked on: nothing more to count.
    let before = card.reads.get();
    s.free_space();
    assert_eq!(card.reads.get(), before);
}

/// Writes `free` as the count of free clusters a FAT32 card's FS
/// information sector keeps, in `image`.
fn set_fsinfo_free(image: &mut [u8], free: u32) {
    let sector = usize::from(u16::from_le_bytes([image[48], image[49]])) * 512;
    image[sector + 488..sector + 492].copy_from_slice(&free.to_le_bytes());
}

/// A card that was not unmounted (the X4 never unmounts), nearly full,
/// whose own count says far more is free than is (as after a power cut,
/// #238): that count is never told. rust-fatfs would read all of the FAT
/// at once; here it is counted a part at a time, and until then the free
/// space is not known, so a reserve is never kept by a count that is not
/// one.
#[test]
fn a_count_the_card_keeps_is_never_told() {
    let card = fat32();
    let first = card.storage();
    count_out(&card, &first);
    let mut n = 0;
    while first.free_space().unwrap() > 2 << 20 {
        first
            .write(&format!("fill/{n}.bin"), &vec![7u8; 1 << 20])
            .unwrap();
        n += 1;
    }
    first.sync().unwrap();
    let mut image = card.image();
    set_fsinfo_free(&mut image, 70_000);
    // Mounted again from what is on the card.
    let card = Cut::new(image);
    let s = card.storage();
    let mut unknown = 0;
    let free = loop {
        match s.free_space() {
            Some(free) => break free,
            None => unknown += 1,
        }
        assert!(unknown < 100, "never counted");
    };
    assert!(unknown >= 1, "a FAT of 320 KB counted at one ask");
    assert_eq!(free, counted(&s));
    assert!(free <= 2 << 20, "{free}");
}

/// What is written and removed while the FAT is counted: the count is
/// done all the same, and says at most what is free (what was taken
/// meanwhile taken off, what was freed not added).
#[test]
fn a_count_with_changes_meanwhile_is_never_more_than_is_free() {
    let card = fat32();
    let s = card.storage();
    s.write("old.bin", &vec![1u8; 100 * 1024]).unwrap();
    s.sync().unwrap();
    let card = Cut::new(card.image());
    let s = card.storage();
    let mut asks = 0_u64;
    let free = loop {
        if let Some(free) = s.free_space() {
            break free;
        }
        // Between every two asks: a file written, one removed.
        s.write(&format!("new{asks}.bin"), &vec![2u8; 30 * 1024])
            .unwrap();
        s.remove("old.bin").unwrap();
        asks += 1;
        assert!(asks < 100, "never counted");
    };
    let exact = counted(&s);
    assert!(free <= exact, "{free} {exact}");
    // At most what was taken meanwhile under.
    assert!(
        free + asks * 30 * 1024 + 100 * 1024 >= exact,
        "{free} {exact}"
    );
}

/// The FAT is counted a sector at a time, into a buffer of that size:
/// nothing large is taken from the heap, which on the X4 is small and in
/// pieces while the radio is up.
#[test]
fn free_space_is_counted_without_a_large_buffer() {
    for card in [
        fat32(),
        Cut::formatted_of(
            8 << 20,
            FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat16),
        ),
        Cut::formatted_of(
            8 << 20,
            FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat12),
        ),
    ] {
        let image = card.image();
        let largest = alloc::rc::Rc::new(core::cell::Cell::new(0));
        let seen = largest.clone();
        let read = move |at: u64, buf: &mut [u8]| {
            seen.set(seen.get().max(buf.len()));
            let at = at as usize;
            buf.copy_from_slice(&image[at..at + buf.len()]);
            true
        };
        let mut free = free::Free::default();
        for _ in 0..100 {
            free.ask(&read);
            if free.exact() {
                break;
            }
        }
        assert!(free.exact());
        assert!(largest.get() <= BLOCK, "{} bytes at once", largest.get());
        let s = card.storage();
        assert_eq!(free.ask(&read), Some(counted(&s)));
    }
}

/// FAT12 and FAT16 keep no count: theirs is counted.
#[test]
fn small_cards_are_counted() {
    for fat in [fatfs::FatType::Fat12, fatfs::FatType::Fat16] {
        let card = Cut::formatted_of(8 << 20, FormatVolumeOptions::new().fat_type(fat));
        let s = card.storage();
        count_out(&card, &s);
        assert_eq!(s.free_space(), Some(counted(&s)));
        s.write("a.bin", &vec![1u8; 9000]).unwrap();
        assert_eq!(s.free_space(), Some(counted(&s)));
    }
}

/// The bytes for files in all, as rust-fatfs counts them, told from the
/// boot sector without reading the FAT; also on a card of its own.
#[test]
fn total_space_is_told_from_the_boot_sector() {
    for fat in [
        fatfs::FatType::Fat12,
        fatfs::FatType::Fat16,
        fatfs::FatType::Fat32,
    ] {
        let card = match fat {
            fatfs::FatType::Fat32 => fat32(),
            _ => Cut::formatted_of(8 << 20, FormatVolumeOptions::new().fat_type(fat)),
        };
        let s = card.storage();
        let before = card.reads.get();
        let total = s.total_space().unwrap();
        assert!(card.reads.get() - before <= 2, "{fat:?}");
        let stats = s.fs.stats().unwrap();
        assert_eq!(
            total,
            u64::from(stats.total_clusters()) * u64::from(stats.cluster_size())
        );
        count_out(&card, &s);
        assert!(s.free_space().unwrap() <= total);
        assert_eq!(mounted(card.image()).total_space(), Some(total));
    }
}
