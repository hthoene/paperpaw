//! Test-only: installs a linked native plugin the way a shell does, through
//! `Kernel::register_linked`: the entry and its metadata are checked against the
//! host, `make` runs in the registered scope, and every declaration family the
//! host admits (screens, actions, settings, menus, slots, stages, widgets, ...)
//! goes through the kernel's declaration store, registry, menus and slots.
//! The host runs the plugin's own checked metadata; nothing here stands in for
//! a service. Families the host refuses on purpose stay refused.
#![allow(
    dead_code,
    reason = "shared by several test files; each uses part of the kit"
)]
use paperpaw_core::{
    Kernel, KernelBuilder,
    native_host::{HostResources, HostWiring, flat::FlatDepth},
};
use paperpaw_native::host_glue::{PpEntry, Services};
use paperpaw_native::metadata::{Feature, Host as AdmissionHost, Profile, Service};
use paperpaw_sdk::{DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat};
use std::alloc::Layout;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicIsize, Ordering},
};

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

/// The host's heap for the plugin, counting what it hands out and what comes
/// back, and what a teardown step failed: [`Installed`] checks both when the
/// plugin is torn down.
#[derive(Clone, Default)]
struct Heap(Arc<Counters>);

#[derive(Default)]
struct Counters {
    live: AtomicIsize,
    failures: Mutex<Vec<String>>,
    /// What the host told the plugin it dropped: (callback token, status).
    dropped: Mutex<Vec<(u32, i32)>>,
}

impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        // SAFETY: the host validates the nonzero layout before forwarding.
        let block = unsafe { std::alloc::alloc(Layout::from_size_align(size, align).unwrap()) };
        if !block.is_null() {
            self.0.live.fetch_add(1, Ordering::Relaxed);
        }
        block
    }
    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        self.0.live.fetch_sub(1, Ordering::Relaxed);
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

fn report(heap: &Heap, _: &str, token: u32, status: i32) {
    heap.0.dropped.lock().unwrap().push((token, status));
}

fn teardown(
    heap: &Heap,
    _: &str,
    step: paperpaw_core::native_host::TeardownStep,
    error: paperpaw_core::native_host::Error,
) {
    heap.0
        .failures
        .lock()
        .unwrap()
        .push(format!("native teardown {step:?}: {error:?}"));
}

/// A plugin installed in a kernel. Dropping it tears the plugin down and fails
/// the test (a panic) on a failed teardown step or a block the host did not get
/// back, as the old harness did.
pub struct Installed {
    kernel: Option<Kernel>,
    heap: Heap,
}

impl Installed {
    /// The kernel the plugin is registered in.
    #[allow(
        dead_code,
        reason = "only the cases that go on to use the kernel call it"
    )]
    pub fn kernel(&mut self) -> &mut Kernel {
        self.kernel.as_mut().expect("installed")
    }

    /// The callbacks the host dropped so far, as (token, status): a draw or an event the
    /// plugin could not finish lands here, not in the kernel's warnings.
    #[allow(
        dead_code,
        reason = "only the cases that go on to use the kernel call it"
    )]
    pub fn dropped(&self) -> Vec<(u32, i32)> {
        self.heap.0.dropped.lock().unwrap().clone()
    }
}

impl Drop for Installed {
    fn drop(&mut self) {
        drop(self.kernel.take());
        if std::thread::panicking() {
            return;
        }
        let failures = self.heap.0.failures.lock().unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            self.heap.0.live.load(Ordering::Relaxed),
            0,
            "the host kept blocks after the plugin was torn down"
        );
    }
}

/// Every button of the X4, as `paperpaw_x4::buttons::ALL` lists them.
const X4_BUTTONS: [&str; 7] = [
    "back",
    "confirm",
    "left",
    "right",
    "page-back",
    "page-forward",
    "power",
];

/// What the kernel a plugin is installed in is like: its device, and the user's
/// configuration text (`language = de`, `help.show = false`, ...).
#[derive(Clone)]
pub struct Setup {
    device: DeviceInfo,
    config: String,
    /// What else the kernel is built with (see [`Setup::with_kernel`]).
    kernel: Vec<Arc<dyn Fn(KernelBuilder) -> KernelBuilder>>,
    /// The host services wired for the plugin (see [`Setup::wired`]).
    wiring: Vec<Arc<dyn Fn(HostWiring) -> HostWiring>>,
}

impl Default for Setup {
    /// A small device (the X4's memory class) and no configuration.
    fn default() -> Self {
        Self {
            device: DeviceInfo::new(
                "test",
                "Test device",
                DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Default::default(), None),
                InputCaps::new(true, true, false, &[]),
                MemoryClass::Small,
            ),
            config: String::new(),
            kernel: Vec::new(),
            wiring: Vec::new(),
        }
    }
}

