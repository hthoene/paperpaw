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
use paperpaw_platform::Storage;
use paperpaw_sdk::MemoryStorage;
use std::rc::Rc;

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
    build_with(builder, Rc::new(MemoryStorage::new()), false)
}

/// [`build`] where the linked plugins' files, library and documents are on
/// `card`, a second handle on the card the kernel was given, as a shell wires
/// them (a reader that opens the kernel's books needs it).
///
/// # Panics
/// As [`build`].
pub fn build_on(builder: KernelBuilder, card: Rc<dyn Storage>) -> Kernel {
    build_with(builder, card, true)
}

/// [`build_on`], with the X4's Wi-Fi (managed by PaperPaw and on) where `wifi`
/// says so: the native Wi-Fi plugin's list of networks closes itself where
/// there is no radio to choose with. The screens measured without it (the
/// store's partial redraws) stay as they were.
fn build_with(builder: KernelBuilder, card: Rc<dyn Storage>, wifi: bool) -> Kernel {
    #[cfg(feature = "native-linked")]
    let builder = if wifi { builder.wifi(true) } else { builder };
    #[allow(unused_mut, reason = "only the native-linked build registers plugins")]
    let mut kernel = builder.build().unwrap();
    #[cfg(feature = "native-linked")]
    {
        linked::register(&mut kernel, card);
        if wifi {
            let _ = kernel.wifi_event(
                paperpaw_platform::Instant(0),
                paperpaw_core::WifiEvent::State(paperpaw_core::WifiState::On),
            );
        }
    }
    #[cfg(not(feature = "native-linked"))]
    let _ = (card, wifi);
    kernel
}

#[cfg(feature = "native-linked")]
mod linked {
    /// What the harness wires the families of files, library, documents, secrets,
    /// work and HTTP with, so that every linked plugin registers as it does in a
    /// shell (#248: the owned-ids test covers all of them). Nothing here does
    /// work: the card is in memory, the network is down, nothing is sealed.
    mod fakes {

        use paperpaw_core::native_host::HostWiring;
        use paperpaw_core::native_host::shell_wiring::{
            CatalogEnv, HttpJob, NetInputs, NetLimits, Rng, Sealer, SecretsInputs, ServedReply,
            ServerReason, ShellHttp, ShellNetwork, WorkDevice, WorkInputs,
        };
        use paperpaw_native::host_glue::ERR_UNSUPPORTED;
        use paperpaw_platform::{MemoryClass, Storage};
        use std::rc::Rc;

        /// No network, no clock, no radio.
        struct Offline;

        impl WorkDevice for Offline {
            fn now_ms(&self) -> u64 {
                0
            }
            fn network_up(&self) -> bool {
                false
            }
            fn radio_on(&self) -> bool {
                false
            }
        }

        impl ShellNetwork for Offline {
            fn now_ms(&self) -> u64 {
                0
            }
            fn network_up(&self) -> bool {
                false
            }
            fn network_demand(&self, _: bool) {}
        }

        impl ShellHttp for Offline {
            fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
                Err(ERR_UNSUPPORTED)
            }
            fn abort(&self, _: u32) {}
            fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
                Err(ServerReason::Refused)
            }
            fn unlisten(&self, _: u32) {}
            fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
                Err(ERR_UNSUPPORTED)
            }
            fn close_served(&self, _: u32, _: u32) {}
        }

        /// A sealer that seals nothing: secrets are refused, not kept open.
        struct NoSeal;

        impl Sealer for NoSeal {
            fn seals(&self) -> bool {
                false
            }
            fn seal(&self, _: &[u8], _: &[u8], _: &mut [u8]) -> Result<usize, i32> {
                Err(ERR_UNSUPPORTED)
            }
            fn open(&self, _: &[u8], _: &[u8], _: &mut [u8]) -> Result<usize, i32> {
                Err(ERR_UNSUPPORTED)
            }
        }

        /// A random source for tests: the same bytes every run (a counter), said
        /// to be strong, as a device's source is (the Wi-Fi plugin draws its own
        /// network's password from it).
        struct Counter(std::cell::Cell<u8>);

        impl Rng for Counter {
            fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
                for byte in out {
                    let next = self.0.get().wrapping_add(1);
                    self.0.set(next);
                    *byte = next;
                }
                Ok(true)
            }
        }

        /// No walk under way, no date.
        struct NoCatalog;

        impl CatalogEnv for NoCatalog {
            fn walking(&self) -> Option<u32> {
                None
            }
            fn today(&self) -> Option<u32> {
                None
            }
        }

        /// `wiring` with the families a shell wires over its card, on `card`.
        pub fn wire(wiring: HostWiring, card: Rc<dyn Storage>) -> HostWiring {
            wiring
                .documents(MemoryClass::Small, 800)
                .library(card.clone(), Rc::new(NoCatalog))
                .files(card.clone())
                .secrets(
                    SecretsInputs::new(card.clone(), Rc::new(NoSeal))
                        .with_rng(Rc::new(Counter(std::cell::Cell::new(0)))),
                )
                .work(WorkInputs::new(Rc::new(Offline), MemoryClass::Small))
                .net(NetInputs::new(
                    card,
                    Rc::new(Offline),
                    Rc::new(Offline),
                    NetLimits::new(0, 8),
                ))
        }

        /// The keys of the families [`wire`] wires.
        pub const WIRED: &[&str] = &[
            "core.document",
            "core.library",
            "core.files",
            "core.secrets",
            "core.work",
            "core.net",
        ];
    }
    use paperpaw_core::Kernel;
    use paperpaw_core::native_host::device_service::{DEVICE_SERVICES, DeviceFacts, DeviceHost};
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

    /// A device that tells what it is (op 1, as every shell does: the Wi-Fi
    /// plugin names its own network after the model) and has no part to tell
    /// about: every other question is "no such part" (the default of
    /// `DeviceHost`).
    struct NoParts;
    impl DeviceHost for NoParts {
        fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
            visit(&DeviceFacts {
                id: "test-device",
                model: "Test device",
                catalog: "",
                class: 0,
                caps: 0,
                controls: 0,
                tls_sessions: 0,
                start: 0,
                slept_ms: None,
            });
            Ok(())
        }
    }

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
            .chain(
                ["core.ui", "core.registry"]
                    .into_iter()
                    .chain(fakes::WIRED.iter().copied())
                    .map(|key| Service {
                        key,
                        major: 1,
                        semantic_revision: 0,
                        min_table_bytes: 0,
                    }),
            )
            .collect();
        Box::leak(all.into_boxed_slice())
    }

    pub fn register(kernel: &mut Kernel, card: Rc<dyn paperpaw_platform::Storage>) {
        let resources = HostResources::try_new().expect("the table of handles");
        let device: Rc<dyn DeviceHost> = Rc::new(NoParts);
        resources
            .wire(fakes::wire(
                HostWiring::new()
                    .ui(paperpaw_platform::MemoryClass::Small)
                    .device(device),
                card,
            ))
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
