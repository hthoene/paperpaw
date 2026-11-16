//! The stack depth of a code path, measured in a host test and held against
//! the X4's main stack (#256).
//!
//! The emulator and the host tests run on stacks of megabytes, so a path that
//! overflows the device's (the first test image's boot loop, #2556) passed
//! every test. Here a route runs on a thread with the **device's** stack
//! ([`on_device_stack`]: a real guard page, an overflow aborts the test), and a
//! [`Meter`] placed at the route's leaves (a block read, a storage call) reads
//! how deep the route went. A route has a [`Route::ceiling`]: the measured
//! depth plus a little room, so a change that makes the path deeper fails
//! the test and its author looks (the frame budgets of
//! `shells/xteink/frame-budget*.txt` work the same way for the static
//! frames). The ceiling may be raised, in a commit that says why.
//!
//! This is an approximation. The host's frames are not the device's: the
//! tests run unoptimised-ish (`opt-level 1`) with 64-bit pointers, the device
//! is `opt-level s` with 32-bit ones, and the host frames are **assumed to be
//! the larger** (the numbers here are then a ceiling for the device's, never a
//! floor). Calls through function pointers are followed here (the host runs
//! them), which the static scans (`tools/check-x4-start-stack.py`,
//! `paperpaw-sign frames-s3`) cannot do; what the scans see of the main loop
//! above the route is theirs.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test support: a card or thread that cannot be made, or a route over its bound, fails the test"
)]

use std::cell::Cell;
use std::hint::black_box;

/// The least main stack the X4 family links with (`STACK_FLOOR` of
/// `paperpaw_x4::s3_band`, the `ppn5` floor of `shells/xteink/build.rs`).
pub const STACK_FLOOR: usize = 50 * 1024;

/// A device's main stack as a route may use it: what lies above the band at
/// the stack's end (`paperpaw_x4::stack_band`, `s3_band`) and the margin the
/// start-stack scan keeps (`--margin 2048`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Device {
    /// For messages.
    pub name: &'static str,
    /// The bytes of the main stack a route may use.
    pub usable: usize,
}

/// The margin the start-stack scan keeps above the band (CI: `--margin 2048`).
pub const MARGIN: usize = 2048;

/// The X4 (ESP32-C3): the floor less the reserve and band at the stack's end
/// (the band's alignment may take one more band) and the margin.
pub const X4: Device = Device {
    name: "x4",
    usable: STACK_FLOOR - (2048 + 2048) - 2048 - 2047 - MARGIN,
};

/// The X4 Pro and X4 Classic (ESP32-S3): six frame windows of 2 KiB and the
/// deepest handler below the band, then the band, its alignment and the margin.
pub const S3: Device = Device {
    name: "x4-pro/x4-classic",
    usable: STACK_FLOOR - (6 * 2048 + 2048) - 2048 - 15 - MARGIN,
};

/// Every device a route is held to.
pub const DEVICES: [Device; 2] = [X4, S3];

/// Where the stack was when a route began, and the deepest it was seen since.
#[derive(Debug, Default)]
pub struct Meter {
    base: Cell<usize>,
    deepest: Cell<usize>,
}

/// The address of a local of this frame: where the stack pointer is.
#[inline(never)]
fn here() -> usize {
    let marker = 0u8;
    black_box(&marker) as *const u8 as usize
}

impl Meter {
    /// A meter, not started.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The route begins here: depth is counted from this frame.
    #[inline(never)]
    pub fn start(&self) {
        self.base.set(here());
        self.deepest.set(0);
    }

    /// A leaf of the route: notes how far below [`Meter::start`] the stack is.
    #[inline(never)]
    pub fn mark(&self) {
        let depth = self.base.get().saturating_sub(here());
        if depth > self.deepest.get() {
            self.deepest.set(depth);
        }
    }

    /// The deepest mark since the start, in bytes.
    #[must_use]
    pub fn deepest(&self) -> usize {
        self.deepest.get()
    }
}

/// Runs `body` on a thread with the stack of `device`: past it the thread
/// overflows its guard page and the test process aborts.
///
/// # Panics
/// If the thread cannot be made, or `body` panics.
pub fn on_device_stack<R: Send>(device: Device, body: impl FnOnce() -> R + Send) -> R {
    on_device_stack_plus(device, 0, body)
}

/// [`on_device_stack`] with `harness` bytes more, for a test whose set-up runs on
/// the same thread before the route begins (a kernel built for the call): the
/// [`Meter`] still holds the route itself to the device's stack, the guard page
/// is only where a runaway recursion stops.
///
/// # Panics
/// If the thread cannot be made, or `body` panics.
pub fn on_device_stack_plus<R: Send>(
    device: Device,
    harness: usize,
    body: impl FnOnce() -> R + Send,
) -> R {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name(format!("stack-{}", device.name))
            .stack_size(device.usable + harness)
            .spawn_scoped(scope, body)
            .expect("a thread with the device's stack")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// A route over the limit: what it reached and what it may.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overrun {
    /// The route's name.
    pub route: &'static str,
    /// The deepest mark, bytes.
    pub depth: usize,
    /// The bound it broke, bytes.
    pub bound: usize,
    /// Which bound: the route's own ceiling, or a device's stack.
    pub against: &'static str,
}

