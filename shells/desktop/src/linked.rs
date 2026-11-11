//! The first-party interface-7 plugins that are linked into the emulator
//! (`paperpaw_app::NATIVE_PLUGINS`, ADR 0034 "One path"), registered through the
//! kernel (`Kernel::register_linked`) before the device starts.
//!
//! The emulator is the shell the application's flows and snapshots run on, so
//! the plugins that left their interface-6 form (`paperpaw_app::install_with`
//! drops it under the feature `native-linked`) must be here, or the app lacks
//! the screens they gave. What a plugin needs of the host is checked at its
//! admission: a plugin that asks for a service this shell does not wire is
//! refused there (the list of what is offered is `offered`) and the refusal
//! is said on stderr, so nothing is half there.
//!
//! One table of handles (with `core.ui` and `core.device` wired) and one
//! dispatch budget are shared by every linked plugin, as the loader's are for
//! the plugins of the card (`native::HostLoader`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_core::native_host::device_service::DEVICE_SERVICES;
use paperpaw_core::native_host::flat::FlatDepth;
use paperpaw_core::native_host::shell::DATA_SERVICES;
use paperpaw_core::native_host::shell_wiring::{
    NetInputs, NetLimits, NetReports, SecretsInputs, SockInputs, SockLimits, SockReports,
    WorkDevice, WorkInputs,
};
use paperpaw_core::native_host::{HostResources, HostWiring, TeardownStep};
use paperpaw_native::metadata::{self, Feature, Service};
use paperpaw_platform::Storage;

use crate::device_host::DesktopDevice;
use crate::host_services::DesktopServices;
use crate::profiles::Profile;

