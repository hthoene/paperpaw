//! Borrowed secret requests and service access. The host enforces owner stamping
//! and `uses.secrets`; an editor can list/change other owners but never read values.
//! Call [`wipe`] on caller-owned buffers after consuming secret values or random
//! bytes used as key material, and clear any separate copies as well.
use super::wire::services_wire::core_secrets_v1 as schema;
use super::{Error, Host, declare::records_wire as wire, services::Service};
mod calls;
pub use calls::wipe;
mod paging;
pub use paging::{SecretEntry, SecretPage, SecretPageEnd};
mod types;
pub use types::{RandomBytes, SecretList, SecretName, SecretValue, SecretWrite};

/// A service handle retaining no callback binding. Release it explicitly.
#[derive(Debug)]
#[must_use = "release the secrets service when finished"]
pub struct Secrets {
    service: Service,
}
impl Secrets {
    /// Open the metadata-declared secrets service.
    ///
    /// # Errors
    /// Returns schema, permission, availability or allocation refusals.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }
    /// Release this handle using the current callback's host.
    ///
    /// # Errors
    /// Returns the host's stale-handle or resource refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod codec_tests;

#[cfg(test)]
mod allocation_tests;
