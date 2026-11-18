//! Author keys beyond the first install (ticket #20): a recovery key the
//! author chose in advance, with which a lost or stolen key is replaced by
//! a statement the reader checks itself, and keys the store says were
//! revoked, whose code the kernel does not load.
//!
//! A **recovery key** is pinned with the author's key at a plugin's first
//! install, and only then (`remember_first_install` (kernel)): never later, not
//! even signed by the author's key, which may be the stolen one. The
//! author keeps it offline. To move
//! to a new key the author signs [`rotation_message`] with it; a store
//! that gets the statement with an update hands it to [`accept_rotation`],
//! which checks it against the pinned keys, so neither a moderator's
//! mistake nor a taken-over server can forge one. The code installed
//! before the rotation is not loaded from then on: it may be the stolen
//! key's, so only an update signed by the new key runs again. Plugins
//! installed without a recovery key keep only the successor way (a new
//! id).
//!
//! **Revoked keys** come from the store (its library lists them) as
//! [`Revocation`]s: a statement ([`revocation_message`]) signed by the
//! revoked key itself or by the plugin's pinned recovery key, about the key
//! pinned as that plugin's author here. The store hands them to
//! [`keep_revoked`], which keeps those that hold, and none is ever taken
//! off; the list is checked again where it is read. So no library can
//! revoke a key it does not hold. A revocation is about one plugin: the
//! kernel does not load that plugin's code
//! ([`PluginProblem::Revoked`](crate::PluginProblem::Revoked)), and other
//! plugins the same key signed have revocations of their own.
//!
//! **Threat model.** Installed plugins are trusted: every plugin has the
//! whole card (owner decision 44: freedom over security; there is no
//! sandbox), so nothing here guards against a plugin on the device. The
//! author, recovery and revocation checks guard against outside attackers:
//! a rogue or compromised library or catalogue, the network between it and
//! the device, and forged or replayed statements. The kernel therefore
//! believes no data from outside without a signature it checks itself: a
//! rotation must be signed by the plugin's pinned recovery key, and a
//! revocation by the revoked key itself or by that recovery key, for a key
//! pinned on this device. It checks this both when it keeps an entry and
//! when it reads its lists from the card. The store only carries bytes.

use alloc::string::String;
use alloc::vec::Vec;

use crate::Storage;

/// Where the recovery keys are kept, beside the authors' keys (a file of
/// its own, as older builds read every line of that one as one key).
const RECOVERY: &str = crate::data_path!("plugin-recovery.txt");

/// Where the revocations are kept: one on each line, the plugin, the key
/// and the signature (hex), apart by a space.
const REVOKED: &str = crate::data_path!("revoked-keys.txt");

/// Revocations kept at most (one holds per plugin installed).
pub const MAX_REVOKED: usize = 256;

/// That the key of `plugin`'s author is revoked: `signature` over
/// [`revocation_message`] by `key` itself (the author, or whoever took
/// the key, can only revoke it) or by the plugin's pinned recovery key
/// (for a key the author lost).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Revocation {
    pub plugin: String,
    pub key: [u8; 32],
    pub signature: [u8; 64],
}

impl Revocation {
    pub fn new(plugin: impl Into<String>, key: [u8; 32], signature: [u8; 64]) -> Self {
        Self {
            plugin: plugin.into(),
            key,
            signature,
        }
    }

    /// Whether it holds on this device: `key` is the author pinned for
    /// `plugin`, and the signature is `key`'s or the recovery key's.
    fn holds(&self, storage: &dyn Storage) -> bool {
        if super::known_author(storage, &self.plugin) != Some(self.key) {
            return false;
        }
        let message = revocation_message(&self.plugin, &self.key);
        let by = |key: &[u8; 32]| {
            paperpaw_native::signature::verify_message(key, message.as_bytes(), &self.signature)
        };
        by(&self.key) || known_recovery(storage, &self.plugin).is_some_and(|key| by(&key))
    }

    /// Its line in the list, or `None` for a plugin id a line cannot hold.
    fn line(&self) -> Option<String> {
        let plain = !self.plugin.is_empty()
            && self.plugin.len() <= 128
            && !self
                .plugin
                .chars()
                .any(|c| c.is_whitespace() || c.is_control());
        plain.then(|| {
            alloc::format!(
                "{} {} {}\n",
                self.plugin,
                paperpaw_digest::hex::encode(&self.key),
                paperpaw_digest::hex::encode(&self.signature)
            )
        })
    }
}

