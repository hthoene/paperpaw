//! A damaged folder (#388): entries of names no file system gives and
//! sizes past the card, as an X4 card showed after resets. Cut off where
//! its parent names it; nothing it points at is freed or written.

use super::*;

/// Where the short entry of name `short` (as FAT keeps it) is on `card`:
/// the only one.
fn short_entry(card: &Cut, short: &[u8; 11]) -> usize {
    let image = card.image();
    let mut found = image
        .chunks(32)
        .enumerate()
        .filter(|(_, entry)| &entry[..11] == short)
        .map(|(i, _)| i * 32);
    let at = found.next().expect("no such entry");
    assert_eq!(found.next(), None, "two such entries");
    at
}

/// The first cluster of the entry at `at` (its high half 0 on FAT12
/// and FAT16).
fn first_cluster(card: &Cut, at: usize) -> u32 {
    let image = card.image();
    u32::from(u16::from_le_bytes([image[at + 26], image[at + 27]]))
        | u32::from(u16::from_le_bytes([image[at + 20], image[at + 21]])) << 16
}

/// Overwrites the entry at `at`: name, attributes, first cluster, size.
fn garble(card: &Cut, at: usize, name: &[u8; 11], attributes: u8, first: u32, size: u32) {
    let mut data = card.data.borrow_mut();
    data[at..at + 11].copy_from_slice(name);
    data[at + 11] = attributes;
    data[at + 20..at + 22].copy_from_slice(&((first >> 16) as u16).to_le_bytes());
    data[at + 26..at + 28].copy_from_slice(&(first as u16).to_le_bytes());
    data[at + 28..at + 32].copy_from_slice(&size.to_le_bytes());
}

/// Fills `card` with files until it is full: none may be given clusters
/// still taken.
fn fill(s: &FatStorage<SharedDisk<Cut>>) {
    let fill = vec![0x55; 64 << 10];
    let mut n = 0;
    while s.write(&format!("fill/{n}.bin"), &fill).is_ok() {
        n += 1;
    }
    assert!(n > 10, "{n}");
}

/// The free space followed: as counted on the card afresh.
fn followed_as_counted(card: &Cut, s: &FatStorage<SharedDisk<Cut>>) {
    count_out(card, s);
    let fresh = card.storage();
    count_out(card, &fresh);
    assert_eq!(s.free_space(), fresh.free_space());
}

fn kept() -> Vec<u8> {
    (0..20_000u32).map(|i| (i * 7 % 251) as u8).collect()
}

/// A folder of a book's cache whose entries were overwritten: two point
/// at the clusters of a file kept elsewhere (one taken for a folder),
/// one past the card. The folder and the one above it are removed, the
/// file kept elsewhere stays whole, also once the card is filled.
#[test]
fn a_damaged_folder_is_removed_and_what_it_points_at_stays() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    s.write("keep/other.bin", &kept()).unwrap();
    for name in ["aaaa.bin", "bbbb.bin", "cccc.bin", "dddd.bin"] {
        s.write(&format!("cache/1e/book/{name}"), b"cached")
            .unwrap();
    }
    s.write("cache/1e/next/toc.dat", b"toc").unwrap();
    s.sync().unwrap();
    let other = first_cluster(&card, short_entry(&card, b"OTHER   BIN"));
    // As the X4 card showed them: bytes FAT does not read, and a size
    // of 2,499,805,184.
    let mut name = [0xbd; 11];
    name[9] = b'@';
    name[10] = 0;
    garble(
        &card,
        short_entry(&card, b"AAAA    BIN"),
        &name,
        0,
        other,
        2_499_805_184,
    );
    garble(
        &card,
        short_entry(&card, b"BBBB    BIN"),
        &[0xbf; 11],
        0x10,
        other,
        64,
    );
    garble(
        &card,
        short_entry(&card, b"CCCC    BIN"),
        b"PAST    BIN",
        0,
        0xfff0,
        10,
    );

    let s = card.storage();
    let mut listed = Vec::new();
    s.list("cache/1e/book", &mut |e| listed.push(e.name.to_string()))
        .unwrap();
    assert_eq!(listed.len(), 4, "{listed:?}");
    assert_eq!(s.remove_dir_all("cache/1e"), Ok(()));
    assert_eq!(s.list("cache/1e", &mut |_| {}), Err(StorageError::NotFound));
    assert_eq!(whole(&s, "keep/other.bin").unwrap(), kept());
    // The card filled: no file is given the clusters kept.
    fill(&s);
    let s = card.storage();
    assert_eq!(whole(&s, "keep/other.bin").unwrap(), kept());
}

