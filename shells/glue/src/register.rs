//! Registration of the app's linked interface-7 plugins with the kernel
//! (`Kernel::register_linked`), for a shell that has parts of its own to give
//! (its device, card, secrets, network) and shares the rest.
//!
//! One table of handles (with `core.ui` and `core.device` wired) and one
//! dispatch budget are shared by every linked plugin. What a plugin needs of
//! the host is checked at its admission: a plugin that asks for a service this
//! shell does not wire is refused there (the list of what is offered is
//! [`crate::offer::offered`]) and the refusal is said on stderr, so nothing is
//! half there.

use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_core::native_host::device_service::DeviceHost;
use paperpaw_core::native_host::flat::FlatDepth;
use paperpaw_core::native_host::shell_wiring::{WiredHosts, WorkInputs};
use paperpaw_core::native_host::{HostResources, HostWiring, TeardownStep};
use paperpaw_native::metadata;
use paperpaw_platform::{MemoryClass, Storage};

use crate::image::{host_profile, rodata_image};
use crate::offer::{Wired, features, offered};
use crate::services::StdServices;
use crate::view::{Catalog, DeviceView};

/// A linked plugin's diagnostics from the kernel (a refused declaration, a
/// failed callback) go to stderr.
fn linked_report(_: &StdServices, plugin: &str, tag: u32, status: i32) {
    eprintln!("native plugin {plugin}: call {tag} ended with {status}");
}

/// The step of a teardown that failed, once, without calling plugin code.
fn teardown_report(
    _: &StdServices,
    id: &str,
    step: TeardownStep,
    why: paperpaw_core::native_host::Error,
) {
    eprintln!("native plugin {id}: teardown step {step:?} failed: {why:?}");
}

/// Why the linked plugins could not be registered at all (a plugin the host
/// refuses is said on stderr and does not stop the others).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unavailable {
    /// No room for the shared table of handles or the dispatch budget.
    NoRoom,
    /// The wiring of `core.ui` or `core.device` was refused.
    Wiring(i32),
}

/// What plugin jobs (the store's install, a removal, a check) are taken for.
#[derive(Debug)]
pub enum PluginJobs {
    /// None: the device installs no plugin files.
    None,
    /// Data and code plugins, checked against `loader` and `card` (the
    /// device's chip and the metadata profile of its firmware, `device.md`
    /// 5.1).
    Code {
        loader: paperpaw_native::header5::Host,
        card: metadata::Profile<'static>,
    },
    /// Data plugins only: the device loads no plugin code (Boox, Android), a
    /// plugin with a code file ends with `ERR_UNSUPPORTED`.
    Data,
}

/// What a shell gives the registration besides the services it wires itself.
pub struct Shell {
    /// The device's memory class: sizes `core.ui`, `core.document` and `core.work`.
    pub memory: MemoryClass,
    /// The display's longer side in pixels (`core.document`).
    pub widest: u32,
    /// The kernel's `core.device` over this device.
    pub device: Rc<dyn DeviceHost>,
    /// The clock and network `core.work` asks for.
    pub view: Rc<DeviceView>,
    /// The card itself (a second handle on what the kernel reads), which
    /// `core.document`, `core.library` and `core.files` serve the plugins
    /// from; `None` where it cannot be opened (none of them is wired then).
    pub card: Option<Rc<dyn Storage>>,
    /// A run on simulated time: the catalog tells no day.
    pub simulated: bool,
    /// The device has the radio `uses.wifi` is backed by.
    pub wifi: bool,
    /// What plugin jobs are taken for.
    pub jobs: PluginJobs,
    /// The wiring so far: the services the shell makes itself (`core.net`,
    /// `core.sockets`, `core.secrets`).
    pub wiring: HostWiring,
}

/// What registration leaves: how many plugins were registered, and the wiring
/// the shell takes its own services' reports from.
#[derive(Debug)]
pub struct Registered {
    pub count: usize,
    pub wired: WiredHosts,
}

