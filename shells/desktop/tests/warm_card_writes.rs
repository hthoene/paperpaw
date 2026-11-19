//! What a second start, an opening and a Back to Home write to the card
//! (#389). On the X4 a write of a file costs 17 to 36 card commands of 5.6 ms
//! (a temporary file and two renames), and start, opening and Back rewrote
//! files exactly as they were (the book list, the page list, the reading
//! widget's file). The FAT layer leaves a small file that already holds what
//! is written alone (`paperpaw-fat`). This mounts the X4's FAT file system on
//! a RAM disk that counts the commands it is sent.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use paperpaw_emulator::{device::Device, headless::run_script, profiles};
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk,
    fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write},
};
use paperpaw_platform::Storage;

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
            b"[help]\nshow = false\n[core]\nhyphenate = off\n",
        )
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
            .write(
                ".paperpaw/plugins/reader/state.ini",
                b"[reader]\nlast = books/b003.epub\n",
            )
            .unwrap();
    }
    storage.sync().unwrap();
    (storage, ram.sent)
}

fn mk(last: bool) -> (Device, Rc<Cell<Commands>>) {
    let (storage, sent) = card(last);
    paperpaw_emulator::budget::override_limit(None);
    {
        let mut first =
            Device::on_storage(profiles::find("xteink-x4").unwrap(), storage.clone(), true)
                .unwrap();
        first.start();
        run_script(&mut first, "wait:6000").unwrap();
        if last {
            for _ in 0..3 {
                run_script(&mut first, "page-forward").unwrap();
                run_script(&mut first, "wait:500").unwrap();
            }
            run_script(&mut first, "back").unwrap();
            run_script(&mut first, "wait:5000").unwrap();
        }
    }
    let device = Device::on_storage(profiles::find("xteink-x4").unwrap(), storage, true).unwrap();
    (device, sent)
}

/// Write commands the card is sent for `steps` on a device.
fn writes_of(device: &mut Device, sent: &Rc<Cell<Commands>>, steps: &[&str]) -> u32 {
    sent.set(Commands::default());
    for step in steps {
        if *step == "START" {
            device.start();
        } else {
            run_script(device, step).unwrap();
        }
    }
    sent.replace(Commands::default()).writes
}

#[test]
fn a_started_card_is_not_written_with_what_it_already_holds() {
    let (mut device, sent) = mk(true);
    let start = writes_of(&mut device, &sent, &["START", "wait:6000"]);
    let back = writes_of(&mut device, &sent, &["back", "wait:3000"]);
    let again = writes_of(
        &mut device,
        &sent,
        &["confirm", "wait:5000", "back", "wait:3000"],
    );
    eprintln!("write commands: start {start}, back {back}, open and back {again}");
    // Measured before the change: 30, 51 and 44 commands (a restart of a
    // card that had all of it written once); 6, 51 and 8 after. The first
    // Back after a start lets the library walk write its list again.
    assert!(start <= 12, "the start sent {start} write commands");
    assert!(back <= 60, "Back sent {back} write commands");
    assert!(
        again <= 14,
        "opening a book and Back sent {again} write commands"
    );
}
