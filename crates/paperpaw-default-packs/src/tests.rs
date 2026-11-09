//! Seeding English hyphenation: what it writes, when it does nothing, and
//! what a power cut or a full card leaves.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use core::cell::Cell;

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;

use super::*;

const INI: &str = "plugins/hyphenation/plugin.ini";
const TABLE: &str = "plugins/hyphenation/hyphenation/en.pphp";

/// A storage that can fail writes after some number and tell its free space.
/// With `full`, writes fail from that many bytes written on, as a card fills
/// up.
struct Card {
    files: MemoryStorage,
    /// Writes allowed before every write fails (`None`: no limit).
    writes: Cell<Option<usize>>,
    free: Option<u64>,
    /// Bytes the card still takes (`None`: unlimited); a write that does not
    /// fit fails, one that does uses it up.
    room: Cell<Option<usize>>,
    /// Calls of `sync`.
    syncs: Cell<usize>,
}

impl Card {
    fn new() -> Self {
        Self {
            files: MemoryStorage::new(),
            writes: Cell::new(None),
            free: None,
            room: Cell::new(None),
            syncs: Cell::new(0),
        }
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if let Some(left) = self.writes.get() {
            if left == 0 {
                return Err(StorageError::Io);
            }
            self.writes.set(Some(left - 1));
        }
        if let Some(room) = self.room.get() {
            if data.len() > room {
                return Err(StorageError::Full);
            }
            self.room.set(Some(room - data.len()));
        }
        self.files.write(path, data)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.syncs.set(self.syncs.get() + 1);
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove_dir_all(path)
    }
    fn free_space(&self) -> Option<u64> {
        self.free
    }
}

fn table_of(card: &Card) -> alloc::vec::Vec<u8> {
    let mut bytes = alloc::vec![0u8; card.size(TABLE).unwrap() as usize];
    assert_eq!(card.read_at(TABLE, 0, &mut bytes).unwrap(), bytes.len());
    bytes
}

#[test]
fn the_package_has_a_manifest_and_a_table_the_kernel_accepts() {
    let files = english();
    let ini = files
        .iter()
        .find(|(p, _)| *p == "plugin.ini")
        .expect("plugin.ini");
    let ini = core::str::from_utf8(ini.1).unwrap();
    assert!(ini.contains("id = hyphenation"), "{ini}");
    assert!(ini.contains("formats = 5"), "{ini}");
    assert!(ini.contains("en = hyphenation/en.pphp"), "{ini}");
    let table = files
        .iter()
        .find(|(p, _)| *p == "hyphenation/en.pphp")
        .expect("the table");
    paperpaw_hyphen::check_table(table.1).expect("a table the reader opens");
    let bytes: usize = files.iter().map(|(_, b)| b.len()).sum();
    assert!(bytes < ROOM as usize / 2, "{bytes} bytes");
}

#[test]
fn a_card_without_the_pack_gets_it_once_as_after_a_beta_3_update() {
    // A card as Beta 3 left it: books, other plugins' folders, no English
    // (it was built in), and no mark.
    let card = Card::new();
    card.files.write("books/a.epub", b"x").unwrap();
    card.files
        .write("plugins/stats/plugin.ini", b"[plugin]\nid = stats\n")
        .unwrap();
    assert_eq!(seed_hyphenation(&card), Seeded::Installed);
    for (path, bytes) in english() {
        let mut read = alloc::vec![0u8; bytes.len()];
        let at = format!("plugins/hyphenation/{path}");
        assert_eq!(
            card.read_at(&at, 0, &mut read).unwrap(),
            bytes.len(),
            "{at}"
        );
        assert_eq!(&read[..], *bytes, "{at}");
    }
    assert!(card.exists(MARK));
    // The next start does nothing.
    let before = card.writes.get();
    assert_eq!(seed_hyphenation(&card), Seeded::Kept);
    assert_eq!(card.writes.get(), before);
}

#[test]
fn a_pack_the_reader_removed_stays_removed() {
    let card = Card::new();
    assert_eq!(seed_hyphenation(&card), Seeded::Installed);
    card.files.remove_dir_all("plugins/hyphenation").unwrap();
    assert!(!card.exists(INI));
    assert_eq!(seed_hyphenation(&card), Seeded::Kept);
    assert!(!card.exists(INI));
}

#[test]
fn a_pack_already_there_is_kept_as_it_is_and_marked() {
    // The flasher's (or a newer one): not overwritten.
    let card = Card::new();
    card.files
        .write(
            INI,
            b"[plugin]\nid = hyphenation\nversion = 9.0.0\nformats = 5\n\n[hyphenation]\nen = hyphenation/newer.pphp\n",
        )
        .unwrap();
    card.files
        .write("plugins/hyphenation/hyphenation/newer.pphp", b"table")
        .unwrap();
    assert_eq!(seed_hyphenation(&card), Seeded::Kept);
    assert!(card.exists(MARK));
    let mut buf = [0u8; 128];
    let n = card.read_at(INI, 0, &mut buf).unwrap();
    assert!(core::str::from_utf8(&buf[..n]).unwrap().contains("9.0.0"));
    assert!(!card.exists(TABLE));
}

