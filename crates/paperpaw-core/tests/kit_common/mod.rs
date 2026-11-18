//! What the kit's tests with an allocator of their own share: a kernel with
//! nothing registered over a storage the test gives (#300).

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use paperpaw_core::{Kernel, Place};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Formats, InputCaps, Instant, MemoryClass, PixelFormat, Storage,
};

pub const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(
        480,
        800,
        220,
        PixelFormat::Gray4,
        paperpaw_platform::Rotation::Deg0,
        None,
    ),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

/// [`DEVICE`] of little memory, as the X4.
pub const SMALL_DEVICE: DeviceInfo = DEVICE.with_memory(MemoryClass::Small);

/// A kernel with nothing registered but the formats a test gives.
pub struct Harness {
    pub formats: Formats,
}

impl Harness {
    pub fn new() -> Self {
        Harness {
            formats: Formats::new(),
        }
    }

    /// Runs `f` with the kernel's state over `storage` as a handler sees it.
    pub fn with_env_on<R>(
        self,
        storage: impl Storage + 'static,
        f: impl FnOnce(&Place<'_>) -> R,
    ) -> R {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .build_bare()
            .unwrap();
        *kernel.formats_mut() = self.formats;
        kernel.with_place(Instant(0), f)
    }
}
