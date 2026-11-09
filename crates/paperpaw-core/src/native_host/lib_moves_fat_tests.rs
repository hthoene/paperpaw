#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The library's moves on a real FAT card (`paperpaw-fat`, blocks in memory),
//! cut off after each block the whole job writes: the journal, `~PPMOVE.BIN`,
//! the directory entries, and every step after. Each card left is mounted
//! anew, the job finished from the journal alone (`Moves::start`), and what
//! it holds checked: every file whole, once, under one spelling, the journal
//! cleared, the catalog carried exactly when the rename took effect.
//!
//! The identity here is what an entry holds (names below it and bytes, as in
//! the fake card of the tests above) until `paperpaw-fat` has its own
//! `entry_identity`; it is stable across a rename, which is all the machine
//! asks.
use super::*;
use crate::native_host::lib_moves_journal::JOURNAL;
use core::cell::RefCell;
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk,
    fatfs::{FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write},
};
use std::{rc::Rc, string::String, vec, vec::Vec};

/// A 4 MB card in memory; with `record` set, every block written to it is
/// kept in order.
#[derive(Clone)]
struct Ram {
    bytes: Rc<RefCell<Vec<u8>>>,
    writes: Rc<RefCell<Vec<(u32, [u8; BLOCK])>>>,
    record: Rc<core::cell::Cell<bool>>,
}

impl Ram {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Rc::new(RefCell::new(bytes)),
            writes: Rc::default(),
            record: Rc::default(),
        }
    }

    /// Formatted, with FAT.
    fn formatted() -> Self {
        let ram = Self::new(vec![0u8; 4 << 20]);
        let mut disk = BlockDisk::new(ram.clone()).unwrap();
        paperpaw_fat::fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
        disk.flush().unwrap();
        ram
    }

    /// The storage on it, as on the X4: the disk shared with the storage.
    fn storage(&self) -> Rc<FatStorage<SharedDisk<Ram>>> {
        let mut disk = BlockDisk::new(self.clone()).unwrap();
        disk.seek(SeekFrom::Start(0)).unwrap();
        let shared = SharedDisk::new(disk);
        let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
        Rc::new(FatStorage::new(std::boxed::Box::leak(std::boxed::Box::new(fs))).with_disk(shared))
    }
}

impl Blocks for Ram {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.bytes.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.bytes
            .borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        if self.record.get() {
            self.writes.borrow_mut().push((index, *block));
        }
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.bytes.borrow().len() / BLOCK) as u32)
    }
}

type Fat = FatStorage<SharedDisk<Ram>>;

/// What the entry at `path` holds, as a number (see the module doc).
struct Content(Rc<Fat>);

fn files_below(s: &Fat, path: &str, out: &mut Vec<String>) {
    let mut names = Vec::new();
    if s.list(path, &mut |e| names.push((String::from(e.name), e.is_dir)))
        .is_err()
    {
        return;
    }
    for (name, is_dir) in names {
        let full = std::format!("{path}/{name}");
        if is_dir {
            files_below(s, &full, out);
        } else {
            out.push(full);
        }
    }
}

fn whole(s: &Fat, path: &str) -> Vec<u8> {
    let mut bytes = vec![0u8; s.size(path).unwrap() as usize];
    let mut at = 0;
    while at < bytes.len() {
        let n = s.read_at(path, at as u64, &mut bytes[at..]).unwrap();
        assert!(n > 0, "{path} ends early");
        at += n;
    }
    bytes
}

