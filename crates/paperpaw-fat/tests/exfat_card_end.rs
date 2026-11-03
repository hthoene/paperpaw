//! The card's own end (#5, the owner's card on the X4): a volume a little
//! longer than its card, on cards that wrap what is written past their
//! end onto their first blocks, drop it, or fail it. The driver never
//! asks any of them for a block past the end, so the books on them stay
//! whole through the updates the X4 was given (a folder made, a large
//! file sent three times and removed, the settings written) and the
//! starts between.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::*;
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, ExFatStorage, SharedDisk};
use paperpaw_platform::Storage;

/// What a card does with a block past its end.
#[derive(Clone, Copy, Debug)]
enum PastEnd {
    /// Reads and writes the block as many blocks earlier as the card
    /// holds: its first blocks (a card whose memory is shorter than said).
    Wraps,
    /// Writes nothing and reads zeros, saying all went well.
    Drops,
    /// Fails.
    Fails,
}

/// A card of `bytes`, shorter than its volume, and how often the driver
/// asked it for a block past its end.
#[derive(Clone)]
struct EndCard {
    bytes: Bytes,
    past_end: PastEnd,
    asked_past: Rc<Cell<u32>>,
}

impl EndCard {
    fn new(bytes: Bytes, past_end: PastEnd) -> Self {
        Self {
            bytes,
            past_end,
            asked_past: Rc::default(),
        }
    }

    fn len(&self) -> u32 {
        (self.bytes.0.borrow().len() / BLOCK) as u32
    }

    /// Where block `index` lies, or none where the card has no such block.
    fn place(&self, index: u32) -> Result<Option<usize>, DiskError> {
        let len = self.len();
        if index < len {
            return Ok(Some(index as usize * BLOCK));
        }
        self.asked_past.set(self.asked_past.get() + 1);
        match self.past_end {
            PastEnd::Wraps => Ok(Some((index % len) as usize * BLOCK)),
            PastEnd::Drops => Ok(None),
            PastEnd::Fails => Err(DiskError),
        }
    }
}

impl Blocks for EndCard {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        match self.place(index)? {
            Some(at) => block.copy_from_slice(&self.bytes.0.borrow()[at..at + BLOCK]),
            None => block.fill(0),
        }
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        if let Some(at) = self.place(index)? {
            self.bytes.0.borrow_mut()[at..at + BLOCK].copy_from_slice(block);
        }
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok(self.len())
    }
}

fn mount(card: &EndCard) -> ExFatStorage<EndCard> {
    let (disk, _) = BlockDisk::find(card.clone()).unwrap();
    ExFatStorage::mount(SharedDisk::new(disk)).unwrap()
}

/// An exFAT card of 64 MiB in clusters of 32 KiB, as `mkfs.exfat` made it,
/// with books in `books/` written whole, then filled to leave `room`
/// bytes free (all where `None`), then its last `cut` blocks cut off.
fn card_with_books(room: Option<u64>, cut: usize) -> (Bytes, Vec<(String, Vec<u8>)>) {
    let card = image(include_bytes!("images/big32k.sparse"));
    let storage = exfat(&card);
    storage.create_dir("books").unwrap();
    let mut books = Vec::new();
    for i in 0..40usize {
        let name = format!("A book with a longer name, number {i}.epub");
        let data: Vec<u8> = (0..20_000 + i * 3_001)
            .map(|j| (i * 31 + j) as u8)
            .collect();
        storage.write(&format!("books/{name}"), &data).unwrap();
        books.push((name, data));
    }
    if let Some(room) = room {
        let free = storage.free_space().unwrap();
        storage
            .write("full.bin", &vec![0x5A; (free - room) as usize])
            .unwrap();
    }
    storage.sync().unwrap();
    drop(storage);
    let len = card.0.borrow().len();
    card.0.borrow_mut().truncate(len - cut * BLOCK);
    (card, books)
}

/// What the remote sends a file as: made empty, then appended to in
/// pieces of 4 KiB, then synced. Whether all of it was kept.
fn send(storage: &dyn Storage, path: &str, size: usize) -> bool {
    if storage.write(path, &[]).is_err() {
        return false;
    }
    let piece = [0xE9u8; 4096];
    let mut sent = 0;
    while sent < size {
        if storage.append(path, &piece).is_err() {
            return false;
        }
        sent += piece.len();
    }
    storage.sync().is_ok()
}

fn assert_books(storage: &dyn Storage, books: &[(String, Vec<u8>)], when: &str) {
    let mut listed = names(storage, "books");
    listed.sort();
    let mut want: Vec<String> = books.iter().map(|(name, _)| name.clone()).collect();
    want.sort();
    assert_eq!(listed, want, "{when}");
    for (name, data) in books {
        assert_eq!(
            &read(storage, &format!("books/{name}")),
            data,
            "{name}, {when}"
        );
    }
}

/// The updates the X4 was given, with a start before each: books whole
/// at every start, and no block past the card's end asked for.
fn updates_leave_the_books(room: Option<u64>, past_end: PastEnd) {
    // 1.5 MiB cut: 48 clusters past the end, within a 32nd of the card.
    let (bytes, books) = card_with_books(room, 3_072);
    let card = EndCard::new(bytes, past_end);
    let storage = mount(&card);
    assert!(storage.clusters_past_end() > 40);
    assert_books(&storage, &books, "first start");
    let _ = storage.create_dir("updates");
    drop(storage);
    for (round, size) in [5_791_328, 5_795_152, 5_310_752].into_iter().enumerate() {
        let storage = mount(&card);
        assert_books(&storage, &books, &format!("start {round}"));
        let _ = storage.write(
            "paperpaw.ini",
            format!("[device]\nround={round}\n").as_bytes(),
        );
        let sent = send(&storage, "updates/update.bin", size);
        // Sent whole where the card has room for it, never past its end.
        assert_eq!(sent, room.is_none(), "update {round}");
        let _ = storage.write(".paperpaw/paperpaw-install-note.txt", &[b'n'; 1200]);
        let _ = storage.sync();
        assert_books(&storage, &books, &format!("update {round}"));
        storage.remove("updates/update.bin").unwrap();
        storage.sync().unwrap();
        drop(storage);
    }
    let storage = mount(&card);
    assert_books(&storage, &books, "last start");
    assert!(!storage.exists("updates/update.bin"));
    assert_eq!(
        card.asked_past.get(),
        0,
        "blocks past the card's end asked for"
    );
}

#[test]
fn a_card_that_wraps_past_its_end_keeps_its_books_through_updates() {
    updates_leave_the_books(None, PastEnd::Wraps);
}

#[test]
fn a_card_that_drops_writes_past_its_end_keeps_its_books_through_updates() {
    updates_leave_the_books(None, PastEnd::Drops);
}

#[test]
fn a_card_that_fails_past_its_end_keeps_its_books_through_updates() {
    updates_leave_the_books(None, PastEnd::Fails);
}

#[test]
fn a_nearly_full_card_that_wraps_keeps_its_books_when_an_update_does_not_fit() {
    // Room for 2 MiB before the end: each update would reach past it.
    updates_leave_the_books(Some(2 << 20), PastEnd::Wraps);
}

#[test]
fn a_nearly_full_card_that_drops_keeps_its_books_when_an_update_does_not_fit() {
    updates_leave_the_books(Some(2 << 20), PastEnd::Drops);
}
