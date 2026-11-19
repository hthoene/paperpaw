//! A write of what a small file already holds sends the card nothing (#389).

use super::*;

fn read(s: &FatStorage<SharedDisk<Cut>>, path: &str) -> Vec<u8> {
    let mut out = vec![0u8; s.size(path).unwrap() as usize];
    s.read_at(path, 0, &mut out).unwrap();
    out
}

fn blocks_written(card: &Cut, write: impl FnOnce()) -> usize {
    card.record_writes();
    write();
    card.writes.borrow().len()
}

#[test]
fn writing_the_same_bytes_again_writes_no_block() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    s.write(".paperpaw/a.ini", b"[a]\nx = 1\n").unwrap();
    s.sync().unwrap();
    let again = blocks_written(&card, || {
        s.write(".paperpaw/a.ini", b"[a]\nx = 1\n").unwrap();
        s.sync().unwrap();
    });
    assert_eq!(again, 0, "an unchanged file was written again");
    let changed = blocks_written(&card, || {
        s.write(".paperpaw/a.ini", b"[a]\nx = 2\n").unwrap();
        s.sync().unwrap();
    });
    assert!(changed > 0, "a changed file was not written");
    assert_eq!(read(&s, ".paperpaw/a.ini"), b"[a]\nx = 2\n");
    // A longer or shorter one is a change too.
    s.write(".paperpaw/a.ini", b"[a]\nx = 22\n").unwrap();
    assert_eq!(read(&s, ".paperpaw/a.ini"), b"[a]\nx = 22\n");
    s.write(".paperpaw/a.ini", b"").unwrap();
    assert_eq!(read(&s, ".paperpaw/a.ini"), b"");
    s.write(".paperpaw/a.ini", b"").unwrap();
}
