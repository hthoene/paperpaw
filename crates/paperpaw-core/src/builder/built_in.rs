//! The fonts the program is built with (`KernelBuilder::built_in_fonts`):
//! a table, registered and listed as a plugin of data is.

use paperpaw_sdk::{BuiltInFonts, Manifest, Plugin, RegisterError, Registrar};

/// A table of built-in fonts in the kernel's list of what registers.
pub(super) struct BuiltIn(pub(super) &'static BuiltInFonts);

impl Plugin for BuiltIn {
    fn manifest(&self) -> Manifest {
        self.0.manifest.clone()
    }

    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        self.0.register(registrar)
    }
}