/// The services this shell wires for linked plugins: the data services of the
/// shared host table, `core.device` over the profile's device, and the ones
/// the table serves from the kernel's own views (`core.ui`, `core.registry`).
/// A service not named here is not offered: a plugin that needs it is refused
/// at admission.
fn offered(wired: Wired) -> &'static [Service<'static>] {
    use std::sync::OnceLock;
    const OWN: &[&str] = &["core.ui", "core.registry"];
    static OFFERED: [OnceLock<Vec<Service<'static>>>; 16] = [const { OnceLock::new() }; 16];
    let index = usize::from(wired.secrets)
        | usize::from(wired.work) << 1
        | usize::from(wired.net) << 2
        | usize::from(wired.sockets) << 3;
    OFFERED[index].get_or_init(|| {
        let wired_keys = [
            (wired.secrets, "core.secrets"),
            (wired.work, "core.work"),
            (wired.net, "core.net"),
            (wired.sockets, "core.sockets"),
        ]
        .into_iter()
        .filter_map(|(on, key)| on.then_some(key));
        DATA_SERVICES
            .iter()
            .chain(DEVICE_SERVICES)
            .copied()
            .chain(OWN.iter().copied().chain(wired_keys).map(|key| Service {
                key,
                major: 1,
                semantic_revision: 0,
                min_table_bytes: 0,
            }))
            .collect()
    })
}

/// Which of the services that are wired only when the shell can are wired.
#[derive(Clone, Copy, Debug, Default)]
struct Wired {
    secrets: bool,
    work: bool,
    net: bool,
    sockets: bool,
}

/// The consent features this shell backs: the registry (the kernel's own),
/// the device's power view (`core.device`) and, where they are wired,
/// `uses.secrets` (it has a sealing key) and `uses.network` (HTTP requests,
/// `core.net`). Not offered, because the desktop does not back them yet:
/// `uses.wifi`, `uses.bluetooth`,
/// `uses.install`, `uses.firmware`, `uses.library`, `uses.storage`. A plugin
/// that needs one is refused at admission.
fn features(wired: Wired) -> &'static [Feature<'static>] {
    const REGISTRY: Feature<'static> = Feature {
        key: "uses.registry",
        revision: 1,
    };
    const POWER: Feature<'static> = Feature {
        key: "uses.power",
        revision: 1,
    };
    const SECRETS: Feature<'static> = Feature {
        key: "uses.secrets",
        revision: 1,
    };
    const NETWORK: Feature<'static> = Feature {
        key: "uses.network",
        revision: 1,
    };
    const BASE: &[Feature<'static>] = &[REGISTRY, POWER];
    const SECRETS_ONLY: &[Feature<'static>] = &[REGISTRY, POWER, SECRETS];
    const NET_ONLY: &[Feature<'static>] = &[REGISTRY, POWER, NETWORK];
    const BOTH: &[Feature<'static>] = &[REGISTRY, POWER, SECRETS, NETWORK];
    // `uses.network` is the one feature of HTTP and sockets (`network.md` 1).
    match (wired.secrets, wired.net || wired.sockets) {
        (false, false) => BASE,
        (true, false) => SECRETS_ONLY,
        (false, true) => NET_ONLY,
        (true, true) => BOTH,
    }
}

/// A linked plugin's diagnostics from the kernel (a refused declaration, a
/// failed callback) go to stderr.
fn linked_report(_: &DesktopServices, tag: u32, status: i32) {
    eprintln!("native plugin: call {tag} ended with {status}");
}

/// What the device tells `core.work` (`WorkDevice`): the kernel's clock as the
/// device last read it and whether the network is up. The device writes both
/// where it reads its clock ([`DeviceView::set`]), so the scheduler and the
/// kernel's tick agree.
#[derive(Debug, Default)]
pub struct DeviceView {
    now_ms: Cell<u64>,
    online: Cell<bool>,
}

impl DeviceView {
    /// The clock and the network as they are now.
    pub fn set(&self, now_ms: u64, online: bool) {
        self.now_ms.set(now_ms);
        self.online.set(online);
    }
}

impl WorkDevice for DeviceView {
    fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }
    fn network_up(&self) -> bool {
        self.online.get()
    }
    /// The pretend radio is on when the network is up.
    fn radio_on(&self) -> bool {
        self.online.get()
    }
}

/// What the device hands the registration besides its profile.
#[derive(Clone)]
pub struct Inputs {
    /// Where the secrets file is kept (sealed): the card of a real run, a
    /// store in memory for a run on simulated time. `None` where the card
    /// cannot be opened: `core.secrets` is then not wired, rather than a
    /// store that loses what is saved.
    pub storage: Option<Rc<dyn Storage>>,
    /// The clock and network `core.work` asks for.
    pub view: Rc<DeviceView>,
    /// A run on simulated time (tests, scripts): its secrets key is kept in
    /// memory and nothing is written outside the run's own folders.
    pub simulated: bool,
    /// Where registration leaves the transport of `core.net` and core's
    /// reports to it, for the device's loop ([`Net`]); `None` inside where
    /// there is no card.
    pub net: Rc<RefCell<Option<Net>>>,
}

/// The transport of `core.net` on the device and the handle core gave for
/// its reports.
#[derive(Debug)]
pub struct Net {
    pub transport: Rc<crate::native_net::NativeNet>,
    pub reports: NetReports,
    /// TCP and UDP broadcast (`core.sockets`), where wired.
    pub sockets: Option<(Rc<crate::native_sock::NativeSock>, SockReports)>,
}

impl std::fmt::Debug for Inputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inputs")
            .field("view", &self.view)
            .field("simulated", &self.simulated)
            .finish_non_exhaustive()
    }
}

/// The sealer of the secrets, or why there is none (then `core.secrets` is
/// not wired and a plugin that needs it is refused: nothing is stored in the
/// clear).
fn sealer(simulated: bool) -> Result<Rc<crate::sealing::KeySealer>, std::io::Error> {
    let sealer = if simulated {
        crate::sealing::KeySealer::for_the_process()?
    } else {
        crate::sealing::KeySealer::for_the_user()?
    };
    Ok(Rc::new(sealer))
}

