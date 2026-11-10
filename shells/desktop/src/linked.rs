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

use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_core::native_host::device_service::DEVICE_SERVICES;
use paperpaw_core::native_host::flat::FlatDepth;
use paperpaw_core::native_host::shell::DATA_SERVICES;
use paperpaw_core::native_host::{HostResources, HostWiring, TeardownStep};
use paperpaw_native::metadata::{self, Service};

use crate::device_host::DesktopDevice;
use crate::host_services::DesktopServices;
use crate::profiles::Profile;

/// The services this shell wires for linked plugins: the data services of the
/// shared host table, `core.device` over the profile's device, and the ones
/// the table serves from the kernel's own views (`core.ui`, `core.registry`).
/// A service not named here is not offered: a plugin that needs it is refused
/// at admission.
fn offered() -> &'static [Service<'static>] {
    use std::sync::OnceLock;
    const OWN: &[&str] = &["core.ui", "core.registry"];
    static OFFERED: OnceLock<Vec<Service<'static>>> = OnceLock::new();
    OFFERED.get_or_init(|| {
        DATA_SERVICES
            .iter()
            .chain(DEVICE_SERVICES)
            .copied()
            .chain(OWN.iter().map(|key| Service {
                key,
                major: 1,
                semantic_revision: 0,
                min_table_bytes: 0,
            }))
            .collect()
    })
}

/// A linked plugin's diagnostics from the kernel (a refused declaration, a
/// failed callback) go to stderr.
fn linked_report(_: &DesktopServices, tag: u32, status: i32) {
    eprintln!("native plugin: call {tag} ended with {status}");
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
pub fn register(kernel: &mut Kernel, profile: &'static Profile) -> Result<usize, Unavailable> {
    let resources = HostResources::try_new().map_err(|_| Unavailable::NoRoom)?;
    let device: Rc<DesktopDevice> = Rc::new(DesktopDevice::new(profile));
    resources
        .wire(HostWiring::new().ui(profile.device.memory).device(device))
        .map_err(Unavailable::Wiring)?;
    let depth = FlatDepth::try_new().map_err(|_| Unavailable::NoRoom)?;
    let host = metadata::Host {
        abi_major: 7,
        minor: paperpaw_native::host_glue::MINOR,
        profile: crate::native_runtime::host_profile(),
        services: offered(),
        features: paperpaw_app::NATIVE_FEATURES,
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
