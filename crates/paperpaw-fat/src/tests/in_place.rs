//! `Storage::write_at` on the card (#305): a slot of a file written where
//! it is costs one block, changes neither the FAT nor a folder, and after
//! a cut the file reads its old bytes or its new ones.

use super::*;

/// A file of two blocks, the first all `a`, the second all `b`.
fn two_slots() -> Vec<u8> {
    let mut data = vec![b'a'; BLOCK];
    data.extend_from_slice(&[b'b'; BLOCK]);
    data
}

#[test]
fn a_slot_written_in_place_is_one_block_and_old_or_new_after_a_cut() {
    let image = prepared(|s| {
        s.write("places/one.txt", &two_slots()).unwrap();
        s.write("places/other.txt", b"beside it").unwrap();
    });
    for slot in 0..2usize {
        let new = vec![b'n'; BLOCK];
        let cuts = cut_images(image.clone(), |s| {
            // Read first, as a writer that looks at the slots does.
            assert_eq!(whole(s, "places/one.txt").unwrap(), two_slots());
            s.write_at("places/one.txt", (slot * BLOCK) as u64, &new)
                .unwrap();
        });
        // Before the write, and after its one block.
        assert_eq!(cuts.len(), 2, "blocks written");
        let mut after = two_slots();
        after[slot * BLOCK..(slot + 1) * BLOCK].copy_from_slice(&new);
        for (cut, expected) in cuts.into_iter().zip([two_slots(), after]) {
            let s = mounted(cut);
            assert_eq!(whole(&s, "places/one.txt").unwrap(), expected);
            assert_eq!(whole(&s, "places/other.txt").unwrap(), b"beside it");
            assert_eq!(cross_linked(&s), Vec::<String>::new());
        }
    }
}

#[test]
fn bytes_are_written_within_a_file_at_its_end_and_not_past_it() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    assert_eq!(s.write_at("none.bin", 0, b"x"), Err(StorageError::NotFound));
    s.write("save.bin", b"0123456789").unwrap();
    s.write_at("save.bin", 2, b"XY").unwrap();
    assert_eq!(whole(&s, "save.bin").unwrap(), b"01XY456789");
    // At its end it grows, over clusters too, and its size is on the card.
    let more = vec![7u8; 5000];
    s.write_at("save.bin", 10, &more).unwrap();
    assert_eq!(s.size("save.bin"), Ok(5010));
    assert_eq!(
        s.write_at("save.bin", 5011, b"x"),
        Err(StorageError::Unsupported)
    );
    let again = mounted(card.image());
    let read = whole(&again, "save.bin").unwrap();
    assert_eq!(&read[..10], b"01XY456789");
    assert_eq!(&read[10..], &more[..]);
    // After appends kept back: written back first, then in their place.
    s.append("log.bin", b"abc").unwrap();
    s.append("log.bin", b"def").unwrap();
    s.write_at("log.bin", 1, b"__").unwrap();
    assert_eq!(whole(&mounted(card.image()), "log.bin").unwrap(), b"a__def");
    assert_eq!(cross_linked(&mounted(card.image())), Vec::<String>::new());
}
