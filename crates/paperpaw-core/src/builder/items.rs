//! Contributions given to the builder as a closure (#244): what registers in
//! the kernel without a plugin file is a manifest and a function over
//! [`Items`] ([`KernelBuilder::items`]).
//!
//! [`Items`] is the kernel's own type (until slice D4b it was the SDK's
//! `Registrar`), with the calls such a function makes: handlers that get the
//! live `Place`, catalogs (`add_translations`), screens that are still an old
//! `Screen`.
//!
//! The builder's list of what registers is a list of [`Source`]s: the kernel
//! registers each through a function over `Items` (`Kernel::add_items`).

use super::KernelBuilder;
use crate::Kernel;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::borrow::Cow;
use paperpaw_sdk::{BuiltInFont, BuiltInFonts, FontFace, Id, Manifest, Priority, RegisterError};

use crate::kit::contribution::item::Contribution;

mod calls;

pub use calls::Items;
pub(crate) use calls::Parts;

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
                    &|items| register_built_in(fonts, &mut |c| items.add(c)),
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

/// What a program's built-in fonts add: their translations, each font kept
/// in the program with its role bound to it, the other roles' bindings, and
/// the faces read from files.
fn register_built_in(
    fonts: &BuiltInFonts,
    add: &mut dyn FnMut(Contribution) -> Result<(), RegisterError>,
) -> Result<(), RegisterError> {
    let binding = |role: &Id, font: &Id| Contribution::FontBinding {
        role: role.clone(),
        font: font.clone(),
        priority: Priority::DEFAULT,
    };
    for source in fonts.translations {
        add(Contribution::Translations(Cow::Borrowed(*source)))?;
    }
    for font in fonts.fonts {
        match font {
            BuiltInFont::Data { id, role, .. } => {
                if let Some(spec) = fonts.font(id) {
                    add(Contribution::Font(Box::new(spec?)))?;
                }
                add(binding(role, id))?;
            }
            BuiltInFont::Role { role, font } => add(binding(role, font))?,
            BuiltInFont::File {
                id,
                path,
                family,
                bold,
                italic,
            } => add(Contribution::FontFace(Box::new(FontFace::new(
                id.clone(),
                *path,
                *family,
                *bold,
                *italic,
            ))))?,
        }
    }
    Ok(())
}
