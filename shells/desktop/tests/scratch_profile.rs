//! scratch: The card cost of the Library's book walk and first open (#297): on the X4
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


fn card(last: bool) -> (Rc<FatStorage<SharedDisk<Ram>>>, Rc<Cell<Commands>>) {
    let ram = Ram {
        bytes: Rc::new(RefCell::new(vec![0u8; 300 << 20])),
        sent: Rc::default(),
    };
    let mut disk = BlockDisk::new(ram.clone()).unwrap();
    paperpaw_fat::fatfs::format_volume(
        &mut disk,
        FormatVolumeOptions::new().fat_type(FatType::Fat32).bytes_per_cluster(4096),
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
        .write("paperpaw.ini", b"[help]\nshow = false\n[core]\nhyphenate = off\n")
        .unwrap();
    for n in 0..6 {
        let mut b = ALICE.to_vec();
        let end = b.len();
        b[end - 2..].copy_from_slice(&(n as u16).to_le_bytes());
        b.extend(std::iter::repeat_n(b'x', n));
        storage.write(&format!("books/b{n:03}.epub"), &b).unwrap();
    }
    if last {
        storage
            .write(".paperpaw/plugins/reader/state.ini", b"[reader]\nlast = books/b003.epub\n")
            .unwrap();
    }
    storage.sync().unwrap();
    (storage, ram.sent)
}

fn step(device: &mut Device, sent: &Rc<Cell<Commands>>, label: &str, steps: &[&str]) {
    sent.set(Commands::default());
    paperpaw_core::native_host::host_slot::slot_counts::reset();
    paperpaw_emulator::trace::take_log();
    for s in steps {
        if *s == "START" {
            device.start();
            continue;
        }
        run_script(device, s).unwrap();
    }
    let c = sent.replace(Commands::default());
    let counts = paperpaw_core::native_host::host_slot::slot_counts::snapshot();
    let total: u32 = counts.iter().map(|(_, n)| *n).sum();
    let top: Vec<_> = {
        let mut v = counts.clone();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        v.into_iter().take(8).collect()
    };
    eprintln!("== {label}: card r{} ({} blk) w{} ({} blk) ; slots {total} {top:?}", c.reads, c.read_blocks, c.writes, c.write_blocks);
    report(label, &paperpaw_emulator::trace::take_log());
}

fn mk(last: bool) -> (Device, Rc<Cell<Commands>>) {
    let (storage, sent) = card(last);
    paperpaw_emulator::budget::override_limit(None);
    let device = Device::on_storage(profiles::find("xteink-x4").unwrap(), storage, true).unwrap();
    let probe = sent.clone();
    paperpaw_emulator::trace::set_cost(Rc::new(move || {
        let c = probe.get();
        u64::from(c.reads + c.writes)
    }));
    paperpaw_emulator::trace::log();
    (device, sent)
}

#[test]
fn scratch_profile() {
    let (mut device, sent) = mk(false);
    step(&mut device, &sent, "start-home", &["START", "wait:3000"]);
    step(&mut device, &sent, "home-right", &["right", "wait:500"]);
    step(&mut device, &sent, "home-right2", &["right", "wait:500"]);
    step(&mut device, &sent, "home-left", &["left", "wait:500"]);
    let (mut device, sent) = mk(true);
    step(&mut device, &sent, "start-book", &["START", "wait:6000"]);
    step(&mut device, &sent, "turn1", &["page-forward", "wait:500"]);
    step(&mut device, &sent, "turn2", &["page-forward", "wait:500"]);
    step(&mut device, &sent, "turn3", &["page-forward", "wait:500"]);
    step(&mut device, &sent, "back-home-cold", &["back", "wait:3000"]);
    step(&mut device, &sent, "home-right", &["right", "wait:500"]);
    step(&mut device, &sent, "confirm-open", &["confirm", "wait:5000"]);
    step(&mut device, &sent, "back-home-warm", &["back", "wait:3000"]);
}
