//! Test-only: installs a linked native plugin the way a shell does, through
//! `Kernel::register_linked`: the entry and its metadata are checked against the
//! host, `make` runs in the registered scope, and every declaration family the
//! host admits (screens, actions, settings, menus, slots, stages, widgets, ...)
//! goes through the kernel's declaration store, registry, menus and slots.
//! The host runs the plugin's own checked metadata; nothing here stands in for
//! a service. Families the host refuses on purpose stay refused.
use paperpaw_core::{
    Kernel,
    native_host::{HostResources, flat::FlatDepth},
};
use paperpaw_native::host_glue::{PpEntry, Services};
use paperpaw_native::metadata::{Feature, Host as AdmissionHost, Profile, Service};
use paperpaw_sdk::{DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat};
use std::alloc::Layout;

/// The consent features the host offers; a plugin's metadata may require any.
const FEATURES: &[Feature<'static>] = &[
    Feature {
        key: "uses.registry",
        revision: 1,
    },
    Feature {
        key: "uses.library",
        revision: 1,
    },
    Feature {
        key: "uses.network",
        revision: 1,
    },
    Feature {
        key: "uses.wifi",
        revision: 1,
    },
    Feature {
        key: "uses.power",
        revision: 1,
    },
    Feature {
        key: "uses.install",
        revision: 1,
    },
    Feature {
        key: "uses.firmware",
        revision: 1,
    },
    Feature {
        key: "uses.bluetooth",
        revision: 1,
    },
    Feature {
        key: "uses.storage",
        revision: 1,
    },
    Feature {
        key: "uses.secrets",
        revision: 1,
    },
];

struct Heap;
impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        // SAFETY: the host validates the nonzero layout before forwarding.
        unsafe { std::alloc::alloc(Layout::from_size_align(size, align).unwrap()) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        // SAFETY: the allocation came from this heap with the same layout.
        unsafe { std::alloc::dealloc(ptr, Layout::from_size_align(size, align).unwrap()) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, size: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: the allocation belongs to this heap and `new` is checked by the host.
        unsafe { std::alloc::realloc(ptr, Layout::from_size_align(size, align).unwrap(), new) }
    }
    fn micros(&self) -> u64 {
        0
    }
}

fn report(_: &Heap, _: u32, _: i32) {}

fn teardown(
    _: &Heap,
    _: &str,
    step: paperpaw_core::native_host::TeardownStep,
    error: paperpaw_core::native_host::Error,
) {
    eprintln!("native teardown {step:?}: {error:?}");
}

/// A device for the kernel the plugin registers in.
fn device() -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Default::default(), None),
        InputCaps::new(true, true, false, &[]),
        MemoryClass::Large,
    )
}

/// Installs `entry` as plugin `name` in a new kernel with the services its
/// metadata declares, and returns the kernel (drop it to tear the plugin
/// down); the host's refusal comes back as its debug text.
pub fn install(
    entry: &'static PpEntry,
    name: &str,
    services: &[&'static str],
) -> Result<Kernel, String> {
    let services: &'static [Service<'static>] = Box::leak(
        services
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
    let host = AdmissionHost {
        abi_major: 7,
        minor: 1,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: 32,
        },
        services,
        features: FEATURES,
    };
    // SAFETY: a linked entry's metadata view is its own static bytes.
    let metadata = unsafe { std::slice::from_raw_parts(entry.metadata.ptr, entry.metadata.len) };
    let mut kernel = Kernel::builder(device())
        .build()
        .map_err(|e| format!("{e:?}"))?;
    // SAFETY: statically linked code and metadata live for the whole test.
    unsafe {
        kernel.register_linked(
            entry,
            metadata,
            &host,
            name,
            Heap,
            HostResources::try_new().map_err(|e| format!("{e:?}"))?,
            &FlatDepth::try_new().map_err(|e| format!("{e:?}"))?,
            teardown,
            report,
        )
    }
    .map_err(|e| format!("{e:?}"))?;
    Ok(kernel)
}

/// [`install`] and tear the plugin down again: `make` ran in a real
/// registered scope and its declarations were admitted.
pub fn try_construct(
    entry: &'static PpEntry,
    name: &str,
    services: &[&'static str],
) -> Result<(), String> {
    install(entry, name, services).map(drop)
}