impl Setup {
    /// The X4's screen (480 by 800, four grays, 220 dpi) with its buttons, as the interface-6
    /// goldens were drawn.
    pub fn x4() -> Self {
        Self {
            device: DeviceInfo::new(
                "xteink-x4",
                "Xteink X4",
                DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Default::default(), None),
                InputCaps::new(true, false, false, &X4_BUTTONS),
                MemoryClass::Small,
            ),
            config: String::new(),
            kernel: Vec::new(),
            wiring: Vec::new(),
        }
    }

    /// Builds the kernel further with `step`, e.g. `|b| b.plugin(DefaultFontPacks)` for the
    /// fonts a screen draws with, or `|b| b.storage(..)` for files it reads. Steps run in the
    /// order given.
    pub fn with_kernel(mut self, step: impl Fn(KernelBuilder) -> KernelBuilder + 'static) -> Self {
        self.kernel.push(Arc::new(step));
        self
    }

    /// Wires host services for the plugin as a shell does
    /// (`HostResources::wire`): `|w| w.lookup()`, `|w| w.library(..)`, ... Steps run in the
    /// order given. Without any, a service the plugin declared answers as an unwired family
    /// does; with one the call reaches the real host, and so the checks of the call path (a
    /// reply buffer smaller than the op's `max_reply` is `ERR_NO_SPACE` there, as on a device).
    pub fn wired(mut self, step: impl Fn(HostWiring) -> HostWiring + 'static) -> Self {
        self.wiring.push(Arc::new(step));
        self
    }

    /// The user's configuration, as the file `config.ini` writes it.
    pub fn config(mut self, text: &str) -> Self {
        self.config = text.to_string();
        self
    }
}

/// Installs `entry` as plugin `name` in a new kernel with the services its
/// metadata declares, and returns it (drop it to tear the plugin down); the
/// host's refusal comes back as its debug text.
pub fn install(
    entry: &'static PpEntry,
    name: &str,
    services: &[&'static str],
) -> Result<Installed, String> {
    install_in(&Setup::default(), entry, name, services)
}

/// [`install`] in a kernel made as `setup` says.
pub fn install_in(
    setup: &Setup,
    entry: &'static PpEntry,
    name: &str,
    services: &[&'static str],
) -> Result<Installed, String> {
    install_with(setup, entry, name, services, FEATURES)
}

/// [`install_in`] on a host that offers only `features` (a shell that does not
/// back a feature, e.g. the desktop without `uses.install`).
pub fn install_with(
    setup: &Setup,
    entry: &'static PpEntry,
    name: &str,
    services: &[&'static str],
    features: &'static [Feature<'static>],
) -> Result<Installed, String> {
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
        features,
    };
    // SAFETY: a linked entry's metadata view is its own static bytes.
    let metadata = unsafe { std::slice::from_raw_parts(entry.metadata.ptr, entry.metadata.len) };
    let heap = Heap::default();
    let resources = HostResources::try_new().map_err(|e| format!("{e:?}"))?;
    if !setup.wiring.is_empty() {
        let wiring = setup
            .wiring
            .iter()
            .fold(HostWiring::new(), |wiring, step| step(wiring));
        resources.wire(wiring).map_err(|e| format!("wire: {e:?}"))?;
    }
    // Held from here on, so a refused install is checked for leaks as well.
    let mut installed = Installed {
        kernel: Some(
            setup
                .kernel
                .iter()
                .fold(
                    Kernel::builder(setup.device).user_config(setup.config.clone()),
                    |builder, step| step(builder),
                )
                .build()
                .map_err(|e| format!("{e:?}"))?,
        ),
        heap: heap.clone(),
    };
    // SAFETY: statically linked code and metadata live for the whole test.
    unsafe {
        installed.kernel().register_linked(
            entry,
            metadata,
            &host,
            name,
            heap.clone(),
            resources,
            &FlatDepth::try_new().map_err(|e| format!("{e:?}"))?,
            teardown,
            report,
            (core::ptr::null(), 0),
            paperpaw_core::native_host::FixedTables::NONE,
        )
    }
    .map_err(|e| format!("{e:?}"))?;
    Ok(installed)
}

/// [`install`] and tear the plugin down again: `make` ran in a real
/// registered scope, its declarations were admitted and the teardown was clean.
pub fn try_construct(
    entry: &'static PpEntry,
    name: &str,
    services: &[&'static str],
) -> Result<(), String> {
    install(entry, name, services).map(drop)
}

/// [`try_construct`] on a host that does not offer the features named in `lacking`.
pub fn try_construct_lacking(
    entry: &'static PpEntry,
    name: &str,
    services: &[&'static str],
    lacking: &[&str],
) -> Result<(), String> {
    let offered: &'static [Feature<'static>] = Box::leak(
        FEATURES
            .iter()
            .filter(|f| !lacking.contains(&f.key))
            .cloned()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    install_with(&Setup::default(), entry, name, services, offered).map(drop)
}