/// Names PaperPaw does not take, made elsewhere (`nul.txt`): removed with
/// their folder, their clusters freed.
#[test]
fn names_not_taken_go_with_their_folder() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    s.write("odd/good.txt", b"good").unwrap();
    s.sync().unwrap();
    let free = counted(&s);
    let dir = s.fs.root_dir().open_dir("odd").unwrap();
    let mut file = dir.create_file("nul.txt").unwrap();
    file.write_all(&kept()).unwrap();
    drop(file);
    s.fs.flush().unwrap();
    assert!(counted(&s) < free);
    assert_eq!(s.remove_dir_all("odd"), Ok(()));
    assert_eq!(s.list("odd", &mut |_| {}), Err(StorageError::NotFound));
    let s = card.storage();
    assert!(counted(&s) > free, "{} {free}", counted(&s));
}

/// A damaged folder whose other entries look sane: a file and a folder
/// of names PaperPaw takes, pointing at the clusters of files kept
/// elsewhere, and a size past the card. None is followed: the kept files
/// stay whole, also once the card is filled, and the free space followed
/// is the card's. On FAT12/16 and FAT32 (where the first cluster has a
/// high half).
#[test]
fn a_damaged_folder_frees_nothing_its_sane_looking_entries_point_at() {
    for card in [Cut::formatted(FormatVolumeOptions::new()), fat32()] {
        let s = card.storage();
        s.write("keep/other.bin", &kept()).unwrap();
        s.write("keep/folder.bin", &kept()[..5_000]).unwrap();
        for name in ["aaaa.bin", "bbbb.bin", "cccc.bin", "dddd.bin"] {
            s.write(&format!("cache/1e/book/{name}"), b"cached")
                .unwrap();
        }
        s.write("cache/1e/book/sub/eeee.bin", b"cached").unwrap();
        s.sync().unwrap();
        let other = first_cluster(&card, short_entry(&card, b"OTHER   BIN"));
        let folder = first_cluster(&card, short_entry(&card, b"FOLDER  BIN"));
        // One entry damaged: a NUL in its name.
        garble(
            &card,
            short_entry(&card, b"AAAA    BIN"),
            b"A\0AA    BIN",
            0,
            other,
            10,
        );
        garble(
            &card,
            short_entry(&card, b"BBBB    BIN"),
            b"JUNK    BIN",
            0,
            other,
            20_000,
        );
        garble(
            &card,
            short_entry(&card, b"CCCC    BIN"),
            b"JUNKDIR    ",
            0x10,
            folder,
            0,
        );
        garble(
            &card,
            short_entry(&card, b"DDDD    BIN"),
            b"HUGE    BIN",
            0,
            first_cluster(&card, short_entry(&card, b"EEEE    BIN")),
            u32::MAX,
        );

        let s = card.storage();
        let total = s.total_space().unwrap();
        assert_eq!(s.remove_dir_all("cache/1e"), Ok(()));
        assert_eq!(s.list("cache/1e", &mut |_| {}), Err(StorageError::NotFound));
        assert_eq!(whole(&s, "keep/other.bin").unwrap(), kept());
        assert_eq!(whole(&s, "keep/folder.bin").unwrap(), kept()[..5_000]);
        assert!(s.free_space().unwrap() <= total);
        followed_as_counted(&card, &s);
        fill(&s);
        let s = card.storage();
        assert_eq!(whole(&s, "keep/other.bin").unwrap(), kept());
        assert_eq!(whole(&s, "keep/folder.bin").unwrap(), kept()[..5_000]);
    }
}

/// A folder whose entry points at a kept file's data (text), read as
/// entries: cut off, the data neither freed nor written (no entry in it
/// marked removed).
#[test]
fn a_folder_in_another_files_data_is_not_written() {
    for card in [Cut::formatted(FormatVolumeOptions::new()), fat32()] {
        let text = b"Chapter one. It was a dark night, and the road was long. ".repeat(300);
        let s = card.storage();
        s.write("keep/other.bin", &text).unwrap();
        s.write("cache/1e/book/aaaa.bin", b"cached").unwrap();
        s.sync().unwrap();
        let other = first_cluster(&card, short_entry(&card, b"OTHER   BIN"));
        garble(
            &card,
            short_entry(&card, b"BOOK       "),
            b"BOOK       ",
            0x10,
            other,
            0,
        );

        let s = card.storage();
        let mut listed = 0;
        s.list("cache/1e/book", &mut |_| listed += 1).unwrap();
        assert!(listed > 0);
        assert_eq!(s.remove_dir_all("cache/1e"), Ok(()));
        assert_eq!(s.list("cache", &mut |_| {}), Ok(()));
        assert_eq!(s.list("cache/1e", &mut |_| {}), Err(StorageError::NotFound));
        assert_eq!(whole(&s, "keep/other.bin").unwrap(), text);
        followed_as_counted(&card, &s);
        fill(&s);
        let s = card.storage();
        assert_eq!(whole(&s, "keep/other.bin").unwrap(), text);
    }
}

