//! Contributions given to the builder as a closure (#244, slice D2's target
//! for tests): what a test fixture wrote as a type with `impl Plugin` it
//! writes as [`KernelBuilder::items`], and names neither `Plugin` nor
//! `Registrar`.
//!
//! [`Items`] is today another name for the SDK's `Registrar`, so every call a
//! fixture made in `register` is the same call here: handlers that get the
//! live `Env`, catalogs (`add_translations`), screens that are still an old
//! `Screen`. The calls stay when `Plugin` and `Registrar` go; a fixture
//! ported to this is not touched again.
//!
//! The builder's list of what registers is a list of [`Source`]s: the kernel
//! registers each through a function over `Items` (`Kernel::add_items`), and
//! an old `Plugin` is one of them in an adapter, never the reverse.

use super::KernelBuilder;
use crate::Kernel;
use alloc::boxed::Box;
use alloc::string::String;
use paperpaw_sdk::{BuiltInFonts, Manifest, Plugin, RegisterError, Registrar};

/// What a closure given to [`KernelBuilder::items`] adds its contributions
/// with. The calls are those of the SDK's `Registrar`; name this, not that.
pub type Items = Registrar;

/// One entry of the builder's list of what registers, in the order given.
pub(super) enum Source {
    /// An old [`Plugin`] ([`KernelBuilder::plugin`]); goes with the trait.
    Old(Box<dyn Plugin>),
    /// The fonts the program is built with ([`KernelBuilder::built_in_fonts`]).
    Fonts(&'static BuiltInFonts),
    /// A closure ([`KernelBuilder::items`]). Boxed, so that an entry of the
    /// list stays three words whatever a manifest holds.
    Given(Box<Given>),
}

/// What a closure given to [`KernelBuilder::items`] is kept as.
type Register = dyn Fn(&mut Items) -> Result<(), RegisterError>;

/// The closure of [`KernelBuilder::items`] and what it registers under.
pub(super) struct Given {
    manifest: Manifest,
    register: Box<Register>,
}

impl Source {
    /// Whose contributions these are.
    pub(super) fn manifest(&self) -> Manifest {
        match self {
            Self::Old(plugin) => plugin.manifest(),
            Self::Fonts(fonts) => fonts.manifest.clone(),
            Self::Given(given) => given.manifest.clone(),
        }
    }

    /// Registers it with `kernel`, or only lists it where `disabled` names it.
    pub(super) fn add_to(&self, kernel: &mut Kernel, disabled: &[String]) {
        match self {
            Self::Old(plugin) => kernel.add_plugin(plugin.as_ref(), disabled),
            Self::Fonts(fonts) => {
                kernel.add_items(
                    fonts.manifest.clone(),
                    &|items| fonts.register(items),
                    disabled,
                );
            }
            Self::Given(given) => {
                kernel.add_items(given.manifest.clone(), &given.register, disabled);
            }
        }
    }
}

impl KernelBuilder {
    /// Adds what `register` contributes, under `manifest` (its id is the
    /// namespace). It is a plugin in every way [`KernelBuilder::plugin`]'s
    /// is: listed, switched off and on, registered again when the plugins
    /// change (so `register` may run more than once), and one that fails
    /// leaves nothing behind.
    pub fn items(
        mut self,
        manifest: Manifest,
        register: impl Fn(&mut Items) -> Result<(), RegisterError> + 'static,
    ) -> Self {
        let register = Box::new(register);
        self.0
            .plugins
            .push(Source::Given(Box::new(Given { manifest, register })));
        self
    }

    /// As [`KernelBuilder::items`], off until the user switches it on
    /// ([`KernelBuilder::plugin_off`]).
    pub fn items_off(
        mut self,
        manifest: Manifest,
        register: impl Fn(&mut Items) -> Result<(), RegisterError> + 'static,
    ) -> Self {
        self.0.off.push(String::from(manifest.id.as_str()));
        self.items(manifest, register)
    }
}
