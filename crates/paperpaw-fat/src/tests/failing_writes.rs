//! A card whose writes fail for a while (an SD card that times out,
//! after its driver's tries): nothing written looks whole, nothing kept
//! before is lost, and what waited is written once the card takes it.

use core::cell::Cell;

use super::*;

/// A card whose writes fail while `failing` is set.
#[derive(Clone)]
struct Failing {
    card: Cut,
    failing: alloc::rc::Rc<Cell<bool>>,
}

impl Blocks for Failing {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.card.read_block(index, block)
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        if self.failing.get() {
            return Err(DiskError);
        }
        self.card.write_block(index, block)
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        self.card.blocks()
    }
}

fn storage(card: &Failing) -> FatStorage<SharedDisk<Failing>> {
    let disk = SharedDisk::new(BlockDisk::new(card.clone()).unwrap());
    let fs = FileSystem::new(disk.clone(), FsOptions::new()).unwrap();
    FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk)
}

#[test]
fn a_write_the_card_refuses_fails_and_loses_nothing() {
    let card = Failing {
        card: Cut::formatted(FormatVolumeOptions::new()),
        failing: alloc::rc::Rc::default(),
    };
    let s = storage(&card);
    s.write("positions/a.txt", b"section = 3").unwrap();
    s.write("books/a.epub", &vec![7u8; 9000]).unwrap();
    s.sync().unwrap();
    // The card refuses: a file written anew, or appended to and synced,
    // says so; none looks written.
    card.failing.set(true);
    assert!(s.write("positions/a.txt", b"section = 4").is_err());
    let appended = s
        .append("books/a.epub", &[1u8; 5000])
        .and_then(|()| s.sync());
    assert!(appended.is_err());
    // On the card meanwhile: what was there before, whole.
    let before = mounted(card.card.image());
    assert_eq!(whole(&before, "positions/a.txt").unwrap(), b"section = 3");
    assert_eq!(whole(&before, "books/a.epub").unwrap(), vec![7u8; 9000]);
    // The card takes writes again: the next ones are whole.
    card.failing.set(false);
    s.write("positions/a.txt", b"section = 5").unwrap();
    s.sync().unwrap();
    let after = mounted(card.card.image());
    assert_eq!(whole(&after, "positions/a.txt").unwrap(), b"section = 5");
    let book = whole(&after, "books/a.epub").unwrap();
    assert!(book.len() == 9000 || book.len() == 14_000, "{}", book.len());
    assert!(book[..9000].iter().all(|&b| b == 7));
}

#[test]
fn a_sync_the_card_refused_is_done_by_the_next_one() {
    let card = Failing {
        card: Cut::formatted(FormatVolumeOptions::new()),
        failing: alloc::rc::Rc::default(),
    };
    let s = storage(&card);
    // Big enough to take clusters ahead when it grows (see `append`).
    s.write("books/a.epub", &vec![7u8; 20_000]).unwrap();
    s.sync().unwrap();
    s.append("books/a.epub", &[1u8; 5000]).unwrap();
    card.failing.set(true);
    assert!(s.sync().is_err());
    // The card takes writes again: the next sync writes back what the
    // refused one did not, its new size in its folder.
    card.failing.set(false);
    s.sync().unwrap();
    let after = mounted(card.card.image());
    assert_eq!(after.size("books/a.epub"), Ok(25_000));
}
