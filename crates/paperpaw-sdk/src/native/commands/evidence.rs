//! The catalog's word for a plugin install (`docs/abi/device.md` 5.2): the author key, the
//! recovery key and the rotation statements that travel in the batch of `install_plugin`
//! (commands 1580 and 1581), and the revocations the store keeps (command 1582).
//!
//! Needs `uses.install`. The host applies the trust rules itself and answers with the reason
//! of `install_done`; nothing here checks a signature.
use super::{CommandSender, Error, InstallRoot, wire};

/// Most commands one batch holds (the wire's `COMMAND_MAX_RECORDS`).
const RECORDS: usize = wire::COMMAND_MAX_RECORDS;

/// Most rotation statements one install carries (a wire batch holds 8 commands: the install,
/// the author record and 6 rotations).
pub const MAX_ROTATIONS: usize = 6;
/// Most revocations one call of [`CommandSender::trust_revoked`] takes. The SDK sends them
/// in wire batches of 8 commands (`docs/abi/device.md` 3).
pub const MAX_REVOCATIONS: usize = 16;

/// One step of an author's key rotation: the key it leads to and the old key's signature over
/// it (command 1581).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Rotation<'a> {
    /// The key the rotation leads to.
    pub to: &'a [u8; 32],
    /// The signature of the key it leaves.
    pub signature: &'a [u8; 64],
}

impl<'a> Rotation<'a> {
    #[must_use]
    pub const fn new(to: &'a [u8; 32], signature: &'a [u8; 64]) -> Self {
        Self { to, signature }
    }
}

/// What the catalog says of a code plugin, sent in the batch of its install. Keys and the
/// digest are borrowed; an absent one is none (the host reads an empty one as none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Evidence<'a> {
    author: Option<&'a [u8; 32]>,
    recovery: Option<&'a [u8; 32]>,
    sha256: Option<&'a [u8; 32]>,
    rotations: &'a [Rotation<'a>],
    from_catalog: bool,
}

impl<'a> Evidence<'a> {
    /// No evidence: a plugin the user sideloads.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            author: None,
            recovery: None,
            sha256: None,
            rotations: &[],
            from_catalog: false,
        }
    }

    /// The author key the catalog names (command 1580).
    #[must_use]
    pub const fn author(mut self, author: &'a [u8; 32]) -> Self {
        self.author = Some(author);
        self
    }

    /// The recovery key the catalog names for a first install.
    #[must_use]
    pub const fn recovery(mut self, recovery: &'a [u8; 32]) -> Self {
        self.recovery = Some(recovery);
        self
    }

    /// The digest the catalog expects of the code (command 1580): the host refuses an
    /// install whose code has another (reason 18).
    #[must_use]
    pub const fn sha256(mut self, sha256: &'a [u8; 32]) -> Self {
        self.sha256 = Some(sha256);
        self
    }

    /// The catalog's rotation statements, in the author's order; at most [`MAX_ROTATIONS`]
    /// (more are refused as `Invalid` when the install is sent), with or without keys.
    #[must_use]
    pub const fn rotations(mut self, rotations: &'a [Rotation<'a>]) -> Self {
        self.rotations = rotations;
        self
    }

    /// The install is a store's, from its catalog (`install_plugin` `flags` bit 2): the
    /// registry lists the version as installed by a store (`reg_plugin.flags` bit 5, as the
    /// store named it) and the store offers its updates. It grants no trust and changes no
    /// check.
    #[must_use]
    pub const fn from_catalog(mut self) -> Self {
        self.from_catalog = true;
        self
    }
}

/// A revocation of a plugin's key, with its proof (command 1582).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Revocation<'a> {
    /// The plugin id, at most 64 bytes.
    pub plugin: &'a str,
    /// The revoked key.
    pub key: &'a [u8; 32],
    /// The signature that proves it (the revoked key's or the recovery key's).
    pub signature: &'a [u8; 64],
}