/// Tells `kernel` what plugin jobs check a plugin file against: the services
/// and features wired here (`on`, with `uses.install`). Whether plugin jobs
/// are taken (the kernel has its firmware host, `device::build`).
fn take_plugin_jobs(kernel: &mut Kernel, jobs: PluginJobs, on: Wired) -> bool {
    match jobs {
        PluginJobs::None => false,
        PluginJobs::Data => kernel.set_data_plugin_host(),
        PluginJobs::Code { loader, card } => {
            let on = Wired {
                install: true,
                ..on
            };
            let offered = metadata::Host {
                abi_major: 7,
                minor: paperpaw_native::host_glue::MINOR,
                profile: card,
                services: offered(on),
                features: features(on),
            };
            kernel.set_plugin_host(loader, offered, &[])
        }
    }
}

/// Registers every plugin of `paperpaw_app::NATIVE_PLUGINS` with `kernel`; the
/// number registered and the wiring. Call before the kernel starts, once.
///
/// # Errors
/// [`Unavailable`] where the shared host views cannot be made; then nothing is
/// registered.
pub fn register(kernel: &mut Kernel, shell: Shell) -> Result<Registered, Unavailable> {
    let resources = HostResources::try_new().map_err(|_| Unavailable::NoRoom)?;
    let mut wiring = shell
        .wiring
        .ui(shell.memory)
        .device(shell.device)
        .lookup()
        .work(WorkInputs::new(shell.view.clone(), shell.memory));
    // The books, the catalog and the card's files, from the card itself.
    let mut on_card = Wired::default();
    if let Some(card) = shell.card {
        wiring = wiring
            .documents(shell.memory, shell.widest)
            .library(
                card.clone(),
                Rc::new(Catalog {
                    simulated: shell.simulated,
                }),
            )
            .files(card);
        on_card.documents = true;
        on_card.library = true;
        on_card.files = true;
    }
    on_card.wifi = shell.wifi;
    let wired = resources.wire(wiring).map_err(Unavailable::Wiring)?;
    // Values an older version kept veiled are sealed now (none to do on a
    // fresh card).
    if let Some(secrets) = wired.secrets()
        && let Err(status) = secrets.upgrade()
    {
        eprintln!("native plugins: the secrets file could not be upgraded ({status})");
    }
    // Interface 6's passwords are opened and written through the sealer from
    // here on, whether or not a native plugin is ever admitted (decision 140);
    // before `Kernel::start`, as `Kernel::wire_secrets` asks.
    if let Some(secrets) = wired.secrets() {
        kernel.wire_secrets(secrets);
    }
    let mut on = crate::offer::wired_services(&wired, on_card);
    on.install = take_plugin_jobs(kernel, shell.jobs, on);
    let depth = FlatDepth::try_new().map_err(|_| Unavailable::NoRoom)?;
    let host = metadata::Host {
        abi_major: 7,
        minor: paperpaw_native::host_glue::MINOR,
        profile: host_profile(),
        services: offered(on),
        features: features(on),
    };
    let outcome = register_linked(kernel, &host, &resources, &depth);
    let mut count = 0;
    for (id, status) in outcome {
        match status {
            Ok(()) => count += 1,
            Err(why) => eprintln!("native plugin {id}: not registered: {why:?}"),
        }
    }
    Ok(Registered { count, wired })
}

/// Registers the app's linked plugins: the ones `paperpaw_app::sleeps` names
/// parked, every other one awake (#248, D1).
fn register_linked(
    kernel: &mut Kernel,
    host: &metadata::Host<'static>,
    resources: &HostResources,
    depth: &FlatDepth,
) -> Vec<(
    &'static str,
    Result<(), paperpaw_core::native_host::shell::Error>,
)> {
    // SAFETY: the offered services are the ones wired by the caller
    // (`offered`), and the plugins are the app's linked ones, as the helper's
    // contract says.
    unsafe {
        paperpaw_app::register_native_plugins(
            kernel,
            host,
            resources,
            depth,
            StdServices::new,
            teardown_report,
            linked_report,
            |id| paperpaw_app::native_default_on(id, paperpaw_app::default_store()),
            rodata_image(),
            paperpaw_app::sleeps,
        )
    }
}
