//! What the kit's tests that were the SDK's share: a kernel with nothing
//! registered, a storage in memory and a clock (#300).

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

extern crate std;

use alloc::rc::Rc;
use core::ops::{Deref, DerefMut};

use crate::{Kernel, Place, Shared};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
    Storage, StorageError, WallClock,
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

/// A storage in memory the harness and its kernel share.
#[derive(Clone, Default)]
pub struct Card(pub Rc<MemoryStorage>);

impl Deref for Card {
    type Target = MemoryStorage;
    fn deref(&self) -> &MemoryStorage {
        &self.0
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        self.0.write_at(path, offset, data)
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.0.truncate(path, len)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.0.entry_identity(path)
    }
}

/// A kernel with nothing registered: tests change its state (through
/// `Deref` to the kernel's shared state) before asking for a [`Place`].
pub(crate) struct Harness {
    kernel: Kernel,
    /// The kernel's storage, as the test sees it.
    pub storage: Card,
}

impl Deref for Harness {
    type Target = Shared;
    fn deref(&self) -> &Shared {
        &self.kernel.shared
    }
}

impl DerefMut for Harness {
    fn deref_mut(&mut self) -> &mut Shared {
        &mut self.kernel.shared
    }
}

impl Harness {
    /// Nothing registered, no clock.
    pub fn new() -> Self {
        Self::on(DEVICE)
    }

    /// [`Harness::new`] on `device`.
    pub fn on(device: DeviceInfo) -> Self {
        let mut kernel = Kernel::builder(device)
            .glyph_cache(16 * 1024)
            .build_bare()
            .unwrap();
        let storage = Card::default();
        kernel.shared.storage = Rc::new(storage.clone());
        kernel.shared.now = Instant(0);
        Harness { kernel, storage }
    }

    /// The device knows the time: `unix_millis` at the harness's `now`,
    /// the system's zone `utc_offset_minutes` east of UTC.
    pub fn with_clock(mut self, unix_millis: i64, utc_offset_minutes: Option<i32>) -> Self {
        let now = self.now;
        self.status.clock = Some(WallClock::new(now, unix_millis, utc_offset_minutes));
        self
    }

    /// Its storage from now on.
    pub fn set_storage(&mut self, storage: MemoryStorage) {
        self.storage = Card(Rc::new(storage));
        self.kernel.shared.storage = Rc::new(self.storage.clone());
    }

    /// The kernel's state as a handler sees it now.
    pub fn env(&self) -> Place<'_> {
        self.kernel.shared.place(self.kernel.shared.now)
    }

    /// [`Harness::env`] over another storage (one that fails, say), which
    /// stays the kernel's until another is set.
    pub fn env_on(&mut self, storage: Rc<dyn Storage>) -> Place<'_> {
        self.kernel.shared.storage = storage;
        self.env()
    }
}