/// A `plugin.ini` that is no hyphenation plugin must not keep the device
/// without English: the pack is written over it, and only then is the mark.
#[test]
fn a_plugin_ini_that_is_no_plugin_does_not_stop_the_pack() {
    let broken: [&[u8]; 5] = [
        b"",
        b"\xff\xfe not text",
        // Another id.
        b"[plugin]\nid = stats\nformats = 5\n\n[hyphenation]\nen = hyphenation/en.pphp\n",
        // Made for formats before the section.
        b"[plugin]\nid = hyphenation\nformats = 4\n\n[hyphenation]\nen = hyphenation/en.pphp\n",
        // Names a table that is not there.
        b"[plugin]\nid = hyphenation\nformats = 5\n\n[hyphenation]\nen = hyphenation/gone.pphp\n",
    ];
    for ini in broken {
        let card = Card::new();
        card.files.write(INI, ini).unwrap();
        assert!(!card.exists(MARK));
        assert_eq!(seed_hyphenation(&card), Seeded::Installed, "{ini:?}");
        assert!(card.exists(MARK));
        assert!(card.exists(TABLE));
        let mut package = alloc::vec![0u8; card.size(INI).unwrap() as usize];
        card.read_at(INI, 0, &mut package).unwrap();
        assert!(core::str::from_utf8(&package).unwrap().contains("id = hyphenation"));
    }
}

/// What the package puts on a card is a plugin by the same rule: with the mark
/// gone it is kept, not written again.
#[test]
fn the_package_itself_is_a_usable_plugin() {
    let card = Card::new();
    assert_eq!(seed_hyphenation(&card), Seeded::Installed);
    let syncs = card.syncs.get();
    card.files.remove(MARK).unwrap();
    assert_eq!(seed_hyphenation(&card), Seeded::Kept);
    assert!(card.exists(MARK));
    assert_eq!(card.syncs.get(), syncs, "nothing was written again");
}

/// A file that cannot be judged (too big to read here) is left alone and not
/// marked: looked at again at the next start.
#[test]
fn a_plugin_ini_that_cannot_be_judged_is_kept_without_a_mark() {
    let card = Card::new();
    let big = alloc::vec![b'#'; INI_MAX as usize + 1];
    card.files.write(INI, &big).unwrap();
    assert_eq!(seed_hyphenation(&card), Seeded::Kept);
    assert!(!card.exists(MARK));
    assert!(!card.exists(TABLE));
    assert_eq!(card.size(INI).unwrap(), INI_MAX + 1);
}

#[test]
fn a_power_cut_while_copying_leaves_no_plugin_and_the_next_start_finishes() {
    let files = english().len();
    // Cut after each number of writes in turn.
    for allowed in 0..files {
        let card = Card::new();
        card.writes.set(Some(allowed));
        assert_eq!(seed_hyphenation(&card), Seeded::Failed, "{allowed}");
        // No manifest, so no plugin; no mark, so it is tried again.
        assert!(!card.exists(INI), "{allowed}");
        assert!(!card.exists(MARK), "{allowed}");
        assert!(
            !card.exists(TABLE),
            "{allowed}: what was written is taken back"
        );
        card.writes.set(None);
        assert_eq!(seed_hyphenation(&card), Seeded::Installed, "{allowed}");
        assert!(card.exists(INI));
        let table = table_of(&card);
        paperpaw_hyphen::check_table(&table).expect("the whole table");
    }
}

#[test]
fn a_card_with_no_room_is_left_alone_and_tried_again() {
    let mut card = Card::new();
    card.free = Some(ROOM - 1);
    assert_eq!(seed_hyphenation(&card), Seeded::NoRoom);
    assert!(!card.exists(INI) && !card.exists(MARK));
    card.free = Some(ROOM);
    assert_eq!(seed_hyphenation(&card), Seeded::Installed);
}

#[test]
fn a_card_that_fills_up_part_way_is_cleaned_and_given_up_after_three_starts() {
    // The X4 does not know the free space at the start: nothing says no
    // room, and the table does not fit.
    let card = Card::new();
    card.room.set(Some(10_000));
    for n in 1..=TRIES {
        card.room.set(Some(10_000));
        assert_eq!(seed_hyphenation(&card), Seeded::Failed, "start {n}");
        assert!(
            !card.exists(TABLE) && !card.exists(INI),
            "start {n}: cleaned"
        );
        assert!(!card.exists(MARK));
        assert_eq!(card.size(TRIED).unwrap(), n as u64, "start {n}");
    }
    // Given up: nothing is written any more, even with room.
    card.room.set(None);
    assert_eq!(seed_hyphenation(&card), Seeded::GaveUp);
    assert!(!card.exists(INI) && !card.exists(TABLE));
    // Removing the notes lets it try once more.
    card.files.remove_dir_all(".paperpaw/seeded").unwrap();
    assert_eq!(seed_hyphenation(&card), Seeded::Installed);
}

#[test]
fn every_file_is_synced_before_the_manifest_is_written() {
    let card = Card::new();
    assert_eq!(seed_hyphenation(&card), Seeded::Installed);
    // One `sync` before `plugin.ini`, one at the end.
    assert_eq!(card.syncs.get(), 2);
}