/// The step of a teardown that failed, once, without calling plugin code.
fn teardown_report(
    _: &DesktopServices,
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

/// Registers every plugin of `paperpaw_app::NATIVE_PLUGINS` with `kernel` for
/// the device of `profile`; the number registered. Call before the kernel
/// starts, once.
///
/// # Errors
/// [`Unavailable`] where the shared host views cannot be made; then nothing is
/// registered.
pub fn register(
    kernel: &mut Kernel,
    profile: &'static Profile,
    inputs: Inputs,
) -> Result<usize, Unavailable> {
    let resources = HostResources::try_new().map_err(|_| Unavailable::NoRoom)?;
    let device: Rc<DesktopDevice> = Rc::new(DesktopDevice::new(profile));
    let mut wiring = HostWiring::new()
        .ui(profile.device.memory)
        .device(device)
        .work(WorkInputs::new(inputs.view.clone(), profile.device.memory));
    // HTTP over the card (saved and sent files); no card, no `core.net`.
    let transport = inputs.storage.clone().map(|storage| {
        let transport = Rc::new(crate::native_net::NativeNet::new(
            storage.clone(),
            inputs.view.clone(),
        ));
        // TCP with TLS (`paperpaw_serve::secure`) and UDP broadcast. The
        // same transport tells core of the network for both services: core
        // combines their demand.
        let sockets = Rc::new(crate::native_sock::NativeSock::new());
        wiring = std::mem::take(&mut wiring)
            .net(NetInputs::new(
                storage.clone(),
                transport.clone(),
                transport.clone(),
                // The one pool and its TLS sessions are the same in both
                // wirings (`SockLimits::large`: 4).
                NetLimits::new(0, 8).tls_sessions(4),
            ))
            .sockets(
                SockInputs::new(
                    storage,
                    sockets.clone(),
                    transport.clone(),
                    SockLimits::large(0),
                )
                .tls(sockets.clone()),
            );
        (transport, sockets)
    });
    match (inputs.storage, sealer(inputs.simulated)) {
        (None, _) => eprintln!("native plugins: no secrets (the card cannot be opened)"),
        (Some(storage), Ok(sealer)) => {
            wiring = wiring.secrets(
                SecretsInputs::new(storage, sealer)
                    .with_rng(Rc::new(crate::sealing::SystemRng))
                    .on_unreadable(Rc::new(|| {
                        eprintln!("the secrets file could not be read");
                    })),
            );
        }
        (Some(_), Err(why)) => eprintln!("native plugins: no secrets (no key: {why})"),
    }
    let wired = resources.wire(wiring).map_err(Unavailable::Wiring)?;
    // Values an older version kept veiled are sealed now (none to do on a
    // fresh card).
    if let Some(secrets) = wired.secrets()
        && let Err(status) = secrets.upgrade()
    {
        eprintln!("native plugins: the secrets file could not be upgraded ({status})");
    }
    let on = Wired {
        secrets: wired.secrets().is_some(),
        work: true,
        net: wired.net().is_some(),
        sockets: wired.sockets().is_some(),
    };
    *inputs.net.borrow_mut() =
        transport
            .zip(wired.net().cloned())
            .map(|((transport, sockets), reports)| Net {
                transport,
                reports,
                sockets: wired.sockets().cloned().map(|wired| (sockets, wired)),
            });
    let depth = FlatDepth::try_new().map_err(|_| Unavailable::NoRoom)?;
    let host = metadata::Host {
        abi_major: 7,
        minor: paperpaw_native::host_glue::MINOR,
        profile: crate::native_runtime::host_profile(),
        services: offered(on),
        features: features(on),
    };
    // SAFETY: the offered services are the ones wired above (`offered`), and
    // the plugins are the app's linked ones, as the helper's contract says.
    let outcome = unsafe {
        paperpaw_app::register_native_plugins(
            kernel,
            &host,
            &resources,
            &depth,
            DesktopServices::new,
            teardown_report,
            linked_report,
        )
    };
    let mut registered = 0;
    for (id, status) in outcome {
        match status {
            Ok(()) => registered += 1,
            Err(why) => eprintln!("native plugin {id}: not registered: {why:?}"),
        }
    }
    Ok(registered)
}
