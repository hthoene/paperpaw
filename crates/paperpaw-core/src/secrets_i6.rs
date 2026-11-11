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
        secrets_text::{open_text_noting, seal_text},
    },
};
use alloc::{rc::Rc, string::String};
use paperpaw_sdk::secrets::Secrets;

/// Whether a secret of this (unescaped) id can be sealed: `owner.name`.
pub(crate) fn sealable_id(id: &str) -> bool {
    id.split_once('.')
        .is_some_and(|(owner, name)| !owner.is_empty() && !name.is_empty())
}

impl Kernel {
    /// Tells the kernel the shell wired `core.secrets` (decision 140): from
    /// now on interface 6's passwords are opened and written through its
    /// sealer, whether or not a native plugin is ever admitted. A shell calls
    /// it after [`HostResources::wire`](crate::native_host::HostResources::wire)
    /// and before [`Kernel::start`]; a device whose `core.secrets` seals would
    /// otherwise write interface 6's values veiled when no native plugin runs.
    pub fn wire_secrets(&mut self, secrets: &crate::native_host::shell_wiring::SecretsLifecycle) {
        self.secrets_wired = Some(secrets.host().clone());
        self.secrets_opened = false;
        self.open_sealed_secrets();
    }

    /// The wired `core.secrets` host, if its sealer seals: the one the shell
    /// gave [`Self::wire_secrets`], else the device pool's once a native
    /// plugin was admitted.
    pub(crate) fn secrets_host(&self) -> Option<Rc<dyn SecretsHost>> {
        let host = match &self.secrets_wired {
            Some(host) => host.clone(),
            None => self
                .shared
                .native_instances
                .hosts()?
                .secrets()?
                .host
                .clone(),
        };
        host.sealer().seals().then_some(host)
    }

    /// The list of `text` (the file), sealed lines opened by `host`'s sealer.
    pub(crate) fn decode_secrets(text: &str, host: Option<&Rc<dyn SecretsHost>>) -> Secrets {
        match host {
            Some(host) => {
                let (opened, unopened) = open_text_noting(text, host.sealer());
                if unopened {
                    host.unopened();
                }
                Secrets::decode(&opened)
            }
            None => Secrets::decode(text),
        }
    }

    /// The text to write for `plain` (what [`Secrets::encode`] gave): sealed
    /// where a sealer seals, a line that did not change as it was in
    /// `previous` (the file as read).
    ///
    /// # Errors
    /// The sealer's status where it cannot seal now.
    pub(crate) fn sealed_secrets(
        plain: String,
        previous: &str,
        host: Option<&Rc<dyn SecretsHost>>,
    ) -> Result<String, i32> {
        match host {
            Some(host) => seal_text(&plain, previous, host.sealer()),
            None => Ok(plain),
        }
    }

    /// Once `core.secrets` is the kernel's (wired by the shell, or the pool
    /// after the first native admission): reads the file again through the
    /// sealer, once.
    pub(crate) fn open_sealed_secrets(&mut self) {
        if !self.secrets_opened && self.secrets_host().is_some() {
            self.secrets_opened = true;
            self.read_secrets();
        }
    }

    /// Tells the user once per start that saved passwords from another device
    /// (or under a lost key) cannot be opened here, whichever path found such
    /// a line first (a native plugin's call, or interface 6's reading). From
    /// the service pump, after the start: the notice waits behind a guard.
    pub(crate) fn secrets_notice(&mut self) {
        if self.secrets_host().is_some_and(|host| host.take_unopened()) {
            // Fallible, as every notice text: short of memory, no notice.
            let words = self.shared.translator.tr("core.secrets-unopened");
            let mut text = String::new();
            if text.try_reserve_exact(words.len()).is_err() {
                return;
            }
            text.push_str(words);
            self.notify(text);
        }
    }
}

#[cfg(test)]
#[path = "secrets_i6_tests.rs"]
mod tests;
