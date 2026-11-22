//! Interface 6's passwords and keys on a device whose `core.secrets` seals
//! (decision 138, #1105 R3): the kernel opens what the file holds sealed and
//! writes what it keeps sealed, through the sealer the shell wired, so that
//! Wi-Fi, mail and the interface-6 plugins keep working beside the native
//! ones. Without a sealer nothing changes: the veiled file as ever.
//!
//! The sealed lines of the file are opened into the kernel's list (a plugin
//! reads its values with `Place::secrets` as before) and every save writes the
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
use alloc::{rc::Rc, string::String, vec::Vec};
use paperpaw_sdk::{Id, digest::fnv1a64, secrets::Secrets};

/// The hashes of the sealed lines the user was told cannot be opened here,
/// one in hex a line (#466): kept on the card beside the secrets file, so the
/// notice comes once, not at every start, and again only for a line that was
/// not told yet. Written only when the user is told.
pub(crate) const TOLD_FILE: &str = ".paperpaw/secrets-told.txt";
/// The dialog's screen (a host screen, `host_screen::SECRETS`).
pub(crate) const SCREEN: Id = Id::new_static("core.secrets-unopened");
/// Its *Remove them*, emitted by the dialog ([`Kernel::remove_unopened_secrets`]).
pub(crate) const REMOVE: Id = Id::new_static("core.secrets-unopened-remove");
/// Its size limit: a hash for each line of the largest secrets file.
const TOLD_MAX: usize = 32 * 1024;

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

    /// The list of `text` (the file), sealed lines opened by `host`'s sealer,
    /// and the hashes of the sealed lines that did not open
    /// ([`open_text_noting`]).
    pub(crate) fn decode_secrets(
        text: &str,
        host: Option<&Rc<dyn SecretsHost>>,
    ) -> (Secrets, Vec<u64>) {
        match host {
            Some(host) => {
                let (opened, unopened) = open_text_noting(text, host.sealer());
                (Secrets::decode(&opened), unopened)
            }
            None => (Secrets::decode(text), Vec::new()),
        }
    }

    /// The secrets of the file as read, and what of it did not open: the
    /// notice looks at it again where that changed ([`Self::secrets_notice`]).
    pub(crate) fn take_secrets(&mut self, (secrets, unopened): (Secrets, Vec<u64>)) {
        self.shared.secrets = secrets;
        if unopened != self.secrets_unopened {
            self.secrets_unopened = unopened;
            self.secrets_unopened_changed = true;
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

    /// Tells the user that saved passwords from another device (or under a
    /// lost key) cannot be opened here, once (#466): a dialog to confirm, with
    /// *Remove them* and *OK*, where the file read through the sealer holds a
    /// line that did not open and that is not in [`TOLD_FILE`]. Then that file
    /// is written with the lines told, the one write; a later start reads it
    /// and says nothing. A line entered again leaves the file as it is. Over
    /// the settled home screen only, as the other start notices
    /// ([`Self::start_notice_may_show`]); the card is read only where what did
    /// not open changed. `true` where the dialog shows.
    pub(crate) fn offer_secrets_notice(&mut self) -> bool {
        if !self.secrets_unopened_changed || !self.start_notice_may_show() {
            return false;
        }
        self.secrets_unopened_changed = false;
        if self.secrets_unopened.is_empty() {
            return false;
        }
        let storage = self.shared.storage.clone();
        let told = told_lines(storage.as_ref());
        if self.secrets_unopened.iter().all(|line| told.contains(line)) {
            return false;
        }
        // Fallible: short of memory, no dialog (and told at the next start).
        let mut mark = String::new();
        if mark
            .try_reserve_exact(17 * self.secrets_unopened.len())
            .is_err()
        {
            return false;
        }
        self.shared
            .registry
            .screens
            .insert_fixed(&crate::host_screen::SECRETS);
        self.open(SCREEN, None, &paperpaw_sdk::Params::new());
        if !self.stack.last().is_some_and(|top| top.id == SCREEN) {
            return false;
        }
        self.waiting_notices.start_notice_shown = true;
        for line in &self.secrets_unopened {
            let _ = core::fmt::Write::write_fmt(&mut mark, format_args!("{line:016x}\n"));
        }
        // A card that takes no write tells again at the next start.
        let _ = storage.write(TOLD_FILE, mark.as_bytes());
        true
    }

    /// The dialog's *Remove them*: the lines of the file that did not open
    /// here (those the dialog told of, byte for byte) are removed; every
    /// other line stays as it is. Then the file is read again.
    pub(crate) fn remove_unopened_secrets(&mut self) {
        use paperpaw_sdk::secrets::FILE;
        let storage = self.shared.storage.clone();
        let Ok(Some(data)) =
            paperpaw_sdk::files::load_for_update(storage.as_ref(), FILE, crate::MAX_SECRETS)
        else {
            return;
        };
        let text = String::from_utf8_lossy(&data);
        let mut kept = String::new();
        if kept.try_reserve_exact(text.len()).is_err() {
            return;
        }
        for line in text.lines().map(|line| line.trim_end_matches('\r')) {
            if !self.secrets_unopened.contains(&fnv1a64(line.as_bytes())) {
                kept.push_str(line);
                kept.push('\n');
            }
        }
        if kept.len() == text.len() {
            return;
        }
        let written = if kept.is_empty() {
            storage.remove(FILE)
        } else {
            storage.write(FILE, kept.as_bytes())
        };
        if let Err(error) = written {
            self.warn_runtime(alloc::format!("{FILE}: not written ({error})"));
            return;
        }
        self.read_secrets();
    }
}

/// The hashes in [`TOLD_FILE`]; none where it is missing or damaged.
fn told_lines(storage: &dyn paperpaw_platform::Storage) -> Vec<u64> {
    paperpaw_sdk::files::read_text(storage, TOLD_FILE, TOLD_MAX)
        .map(|text| {
            text.lines()
                .filter_map(|line| u64::from_str_radix(line.trim(), 16).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "secrets_i6_tests.rs"]
mod tests;
