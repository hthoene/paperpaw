//! Where the blocks of the X4's heap lie once Home is shown (#308). The
//! emulator's budget counts bytes; it does not say where they lie, and its
//! memory probe tells no largest block, so the kernel never takes a frame's
//! taller band here (`paperpaw-core`'s `tall_band`) as it does on the device.
//! On the X4 that band cost 35 KB of the largest block: 75 KB before, 52 KB
//! after a start whose allocations fell a little differently (reader 1.0.19),
//! under the 61 KB a taller band needs, so every frame of more than two bands
//! was drawn in all its bands for the rest of the run.
//!
//! This test lets the probe tell a largest block, keeps every allocation the
//! kernel and its plugins make from the start to Home and a few moves on it,
//! and replays them through the X4's kind of heap: the first block that fits,
//! lowest address first, and a block moved to be resized (esp-alloc's default
//! heap has no resize of its own). The sizes are the emulator's (pointers of 8
//! bytes), so the numbers are not the device's; where a hole is left is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;
use std::rc::Rc;

use paperpaw_core::sdk::{MemoryStorage, Storage};
use paperpaw_emulator::{budget, device::Device, headless::run_script, profiles};

const ALICE: &str = "books/alice-in-wonderland.epub";
const OTHER: &str = "books/die-verwandlung.epub";

/// What a block costs beside its bytes, and what its size is a multiple of.
const HEADER: usize = 8;
/// A row of the X4's frame in bytes, its usual band and one of half the screen.
const STRIDE: usize = 480 / 8;
const BAND: usize = 192 * STRIDE;
const HALF: usize = 400 * STRIDE;

/// A heap that gives the first block that fits, lowest address first, and
/// joins what is given back with the free blocks beside it.
#[derive(Default)]
struct FirstFit {
    /// The free blocks below `top`: where each starts and its length.
    holes: BTreeMap<usize, usize>,
    /// Where the memory never used yet starts.
    top: usize,
    /// The blocks in use by the address the emulator gave them: where each
    /// lies here and its length.
    live: BTreeMap<usize, (usize, usize)>,
}

impl FirstFit {
    fn replay(&mut self, events: &[(bool, usize, usize)]) {
        for &(made, address, size) in events {
            if made {
                let need = (size + HEADER).div_ceil(8) * 8;
                let hole = self
                    .holes
                    .iter()
                    .find(|(_, len)| **len >= need)
                    .map(|(start, len)| (*start, *len));
                let start = match hole {
                    Some((start, len)) => {
                        self.holes.remove(&start);
                        if len > need {
                            self.holes.insert(start + need, len - need);
                        }
                        start
                    }
                    None => {
                        self.top += need;
                        self.top - need
                    }
                };
                self.live.insert(address, (start, need));
            } else if let Some((mut start, mut len)) = self.live.remove(&address) {
                if let Some((&before, &its)) = self.holes.range(..start).next_back()
                    && before + its == start
                {
                    self.holes.remove(&before);
                    start = before;
                    len += its;
                }
                if let Some(after) = self.holes.remove(&(start + len)) {
                    len += after;
                }
                if start + len == self.top {
                    self.top = start;
                } else {
                    self.holes.insert(start, len);
                }
            }
        }
    }

    /// The largest free block under what lies highest.
    fn largest_hole(&self) -> usize {
        self.holes.values().copied().max().unwrap_or(0)
    }
}

/// An X4's card after Kafka was read: Alice is the book it opens.
fn card() -> Rc<MemoryStorage> {
    let files = Rc::new(
        MemoryStorage::new()
            .with(
                ALICE,
                include_bytes!("../../../samples/library/books/alice-in-wonderland.epub")
                    .as_slice(),
            )
            .with(
                OTHER,
                include_bytes!("../../../samples/library/books/die-verwandlung.epub").as_slice(),
            )
            .with(".paperpaw/plugins/setup/done", "")
            .with(
                "paperpaw.ini",
                "[core]\nhyphenate = off\n[help]\nshow = false\n",
            ),
    );
    let last = |book: &str| {
        files
            .write(
                ".paperpaw/plugins/reader/state.ini",
                format!("[reader]\nlast = {book}\n").as_bytes(),
            )
            .unwrap();
    };
    last(OTHER);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, files.clone(), true, None).unwrap();
    device.start();
    run_script(&mut device, "wait:3000,long:back,wait:6000").unwrap();
    drop(device);
    last(ALICE);
    files
}

#[test]
fn a_taller_band_leaves_no_hole_under_the_band() {
    let profile = profiles::find("xteink-x4").unwrap();
    let budget_bytes = profile.heap_budget.unwrap();
    budget::tell_largest_free();
    let files = card();
    budget::record_events();
    let mut device = Device::in_memory(profile, None, files, true, None).unwrap();
    // The book it was left in, Back to Home, and the focus moved about.
    device.start();
    run_script(&mut device, "wait:3000,long:back,wait:6000").unwrap();
    run_script(&mut device, "right,wait:1500").unwrap();
    for step in ["right", "right", "right", "left", "left", "left", "left"] {
        run_script(&mut device, step).unwrap();
        run_script(&mut device, "wait:300").unwrap();
    }
    let events = budget::take_events();
    // The taller bands made for a frame, more than the usual band's 192 rows
    // of 60 bytes and half the screen's 400 at most: this test is about
    // them, so they must have been made.
    let taller = events
        .iter()
        .filter(|(made, _, size)| *made && *size > BAND && *size <= HALF && size % STRIDE == 0)
        .count();
    assert!(taller > 0, "no frame was drawn in a taller band");
    let mut heap = FirstFit::default();
    heap.replay(&events);
    let hole = heap.largest_hole();
    let block = budget_bytes.saturating_sub(heap.top);
    eprintln!(
        "X4 heap replayed first fit: {taller} taller bands, largest hole {hole} B, top {} B, \
         largest block {block} B of {budget_bytes}",
        heap.top
    );
    // 24,008 B before the taller band's block went first: a band of half
    // the screen, given back under the usual band that had come to lie
    // behind it. 5,496 B with it.
    assert!(
        hole < 16 * 1024,
        "a hole of {hole} B lies under the top of the heap ({} B)",
        heap.top
    );
    // 102,856 B before, 126,864 B now.
    assert!(
        block >= 110 * 1024,
        "the largest block is {block} B of {budget_bytes}"
    );
}
