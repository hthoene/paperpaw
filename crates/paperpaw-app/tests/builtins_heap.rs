//! The host's heap part of each statically linked built-in
//! (`abi/heap-word.md` section 9): what the kernel keeps for as long as the
//! plugin is registered (its owner, id, resources, row and declaration index),
//! not what its own `make` allocates (the frame owner pays that).
//!
//! Every plugin of `NATIVE_PLUGINS` is registered through
//! `Kernel::register_linked` with a counting allocator, so the measured set is
//! the linked set. The firmware passes the flash-mapped read-only data as the
//! image, so the built-ins' static declaration batches are kept as offsets;
//! here that range is this test binary's read-only mapping.
//!
//! The numbers are x86_64's. The same structures are never larger with 4-byte
//! pointers and alignment (the index fields are `u32` anyway), so they bound
//! the X4's. The budget is checked against their sum.
#![cfg(feature = "native-linked")]
#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test measures a foreign factory and panics when something is wrong"
)]

use core::cell::Cell;
use paperpaw_app::NATIVE_PLUGINS;
use paperpaw_core::native_host::{HostResources, flat::FlatDepth};
use paperpaw_native::host_glue::{PpEntry, Services};
use paperpaw_native::metadata::{Feature, Host, Profile, Service};
use paperpaw_platform::{DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation};
use std::alloc::{GlobalAlloc, Layout, System};

std::thread_local! {
    /// Bytes this thread holds from the heap now, outside the plugin's own.
    static LIVE: Cell<usize> = const { Cell::new(0) };
    /// The plugin's own allocations (its `Services::alloc`) are not counted.
    static SHIELD: Cell<bool> = const { Cell::new(false) };
}

struct Counting;

// SAFETY: forwards to the system allocator and only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, forwarded.
        let block = unsafe { System.alloc(layout) };
        if !block.is_null() && SHIELD.try_with(Cell::get) == Ok(false) {
            let _ = LIVE.try_with(|live| live.set(live.get() + layout.size()));
        }
        block
    }
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        if SHIELD.try_with(Cell::get) == Ok(false) {
            let _ = LIVE.try_with(|live| live.set(live.get().saturating_sub(layout.size())));
        }
        // SAFETY: allocated by `alloc` above with this layout.
        unsafe { System.dealloc(block, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The plugin's own heap: not the host's part, so not counted.
struct Own;
impl Services for Own {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        SHIELD.with(|shield| shield.set(true));
        // SAFETY: the host passes a valid nonzero layout.
        let block = unsafe { System.alloc(Layout::from_size_align(size, align).unwrap()) };
        SHIELD.with(|shield| shield.set(false));
        block
    }
    unsafe fn dealloc(&self, block: *mut u8, size: usize, align: usize) {
        // SAFETY: this heap's block, freed once.
        unsafe { System.dealloc(block, Layout::from_size_align(size, align).unwrap()) };
    }
    unsafe fn realloc(&self, block: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: as `dealloc`.
        unsafe { System.realloc(block, Layout::from_size_align(old, align).unwrap(), new) }
    }
    fn micros(&self) -> u64 {
        0
    }
}

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const SERVICES: &[&str] = &["core.device", "core.library", "core.registry", "core.ui"];
const FEATURES: &[Feature<'static>] = &[
    Feature {
        key: "uses.library",
        revision: 1,
    },
    Feature {
        key: "uses.power",
        revision: 1,
    },
];

/// The shell's card-plugin host, as the desktop shell builds it for the test.
fn shell_host(services: &'static [Service<'static>]) -> Host<'static> {
    Host {
        abi_major: 7,
        minor: 1,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: u16::try_from(size_of::<PpEntry>()).unwrap(),
        },
        services,
        features: FEATURES,
    }
}

/// The read-only mapping of this test binary that holds `probe`, found in
/// `/proc/self/maps`: what the X4 shell gets from the linker's bounds.
fn read_only_mapping(probe: *const u8) -> &'static [u8] {
    let at = probe as usize;
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    for line in maps.lines() {
        let mut fields = line.split_whitespace();
        let (range, perms) = (fields.next().unwrap(), fields.next().unwrap());
        let (start, end) = range.split_once('-').unwrap();
        let (start, end) = (
            usize::from_str_radix(start, 16).unwrap(),
            usize::from_str_radix(end, 16).unwrap(),
        );
        if (start..end).contains(&at) && perms.starts_with("r--") {
            // SAFETY: the mapping of this program's read-only data stays
            // mapped and unchanged for the whole run; staging only compares
            // addresses with it and never reads through it.
            return unsafe { core::slice::from_raw_parts(start as *const u8, end - start) };
        }
    }
    panic!("no read-only mapping holds {at:#x}");
}

/// The host heap bytes each built-in keeps, in registration order.
fn measure(with_image: bool) -> Vec<(&'static str, usize)> {
    let services: &'static [Service<'static>] = Box::leak(
        SERVICES
            .iter()
            .map(|key| Service {
                key,
                major: 1,
                semantic_revision: 0,
                min_table_bytes: 0,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let host = shell_host(services);
    let first = NATIVE_PLUGINS[0].1.entry();
    let image = if with_image {
        read_only_mapping(first.metadata.ptr)
    } else {
        &[]
    };
    let mut kernel = paperpaw_core::Kernel::builder(DEVICE).build().unwrap();
    let depth = FlatDepth::try_new().unwrap();
    let mut each = Vec::new();
    for (id, linked) in &NATIVE_PLUGINS {
        let entry = linked.entry();
        // SAFETY: a statically linked entry and its metadata, which are both
        // in this binary's read-only data; the mapping outlives the test.
        let metadata =
            unsafe { core::slice::from_raw_parts(entry.metadata.ptr, entry.metadata.len) };
        let resources = HostResources::try_new().unwrap();
        let before = LIVE.with(Cell::get);
        // SAFETY: the built-ins obey the SDK's native factory contract and are
        // linked for the whole run; `image` stays mapped.
        unsafe {
            kernel.register_linked(
                entry,
                metadata,
                image,
                &host,
                id,
                Own,
                resources,
                &depth,
                |_, _, _, _| {},
                |_, _, _| {},
            )
        }
        .unwrap_or_else(|error| panic!("{id}: {error:?}"));
        each.push((*id, LIVE.with(Cell::get) - before));
    }
    // Keep the kernel registered until every number is read.
    drop(kernel);
    each
}

#[test]
fn each_linked_built_in_keeps_a_measured_host_heap() {
    let kept = measure(true);
    let copied = measure(false);
    let (mut sum_kept, mut sum_copied) = (0, 0);
    for ((id, kept), (_, copied)) in kept.iter().zip(&copied) {
        std::println!("{id:>15}: {kept:>5} B with the image, {copied:>5} B without");
        sum_kept += kept;
        sum_copied += copied;
    }
    std::println!(
        "{:>15}: {sum_kept:>5} B with the image, {sum_copied:>5} B without",
        "all"
    );
    // Staging keeps static batches as offsets only when it is given the image.
    assert!(sum_kept < sum_copied, "{sum_kept} vs {sum_copied}");
}
