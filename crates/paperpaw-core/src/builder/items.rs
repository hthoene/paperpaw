//! Contributions given to the builder as a closure (#244): what registers in
//! the kernel without a plugin file is a manifest and a function over
//! [`Items`] ([`KernelBuilder::items`]).
//!
//! [`Items`] is the kernel's own type (until slice D4b it was the SDK's
//! `Registrar`), with the calls such a function makes: handlers that get the
//! live `Env`, catalogs (`add_translations`), screens that are still an old
//! `Screen`.
//!
//! The builder's list of what registers is a list of [`Source`]s: the kernel
//! registers each through a function over `Items` (`Kernel::add_items`).

use super::KernelBuilder;
use crate::Kernel;
use alloc::boxed::Box;
use alloc::string::String;
use paperpaw_sdk::{BuiltInFonts, Manifest, RegisterError};

mod calls;

pub use calls::Items;

/// One entry of the builder's list of what registers, in the order given.
pub(super) enum Source {
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
            Self::Fonts(fonts) => fonts.manifest.clone(),
            Self::Given(given) => given.manifest.clone(),
        }
    }

    /// Registers it with `kernel`, or only lists it where `disabled` names it.
    pub(super) fn add_to(&self, kernel: &mut Kernel, disabled: &[String]) {
        match self {
            Self::Fonts(fonts) => {
                kernel.add_items(
                    fonts.manifest.clone(),
                    &|items| fonts.register(&mut |c| items.add(c)),
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
    /// namespace). It is a plugin in every way: listed, switched off and on,
    /// registered again when the plugins change (so `register` may run more
    /// than once), and one that fails leaves nothing behind. Plugins added
    /// later win priority ties.
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

    /// As [`KernelBuilder::items`], off until the user switches it on (e.g.
    /// in the plugin manager), for features not everyone wants.
    pub fn items_off(
        mut self,
        manifest: Manifest,
        register: impl Fn(&mut Items) -> Result<(), RegisterError> + 'static,
    ) -> Self {
        self.0.off.push(String::from(manifest.id.as_str()));
        self.items(manifest, register)
    }
}
