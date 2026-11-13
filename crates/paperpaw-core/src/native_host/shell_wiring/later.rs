//! A service wired now and made at its first call (#248): the shell's inputs
//! are kept until then, the device after. A device that no plugin asks for is
//! never made; one whose making finds no room is `ERR_NO_MEMORY` for that
//! call, and the inputs stay for the next.

use crate::native_host::shared::Shared;
use core::cell::{Cell, OnceCell};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};

pub(crate) struct Later<I, D> {
    inputs: Cell<Option<I>>,
    made: OnceCell<Shared<D>>,
}

impl<I, D> core::fmt::Debug for Later<I, D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Later")
            .field("made", &self.made.get().is_some())
            .finish_non_exhaustive()
    }
}

impl<I, D> Later<I, D> {
    pub(crate) fn new(inputs: I) -> Self {
        Self {
            inputs: Cell::new(Some(inputs)),
            made: OnceCell::new(),
        }
    }

    /// The device, where a call made it.
    pub(crate) fn made(&self) -> Option<&D> {
        self.made.get().map(|device| &**device)
    }

    /// The device, made now from the inputs and `parts` (what the host lends
    /// it) where it is not yet. The block is had before anything is built:
    /// without room nothing is, and the inputs stay.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`; what `parts` answers; `ERR_STATE` when a making is
    /// under way already.
    pub(crate) fn make<P>(
        &self,
        parts: impl FnOnce(&I) -> Result<P, i32>,
        build: impl FnOnce(I, P) -> D,
    ) -> Result<&Shared<D>, i32> {
        if let Some(device) = self.made.get() {
            return Ok(device);
        }
        let inputs = self.inputs.take().ok_or(ERR_STATE)?;
        let parts = match parts(&inputs) {
            Ok(parts) => parts,
            Err(status) => {
                self.inputs.set(Some(inputs));
                return Err(status);
            }
        };
        match Shared::try_new_from((inputs, parts), |(inputs, parts)| build(inputs, parts)) {
            Ok(device) => {
                let _ = self.made.set(device);
                self.made.get().ok_or(ERR_STATE)
            }
            Err((inputs, _)) => {
                self.inputs.set(Some(inputs));
                Err(ERR_NO_MEMORY)
            }
        }
    }
}