/// The statement that revokes `key`, the key of `plugin`'s author: these
/// bytes, exactly (`paperpaw-sign revocation` signs it).
pub fn revocation_message(plugin: &str, key: &[u8; 32]) -> String {
    paperpaw_native::signature::revocation_message(plugin, key)
}

/// The recovery key of `plugin`'s author, as pinned at its first install.
pub fn known_recovery(storage: &dyn Storage, plugin: &str) -> Option<[u8; 32]> {
    super::entry(storage, RECOVERY, plugin)
}

/// Remembers `author` as the key of `plugin`'s author and pins `recovery`
/// as its recovery key, at the plugin's first install: only where no
/// author is known for it yet (so an update can never bring a recovery
/// key); `false` otherwise or where they could not be kept. The author's
/// key is kept first: cut short after it, the plugin has no recovery key
/// (only the successor way), never a recovery key without its author.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn remember_first_install(
    storage: &dyn Storage,
    plugin: &str,
    author: &[u8; 32],
    recovery: &[u8; 32],
) -> bool {
    if super::known_author(storage, plugin).is_some() {
        return false;
    }
    super::remember_author(storage, plugin, author)
        && super::keep_entry(storage, RECOVERY, plugin, recovery)
}

/// The statement the recovery key signs to move `plugin` from the
/// author's key `old` to `new`: these bytes, exactly.
pub fn rotation_message(plugin: &str, old: &[u8; 32], new: &[u8; 32]) -> String {
    paperpaw_native::signature::rotation_message(plugin, old, new)
}

/// Why a rotation was not taken ([`accept_rotation`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RotationError {
    /// No author is known for the plugin: a first install needs none.
    NoAuthor,
    /// No recovery key was pinned for it: only a successor (a new id).
    NoRecoveryKey,
    /// The signature is not the recovery key's over this rotation.
    BadSignature,
    /// The new key is the one known already.
    Refused,
    /// The new key could not be kept.
    NotKept,
}

/// Whether [`accept_rotation`] takes this rotation, without writing
/// anything: for the store's verdict before it fetches the update.
pub fn check_rotation(
    storage: &dyn Storage,
    plugin: &str,
    new: &[u8; 32],
    signature: &[u8; 64],
) -> Result<(), RotationError> {
    let old = super::known_author(storage, plugin).ok_or(RotationError::NoAuthor)?;
    let recovery = known_recovery(storage, plugin).ok_or(RotationError::NoRecoveryKey)?;
    if *new == old {
        return Err(RotationError::Refused);
    }
    let message = rotation_message(plugin, &old, new);
    match paperpaw_native::signature::verify_message(&recovery, message.as_bytes(), signature) {
        true => Ok(()),
        false => Err(RotationError::BadSignature),
    }
}

/// Takes the author of `plugin` to `new`, where `signature` is the pinned
/// recovery key's over [`rotation_message`] from the known key: from then
/// on the plugin's code is `new`'s (updates signed by it are taken, and
/// the old key's are not). The code installed or fetched before is no
/// longer named as installed ([`installed_code`](super::installed_code),
/// [`fetched_code`](super::fetched_code)), so it is not loaded at the next
/// start: it may be what a stolen key signed. The recovery key stays.
pub fn accept_rotation(
    storage: &dyn Storage,
    plugin: &str,
    new: &[u8; 32],
    signature: &[u8; 64],
) -> Result<(), RotationError> {
    check_rotation(storage, plugin, new, signature)?;
    // The old key's code first: cut short after it, nothing is loaded,
    // and the rotation can be taken again.
    let forgotten = super::write_entry(storage, super::INSTALLED, plugin, None)
        && super::write_entry(storage, super::FETCHED, plugin, None);
    if forgotten && super::keep_entry(storage, super::AUTHORS, plugin, new) {
        Ok(())
    } else {
        Err(RotationError::NotKept)
    }
}