/// A folder of a name PaperPaw does not take (made elsewhere: `con`),
/// with a folder and files in it: removed with its folder, all their
/// clusters freed.
#[test]
fn folders_not_taken_go_with_what_they_hold() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    s.write("odd/good.txt", b"good").unwrap();
    s.sync().unwrap();
    let free = counted(&s);
    let odd = s.fs.root_dir().open_dir("odd").unwrap();
    let con = odd.create_dir("con").unwrap();
    for dir in [con.clone(), con.create_dir("deeper").unwrap()] {
        let mut file = dir.create_file("nul.txt").unwrap();
        file.write_all(&kept()).unwrap();
    }
    drop((odd, con));
    s.fs.flush().unwrap();
    assert!(counted(&s) < free);
    assert_eq!(s.remove_dir_all("odd"), Ok(()));
    assert_eq!(s.list("odd", &mut |_| {}), Err(StorageError::NotFound));
    let s = card.storage();
    assert!(counted(&s) > free, "{} {free}", counted(&s));
}

/// A sane folder whose sub-folder entry points at another live folder
/// (cross-linked): its `..` names that folder's parent, so it is cut off,
/// and the live folder keeps what it holds, also once the card is filled.
#[test]
fn a_cross_linked_folder_is_not_followed() {
    for card in [Cut::formatted(FormatVolumeOptions::new()), fat32()] {
        let s = card.storage();
        s.write("keep/live/other.bin", &kept()).unwrap();
        s.write("cache/1e/book/sub/aaaa.bin", b"cached").unwrap();
        s.write("cache/1e/book/bbbb.bin", b"cached").unwrap();
        s.sync().unwrap();
        let live = first_cluster(&card, short_entry(&card, b"LIVE       "));
        garble(
            &card,
            short_entry(&card, b"SUB        "),
            b"SUB        ",
            0x10,
            live,
            0,
        );

        let s = card.storage();
        assert_eq!(s.remove_dir_all("cache"), Ok(()));
        assert_eq!(s.list("cache", &mut |_| {}), Err(StorageError::NotFound));
        assert_eq!(whole(&s, "keep/live/other.bin").unwrap(), kept());
        followed_as_counted(&card, &s);
        fill(&s);
        let s = card.storage();
        assert_eq!(whole(&s, "keep/live/other.bin").unwrap(), kept());
    }
}

/// Where cluster `n` of the card `image` is: its first byte, its size
/// (from the boot sector).
fn cluster_at(image: &[u8], n: u32) -> (usize, usize) {
    let word = |at: usize| usize::from(u16::from_le_bytes([image[at], image[at + 1]]));
    let sector = word(11);
    let fat = match word(22) {
        0 => u32::from_le_bytes([image[36], image[37], image[38], image[39]]) as usize,
        size => size,
    };
    let data = (word(14) + usize::from(image[16]) * fat) * sector + word(17) * 32;
    let size = usize::from(image[13]) * sector;
    (data + (n as usize - 2) * size, size)
}

/// A folder moved, the move cut off at any point, and the folder's
/// cluster then garbled: the move settles (its record cleared, no path
/// held), nothing is written into the folder, and its new path and the
/// old one can be removed.
#[test]
fn a_damaged_folder_moved_settles_after_any_cut() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    s.write("Books/Sub/x.txt", b"x").unwrap();
    s.write("Other/d.txt", b"d").unwrap();
    s.rename("Other/d.txt", "Other/e.txt").unwrap();
    s.sync().unwrap();
    let sub = first_cluster(&card, short_entry(&card, b"SUB        "));
    let text = b"Chapter one. It was a dark night. ".repeat(400);
    let cuts = cut_images(card.image(), |s| {
        s.rename("Books/Sub", "Other/Sub").unwrap()
    });
    for (n, mut image) in cuts.into_iter().enumerate() {
        let (at, size) = cluster_at(&image, sub);
        image[at..at + size].copy_from_slice(&text[..size]);
        let card = Cut::new(image);
        let s = card.storage();
        assert_eq!(s.write("other.txt", b"other"), Ok(()), "{n}");
        assert_eq!(s.recorded(), Ok(None), "{n}");
        assert!(s.stuck.borrow().is_none(), "{n}");
        assert_eq!(card.image()[at..at + size], text[..size], "{n}");
        assert_eq!(s.remove_dir_all("Other/Sub"), Ok(()), "{n}");
        assert_eq!(s.remove_dir_all("Books/Sub"), Ok(()), "{n}");
        assert_eq!(whole(&s, "Other/e.txt").unwrap(), b"d", "{n}");
    }
}
