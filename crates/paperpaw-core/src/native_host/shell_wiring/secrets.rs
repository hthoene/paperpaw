//! What a shell gives `core.secrets` (`docs/abi/secrets.md`): the storage the
//! secrets file lives on, how values are kept ([`Sealer`]) and, if the
//! device has one, its random source ([`Rng`]).
//!
//! Core owns the file, its format, the ops and every rule of the service; the
//! shell chooses where the file is and the key that protects it. Values are
//! only ever written sealed (the X4: AES-256-GCM under a device key, decision
//! 138). A shell that has no key does not wire `core.secrets`: a plugin that
//! needs it is refused, and a [`Sealer`] that does not seal makes `keep`
//! answer `ERR_STATE` with nothing written (decision 140: no veiled or plain
//! value, ever). Values interface 6 wrote veiled are still read.
//!
//! The file is `.paperpaw/secrets.txt` on the storage given. A card removed
//! or a file that cannot be read answers `ERR_IO` to the plugin and calls
//! the notice given with [`SecretsInputs::on_unreadable`] once.
//!
//! After wiring, a shell calls [`SecretsLifecycle::upgrade`] once at start
//! (veiled values found on a device that seals are rewritten sealed) and
//! [`SecretsLifecycle::forget_plugin`] when the user deletes a plugin's data
//! (decision 124).
use super::*;
pub use crate::native_host::secrets_file::{SEALED_MAX, Sealer};
pub use crate::native_host::secrets_service::Rng;
use crate::native_host::secrets_service::{SecretsHost, SecretsServices};
use core::cell::Cell;

/// What a shell names for `core.secrets`.
#[derive(Clone)]
pub struct SecretsInputs {
    storage: Rc<dyn Storage>,
    sealer: Rc<dyn Sealer>,
    rng: Option<Rc<dyn Rng>>,
    unreadable: Option<Rc<dyn Fn()>>,
}

impl core::fmt::Debug for SecretsInputs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SecretsInputs")
            .field("seals", &self.sealer.seals())
            .field("rng", &self.rng.is_some())
            .finish_non_exhaustive()
    }
}

impl SecretsInputs {
    /// The file lives on `storage` (the card, or the app's private folder);
    /// `sealer` keeps the values.
    #[must_use]
    pub fn new(storage: Rc<dyn Storage>, sealer: Rc<dyn Sealer>) -> Self {
        Self {
            storage,
            sealer,
            rng: None,
            unreadable: None,
        }
    }

    /// The device's random source, for `random` (a device without one is not
    /// given one: there is no weaker stand-in).
    #[must_use]
    pub fn with_rng(mut self, rng: Rc<dyn Rng>) -> Self {
        self.rng = Some(rng);
        self
    }

    /// Called once, from inside a plugin's call, when the file cannot be read
    /// (show the user a notice; do not call the kernel).
    #[must_use]
    pub fn on_unreadable(mut self, notice: Rc<dyn Fn()>) -> Self {
        self.unreadable = Some(notice);
        self
    }

    pub(crate) fn host(self) -> Rc<dyn SecretsHost> {
        Rc::new(Adapter {
            inputs: self,
            unopened: Cell::new(false),
            told: Cell::new(false),
        })
    }
}

struct Adapter {
    inputs: SecretsInputs,
    /// A sealed value did not open since the start.
    unopened: Cell<bool>,
    /// The user was told of it this start.
    told: Cell<bool>,
}

impl SecretsHost for Adapter {
    fn storage(&self) -> &dyn Storage {
        &*self.inputs.storage
    }
    fn sealer(&self) -> &dyn Sealer {
        &*self.inputs.sealer
    }
    fn rng(&self) -> Option<&dyn Rng> {
        self.inputs.rng.as_deref()
    }
    fn unreadable(&self) {
        if let Some(notice) = &self.inputs.unreadable {
            notice();
        }
    }
    fn unopened(&self) {
        self.unopened.set(true);
    }
    fn take_unopened(&self) -> bool {
        self.unopened.get() && !self.told.replace(true)
    }
}

/// The calls the shell makes outside a plugin's call, once `core.secrets` is
/// wired ([`WiredHosts::secrets`]).
pub struct SecretsLifecycle(SecretsServices);

impl core::fmt::Debug for SecretsLifecycle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SecretsLifecycle").finish_non_exhaustive()
    }
}

impl SecretsLifecycle {
    pub(crate) fn new(host: Rc<dyn SecretsHost>) -> Self {
        Self(SecretsServices::new(host))
    }

    /// The wired host (the kernel's interface-6 code seals through it).
    pub(crate) fn host(&self) -> &Rc<dyn SecretsHost> {
        &self.0.host
    }

    /// First start after an update on a device that seals: rewrites the veiled
    /// values sealed. `Ok(true)` when the file was rewritten; `Ok(false)` when
    /// nothing had to change (or the device does not seal).
    ///
    /// # Errors
    /// `ERR_IO` (nothing changed), `ERR_NO_MEMORY`.
    pub fn upgrade(&self) -> Result<bool, i32> {
        self.0.upgrade()
    }

    /// Removes every secret of plugin `plugin` (its data was deleted by the
    /// user, decision 124), also those that do not open here.
    ///
    /// # Errors
    /// `ERR_IO` (nothing changed), `ERR_NO_MEMORY`, `ERR_INVALID`.
    pub fn forget_plugin(&self, plugin: &str) -> Result<(), i32> {
        self.0.forget_plugin(plugin)
    }
}
