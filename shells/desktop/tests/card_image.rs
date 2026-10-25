//! The X4 on a card image, its file system mounted as on the device: an
//! exFAT card (as most cards of 64 GB and more are sold) and a FAT one,
//! started, its library scanned, a book read and its place kept, then
//! started again. The heap the storage takes counts against the X4's
//! budget, and stays well within it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::sdk::builtin::actions;
use paperpaw_emulator::budget;
use paperpaw_emulator::card_image::{self, ImageBlocks};
use paperpaw_emulator::{device::Device, profiles};
use paperpaw_platform::{ButtonState, InputEvent, buttons};

const BOOK: &str = "books/alice-in-wonderland.txt";

/// An exFAT card of the storage's tests (`mkfs.exfat`; filled by
/// exfat-fuse: books, a CrossPoint cache), kept sparse.
fn exfat_image(filled: bool) -> Vec<u8> {
    let sparse: &[u8] = if filled {
        include_bytes!("../../../crates/paperpaw-fat/tests/images/filled.sparse")
    } else {
        include_bytes!("../../../crates/paperpaw-fat/tests/images/small.sparse")
    };
    let size = u64::from_le_bytes(sparse[8..16].try_into().unwrap()) as usize;
    let mut bytes = vec![0u8; size];
    for chunk in sparse[16..].chunks(516) {
        let index = u32::from_le_bytes(chunk[..4].try_into().unwrap()) as usize;
        bytes[index * 512..(index + 1) * 512].copy_from_slice(&chunk[4..]);
    }
    bytes
}

/// A FAT card of 8 MB, formatted.
fn fat_image() -> Vec<u8> {
    use paperpaw_fat::fatfs::{FormatVolumeOptions, Write};
    let bytes = Rc::new(RefCell::new(vec![0u8; 8 << 20]));
    let mut disk = paperpaw_fat::BlockDisk::new(ImageBlocks::Memory(bytes.clone())).unwrap();
    paperpaw_fat::fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
    disk.flush().unwrap();
    drop(disk);
    bytes.borrow().clone()
}

/// `image` with a book, past the first-run setup and its hints.
fn prepared(image: Vec<u8>) -> ImageBlocks {
    let blocks = ImageBlocks::Memory(Rc::new(RefCell::new(image)));
    let storage = card_image::mount(blocks.clone()).unwrap();
    let alice = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, alice.as_bytes()).unwrap();
    storage.write(".paperpaw/plugins/setup/done", b"").unwrap();
    storage
        .write("paperpaw.ini", b"[help]\nshow = false\n")
        .unwrap();
    storage.sync().unwrap();
    blocks
}

fn press(device: &mut Device, name: &'static str) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        device.input(InputEvent::Button { name, state });
        device.advance(100);
        device.tick_if_due();
    }
    device.advance(400);
    device.tick_if_due();
}

/// Lets `ms` pass, doing what is due.
fn idle(device: &mut Device, ms: u64) {
    for _ in 0..ms / 100 {
        device.advance(100);
        device.tick_if_due();
    }
}

fn screen(device: &mut Device) -> String {
    let mut screen = String::new();
    device.call(|kernel, _| {
        screen = kernel
            .active_screen()
            .map(ToString::to_string)
            .unwrap_or_default();
        paperpaw_core::Effects::default()
    });
    screen
}

/// The peaks of the heap, in KiB: started, the library scanned, a book
/// read (its place kept, found again after a restart).
fn run(blocks: ImageBlocks) -> [usize; 3] {
    let profile = profiles::find("xteink-x4").unwrap();
    let limit = profile.heap_budget.unwrap();
    // What earlier cards left (a FAT file system lives for ever, as on a
    // device) is not this card's.
    let before = budget::stats().live;
    budget::reset_peak();
    let mut device = Device::on_card_image(profile, None, blocks.clone(), true, None).unwrap();
    device.start();
    idle(&mut device, 3000);
    let started = budget::stats().peak - before;
    assert_eq!(screen(&mut device), "home.dashboard");

    budget::reset_peak();
    device.run_action(actions::LIBRARY);
    idle(&mut device, 5000);
    let scanned = budget::stats().peak - before;
    assert_eq!(screen(&mut device), "files.browser");
    let mut shown = String::new();
    device.call(|kernel, _| {
        for part in kernel.describe() {
            shown.push_str(&part.label);
            shown.push('\n');
        }
        paperpaw_core::Effects::default()
    });
    assert!(shown.contains("Alice"), "{shown}");

    budget::reset_peak();
    press(&mut device, buttons::CONFIRM);
    idle(&mut device, 3000);
    assert_eq!(screen(&mut device), "reader.screen");
    for _ in 0..5 {
        press(&mut device, buttons::PAGE_FORWARD);
    }
    device.shut_down();
    let read = budget::stats().peak - before;
    assert_eq!(budget::stats().failed, 0, "an allocation was refused");
    for peak in [started, scanned, read] {
        assert!(peak * 100 <= limit * 85, "peak {peak} of {limit}");
    }

    // Started again: the book where it was left.
    drop(device);
    let mut device = Device::on_card_image(profile, None, blocks, true, None).unwrap();
    device.start();
    idle(&mut device, 3000);
    let state = {
        let storage = card_image::mount(device_blocks(&device)).ok();
        storage.and_then(|s| {
            paperpaw_core::sdk::read_file(&*s, ".paperpaw/plugins/reader/state.ini", 4096).ok()
        })
    };
    let state = String::from_utf8(state.unwrap_or_default()).unwrap_or_default();
    // The book opened first (the card may hold others besides).
    assert!(state.contains("last = books/"), "{state}");
    [started / 1024, scanned / 1024, read / 1024]
}

/// The card image of `device`.
fn device_blocks(device: &Device) -> ImageBlocks {
    match device.library() {
        paperpaw_emulator::device::Library::Image(blocks) => blocks.clone(),
        other => panic!("{other:?}"),
    }
}

/// One after the other (the budget counts every thread's allocations
/// at once).
#[test]
fn the_x4_runs_on_exfat_and_fat_cards_within_its_heap() {
    for (name, image) in [
        ("exFAT", exfat_image(false)),
        ("exFAT as CrossPoint left it", exfat_image(true)),
        ("FAT", fat_image()),
    ] {
        let [started, scanned, read] = run(prepared(image));
        println!("{name}: started {started} KiB, scanned {scanned} KiB, read {read} KiB");
    }
}