impl<'a> Revocation<'a> {
    #[must_use]
    pub const fn new(plugin: &'a str, key: &'a [u8; 32], signature: &'a [u8; 64]) -> Self {
        Self {
            plugin,
            key,
            signature,
        }
    }
}

impl CommandSender<'_, '_> {
    /// [`install_plugin`](Self::install_plugin) with the catalog's evidence in the same batch
    /// (`docs/abi/device.md` 5.2): the install, then the author and recovery keys when
    /// `evidence` names either, then each rotation in order. The host applies pins and
    /// rotations only when the install commits.
    ///
    ///
    /// # Errors
    /// Returns `Invalid` for a zero request, an empty path, or more than [`MAX_ROTATIONS`]
    /// rotations; `NoSpace` for a path over 512 bytes or a scratch buffer too small for the
    /// batch (about 1 KiB for the most); or the host's refusal (`Denied` without `uses.install`). Nothing is sent on a local refusal.
    pub fn install_plugin_with(
        &mut self,
        request: u32,
        root: InstallRoot,
        path: &str,
        update: bool,
        allow_older: bool,
        evidence: &Evidence<'_>,
    ) -> Result<(), Error> {
        if request == 0 || path.is_empty() || evidence.rotations.len() > MAX_ROTATIONS {
            return Err(Error::Invalid);
        }
        if path.len() > 512 {
            return Err(Error::NoSpace);
        }
        let mut batch = [wire::Command::FirmwareRestart {}; RECORDS];
        batch[0] = wire::Command::InstallPlugin {
            request,
            root: root.wire(),
            path,
            flags: u32::from(update)
                | (u32::from(allow_older) << 1)
                | (u32::from(evidence.from_catalog) << 2),
        };
        let mut count = 1;
        if evidence.author.is_some() || evidence.recovery.is_some() || evidence.sha256.is_some() {
            batch[count] = wire::Command::InstallAuthor {
                request,
                author: evidence.author.map_or(&[][..], |key| &key[..]),
                recovery: evidence.recovery.map_or(&[][..], |key| &key[..]),
                sha256: evidence.sha256.map_or(&[][..], |digest| &digest[..]),
            };
            count += 1;
        }
        for rotation in evidence.rotations {
            batch[count] = wire::Command::InstallRotation {
                request,
                to: &rotation.to[..],
                signature: &rotation.signature[..],
            };
            count += 1;
        }
        self.submit_batch(&batch[..count])
    }

    /// Hand the host revocations the store's library lists (`docs/abi/device.md` 5.2): each
    /// is checked by the host against the revoked key or the plugin's recovery key and kept
    /// only if it holds; one that does not is dropped and the others stay. They answer
    /// nothing. A wire batch holds 8 commands, so more are sent in batches of 8, in order;
    /// if one batch is refused the earlier ones were sent already.
    ///
    /// # Errors
    /// Returns `Invalid` for none, more than [`MAX_REVOCATIONS`], or a plugin id that is
    /// empty or over 64 bytes; `NoSpace` for a scratch buffer too small for a batch (about
    /// 1.9 KiB for 8); or the host's refusal (`Denied` without `uses.install`).
    pub fn trust_revoked(&mut self, revocations: &[Revocation<'_>]) -> Result<(), Error> {
        if revocations.is_empty()
            || revocations.len() > MAX_REVOCATIONS
            || revocations
                .iter()
                .any(|r| r.plugin.is_empty() || r.plugin.len() > 64)
        {
            return Err(Error::Invalid);
        }
        for chunk in revocations.chunks(RECORDS) {
            let mut batch = [wire::Command::FirmwareRestart {}; RECORDS];
            for (slot, revocation) in batch.iter_mut().zip(chunk) {
                *slot = wire::Command::TrustRevoked {
                    plugin: revocation.plugin,
                    key: &revocation.key[..],
                    signature: &revocation.signature[..],
                };
            }
            self.submit_batch(&batch[..chunk.len()])?;
        }
        Ok(())
    }
}
