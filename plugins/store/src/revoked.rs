//! Keys the library says were withdrawn (#20): its list carries
//! `revoked_keys: [{"plugin": …, "key": <64 hex>, "signature": <128 hex>}]`.
//! The store only carries them: the kernel keeps an entry only where its
//! key is the plugin's pinned author here and the signature is that key's
//! or the plugin's recovery key's ([`keys::keep_revoked`]), so a library
//! that lies stops no plugin and fills no list.
//!
//! Only entries that could hold are kept for the kernel: of a plugin
//! here, naming its author's key, not stopped already. So the list handed
//! over is bounded by the plugins on the device, and what the library
//! fills a list with besides takes no place from them.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::keys::{self, MAX_REVOKED, Revocation};
use paperpaw_sdk::native::known_author;

/// Entries of a list looked at, most: a bound on the reading, apart from
/// what is kept. Past it a list is not read further.
const MAX_READ: usize = 4096;

/// The entries of one list being read.
#[derive(Default)]
pub(crate) struct Revocations {
    plugin: String,
    key: String,
    signature: String,
    /// The entries to hand over: of a plugin whose author's key is the one
    /// named, and not kept yet. At most [`MAX_REVOKED`], and only these
    /// count to that: a list of entries for plugins that are not here, as
    /// long as it is, takes no place from those that are.
    pending: Vec<Revocation>,
    /// The plugins already stopped, read when the first entry that could
    /// be new is met.
    kept: Option<Vec<String>>,
    /// Entries read, whatever they were.
    read: usize,
}

impl Revocations {
    /// Takes one field of the entry being read (`plugin`, `key`,
    /// `signature`); others are left.
    pub(crate) fn field(&mut self, field: &str, value: &str) {
        let (to, most) = match field {
            "plugin" => (&mut self.plugin, 64),
            "key" => (&mut self.key, 64),
            "signature" => (&mut self.signature, 128),
            _ => return,
        };
        *to = value.chars().take(most + 1).collect();
    }

    /// The entry is complete: kept for handing over if it is well formed,
    /// is of a plugin here whose author's key it names, and is new. The
    /// kernel checks the signature; a bad entry is left out on its own.
    /// `here` says in memory whether a plugin is on the device, so the
    /// authors file is read only for those, not once for each entry of a
    /// long list.
    pub(crate) fn entry_done(&mut self, storage: &dyn Storage, here: &dyn Fn(&str) -> bool) {
        let entry = (
            bytes::<32>(&self.key),
            bytes::<64>(&self.signature),
            core::mem::take(&mut self.plugin),
        );
        self.key.clear();
        self.signature.clear();
        self.read += 1;
        let (Some(key), Some(signature), plugin) = entry else {
            return;
        };
        if self.read > MAX_READ
            || self.pending.len() >= MAX_REVOKED
            || plugin.is_empty()
            || !here(&plugin)
            || known_author(storage, &plugin) != Some(key)
            || self.pending.iter().any(|r| r.plugin == plugin)
        {
            return;
        }
        let kept = self.kept.get_or_insert_with(|| {
            keys::revoked(storage)
                .into_iter()
                .map(|r| r.plugin)
                .collect()
        });
        if !kept.contains(&plugin) {
            self.pending.push(Revocation::new(plugin, key, signature));
        }
    }

    /// Hands over what is new, at once and only if there is any. A card
    /// that could not be written (`NotKept`) is tried again at the next
    /// list: what was not kept is new again then. `Full` cannot happen:
    /// what is handed over is at most one entry for each plugin here, as
    /// the kernel keeps one for each, and a plugin has no logging to say
    /// so to the reader.
    pub(crate) fn hand_over(&mut self, storage: &dyn Storage) {
        if !self.pending.is_empty() {
            let _ = keys::keep_revoked(storage, &self.pending);
            self.pending.clear();
        }
    }
}

fn bytes<const N: usize>(hex: &str) -> Option<[u8; N]> {
    paperpaw_sdk::digest::hex::decode_array(hex)
}
