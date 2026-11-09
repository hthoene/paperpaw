//! What this device knows about a plugin's author, read in bounded pieces
//! (`docs/abi/device.md` 5.1, 5.2): the key pinned for an id, its recovery
//! key and whether the pinned key is revoked.
//!
//! The files are Beta 3's, unchanged: `.paperpaw/plugin-authors.txt` and
//! `.paperpaw/plugin-recovery.txt` hold a line `<plugin>\t<64 hex digits>` per
//! plugin (the first line of a plugin decides), and
//! `.paperpaw/revoked-keys.txt` a line `<plugin> <key hex> <signature hex>`.
//! `paperpaw_sdk::native` (`remember_author`, `keys::keep_revoked`) writes them
//! and the loader reads them whole; this reads a line at a time through the
//! job's I/O buffer, so a check never takes a file's size from the heap.
//!
//! A file that cannot be read, is larger than [`FILE_MAX`], or has a pin that
//! is not 32 bytes of hex is an error, never "no pin": a device whose pins
//! cannot be read does not install a plugin on the strength of a missing one.
use paperpaw_native::signature::{revocation_message, verify_message};
use paperpaw_platform::{Storage, StorageError, firmware::Text};

use super::plugin_job::SIGNER_MAX;

/// Where the author keys, the recovery keys and the revocations are kept.
const AUTHORS: &str = paperpaw_sdk::data_path!("plugin-authors.txt");
const RECOVERY: &str = paperpaw_sdk::data_path!("plugin-recovery.txt");
const REVOKED: &str = paperpaw_sdk::data_path!("revoked-keys.txt");
/// Largest file read (the loader's 64 KiB for the pins, 96 KiB for the list).
const FILE_MAX: u64 = 96 * 1024;

/// A pin file that cannot be read or does not hold what it should.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreadable;

/// What this device knows of an id's author key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Standing {
    /// The key pinned for the id.
    pub(crate) pinned: Option<[u8; 32]>,
    /// Its recovery key, pinned with it at the first install.
    pub(crate) recovery: Option<[u8; 32]>,
    /// A revocation of the pinned key holds.
    pub(crate) revoked: bool,
}

/// Calls `visit` with each line of `file` (without its line end) until it
/// answers `true`. A line longer than `buf` is skipped whole.
///
/// # Errors
/// [`Unreadable`] for a card error or a file past [`FILE_MAX`]; a missing
/// file has no lines.
fn each_line(
    storage: &dyn Storage,
    file: &str,
    buf: &mut [u8],
    visit: &mut dyn FnMut(&str) -> bool,
) -> Result<(), Unreadable> {
    let size = match storage.size(file) {
        Ok(size) => size,
        Err(StorageError::NotFound) => return Ok(()),
        Err(_) => return Err(Unreadable),
    };
    if size > FILE_MAX {
        return Err(Unreadable);
    }
    let (mut at, mut have, mut skipping) = (0_u64, 0_usize, false);
    loop {
        let read = match storage.read_at(file, at, &mut buf[have..]) {
            Ok(n) => n,
            Err(_) => return Err(Unreadable),
        };
        at += read as u64;
        have += read;
        let mut start = 0;
        while let Some(end) = buf[start..have].iter().position(|byte| *byte == b'\n') {
            let line = &buf[start..start + end];
            start += end + 1;
            if skipping {
                skipping = false;
            } else if let Ok(text) = core::str::from_utf8(line)
                && visit(text.trim_end_matches('\r'))
            {
                return Ok(());
            }
        }
        buf.copy_within(start..have, 0);
        have -= start;
        if read == 0 {
            // The last line has no line end.
            if !skipping && let Ok(text) = core::str::from_utf8(&buf[..have]) {
                visit(text.trim_end_matches('\r'));
            }
            return Ok(());
        }
        if have == buf.len() {
            skipping = true;
            have = 0;
        }
    }
}

/// The key pinned for `plugin` in `file`: the first line that names it
/// decides, as the loader reads it.
fn pin(
    storage: &dyn Storage,
    file: &str,
    plugin: &str,
    buf: &mut [u8],
) -> Result<Option<[u8; 32]>, Unreadable> {
    let mut found: Option<Option<[u8; 32]>> = None;
    each_line(storage, file, buf, &mut |line| {
        let Some(hex) = line
            .strip_prefix(plugin)
            .and_then(|rest| rest.strip_prefix('\t'))
        else {
            return false;
        };
        found = Some(paperpaw_sdk::digest::hex::decode_array(hex.trim()));
        true
    })?;
    match found {
        None => Ok(None),
        Some(Some(key)) => Ok(Some(key)),
        // A pin that is not a key: not read as no pin.
        Some(None) => Err(Unreadable),
    }
}

/// Whether a revocation of `key`, the key pinned for `plugin`, holds: its
/// signature is the key's own or the recovery key's over the statement.
fn revoked(
    storage: &dyn Storage,
    plugin: &str,
    key: &[u8; 32],
    recovery: Option<&[u8; 32]>,
    buf: &mut [u8],
) -> Result<bool, Unreadable> {
    let mut holds = false;
    each_line(storage, REVOKED, buf, &mut |line| {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(named), Some(signature)) = (parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        if name != plugin {
            return false;
        }
        let (Some(named), Some(signature)) = (
            paperpaw_sdk::digest::hex::decode_array::<32>(named),
            paperpaw_sdk::digest::hex::decode_array::<64>(signature),
        ) else {
            return false;
        };
        if named != *key {
            return false;
        }
        let message = revocation_message(plugin, key);
        let by = |signer: &[u8; 32]| verify_message(signer, message.as_bytes(), &signature);
        holds = by(key) || recovery.is_some_and(by);
        holds
    })?;
    Ok(holds)
}

/// What the device knows of `plugin`'s author: the key pinned for it, its
/// recovery key and whether the pinned key is revoked.
///
/// # Errors
/// [`Unreadable`] where a file cannot be read.
pub(crate) fn standing(
    storage: &dyn Storage,
    plugin: &str,
    buf: &mut [u8],
) -> Result<Standing, Unreadable> {
    let pinned = pin(storage, AUTHORS, plugin, buf)?;
    let Some(key) = pinned else {
        return Ok(Standing {
            pinned: None,
            recovery: None,
            revoked: false,
        });
    };
    let recovery = pin(storage, RECOVERY, plugin, buf)?;
    let revoked = revoked(storage, plugin, &key, recovery.as_ref(), buf)?;
    Ok(Standing {
        pinned,
        recovery,
        revoked,
    })
}

/// The name of `key` among the keys the firmware knows by name, empty for
/// any other.
pub(crate) fn signer_name(
    signers: &[(&'static str, [u8; 32])],
    key: &[u8; 32],
) -> Text<SIGNER_MAX> {
    signers
        .iter()
        .find(|(_, known)| known == key)
        .map_or_else(|| Text::new(""), |(name, _)| Text::new(name))
}
