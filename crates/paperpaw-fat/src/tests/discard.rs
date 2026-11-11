//! `SharedDisk::discard_changes` (#5, Senior M1 on the exFAT write gate):
//! changed blocks it forgets never reach the card, not even when later
//! reads evict their slots or the disk is flushed, and reads see the card.

use super::*;
use fatfs::{Read as _, Seek as _, SeekFrom, Write as _};

#[test]
fn discarded_changes_never_reach_the_card() {
    let blocks = 3 * disk::SLOTS;
    let card = Cut::new(vec![0; blocks * BLOCK]);
    let disk = SharedDisk::new(BlockDisk::new(card.clone()).unwrap());
    let mut io = disk.clone();
    // Parts of blocks: they are changed in their slots, not written.
    for block in 0..5u64 {
        io.seek(SeekFrom::Start(block * BLOCK as u64 + 3)).unwrap();
        io.write_all(b"half-made").unwrap();
    }
    assert!(
        card.writes.borrow().is_empty(),
        "kept back before the discard"
    );
    disk.discard_changes();
    // Parts of more blocks than are kept: every slot is evicted.
    let mut byte = [0u8; 1];
    for block in 0..blocks as u64 {
        io.seek(SeekFrom::Start(block * BLOCK as u64 + 5)).unwrap();
        io.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [0], "block {block} read what was discarded");
    }
    disk.flush_all().unwrap();
    assert!(
        card.writes.borrow().is_empty(),
        "a discarded change was written"
    );
    assert!(card.image().iter().all(|&b| b == 0));
    // Changes after it are written as before.
    io.seek(SeekFrom::Start(7)).unwrap();
    io.write_all(b"kept").unwrap();
    disk.flush_all().unwrap();
    assert_eq!(&card.image()[7..11], b"kept");
}