/// The revocations kept that hold: each says the key of its plugin's
/// author, as pinned here, is revoked for that plugin.
pub fn revoked(storage: &dyn Storage) -> Vec<Revocation> {
    match crate::files::load_for_update(storage, REVOKED, MAX_REVOKED_BYTES) {
        Ok(Some(data)) => holding(storage, &data),
        _ => Vec::new(),
    }
}

/// How large the list of revocations may be: [`MAX_REVOKED`] lines of a
/// plugin id and 194 bytes, with room.
const MAX_REVOKED_BYTES: usize = 96 * 1024;

/// The revocations in `data` that hold, at most [`MAX_REVOKED`]: lines
/// that do not (written onto the card past [`keep_revoked`], or no
/// longer about the pinned author) neither count nor hide the others.
fn holding(storage: &dyn Storage, data: &[u8]) -> Vec<Revocation> {
    let mut all = revocations_in(data);
    all.retain(|r| r.holds(storage));
    all.truncate(MAX_REVOKED);
    all
}

/// The revocations in `data`, a line each. A line that is none (a
/// damaged byte, a key or signature of another length) is left out on
/// its own: the others stay.
fn revocations_in(data: &[u8]) -> Vec<Revocation> {
    data.split(|&b| b == b'\n')
        .filter_map(|line| core::str::from_utf8(line).ok())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (plugin, key, signature) = (parts.next()?, parts.next()?, parts.next()?);
            Some(Revocation::new(
                plugin,
                paperpaw_digest::hex::decode_array(key)?,
                paperpaw_digest::hex::decode_array(signature)?,
            ))
        })
        .collect()
}

/// Whether the key of `plugin`'s author, as pinned here, is revoked. Per
/// plugin: a key revoked for one says nothing about another plugin its
/// author signed (each has its own revocation), and a revocation holds
/// only while the key it names is the plugin's author.
pub fn is_revoked(storage: &dyn Storage, plugin: &str) -> bool {
    revoked(storage).iter().any(|r| r.plugin == plugin)
}

/// Why revocations were not all kept ([`keep_revoked`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RevokedError {
    /// More than [`MAX_REVOKED`] in all: those kept before and the first
    /// of the new ones up to it are kept, the rest not (tell the reader).
    Full,
    /// They could not be written, or the list kept could not be read
    /// (too large, a failing card): it stays as it is.
    NotKept,
}

