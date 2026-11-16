//! Contributions given to the builder as a closure (#244, slice D2's target
//! for tests): what a test fixture wrote as a type with `impl Plugin` it
//! writes as [`KernelBuilder::items`], and names neither `Plugin` nor
//! `Registrar`.
//!
//! [`Items`] is today another name for the SDK's `Registrar`, so every call a
//! fixture made in `register` is the same call here: handlers that get the
//! live `Env`, catalogs (`add_translations`), screens that are still an old
//! `Screen`. Slice D2 puts the kernel's own builder behind the name and the
//! calls stay; a fixture ported to this is not touched again when `Plugin`
//! and `Registrar` go.

use super::KernelBuilder;
use paperpaw_sdk::{Manifest, Plugin, RegisterError, Registrar};

/// What a closure given to [`KernelBuilder::items`] adds its contributions
/// with. The calls are those of the SDK's `Registrar`; name this, not that.
pub type Items = Registrar;

/// The closure of [`KernelBuilder::items`] as the plugin the kernel still
/// keeps its contributions by.
struct Given<F> {
    manifest: Manifest,
    register: F,
}

impl<F: Fn(&mut Items) -> Result<(), RegisterError>> Plugin for Given<F> {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    fn register(&self, items: &mut Registrar) -> Result<(), RegisterError> {
        (self.register)(items)
    }
}

impl KernelBuilder {
    /// Adds what `register` contributes, under `manifest` (its id is the
    /// namespace). It is a plugin in every way [`KernelBuilder::plugin`]'s
    /// is: listed, switched off and on, registered again when the plugins
    /// change (so `register` may run more than once), and one that fails
    /// leaves nothing behind.
    pub fn items(
        self,
        manifest: Manifest,
        register: impl Fn(&mut Items) -> Result<(), RegisterError> + 'static,
    ) -> Self {
        self.plugin(Given { manifest, register })
    }

    /// As [`KernelBuilder::items`], off until the user switches it on
    /// ([`KernelBuilder::plugin_off`]).
    pub fn items_off(
        self,
        manifest: Manifest,
        register: impl Fn(&mut Items) -> Result<(), RegisterError> + 'static,
    ) -> Self {
        self.plugin_off(Given { manifest, register })
    }
}
