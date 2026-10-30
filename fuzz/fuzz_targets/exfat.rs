//! A damaged exFAT card: the bytes of a card image are patched where the
//! file system keeps its structures (the boot sector, the FAT, the
//! allocation bitmap, the folders), then the storage mounts it and does
//! what the reader does: lists, reads, writes, renames, removes, settles.
//! A panic at the mount is a reader that boots in a loop until the card is
//! taken out, so none may escape (errors are fine).

#![no_main]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, ExFatStorage, Kind, SharedDisk};
use paperpaw_platform::Storage;

/// The card the patches go on: an exFAT image with files and folders
/// (made by `mkfs.exfat` and filled by exfat-fuse), 8 MiB.
const FILLED: &[u8] = include_bytes!("../../crates/paperpaw-fat/tests/images/filled.sparse");

/// The image, and the blocks of it that are not zeros: where a patch goes.
fn base() -> &'static (Vec<u8>, Vec<usize>) {
    static BASE: OnceLock<(Vec<u8>, Vec<usize>)> = OnceLock::new();
    BASE.get_or_init(|| {
        let size = u64::from_le_bytes(FILLED[8..16].try_into().unwrap()) as usize;
        let mut bytes = vec![0u8; size];
        let mut used = Vec::new();
        for chunk in FILLED[16..].chunks(516) {
            let index = u32::from_le_bytes(chunk[..4].try_into().unwrap()) as usize;
            bytes[index * BLOCK..(index + 1) * BLOCK].copy_from_slice(&chunk[4..]);
            used.push(index);
        }
        (bytes, used)
    })
}

#[derive(Clone)]
struct Memory(Rc<RefCell<Vec<u8>>>);

impl Blocks for Memory {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.0.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.0
            .borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.0.borrow().len() / BLOCK) as u32)
    }
}

/// Everything of a folder, to a depth and a count: lists it, reads a
/// little of each file.
fn walk(s: &dyn Storage, dir: &str, depth: usize, seen: &mut usize) {
    let mut entries: Vec<(String, bool)> = Vec::new();
    let _ = s.list(dir, &mut |e| {
        if entries.len() < 64 {
            entries.push((e.name.to_string(), e.is_dir));
        }
    });
    for (name, is_dir) in entries {
        *seen += 1;
        if *seen > 200 {
            return;
        }
        let path = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        if is_dir {
            if depth < 3 {
                walk(s, &path, depth + 1, seen);
            }
        } else {
            let mut buf = [0u8; 2048];
            let _ = s.size(&path);
            let _ = s.modified(&path);
            let _ = s.read_at(&path, 0, &mut buf);
            let _ = s.read_at(&path, 1 << 20, &mut buf);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let (image, used) = base();
    let mut image = image.clone();
    // Records of five bytes and what they patch: a block among those in
    // use, a place in it, a count, the bytes.
    let mut rest = data;
    while rest.len() >= 5 {
        let block = used[usize::from(u16::from_le_bytes([rest[0], rest[1]])) % used.len()];
        let at = usize::from(u16::from_le_bytes([rest[2], rest[3]])) % BLOCK;
        let count = usize::from(rest[4] % 16) + 1;
        rest = &rest[5..];
        let take = count.min(rest.len());
        for (i, byte) in rest[..take].iter().enumerate() {
            image[block * BLOCK + (at + i) % BLOCK] = *byte;
        }
        rest = &rest[take..];
    }
    let card = Memory(Rc::new(RefCell::new(image)));
    let Ok((disk, Kind::ExFat)) = BlockDisk::find(card) else {
        return;
    };
    let Ok(storage) = ExFatStorage::mount(SharedDisk::new(disk)) else {
        return;
    };
    let mut seen = 0;
    walk(&storage, "", 0, &mut seen);
    let _ = storage.free_space();
    let _ = storage.total_space();
    // What the reader writes: a file anew, an append, a move, a folder.
    let _ = storage.create_dir("fuzz");
    let _ = storage.write("fuzz/a.txt", &[1u8; 5000]);
    let _ = storage.write("fuzz/a.txt", &[2u8; 700]);
    let _ = storage.append("fuzz/b.txt", &[3u8; 3000]);
    let _ = storage.rename("fuzz/a.txt", "fuzz/c/d.txt");
    let _ = storage.write("notes.txt", b"notes");
    let _ = storage.remove("fuzz/b.txt");
    let mut seen = 0;
    walk(&storage, "", 0, &mut seen);
    let _ = storage.remove_dir_all("fuzz");
    let _ = storage.sync();
});
