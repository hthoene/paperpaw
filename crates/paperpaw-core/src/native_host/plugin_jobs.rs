//! The plugin jobs the queue can hold, kept inline (no box: a job is accepted
//! at submit with the queue's room reserved, and nothing here allocates a
//! second time when a plugin triggers it). A new kind of job is a new
//! variant that delegates to its [`PluginJob`].
use super::dual_boot_job::DualBoot;
use super::plugin_check::PluginCheck;
use super::plugin_install::PluginInstall;
use super::plugin_job::{Answer, JobEnv, JobStep, PluginJob};
use super::plugin_remove::PluginRemove;

/// One plugin job.
#[allow(
    clippy::large_enum_variant,
    reason = "kept inline in the queue's reserved room: a box would be an allocation a plugin can make fail"
)]
pub(crate) enum PluginJobs {
    /// `check_file` of a plugin folder.
    Check(PluginCheck),
    /// `install_plugin` of a plugin folder.
    Install(PluginInstall),
    /// `remove_plugin` of an installed plugin.
    Remove(PluginRemove),
    /// `firmware_dual_boot`.
    DualBoot(DualBoot),
    /// A scripted job of the driver's tests.
    #[cfg(test)]
    Script(super::firmware_driver::plugin_tests::Script),
}

macro_rules! each {
    ($self:expr, $job:ident => $body:expr) => {
        match $self {
            Self::Check($job) => $body,
            Self::Install($job) => $body,
            Self::Remove($job) => $body,
            Self::DualBoot($job) => $body,
            #[cfg(test)]
            Self::Script($job) => $body,
        }
    };
}

impl PluginJob for PluginJobs {
    fn owner(&self) -> u64 {
        each!(self, job => job.owner())
    }
    fn request(&self) -> u32 {
        each!(self, job => job.request())
    }
    fn kind(&self) -> u32 {
        each!(self, job => job.kind())
    }
    #[inline(never)]
    fn step(&mut self, env: &JobEnv<'_>) -> JobStep {
        each!(self, job => job.step(env))
    }
    fn answer(&mut self, answer: Answer) {
        each!(self, job => job.answer(answer));
    }
    fn unloaded(&mut self) {
        each!(self, job => job.unloaded());
    }
    fn forgotten(&mut self, ok: bool) {
        each!(self, job => job.forgotten(ok));
    }
    fn abort(&mut self, reason: u8) {
        each!(self, job => job.abort(reason));
    }
    fn must_finish(&self) -> bool {
        each!(self, job => job.must_finish())
    }
    fn keep_without_sender(&self) -> bool {
        each!(self, job => job.keep_without_sender())
    }
    fn image_waiting(&mut self, waiting: bool) {
        each!(self, job => job.image_waiting(waiting));
    }
    fn after(&self) -> super::dual_boot_job::After {
        each!(self, job => job.after())
    }
}