impl EntryIdentity for Content {
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        let mut files = Vec::new();
        match self.0.size(path) {
            Ok(_) => files.push(String::from(path)),
            Err(StorageError::InvalidPath) => files_below(&self.0, path, &mut files),
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        }
        if files.is_empty() {
            return Ok(None);
        }
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for file in &files {
            let rest = file.strip_prefix(path).unwrap_or("");
            for b in rest.bytes().chain(whole(&self.0, file)) {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        Ok(Some(hash))
    }
}

/// A book of several clusters (2 KB each on this card), distinct per `seed`.
fn book(seed: u8) -> Vec<u8> {
    (0..5000u32)
        .map(|i| 1 + ((i as u8).wrapping_add(seed) % 200))
        .collect()
}

fn card_host(ram: &Ram) -> (CardMoves, Rc<Fat>) {
    let storage = ram.storage();
    let host = CardMoves::new(storage.clone(), Some(Rc::new(Content(storage.clone()))));
    (host, storage)
}

/// The card after `job` (which gets the host and starts it), as the image
/// before the job and after each block the job wrote.
fn cuts(
    files: &[(&str, Vec<u8>)],
    job: impl FnOnce(&CardMoves, &Carry, &Moves),
) -> Vec<Vec<u8>> {
    let ram = Ram::formatted();
    {
        let s = ram.storage();
        for (path, data) in files {
            s.write(path, data).unwrap();
        }
        s.sync().unwrap();
    }
    let mut image = ram.bytes.borrow().clone();
    ram.record.set(true);
    let (host, _storage) = card_host(&ram);
    let (carry, moves) = (Carry::default(), Moves::default());
    job(&host, &carry, &moves);
    let mut cuts = vec![image.clone()];
    for (index, block) in ram.writes.borrow().iter() {
        let at = *index as usize * BLOCK;
        image[at..at + BLOCK].copy_from_slice(block);
        cuts.push(image.clone());
    }
    cuts
}

/// The card `image` mounted anew, the job finished from the journal alone.
fn restart(image: Vec<u8>) -> (Rc<Fat>, Carry) {
    let ram = Ram::new(image);
    let (host, storage) = card_host(&ram);
    let (carry, moves) = (Carry::default(), Moves::default());
    moves.start(&host).unwrap();
    run(&moves, &host, &carry);
    (storage, carry)
}

/// Whether the card `image` still has the journal (the job is not done).
fn has_journal(image: &[u8]) -> bool {
    Ram::new(image.to_vec()).storage().exists(JOURNAL)
}

/// Whether the file at `path` is on the card, whole as `bytes`.
fn holds(s: &Fat, path: &str, bytes: &[u8]) -> bool {
    s.size(path).is_ok() && whole(s, path) == bytes
}

/// The moved folder, in `from` and `to` spelling, whole in one of them.
fn folder_moves(from: &str, to: &str, other_folder: bool) {
    let files = [
        (std::format!("{from}/a.epub"), book(1)),
        (std::format!("{from}/sub/b.epub"), book(2)),
        ("Books/keep.epub".into(), book(3)),
        ("Archive/z.epub".into(), book(7)),
    ];
    let named: Vec<(&str, Vec<u8>)> = files.iter().map(|(p, d)| (p.as_str(), d.clone())).collect();
    let cuts = cuts(&named, |host, _, moves| {
        moves.begin(host, from, to, false).unwrap();
    });
    assert!(cuts.len() > 3, "the job writes blocks: {}", cuts.len());
    let (mut before, mut after) = (0, 0);
    for (n, image) in cuts.into_iter().enumerate() {
        let open = has_journal(&image);
        let (s, carry) = restart(image);
        assert!(holds(&s, "Books/keep.epub", &book(3)), "{n}: a neighbour");
        let moved = holds(&s, &std::format!("{to}/a.epub"), &book(1))
            && holds(&s, &std::format!("{to}/sub/b.epub"), &book(2));
        let stayed = holds(&s, &std::format!("{from}/a.epub"), &book(1))
            && holds(&s, &std::format!("{from}/sub/b.epub"), &book(2));
        // Under one spelling: the case-only rename names one folder either
        // way, so only a cross-folder move can tell the two apart by name.
        if other_folder {
            assert!(moved != stayed, "{n}: moved {moved}, stayed {stayed}");
        } else {
            assert!(moved || stayed, "{n}: the folder is lost");
        }
        assert!(!s.exists(JOURNAL), "{n}: the journal is cleared");
        let spelled_new = {
            let mut found = false;
            let (parent, name) = split(to);
            s.list(parent, &mut |e| found |= e.name == name).unwrap();
            found
        };
        if moved && spelled_new {
            after += 1;
            // A job whose entry is cleared was carried before the cut.
            assert_eq!(carry.count("Books("), usize::from(open), "{n}: carried once");
        } else {
            before += 1;
            assert_eq!(carry.count("Books("), 0, "{n}: nothing carried");
        }
    }
    assert!(before > 0 && after > 0, "both sides of the rename are cut");
}

#[test]
fn a_folder_moved_across_folders_is_whole_in_one_place_after_any_cut() {
    folder_moves("Books/old", "Archive/old", true);
}

#[test]
fn a_case_only_rename_through_a_temporary_name_is_whole_after_any_cut() {
    folder_moves("Books/Old", "Books/old", false);
}

#[test]
fn a_file_renamed_in_case_only_is_whole_after_any_cut() {
    let files = [("Books/Aa.epub", book(4)), ("Books/keep.epub", book(3))];
    let cuts = cuts(&files, |host, _, moves| {
        moves
            .begin(host, "Books/Aa.epub", "Books/aa.epub", false)
            .unwrap();
    });
    assert!(cuts.len() > 3);
    for (n, image) in cuts.into_iter().enumerate() {
        let (s, carry) = restart(image);
        assert!(holds(&s, "Books/aa.epub", &book(4)), "{n}: the book is whole");
        assert!(holds(&s, "Books/keep.epub", &book(3)), "{n}: a neighbour");
        assert!(!s.exists(JOURNAL));
        let mut spelled = Vec::new();
        s.list("Books", &mut |e| spelled.push(String::from(e.name)))
            .unwrap();
        assert_eq!(spelled.iter().filter(|n| n.eq_ignore_ascii_case("aa.epub")).count(), 1);
        assert!(carry.count("Books(") <= 1, "{n}");
    }
}

#[test]
fn a_replacing_move_is_old_or_new_and_discards_at_most_once_after_any_cut() {
    let files = [
        ("Books/other.epub", book(5)),
        ("Books/b.epub", book(6)),
        ("Books/keep.epub", book(3)),
    ];
    let cuts = cuts(&files, |host, carry, moves| {
        moves
            .begin(host, "Books/other.epub", "Books/b.epub", true)
            .unwrap();
        // The first steps, in the job's own memory: the discard and its flag.
        run(moves, host, carry);
    });
    assert!(cuts.len() > 5);
    let (mut renamed, mut kept) = (0, 0);
    for (n, image) in cuts.into_iter().enumerate() {
        let open = has_journal(&image);
        let (s, carry) = restart(image);
        assert!(holds(&s, "Books/keep.epub", &book(3)), "{n}: a neighbour");
        assert!(!s.exists(JOURNAL), "{n}");
        let (new, old) = (
            holds(&s, "Books/b.epub", &book(5)) && !s.exists("Books/other.epub"),
            holds(&s, "Books/b.epub", &book(6)) && holds(&s, "Books/other.epub", &book(5)),
        );
        assert!(new != old, "{n}: new {new}, old {old}");
        if new {
            renamed += 1;
            assert!(carry.count("discard") <= 1, "{n}: discarded twice");
            assert_eq!(carry.count("Books("), usize::from(open), "{n}");
        } else {
            kept += 1;
            assert_eq!(carry.count("discard") + carry.count("Books("), 0, "{n}");
        }
    }
    assert!(renamed > 0 && kept > 0);
}
