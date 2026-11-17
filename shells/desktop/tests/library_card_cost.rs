//! The card cost of the Library's book walk and first open (#297): on the X4
//! the first draw after the walk took 98 writes and 11,760 card blocks (1,491
//! read commands) for 14 books. The emulator's storage counts calls, not what
//! the card is sent: this mounts the X4's FAT file system (`paperpaw-fat`, its
//! 24-block cache and read-ahead) on a RAM disk that counts the commands and
//! blocks it is sent.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use paperpaw_emulator::{device::Device, headless::run_script, profiles};
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk,
    fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write},
};
use paperpaw_platform::Storage;

const BOOKS: usize = 14;
const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// What the card was sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Commands {
    reads: u32,
    read_blocks: u32,
    writes: u32,
    write_blocks: u32,
}

#[derive(Clone)]
struct Ram {
    bytes: Rc<RefCell<Vec<u8>>>,
    sent: Rc<Cell<Commands>>,
}

impl Ram {
    fn count(&self, f: impl FnOnce(&mut Commands)) {
        let mut sent = self.sent.get();
        f(&mut sent);
        self.sent.set(sent);
    }
}

impl Blocks for Ram {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.read_blocks(index, block)
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.write_blocks(index, block)
    }

    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        data.copy_from_slice(
            self.bytes
                .borrow()
                .get(at..at + data.len())
                .ok_or(DiskError)?,
        );
        self.count(|c| {
            c.reads += 1;
            c.read_blocks += (data.len() / BLOCK) as u32;
        });
        Ok(())
    }

    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.bytes
            .borrow_mut()
            .get_mut(at..at + data.len())
            .ok_or(DiskError)?
            .copy_from_slice(data);
        self.count(|c| {
            c.writes += 1;
            c.write_blocks += (data.len() / BLOCK) as u32;
        });
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.bytes.borrow().len() / BLOCK) as u32)
    }
}

/// A book of the Beta 3 sample, made its own by a zip comment.
fn book(n: usize) -> Vec<u8> {
    let mut book = ALICE.to_vec();
    let end = book.len();
    book[end - 2..].copy_from_slice(&(n as u16).to_le_bytes());
    book.extend(std::iter::repeat_n(b'x', n));
    book
}

/// A card of 300 MB with FAT32 (4 KiB clusters), `BOOKS` books and the
/// files a started device has; the commands it was sent so far forgotten.
fn card() -> (Rc<FatStorage<SharedDisk<Ram>>>, Rc<Cell<Commands>>) {
    let ram = Ram {
        bytes: Rc::new(RefCell::new(vec![0u8; 300 << 20])),
        sent: Rc::default(),
    };
    let mut disk = BlockDisk::new(ram.clone()).unwrap();
    paperpaw_fat::fatfs::format_volume(
        &mut disk,
        FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .bytes_per_cluster(4096),
    )
    .unwrap();
    disk.flush().unwrap();
    let mut disk = BlockDisk::new(ram.clone()).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    let shared = SharedDisk::new(disk);
    let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
    let storage = Rc::new(FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared));
    storage.write(".paperpaw/plugins/setup/done", b"").unwrap();
    storage
        .write(
            "paperpaw.ini",
            b"[help]\nshow = false\n[reader]\nopen-last = false\n",
        )
        .unwrap();
    for n in 0..BOOKS {
        storage
            .write(&format!("books/b{n:03}.epub"), &book(n))
            .unwrap();
    }
    storage.sync().unwrap();
    (storage, ram.sent)
}

/// The card commands by what asked for them, the most first.
fn report(phase: &str, log: &[paperpaw_emulator::trace::Call]) {
    let mut by: std::collections::BTreeMap<String, (u32, u64)> = Default::default();
    for call in log {
        let path: Vec<&str> = call
            .path
            .split('/')
            .map(|part| {
                if part.len() >= 16 && part.chars().all(|c| c.is_ascii_hexdigit()) {
                    "<h>"
                } else if part.starts_with('b') && part.ends_with(".epub") {
                    "<book>"
                } else {
                    part
                }
            })
            .collect();
        let entry = by
            .entry(format!("{} {}", call.op, path.join("/")))
            .or_default();
        entry.0 += 1;
        entry.1 += call.cost;
    }
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by_key(|(_, (_, cost))| std::cmp::Reverse(*cost));
    for (key, (calls, cost)) in rows.iter().take(25) {
        eprintln!("{phase}: {cost:5} commands, {calls:4} calls: {key}");
    }
}

#[test]
fn the_walk_and_first_open_of_the_library_stay_within_their_card_commands() {
    let (storage, sent) = card();
    paperpaw_emulator::budget::override_limit(None);
    let mut device =
        Device::on_storage(profiles::find("xteink-x4").unwrap(), storage, true).unwrap();
    sent.set(Commands::default());
    let probe = sent.clone();
    paperpaw_emulator::trace::set_cost(Rc::new(move || {
        let c = probe.get();
        u64::from(c.reads + c.writes)
    }));
    paperpaw_emulator::trace::log();
    device.start();
    run_script(&mut device, "wait:30000").unwrap();
    let walk = sent.replace(Commands::default());
    report("walk", &paperpaw_emulator::trace::take_log());
    device.shown.clear();
    run_script(&mut device, "open:files.browser").unwrap();
    run_script(&mut device, "wait:300").unwrap();
    let open = sent.replace(Commands::default());
    report("open", &paperpaw_emulator::trace::take_log());
    eprintln!("card: start and walk of {BOOKS} books {walk:?}; first open {open:?}");
    assert!(!device.shown.is_empty(), "the screen drew nothing");
    // Measured with 14 books: 607 read and 684 write commands for the start
    // and the walk (a stand-in details file written for every native book
    // before the host's job wrote the real one: two writes of a book, 22
    // commands each); 541 and 515 with the job's write alone.
    assert!(
        walk.writes <= 560,
        "the walk sent {} write commands: {walk:?}",
        walk.writes
    );
    assert!(
        walk.reads <= 580,
        "the walk sent {} read commands: {walk:?}",
        walk.reads
    );
    // The first open reads a little and writes nothing (20 and 0 measured).
    assert!(
        open.writes == 0 && open.reads <= 40,
        "the first open sent {open:?}"
    );
}
