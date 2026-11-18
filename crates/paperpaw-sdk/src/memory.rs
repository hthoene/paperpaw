//! How much memory is free now (`Place::memory`), where the shell can
//! tell: for diagnostics, and for plugins that check their room before
//! large work. A device's memory is mostly its heap; on small devices the
//! stack's depth matters as well, as running over it is fatal.
//!
//! Each shell gives the kernel its probe (`KernelBuilder::memory`): the
//! allocator's numbers on the X4 and the emulator's budget on desktops.
//! Where a shell gives none, nothing is known.
//!

use core::fmt;

/// Memory in use and free, in bytes, at the moment it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MemoryUse {
    /// Bytes of the heap free now.
    pub heap_free: u32,
    /// The heap's size.
    pub heap_size: u32,
    /// Bytes of the stack never used since start (its depth left at its
    /// deepest), where the shell measures it.
    pub stack_free: Option<u32>,
    /// The stack's size, where the shell knows it.
    pub stack_size: Option<u32>,
    /// The largest block the heap could give now, where the shell tells:
    /// less than [`MemoryUse::heap_free`] where the free memory is in
    /// pieces. What a large buffer can have at most.
    pub largest_free: Option<u32>,
    /// The heap's regions where it has more than one (the X4 Pro's
    /// internal RAM and PSRAM), the first ones; the rest `None`.
    pub regions: [Option<MemoryRegion>; 3],
}

/// A region of the heap, e.g. a device's internal RAM or its PSRAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MemoryRegion {
    /// Bytes of it free now.
    pub free: u32,
    pub size: u32,
    /// Memory outside the chip (PSRAM): larger, slower, not for
    /// everything.
    pub external: bool,
}

impl MemoryRegion {
    /// `free` bytes of `size`, of internal RAM.
    pub const fn new(free: u32, size: u32) -> Self {
        Self {
            free,
            size,
            external: false,
        }
    }

    /// Of memory outside the chip (PSRAM).
    pub const fn external(mut self) -> Self {
        self.external = true;
        self
    }
}

impl MemoryUse {
    /// The heap's free bytes and size; the stack not measured.
    pub const fn new(heap_free: u32, heap_size: u32) -> Self {
        Self {
            heap_free,
            heap_size,
            stack_free: None,
            stack_size: None,
            largest_free: None,
            regions: [None; 3],
        }
    }

    /// With the stack measured: `free` bytes never used of `size`.
    pub const fn with_stack(mut self, free: u32, size: u32) -> Self {
        self.stack_free = Some(free);
        self.stack_size = Some(size);
        self
    }

    /// With the largest block the heap could give now.
    pub const fn with_largest_free(mut self, bytes: u32) -> Self {
        self.largest_free = Some(bytes);
        self
    }

    /// With `region` after those given before (three at most; more are
    /// left out).
    pub fn with_region(mut self, region: MemoryRegion) -> Self {
        if let Some(slot) = self.regions.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(region);
        }
        self
    }
}

/// The shell's probe of memory; see the [module documentation](self).
#[derive(Clone, Copy, Default)]
pub struct Memory {
    read: Option<fn() -> Option<MemoryUse>>,
}

impl Memory {
    /// No probe: nothing is known.
    pub const NONE: Self = Self { read: None };

    /// The probe `read`, which says what is free now, or `None` where it
    /// cannot tell now (for shells).
    pub const fn new(read: fn() -> Option<MemoryUse>) -> Self {
        Self { read: Some(read) }
    }

    /// What is free now; `None` where the shell cannot tell. Cheap, but
    /// not free: a shell may walk the stack to measure its depth.
    pub fn read(&self) -> Option<MemoryUse> {
        self.read.and_then(|read| read())
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memory")
            .field("available", &self.read.is_some())
            .finish()
    }
}

/// Where data is kept, for what reads it all the time and what is large
/// ([`vec_in`]). Devices with one kind of memory keep both alike. Places
/// are constants of one type, so more can come without a new plugin
/// interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Placement(u32);

impl Placement {
    /// Read and written all the time, by a loop that runs every frame:
    /// internal RAM where the device has more than one kind (the X4 Pro).
    pub const FAST: Self = Self(1);
    /// Much data, read now and then (a whole file kept): PSRAM where the
    /// device has it, so internal RAM stays for the rest.
    pub const LARGE: Self = Self(2);

    /// Its number in the firmware's table (`Api::alloc_in`).
    pub(crate) const fn code(self) -> u32 {
        self.0
    }

    /// The placement of number `code`, `None` for 0 (anywhere).
    pub(crate) const fn of(code: u32) -> Option<Self> {
        match code {
            0 => None,
            code => Some(Self(code)),
        }
    }
}

/// Why memory could not be had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemoryError {
    /// No room for `bytes` where asked.
    Full { bytes: usize },
    /// More than an address can count.
    TooLarge,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::Full { bytes } => write!(f, "no room for {bytes} bytes"),
            MemoryError::TooLarge => write!(f, "more memory than can be counted"),
            MemoryError::Other(code) => write!(f, "memory error {code}"),
        }
    }
}

impl core::error::Error for MemoryError {}

