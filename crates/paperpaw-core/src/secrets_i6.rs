//! Interface 6's passwords and keys on a device whose `core.secrets` seals
//! (decision 138, #1105 R3): the kernel opens what the file holds sealed and
//! writes what it keeps sealed, through the sealer the shell wired, so that
//! Wi-Fi, mail and the interface-6 plugins keep working beside the native
//! ones. Without a sealer nothing changes: the veiled file as ever.
//!
//! The sealed lines of the file are opened into the kernel's list (a plugin
//! reads its values with `Env::secrets` as before) and every save writes the
//! whole list sealed ([`native_host::secrets_text`]). A save that cannot seal
//! (on the X4 every seal needs the radio's noise for its nonce) is not
//! written: the change applies until the end and is not
//! [`saved`](paperpaw_sdk::secrets::Secrets::saved), as for a card that could
//! not be written.
use crate::{
    Kernel,
    native_host::{
        secrets_service::SecretsHost,
        secrets_text::{open_text, seal_text},
    },
};
use alloc::{rc::Rc, string::String};
use paperpaw_sdk::secrets::Secrets;

impl Kernel {
    /// The wired `core.secrets` host, if its sealer seals.
    pub(crate) fn secrets_host(&self) -> Option<Rc<dyn SecretsHost>> {
        let host = self
            .shared
            .native_instances
            .hosts()?
            .secrets()?
            .host
            .clone();
        host.sealer().seals().then_some(host)
    }

    /// The list of `text` (the file), sealed lines opened by `host`'s sealer.
    pub(crate) fn decode_secrets(text: &str, host: Option<&Rc<dyn SecretsHost>>) -> Secrets {
        match host {
            Some(host) => Secrets::decode(&open_text(text, host.sealer())),
            None => Secrets::decode(text),
        }
    }

    /// The text to write for `plain` (what [`Secrets::encode`] gave): sealed
    /// where a sealer seals.
    ///
    /// # Errors
    /// The sealer's status where it cannot seal now.
    pub(crate) fn sealed_secrets(
        plain: String,
        host: Option<&Rc<dyn SecretsHost>>,
    ) -> Result<String, i32> {
        match host {
            Some(host) => seal_text(&plain, host.sealer()),
            None => Ok(plain),
        }
    }

    /// After the first native plugin was admitted (the pool, and with it
    /// `core.secrets`, is the kernel's from then on): reads the file again
    /// through the sealer, once.
    pub(crate) fn open_sealed_secrets(&mut self) {
        if !self.secrets_opened && self.secrets_host().is_some() {
            self.secrets_opened = true;
            self.read_secrets();
        }
    }
}

#[cfg(test)]
#[path = "secrets_i6_tests.rs"]
mod tests;
