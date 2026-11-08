//! Test-only: constructs a linked native plugin in a real registered scope of
//! the kernel's native host (`Hosted::construct_registered`), the scope the
//! host gives `make`. The host runs the plugin's own checked metadata; nothing
//! here stands in for a service.
use paperpaw_core::native_host::{HostResources, shell};
use paperpaw_native::host_glue::Services;
use paperpaw_native::metadata::{self, Feature, Host as AdmissionHost, Profile, Service};
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

fn report(
    _: &Heap,
    _: &str,
    step: paperpaw_core::native_host::TeardownStep,
    error: paperpaw_core::native_host::Error,
) {
    eprintln!("native teardown {step:?}: {error:?}");
}

/// Constructs `entry` with the services its metadata declares and tears it
/// down; the host's refusal of construction comes back as its debug text.
pub fn try_construct(
    entry: &paperpaw_native::host_glue::PpEntry,
    metadata_bytes: &[u8],
    name: &str,
    services: &[&'static str],
) -> Result<(), String> {
    let services: Vec<Service<'_>> = services
        .iter()
        .map(|key| Service {
            key,
            major: 1,
            semantic_revision: 0,
            min_table_bytes: 0,
        })
        .collect();
    let host = AdmissionHost {
        abi_major: 7,
        minor: 1,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: 32,
        },
        services: &services,
        features: FEATURES,
    };
    let checked = metadata::check(metadata_bytes, &host).expect("metadata admitted");
    let declared = shell::DeclaredServices::try_from_iter(checked.services()).unwrap();
    // SAFETY: statically linked code and metadata live for the whole test; no image range.
    let hosted = unsafe {
        shell::Hosted::new(
            Heap,
            (),
            entry.make.expect("the entry has a factory"),
            name,
            (core::ptr::null(), 0),
            report,
            HostResources::try_new().unwrap(),
            declared,
        )
    }
    .expect("hosted");
    let constructed = hosted.construct_registered(hosted.begin().unwrap());
    let outcome = constructed
        .as_ref()
        .map(|_| ())
        .map_err(|e| format!("{e:?}"));
    drop(constructed);
    hosted.stop();
    hosted.owner().finish().unwrap();
    outcome
}