impl core::fmt::Display for Overrun {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "route `{}` went {} B deep: over {} ({} B); a change made it deeper \
             (a large local moved or copied by value is the usual cause), or the \
             ceiling needs raising in a commit that says why",
            self.route, self.depth, self.against, self.bound
        )
    }
}

/// A route of the device's main stack and how deep it may go (host bytes).
#[derive(Debug, Clone, Copy)]
pub struct Route {
    /// For messages.
    pub name: &'static str,
    /// The depth that may not be exceeded: measured, plus room.
    pub ceiling: usize,
}

impl Route {
    /// Whether `meter`'s deepest mark is within the ceiling and every device's stack.
    ///
    /// # Errors
    /// The first bound broken.
    pub fn verdict(&self, meter: &Meter) -> Result<usize, Overrun> {
        self.verdict_of(meter.deepest())
    }

    /// [`Route::verdict`] of a depth read earlier.
    ///
    /// # Errors
    /// The first bound broken.
    pub fn verdict_of(&self, depth: usize) -> Result<usize, Overrun> {
        if depth > self.ceiling {
            return Err(Overrun {
                route: self.name,
                depth,
                bound: self.ceiling,
                against: "the route's ceiling",
            });
        }
        for device in DEVICES {
            if depth > device.usable {
                return Err(Overrun {
                    route: self.name,
                    depth,
                    bound: device.usable,
                    against: device.name,
                });
            }
        }
        Ok(depth)
    }

    /// [`Route::verdict`], and a panic naming the route where it is over.
    ///
    /// # Panics
    /// Where the route is over a bound.
    pub fn assert_within(&self, meter: &Meter) -> usize {
        self.assert_within_depth(meter.deepest())
    }

    /// [`Route::assert_within`] of a depth read earlier.
    ///
    /// # Panics
    /// Where the route is over a bound.
    pub fn assert_within_depth(&self, depth: usize) -> usize {
        match self.verdict_of(depth) {
            Ok(depth) => depth,
            Err(over) => panic!("{over}"),
        }
    }
}

/// Takes at least `bytes` of stack below the caller while `leaf` runs: a route
/// made deeper on purpose, for the test that proves the check fails.
pub fn deepen(bytes: usize, leaf: &mut dyn FnMut()) {
    const STEP: usize = 512;
    #[inline(never)]
    fn eat(left: usize, leaf: &mut dyn FnMut()) {
        let pad = [left as u8; STEP];
        black_box(&pad);
        if left > STEP {
            eat(left - STEP, leaf);
        } else {
            leaf();
        }
        black_box(&pad);
    }
    if bytes == 0 {
        leaf();
    } else {
        eat(bytes, leaf);
    }
}

/// A FAT card in memory, as the X4 has one, whose every block access is a leaf
/// of the route ([`Meter::mark`]): the FAT's deepest frames end in one.
pub mod card {
    use std::cell::RefCell;
    use std::rc::Rc;

    use paperpaw_fat::fatfs::{FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write};
    use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk};

    use super::Meter;

    /// The blocks of the card, marked on the meter.
    #[derive(Clone, Debug)]
    pub struct Marked {
        bytes: Rc<RefCell<Vec<u8>>>,
        meter: Rc<Meter>,
    }

    impl Blocks for Marked {
        fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
            self.meter.mark();
            let at = index as usize * BLOCK;
            block.copy_from_slice(self.bytes.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
            Ok(())
        }

        fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
            self.meter.mark();
            let at = index as usize * BLOCK;
            self.bytes
                .borrow_mut()
                .get_mut(at..at + BLOCK)
                .ok_or(DiskError)?
                .copy_from_slice(block);
            Ok(())
        }

        fn blocks(&mut self) -> Result<u32, DiskError> {
            Ok((self.bytes.borrow().len() / BLOCK) as u32)
        }
    }

    /// The storage on the card.
    pub type Fat = FatStorage<SharedDisk<Marked>>;

    /// A formatted 8 MB card and the storage on it (leaked: the file system
    /// lives for ever, as on the device).
    ///
    /// # Panics
    /// If the card cannot be formatted.
    #[must_use]
    pub fn fat(meter: &Rc<Meter>) -> &'static Fat {
        let card = Marked {
            bytes: Rc::new(RefCell::new(vec![0u8; 8 << 20])),
            meter: Rc::clone(meter),
        };
        let mut disk = BlockDisk::new(card).expect("a disk");
        paperpaw_fat::fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).expect("format");
        disk.flush().expect("flush");
        disk.seek(SeekFrom::Start(0)).expect("seek");
        let shared = SharedDisk::new(disk);
        let fs = FileSystem::new(shared.clone(), FsOptions::new()).expect("mount");
        Box::leak(Box::new(
            FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared),
        ))
    }
}
