//! A desktop native plugin handed to the kernel to admit (H1b, todo 367).
//!
//! The loader checks the library and hosts it ([`super::admission`]); the
//! kernel admits it when it loads the code plugins at the start
//! ([`paperpaw_core::CodeLoader::load_native`]): `make` runs there, under the
//! kernel's registration transaction, and its declarations (screens,
//! settings, actions, …) and callbacks are registered together or not at all
//! ([`HostedAdmission`], the same on every shell). A refusal destroys the
//! hosted plugin (its library is unmapped, its private copy removed) and says
//! why as the plugin's problem.

use paperpaw_core::native_host::flat::FlatDepth;
use paperpaw_core::native_host::hosted_admission::HostedAdmission;
use paperpaw_core::native_host::shell;

use super::owned::dev::CopyModule;
use crate::host_services::DesktopServices;

/// A hosted plugin and the device-wide dispatch budget it runs under.
pub(crate) fn admission(
    hosted: shell::Hosted<DesktopServices, CopyModule>,
    depth: FlatDepth,
) -> HostedAdmission<DesktopServices, CopyModule> {
    HostedAdmission::new(hosted, depth, report, refused)
}

/// The kernel's diagnostics of an admitted plugin (a refused declaration, a
/// failed callback) go to the log.
fn report(_: &DesktopServices, plugin: &str, step: u32, status: i32) {
    eprintln!("native plugin {plugin}: admission step {step}: status {status}");
}

fn refused(folder: &str, error: &shell::Error) {
    eprintln!("native plugin {folder}: refused: {error:?}");
}