/// Allocates by placement, as the firmware's table's `alloc_in`: size,
/// alignment, the [`Placement`]'s number; null when there is no room.
pub type Placer = unsafe extern "C" fn(size: usize, align: usize, placement: u32) -> *mut u8;

/// The firmware's allocation by placement, where it gave one. Not in a
/// code plugin file, which has no writable data: it reads the
/// firmware's table instead ([`placer`]).
#[cfg(not(all(paperpaw_plugin, target_os = "none")))]
static PLACER: core::sync::atomic::AtomicPtr<()> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// Gives the SDK the firmware's allocation by placement, at start.
#[cfg(feature = "kernel")]
pub fn set_placer(placer: Placer) {
    PLACER.store(placer as *mut (), core::sync::atomic::Ordering::Release);
}

/// The firmware's allocation by placement, where there is one.
#[cfg(not(all(paperpaw_plugin, target_os = "none")))]
fn placer() -> Option<Placer> {
    let placer = PLACER.load(core::sync::atomic::Ordering::Acquire);
    // SAFETY: only `set_placer` stores here, a `Placer`.
    (!placer.is_null()).then(|| unsafe { core::mem::transmute::<*mut (), Placer>(placer) })
}

/// The firmware's allocation by placement, from its table, where the
/// firmware that loaded this plugin file has it.
#[cfg(all(paperpaw_plugin, target_os = "none"))]
fn placer() -> Option<Placer> {
    unsafe extern "C" {
        /// The firmware's table: `paperpaw-plugin.x` makes the word, the
        /// loader writes its address before the plugin's code runs.
        static PAPERPAW_API: *const paperpaw_native::Api;
    }
    // SAFETY: a table that lives as long as the firmware runs, written
    // before any code of the plugin ran.
    let api = unsafe { &*PAPERPAW_API };
    let has = core::mem::offset_of!(paperpaw_native::Api, alloc_in) + core::mem::size_of::<usize>()
        <= api.size as usize;
    has.then_some(api.alloc_in)
}

/// An empty vector with room for `capacity` values, kept where
/// `placement` says (where the device has more than one kind of memory;
/// elsewhere as `Vec::with_capacity`). It grows and is dropped as any
/// vector, wherever it went: keep it from growing (room for all of it at
/// once) to keep it where it is.
pub fn vec_in<T>(placement: Placement, capacity: usize) -> Result<alloc::vec::Vec<T>, MemoryError> {
    let layout = core::alloc::Layout::array::<T>(capacity).map_err(|_| MemoryError::TooLarge)?;
    let placer = placer().filter(|_| layout.size() > 0);
    let Some(placer) = placer else {
        let mut vec = alloc::vec::Vec::new();
        vec.try_reserve_exact(capacity)
            .map_err(|_| MemoryError::Full {
                bytes: layout.size(),
            })?;
        return Ok(vec);
    };
    // SAFETY: a layout of non-zero size, as the firmware's heap takes it.
    let ptr = unsafe { placer(layout.size(), layout.align(), placement.code()) };
    if ptr.is_null() {
        return Err(MemoryError::Full {
            bytes: layout.size(),
        });
    }
    // SAFETY: allocated by the global allocator's heap (the firmware's,
    // which frees blocks of any of its regions) for `capacity` values of
    // `T`, none of them set yet.
    Ok(unsafe { alloc::vec::Vec::from_raw_parts(ptr.cast::<T>(), 0, capacity) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cause_of_a_later_build_says_its_code() {
        assert_eq!(
            alloc::format!("{}", MemoryError::Other(7)),
            "memory error 7"
        );
    }

    #[test]
    fn vectors_go_anywhere_without_a_placer() {
        let mut vec = vec_in::<u32>(Placement::LARGE, 100).unwrap();
        assert!(vec.capacity() >= 100 && vec.is_empty());
        vec.push(7);
        assert_eq!(vec_in::<u8>(Placement::FAST, 0).unwrap().capacity(), 0);
        assert_eq!(
            vec_in::<u64>(Placement::FAST, usize::MAX),
            Err(MemoryError::TooLarge)
        );
    }

    #[test]
    fn a_probe_is_read_when_asked() {
        fn probe() -> Option<MemoryUse> {
            Some(MemoryUse::new(10, 20))
        }
        assert_eq!(Memory::NONE.read(), None);
        assert_eq!(Memory::new(probe).read().map(|m| m.heap_free), Some(10));
    }

    #[test]
    fn regions_go_in_order_three_at_most() {
        let used = MemoryUse::new(10, 20)
            .with_largest_free(8)
            .with_region(MemoryRegion::new(4, 8))
            .with_region(MemoryRegion::new(6, 12).external());
        assert_eq!(used.largest_free, Some(8));
        assert_eq!(
            used.regions,
            [
                Some(MemoryRegion::new(4, 8)),
                Some(MemoryRegion::new(6, 12).external()),
                None
            ]
        );
        let full = (0..4).fold(used, |u, i| u.with_region(MemoryRegion::new(i, 9)));
        assert_eq!(full.regions[2], Some(MemoryRegion::new(0, 9)));
    }
}
