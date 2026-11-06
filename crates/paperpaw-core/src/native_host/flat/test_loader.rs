//! Shared fixture route: the kernel supplies the host through CodeFile.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use crate::{CodeFile, CodeLoader, Kernel, KernelBuilder};
use alloc::{boxed::Box, format};
use paperpaw_sdk::{MemoryStorage, Plugin, PluginProblem, native::Native};

struct Loader<F>(F);
impl<F> CodeLoader for Loader<F>
where
    F: FnMut(&CodeFile<'_>) -> Result<Box<dyn Plugin>, PluginProblem>,
{
    fn load(
        &mut self,
        _: &dyn paperpaw_platform::Storage,
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        (self.0)(code)
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
