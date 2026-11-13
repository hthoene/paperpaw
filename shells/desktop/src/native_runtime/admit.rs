//! A desktop native plugin handed to the kernel to admit (H1b, todo 367).
//!
//! The loader checks the library and hosts it ([`super::admission`]); the
//! kernel admits it when it loads the code plugins at the start
//! ([`paperpaw_core::CodeLoader::load_native`]): `make` runs there, under the
//! kernel's registration transaction, and its declarations (screens,
//! settings, actions, …) and callbacks are registered together or not at all.
//! A refusal destroys the hosted plugin (its library is unmapped, its private
//! copy removed) and says why as the plugin's problem.

use paperpaw_core::native_host::{self, flat::FlatDepth, shell};
use paperpaw_core::{Kernel, NativeAdmission, sdk::PluginProblem};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};

use super::owned::dev::CopyModule;
use crate::host_services::DesktopServices;

/// A hosted plugin and the device-wide dispatch budget it runs under.
pub(crate) struct DesktopAdmission {
    hosted: shell::Hosted<DesktopServices, CopyModule>,
    depth: FlatDepth,
}

impl DesktopAdmission {
    pub(crate) fn new(
        hosted: shell::Hosted<DesktopServices, CopyModule>,
        depth: FlatDepth,
    ) -> Self {
        Self { hosted, depth }
    }
}

/// The kernel's diagnostics of an admitted plugin (a refused declaration, a
/// failed callback) go to the log.
fn report(_: &DesktopServices, plugin: &str, step: u32, status: i32) {
    eprintln!("native plugin {plugin}: admission step {step}: status {status}");
}

/// Why a plugin was refused, as the plugin manager says it.
fn problem(error: &shell::Error) -> PluginProblem {
    match error {
        shell::Error::Allocation | shell::Error::Status(ERR_NO_MEMORY) => PluginProblem::NoRoom,
        // A declaration or requirement this PaperPaw does not serve.
        shell::Error::Status(ERR_UNSUPPORTED) => PluginProblem::NeedsNewerPaperPaw,
        // No plugin at all (nothing declared, nothing registered), or a record
        // that does not decode.
        shell::Error::Status(ERR_INVALID) => PluginProblem::Damaged,
        shell::Error::Lifecycle(native_host::Error::Memory | native_host::Error::Capacity) => {
            PluginProblem::NoRoom
        }
        _ => PluginProblem::Damaged,
    }
}

impl NativeAdmission for DesktopAdmission {
    fn admit(self: Box<Self>, kernel: &mut Kernel, folder: &str) -> Result<(), PluginProblem> {
        let Self { hosted, depth } = *self;
        kernel
            .register_native_in(hosted, &depth, Some(folder), report)
            .map_err(|error| {
                eprintln!("native plugin {folder}: refused: {error:?}");
                problem(&error)
            })
    }
}
