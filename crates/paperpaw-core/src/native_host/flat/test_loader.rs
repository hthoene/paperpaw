//! Shared fixture route: the kernel supplies the host through CodeFile.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use crate::Plugin;
use crate::{CodeFile, CodeLoader, Kernel, KernelBuilder};
use alloc::{boxed::Box, format};
use paperpaw_sdk::{MemoryStorage, PluginProblem, native::Native};

struct Loader<F>(F);

/// The adapter a test made, registered as the plugin's own when the kernel
/// admits it (a hosted plugin's registrations reach the kernel so too).
struct Made(Box<dyn Plugin>);
impl crate::NativeAdmission for Made {
    fn admit(self: Box<Self>, kernel: &mut Kernel, _: &str) -> Result<(), PluginProblem> {
        let manifest = self.0.manifest();
        kernel.register_plugin(self.0.as_ref(), manifest);
        Ok(())
    }
}

impl<F> CodeLoader for Loader<F>
where
    F: FnMut(&CodeFile<'_>) -> Result<Box<dyn Plugin>, PluginProblem>,
{
    fn load_native(
        &mut self,
        _: &dyn paperpaw_platform::Storage,
        code: &CodeFile<'_>,
    ) -> Result<Option<Box<dyn crate::NativeAdmission>>, PluginProblem> {
        let made = (self.0)(code)?;
        Ok(Some(Box::new(Made(made))))
    }
}

pub(crate) fn load<F>(builder: KernelBuilder, ids: &[&str], factory: F) -> Kernel
where
    F: FnMut(&CodeFile<'_>) -> Result<Box<dyn Plugin>, PluginProblem> + 'static,
{
    let mut storage = MemoryStorage::new();
    for id in ids {
        storage = storage
            .with(
                &format!("plugins/{id}/plugin.ini"),
                "[plugin]\nversion = 1.0.0\n",
            )
            .with(&format!("plugins/{id}/code.ppn"), "PPNP");
        assert!(paperpaw_sdk::native::remember_code(&storage, id, &[7; 32]));
    }
    builder
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(Loader(factory)))
        .build()
        .unwrap()
}