/// Adds the `revocations` that hold on this device (see [`Revocation`];
/// the store's library lists them) to those kept, and leaves out the
/// others without a word: a key once revoked stays so, whatever a later
/// list says, as the list may come from a taken-over server. At most
/// [`MAX_REVOKED`] in all; past that, [`RevokedError::Full`].
pub fn keep_revoked(storage: &dyn Storage, revocations: &[Revocation]) -> Result<(), RevokedError> {
    // A list there that cannot be read is not written over by a shorter
    // one; one with damaged lines is written anew from the others.
    // Those that no longer hold are left out, and the list written anew
    // without them.
    let (mut all, lines) = match crate::files::load_for_update(storage, REVOKED, MAX_REVOKED_BYTES)
    {
        Ok(Some(data)) => (holding(storage, &data), revocations_in(&data).len()),
        Ok(None) => (Vec::new(), 0),
        Err(_) => return Err(RevokedError::NotKept),
    };
    let before = all.len();
    let mut full = false;
    for revocation in revocations {
        let known = all
            .iter()
            .any(|r| r.plugin == revocation.plugin && r.key == revocation.key);
        if known || revocation.line().is_none() || !revocation.holds(storage) {
            continue;
        }
        if all.len() >= MAX_REVOKED {
            full = true;
            break;
        }
        all.push(revocation.clone());
    }
    if all.len() > before || lines > before {
        let text: String = all.iter().filter_map(Revocation::line).collect();
        let _ = storage.create_dir(crate::data::DIR);
        if storage.write(REVOKED, text.as_bytes()).is_err() {
            return Err(RevokedError::NotKept);
        }
    }
    match full {
        true => Err(RevokedError::Full),
        false => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DirEntry, StorageError};
    use paperpaw_native::signature::{public_key, sign_message};

    const AUTHOR: [u8; 32] = [1; 32];
    const RECOVERY_SEED: [u8; 32] = [7; 32];
    const NEW: [u8; 32] = [9; 32];

    fn installed() -> crate::MemoryStorage {
        let storage = crate::MemoryStorage::new();
        let recovery = public_key(&RECOVERY_SEED);
        assert!(remember_first_install(
            &storage, "clock", &AUTHOR, &recovery
        ));
        assert!(super::super::remember_code(&storage, "clock", &[6; 32]));
        assert!(super::super::remember_fetched(&storage, "clock", &[5; 32]));
        assert!(super::super::remember_code(&storage, "other", &[4; 32]));
        storage
    }

    /// Pinned only with the first author's key: an update of a plugin
    /// whose author is known brings none, though none was pinned (#20,
    /// review M1).
    #[test]
    fn a_recovery_key_is_pinned_only_at_the_first_install() {
        let storage = crate::MemoryStorage::new();
        assert!(super::super::remember_author(&storage, "clock", &AUTHOR));
        assert!(!remember_first_install(
            &storage, "clock", &AUTHOR, &[5; 32]
        ));
        assert!(!remember_first_install(
            &storage, "clock", &[3; 32], &[5; 32]
        ));
        assert_eq!(known_recovery(&storage, "clock"), None);
        assert_eq!(super::super::known_author(&storage, "clock"), Some(AUTHOR));
        // A first install pins both, and a second one neither.
        assert!(remember_first_install(&storage, "new", &AUTHOR, &[5; 32]));
        assert_eq!(known_recovery(&storage, "new"), Some([5; 32]));
        assert!(!remember_first_install(&storage, "new", &[3; 32], &[8; 32]));
        assert_eq!(known_recovery(&storage, "new"), Some([5; 32]));
        assert_eq!(super::super::known_author(&storage, "new"), Some(AUTHOR));
    }

    #[test]
    fn a_recovery_key_moves_the_author_to_a_new_key() {
        let storage = installed();
        let message = rotation_message("clock", &AUTHOR, &NEW);
        let signature = sign_message(message.as_bytes(), &RECOVERY_SEED);
        assert_eq!(accept_rotation(&storage, "clock", &NEW, &signature), Ok(()));
        assert_eq!(super::super::known_author(&storage, "clock"), Some(NEW));
        assert_eq!(
            super::super::author(&storage, "clock", &AUTHOR),
            super::super::Author::Other
        );
        // The recovery key stays, and is not replaced.
        assert_eq!(
            known_recovery(&storage, "clock"),
            Some(public_key(&RECOVERY_SEED))
        );
        assert!(!remember_first_install(&storage, "clock", &NEW, &[5; 32]));
        // The old key's code is no longer named as installed or fetched:
        // not loaded until an update by the new key (review M2); other
        // plugins' as they were.
        assert_eq!(super::super::installed_code(&storage, "clock"), None);
        assert_eq!(super::super::fetched_code(&storage, "clock"), None);
        assert_eq!(
            super::super::installed_code(&storage, "other"),
            Some([4; 32])
        );
        // The same statement again: from a key no longer known.
        assert_eq!(
            accept_rotation(&storage, "clock", &NEW, &signature),
            Err(RotationError::Refused)
        );
    }

    #[test]
    fn a_rotation_is_refused_unless_the_recovery_key_signed_it() {
        let storage = installed();
        let message = rotation_message("clock", &AUTHOR, &NEW);
        // Signed by the author's (stolen) key, or another's.
        for seed in [[2; 32], [8; 32]] {
            let signature = sign_message(message.as_bytes(), &seed);
            assert_eq!(
                accept_rotation(&storage, "clock", &NEW, &signature),
                Err(RotationError::BadSignature)
            );
        }
        // For another plugin, or to another key than signed.
        let signature = sign_message(message.as_bytes(), &RECOVERY_SEED);
        assert_eq!(
            accept_rotation(&storage, "clock", &[4; 32], &signature),
            Err(RotationError::BadSignature)
        );
        assert_eq!(
            accept_rotation(&storage, "other", &NEW, &signature),
            Err(RotationError::NoAuthor)
        );
        // No recovery key pinned: none.
        let bare = crate::MemoryStorage::new();
        assert!(super::super::remember_author(&bare, "clock", &AUTHOR));
        assert_eq!(
            accept_rotation(&bare, "clock", &NEW, &signature),
            Err(RotationError::NoRecoveryKey)
        );
        // To the key known already: refused, and the code stays named.
        let message = rotation_message("clock", &AUTHOR, &AUTHOR);
        let same = sign_message(message.as_bytes(), &RECOVERY_SEED);
        assert_eq!(
            accept_rotation(&storage, "clock", &AUTHOR, &same),
            Err(RotationError::Refused)
        );
        assert_eq!(super::super::known_author(&storage, "clock"), Some(AUTHOR));
        assert_eq!(
            super::super::installed_code(&storage, "clock"),
            Some([6; 32])
        );
    }

    /// check_rotation says what accept_rotation would, and writes
    /// nothing (#20, review of the store side).
    #[test]
    fn a_rotation_is_checked_without_writing() {
        let storage = installed();
        let message = rotation_message("clock", &AUTHOR, &NEW);
        let signature = sign_message(message.as_bytes(), &RECOVERY_SEED);
        let forged = sign_message(message.as_bytes(), &[8; 32]);
        assert_eq!(check_rotation(&storage, "clock", &NEW, &signature), Ok(()));
        assert_eq!(
            check_rotation(&storage, "clock", &NEW, &forged),
            Err(RotationError::BadSignature)
        );
        assert_eq!(
            check_rotation(&storage, "clock", &AUTHOR, &signature),
            Err(RotationError::Refused)
        );
        assert_eq!(
            check_rotation(&storage, "gone", &NEW, &signature),
            Err(RotationError::NoAuthor)
        );
        assert_eq!(super::super::known_author(&storage, "clock"), Some(AUTHOR));
        assert_eq!(
            super::super::installed_code(&storage, "clock"),
            Some([6; 32])
        );
        assert_eq!(super::super::fetched_code(&storage, "clock"), Some([5; 32]));
    }

    /// A storage that takes `left` more writes, then fails every one: the
    /// power cut between them.
    struct CutAfter {
        storage: crate::MemoryStorage,
        left: core::cell::Cell<usize>,
    }

    impl Storage for CutAfter {
        fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.storage.read_at(path, at, buf)
        }

        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.storage.size(path)
        }

        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            let left = self.left.get().checked_sub(1).ok_or(StorageError::Io)?;
            self.left.set(left);
            self.storage.write(path, data)
        }

        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }

        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.storage.list(dir, visit)
        }

        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }
    }

    /// Cut short after any of its writes, a rotation leaves the old
    /// author with no code to load (or all as it was), never the new
    /// author with the old key's code; and it can be taken again (#20,
    /// review of the store side).
    #[test]
    fn a_rotation_cut_short_can_be_taken_again() {
        let message = rotation_message("clock", &AUTHOR, &NEW);
        let signature = sign_message(message.as_bytes(), &RECOVERY_SEED);
        for writes in 0..3 {
            let cut = CutAfter {
                storage: installed(),
                left: core::cell::Cell::new(writes),
            };
            assert_eq!(
                accept_rotation(&cut, "clock", &NEW, &signature),
                Err(RotationError::NotKept),
                "{writes}"
            );
            assert_eq!(super::super::known_author(&cut, "clock"), Some(AUTHOR));
            let code = super::super::installed_code(&cut, "clock");
            assert_eq!(code, (writes == 0).then_some([6; 32]), "{writes}");
            cut.left.set(usize::MAX);
            assert_eq!(accept_rotation(&cut, "clock", &NEW, &signature), Ok(()));
            assert_eq!(super::super::known_author(&cut, "clock"), Some(NEW));
            assert_eq!(super::super::installed_code(&cut, "clock"), None);
            assert_eq!(super::super::fetched_code(&cut, "clock"), None);
        }
    }

    /// The keys of the revocations that hold.
    fn keys(storage: &dyn Storage) -> Vec<[u8; 32]> {
        revoked(storage).into_iter().map(|r| r.key).collect()
    }

    /// Revoked per plugin: a key that signs two plugins, revoked for one,
    /// leaves the other as it is, also once the first moves to a new key
    /// (review M1 of #277). So a library that pins a victim's key for a
    /// plugin of its own and revokes it there stops only its own.
    #[test]
    fn a_key_is_revoked_for_one_plugin_only() {
        let storage = installed();
        let seed = [2; 32];
        let key = public_key(&seed);
        for plugin in ["notes", "atlas"] {
            assert!(remember_first_install(
                &storage,
                plugin,
                &key,
                &public_key(&RECOVERY_SEED)
            ));
        }
        assert!(remember_first_install(
            &storage,
            "evil",
            &key,
            &public_key(&[8; 32])
        ));
        let evil = revocation("evil", key, &[8; 32]);
        assert_eq!(keep_revoked(&storage, &[evil]), Ok(()));
        assert!(is_revoked(&storage, "evil"));
        assert!(!is_revoked(&storage, "notes") && !is_revoked(&storage, "atlas"));
        assert_eq!(
            keep_revoked(&storage, &[revocation("notes", key, &seed)]),
            Ok(())
        );
        assert!(is_revoked(&storage, "notes") && !is_revoked(&storage, "atlas"));
        let message = rotation_message("notes", &key, &NEW);
        let signature = sign_message(message.as_bytes(), &RECOVERY_SEED);
        assert_eq!(accept_rotation(&storage, "notes", &NEW, &signature), Ok(()));
        assert!(!is_revoked(&storage, "notes") && !is_revoked(&storage, "atlas"));
        assert!(is_revoked(&storage, "evil"));
    }

    /// A revocation of `key` for `plugin`, signed with `seed`.
    fn revocation(plugin: &str, key: [u8; 32], seed: &[u8; 32]) -> Revocation {
        let message = revocation_message(plugin, &key);
        Revocation::new(plugin, key, sign_message(message.as_bytes(), seed))
    }

    /// A revocation holds where it is about the key pinned as the
    /// plugin's author and signed by that key or the recovery key; any
    /// other is left out, so no taken-over library can revoke another
    /// author's key (#20, review of #277).
    #[test]
    fn only_signed_revocations_of_the_pinned_author_are_kept() {
        let storage = installed();
        let notes_seed = [2; 32];
        let notes = public_key(&notes_seed);
        assert!(super::super::remember_author(&storage, "notes", &notes));
        assert!(revoked(&storage).is_empty());
        // Signed by another, about another plugin's key, about a key not
        // pinned, for a plugin with no author: none kept.
        let forged = [
            revocation("notes", notes, &[8; 32]),
            revocation("clock", AUTHOR, &notes_seed),
            revocation("clock", notes, &notes_seed),
            revocation("notes", NEW, &notes_seed),
            revocation("gone", notes, &notes_seed),
        ];
        assert_eq!(keep_revoked(&storage, &forged), Ok(()));
        assert!(revoked(&storage).is_empty());
        assert!(storage.size(REVOKED).is_err(), "nothing written");
        // By the key itself, and by the recovery key for a lost one.
        let real = [
            revocation("notes", notes, &notes_seed),
            revocation("clock", AUTHOR, &RECOVERY_SEED),
        ];
        assert_eq!(keep_revoked(&storage, &real), Ok(()));
        assert_eq!(keys(&storage), [notes, AUTHOR]);
        assert!(is_revoked(&storage, "clock") && !is_revoked(&storage, "other"));
        // A later list without them does not take them off (review S1),
        // nor are they kept twice.
        assert_eq!(keep_revoked(&storage, &real[..1]), Ok(()));
        assert_eq!(keep_revoked(&storage, &[]), Ok(()));
        assert_eq!(revoked(&storage).len(), 2);
        assert_eq!(
            revocations_in(&crate::read_file(&storage, REVOKED, MAX_REVOKED_BYTES).unwrap()).len(),
            2
        );
    }

    /// The list is checked where it is read: a line any plugin wrote onto
    /// the card itself revokes nothing unless it holds.
    #[test]
    fn a_list_written_past_keep_revoked_is_checked_when_read() {
        let storage = installed();
        let forged = revocation("clock", AUTHOR, &[8; 32]).line().unwrap();
        let real = revocation("clock", AUTHOR, &RECOVERY_SEED).line().unwrap();
        storage.write(REVOKED, forged.as_bytes()).unwrap();
        assert!(revoked(&storage).is_empty());
        storage
            .write(REVOKED, alloc::format!("{forged}{real}").as_bytes())
            .unwrap();
        assert_eq!(keys(&storage), [AUTHOR]);
        // A plugin id a line cannot hold is not written.
        assert_eq!(revocation("a b", AUTHOR, &RECOVERY_SEED).line(), None);
    }

    /// Lines written onto the card past keep_revoked that do not hold
    /// neither fill the list nor hide one that holds after them, and
    /// keep_revoked writes the list anew without them.
    #[test]
    fn lines_that_do_not_hold_take_no_place() {
        let storage = installed();
        let notes_seed = [2; 32];
        let notes = public_key(&notes_seed);
        assert!(super::super::remember_author(&storage, "notes", &notes));
        let forged = revocation("clock", AUTHOR, &[8; 32]).line().unwrap();
        let real = revocation("clock", AUTHOR, &RECOVERY_SEED).line().unwrap();
        let mut text = forged.repeat(MAX_REVOKED + 10);
        text.push_str(&real);
        storage.write(REVOKED, text.as_bytes()).unwrap();
        assert_eq!(keys(&storage), [AUTHOR]);
        assert_eq!(
            keep_revoked(&storage, &[revocation("notes", notes, &notes_seed)]),
            Ok(())
        );
        assert_eq!(keys(&storage), [AUTHOR, notes]);
        let kept = crate::read_file(&storage, REVOKED, MAX_REVOKED_BYTES).unwrap();
        assert_eq!(revocations_in(&kept).len(), 2);
    }

    /// Past the most: those before and the first new ones kept, and said
    /// so (review S2). (One holds per plugin installed, so a device meets
    /// it only with more than MAX_REVOKED plugins.)
    #[test]
    fn past_the_most_revocations_say_so() {
        let storage = crate::MemoryStorage::new();
        let many: Vec<Revocation> = (0..MAX_REVOKED as u32 + 2)
            .map(|n| {
                let mut seed = [0; 32];
                seed[..4].copy_from_slice(&n.to_le_bytes());
                seed[31] = 1;
                let plugin = alloc::format!("p{n}");
                assert!(super::super::remember_author(
                    &storage,
                    &plugin,
                    &public_key(&seed)
                ));
                revocation(&plugin, public_key(&seed), &seed)
            })
            .collect();
        assert_eq!(keep_revoked(&storage, &many), Err(RevokedError::Full));
        let kept = revoked(&storage);
        assert_eq!(kept.len(), MAX_REVOKED);
        assert!(is_revoked(&storage, &many[MAX_REVOKED - 1].plugin));
        assert!(!is_revoked(&storage, &many[MAX_REVOKED].plugin));
    }

    /// A damaged line leaves the others revoked, and does not keep new
    /// ones out: the list is written anew from what can be read. One that
    /// cannot be read at all (too large) is not written over.
    #[test]
    fn a_damaged_line_costs_only_itself() {
        let storage = installed();
        let notes_seed = [2; 32];
        let notes = public_key(&notes_seed);
        assert!(super::super::remember_author(&storage, "notes", &notes));
        let mut text = revocation("clock", AUTHOR, &RECOVERY_SEED)
            .line()
            .unwrap()
            .into_bytes();
        text.extend_from_slice(&[0xff, 0xfe, b'\n']);
        text.extend_from_slice(b"clock 0909 0909\n");
        storage.write(REVOKED, &text).unwrap();
        assert_eq!(keys(&storage), [AUTHOR]);
        assert_eq!(
            keep_revoked(&storage, &[revocation("notes", notes, &notes_seed)]),
            Ok(())
        );
        assert_eq!(keys(&storage), [AUTHOR, notes]);
        assert!(
            !crate::read_file(&storage, REVOKED, MAX_REVOKED_BYTES)
                .unwrap()
                .contains(&0xff)
        );

        storage
            .write(REVOKED, &alloc::vec![b'0'; MAX_REVOKED_BYTES + 1])
            .unwrap();
        assert_eq!(
            keep_revoked(&storage, &[revocation("notes", notes, &notes_seed)]),
            Err(RevokedError::NotKept)
        );
        assert_eq!(storage.size(REVOKED), Ok(MAX_REVOKED_BYTES as u64 + 1));
    }
}
