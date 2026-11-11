//! Test-only: builds a kernel as the standard app is built, and under the
//! feature `native-linked` registers the app's linked interface-7 plugins
//! (`paperpaw_app::NATIVE_PLUGINS`) in it before it starts, as a shell does
//! (`paperpaw_app::register_native_plugins`): the plugins that left their
//! interface-6 form are there, or the app lacks the screens they gave.
//! Without the feature it is the plain `KernelBuilder::build`.
#![allow(
    dead_code,
    reason = "shared by several test files; each uses part of it"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the test"
)]

use paperpaw_core::{Kernel, KernelBuilder};

/// Every consent feature, offered to plugins in these test kernels (they back
/// none: the tests only need the plugins' screens; a shell offers only what it
/// backs).
#[cfg(feature = "native-linked")]
const FEATURES: &[paperpaw_native::metadata::Feature<'static>] = &[
    paperpaw_native::metadata::Feature {
        key: "uses.registry",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.library",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.network",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.wifi",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.power",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.install",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.firmware",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.bluetooth",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.storage",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.secrets",
        revision: 1,
    },
];

/// `build` for a test: the kernel built, with the linked plugins registered where the feature says.
pub trait BuildNative {
    /// See [`build`].
    fn build_native(self) -> Kernel;
}

impl BuildNative for KernelBuilder {
    fn build_native(self) -> Kernel {
        build(self)
    }
}

/// Builds the kernel; with `native-linked` the linked plugins are registered in it.
///
/// # Panics
/// When the kernel cannot be built or the shared host tables cannot be made (a
/// test cannot go on then). A plugin the host refuses is named on stderr and
/// its screens are missing, which the test that needs them reports.
pub fn build(builder: KernelBuilder) -> Kernel {
    build_with(builder, paperpaw_app::default_store())
}

/// [`build`] where the plugins the store offers are as `store` says.
///
/// # Panics
/// As [`build`].
pub fn build_with(builder: KernelBuilder, store: paperpaw_app::StorePlugins<'static>) -> Kernel {
    #[allow(unused_mut, reason = "only the native-linked build registers plugins")]
    let mut kernel = builder.build().unwrap();
    #[cfg(feature = "native-linked")]
    linked::register(&mut kernel, store);
    #[cfg(not(feature = "native-linked"))]
    let _ = store;
    kernel
}

#[cfg(feature = "native-linked")]
mod linked {
    use paperpaw_core::Kernel;
    use paperpaw_core::native_host::device_service::{DEVICE_SERVICES, DeviceHost};
    use paperpaw_core::native_host::flat::FlatDepth;
    use paperpaw_core::native_host::shell::DATA_SERVICES;
    use paperpaw_core::native_host::{HostResources, HostWiring, TeardownStep};
    use paperpaw_native::host_glue::Services;
    use paperpaw_native::metadata::{Host, Profile, Service};
    use std::alloc::Layout;
    use std::rc::Rc;

    /// The heap and clock the host gives a plugin: the process's, uncounted.
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

    /// A device that has no part to tell about: every question is "no such
    /// part" (the default of `DeviceHost`).
    struct NoParts;
    impl DeviceHost for NoParts {}

    fn teardown(_: &Heap, id: &str, step: TeardownStep, why: paperpaw_core::native_host::Error) {
        eprintln!("native plugin {id}: teardown step {step:?} failed: {why:?}");
    }

    fn report(_: &Heap, tag: u32, status: i32) {
        eprintln!("native plugin: call {tag} ended with {status}");
    }

    /// What the host offers: the data services, `core.device`, and the ones the
    /// table serves from the kernel's own views.
    fn offered() -> &'static [Service<'static>] {
        let all: Vec<Service<'static>> = DATA_SERVICES
            .iter()
            .chain(DEVICE_SERVICES)
            .copied()
            .chain(["core.ui", "core.registry"].into_iter().map(|key| Service {
                key,
                major: 1,
                semantic_revision: 0,
                min_table_bytes: 0,
            }))
            .collect();
        Box::leak(all.into_boxed_slice())
    }

    pub fn register(kernel: &mut Kernel, store: paperpaw_app::StorePlugins<'static>) {
        let resources = HostResources::try_new().expect("the table of handles");
        let device: Rc<dyn DeviceHost> = Rc::new(NoParts);
        resources
            .wire(
                HostWiring::new()
                    .ui(paperpaw_platform::MemoryClass::Small)
                    .device(device),
            )
            .expect("core.ui and core.device wired");
        let depth = FlatDepth::try_new().expect("the dispatch budget");
        let host = Host {
            abi_major: 7,
            minor: paperpaw_native::host_glue::MINOR,
            profile: Profile {
                target: "linux-x86_64-sysv64-le",
                entry_layout: 1,
                entry_bytes: 32,
            },
            services: offered(),
            features: super::FEATURES,
        };
        // SAFETY: the offered services are the ones wired above, the plugins are the app's
        // linked ones, and the image is this process's read-only data, mapped and never
        // written for the whole run, as the helper's contract says.
        let outcome = unsafe {
            paperpaw_app::register_native_plugins(
                kernel,
                &host,
                &resources,
                &depth,
                || Heap,
                teardown,
                report,
                |id| paperpaw_app::native_default_on(id, store),
                rodata_image(),
            )
        };
        for (id, status) in outcome {
            if let Err(why) = status {
                eprintln!("native plugin {id}: not registered: {why:?}");
            }
        }
    }

    /// A value in this test's read-only data, the segment that holds the
    /// plugins' declaration bytes, to find it by.
    static RODATA_ANCHOR: [u8; 16] = *b"paperpaw-rodata\0";

    /// This test's read-only data as the shells pass it (`paperpaw-desktop`'s
    /// `rodata_image`): the read-only, non-executable mapping that holds
    /// `RODATA_ANCHOR` (`/proc/self/maps`), or none elsewhere. With it the
    /// linked plugins' titles are borrowed, as on the device (#248).
    fn rodata_image() -> (*const u8, usize) {
        let anchor = std::ptr::addr_of!(RODATA_ANCHOR) as usize;
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        for line in maps.lines() {
            let mut parts = line.split_ascii_whitespace();
            let (Some(range), Some(perms)) = (parts.next(), parts.next()) else {
                continue;
            };
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) = (
                usize::from_str_radix(start, 16),
                usize::from_str_radix(end, 16),
            ) else {
                continue;
            };
            if (start..end).contains(&anchor) && perms.starts_with("r--") {
                return (start as *const u8, end - start);
            }
        }
        (std::ptr::null(), 0)
    }
}
