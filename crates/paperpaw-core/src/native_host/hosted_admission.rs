//! A hosted native plugin handed to the kernel to admit, for every shell
//! (`CodeLoader::load_native`): the desktop's, the Android app's and the
//! X4's differ only in the host type they run it on and where a refusal is
//! said. What the kernel does with it is [`Kernel::register_native_in`]: `make`
//! runs there, under the kernel's registration transaction, and the plugin's
//! declarations (screens, settings, actions, fonts, ...) and the callbacks it
//! committed are registered together or not at all. A refusal destroys the
//! hosted plugin and says why as the plugin's problem, the same on every shell.
use alloc::boxed::Box;

use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED, Services};
use paperpaw_sdk::PluginProblem;

use super::flat::FlatDepth;
use super::shell::{Error, Hosted};
use crate::{Kernel, NativeAdmission};

/// A hosted plugin and the device-wide dispatch budget it runs under, ready
/// for the kernel to admit.
#[allow(missing_debug_implementations, reason = "holds the hosted plugin")]
pub struct HostedAdmission<S: Services + 'static, M: 'static> {
    hosted: Hosted<S, M>,
    depth: FlatDepth,
    report: fn(&S, &str, u32, i32),
    refused: fn(&str, &Error),
}

impl<S: Services + 'static, M: 'static> HostedAdmission<S, M> {
    /// `hosted`, to run under `depth` (a share of the device's one counter:
    /// the kernel refuses a plugin that brings another). `report` is the
    /// shell's sink for what the kernel tells of the plugin's callbacks, and
    /// `refused` is told, once, why the kernel refused the plugin and which
    /// folder it was (the shell logs it; nothing allocates).
    #[must_use]
    pub fn new(
        hosted: Hosted<S, M>,
        depth: FlatDepth,
        report: fn(&S, &str, u32, i32),
        refused: fn(&str, &Error),
    ) -> Self {
        // A plugin admitted from a file is announced to the shell at every entry.
        hosted.owner().context().announce_entries();
        Self {
            hosted,
            depth,
            report,
            refused,
        }
    }
}

/// Why the kernel refused a plugin, as the plugin manager says it.
#[must_use]
pub fn problem_of(error: &Error) -> PluginProblem {
    match error {
        Error::Allocation | Error::Status(ERR_NO_MEMORY) => PluginProblem::NoRoom,
        // A declaration or requirement this PaperPaw does not serve.
        Error::Status(ERR_UNSUPPORTED) => PluginProblem::NeedsNewerPaperPaw,
        // No plugin at all (nothing declared, nothing registered), or a record
        // that does not decode.
        Error::Status(ERR_INVALID) => PluginProblem::Damaged,
        Error::Lifecycle(super::Error::Memory | super::Error::Capacity) => PluginProblem::NoRoom,
        // The shell refused to run its code (the X4: its stack's band is not armed),
        // as `load_native` says it.
        Error::Refused => PluginProblem::Unsupported,
        _ => PluginProblem::Damaged,
    }
}

impl<S: Services + 'static, M: 'static> HostedAdmission<S, M> {
    /// [`NativeAdmission::admit`] on the value: a shell that wraps the
    /// admission (to measure it) holds it without a second allocation.
    ///
    /// # Errors
    /// Why the kernel refused the plugin, as a code plugin's problem.
    pub fn admit_in(self, kernel: &mut Kernel, folder: &str) -> Result<(), PluginProblem> {
        let Self {
            hosted,
            depth,
            report,
            refused,
        } = self;
        kernel
            .register_native_in(hosted, &depth, Some(folder), report)
            .map_err(|error| {
                refused(folder, &error);
                problem_of(&error)
            })
    }
}

impl<S: Services + 'static, M: 'static> NativeAdmission for HostedAdmission<S, M> {
    fn admit(self: Box<Self>, kernel: &mut Kernel, folder: &str) -> Result<(), PluginProblem> {
        (*self).admit_in(kernel, folder)
    }
}
