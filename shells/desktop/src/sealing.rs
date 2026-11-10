//! What keeps the secrets of linked native plugins (`core.secrets`) on the
//! computer: values are sealed with AES-256-GCM under a key of this user's,
//! never written in the clear and never beside the card's files.
//!
//! The key is 32 random bytes in `device.key` in the user's configuration
//! folder (`$XDG_CONFIG_HOME/paperpaw-emulator`, or `~/.config/paperpaw-emulator`;
//! the folder and the file are this user's alone, 0700 and 0600, where the
//! system has such rights). It is made at the first use, atomically, and is
//! never replaced: a key file of another size, or one this user cannot be
//! the only reader of, is refused. Without a key nothing is sealed and
//! `core.secrets` is not wired: a plugin that needs it is refused. A secrets
//! file copied to another computer (or kept with the card's files) does not
//! open there: its values are absent, as on a device.
//!
//! A run on simulated time (tests, scripts) keeps its key in memory for the
//! process: it writes nothing outside its own folders.
//!
//! Every `seal` draws a fresh 12-byte nonce from the system's random source
//! ([`crate::host::random`]); where that fails the seal fails and nothing is
//! written (the `Sealer` duties in `docs/abi/secrets.md`).

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use aes_gcm::aead::{AeadInPlace, KeyInit, generic_array::GenericArray};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use paperpaw_core::native_host::shell_wiring::{Rng, Sealer};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_SPACE};

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const FOLDER: &str = "paperpaw-emulator";
const FILE: &str = "device.key";

/// Seals values under one key (see the module).
pub struct KeySealer {
    cipher: Aes256Gcm,
}

impl core::fmt::Debug for KeySealer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("KeySealer").finish_non_exhaustive()
    }
}

impl KeySealer {
    fn from_key(key: &[u8; KEY_BYTES]) -> Self {
        Self {
            cipher: Aes256Gcm::new(GenericArray::from_slice(key)),
        }
    }

    /// A sealer under a key made now and kept in memory only, for the rest of
    /// the process (a run on simulated time).
    ///
    /// # Errors
    /// No random numbers.
    pub fn for_the_process() -> io::Result<Self> {
        static KEY: OnceLock<[u8; KEY_BYTES]> = OnceLock::new();
        if KEY.get().is_none() {
            let mut key = [0u8; KEY_BYTES];
            if !crate::host::random(&mut key) {
                return Err(io::Error::other("no random numbers for a key"));
            }
            let _ = KEY.set(key);
        }
        let key = KEY.get().ok_or_else(|| io::Error::other("no key"))?;
        Ok(Self::from_key(key))
    }

    /// A sealer under this user's key in their configuration folder, made
    /// at the first use.
    ///
    /// # Errors
    /// No configuration folder, no random numbers, a key file that is not
    /// 32 bytes or is open to other users, or the folder's own errors.
    pub fn for_the_user() -> io::Result<Self> {
        let folder = config_folder()?;
        Self::in_folder(&folder)
    }

    /// [`KeySealer::for_the_user`] with the key in `folder`.
    ///
    /// # Errors
    /// As [`KeySealer::for_the_user`].
    pub fn in_folder(folder: &Path) -> io::Result<Self> {
        make_folder(folder)?;
        let path = folder.join(FILE);
        match read_key(&path) {
            Ok(key) => Ok(Self::from_key(&key)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let mut key = [0u8; KEY_BYTES];
                if !crate::host::random(&mut key) {
                    return Err(io::Error::other("no random numbers for a key"));
                }
                match create_key(&path, &key) {
                    Ok(()) => Ok(Self::from_key(&key)),
                    // Another start made it first: its key is the key.
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        Ok(Self::from_key(&read_key(&path)?))
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }
}

fn config_folder() -> io::Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .ok_or_else(|| io::Error::other("no configuration folder (HOME is not set)"))?;
            PathBuf::from(home).join(".config")
        }
    };
    if !base.is_absolute() {
        return Err(io::Error::other("the configuration folder is not absolute"));
    }
    Ok(base.join(FOLDER))
}

fn make_folder(folder: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(folder)
}

fn read_key(path: &Path) -> io::Result<[u8; KEY_BYTES]> {
    let file = std::fs::File::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = file.metadata()?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(io::Error::other(format!(
                "{} is open to other users (mode {:o}); it is refused",
                path.display(),
                mode & 0o777
            )));
        }
    }
    let mut bytes = Vec::new();
    file.take(KEY_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    <[u8; KEY_BYTES]>::try_from(bytes.as_slice())
        .map_err(|_| io::Error::other(format!("{} is not a 32-byte key", path.display())))
}

fn create_key(path: &Path, key: &[u8; KEY_BYTES]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(key)?;
    file.sync_all()
}

impl Sealer for KeySealer {
    fn seals(&self) -> bool {
        true
    }

    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let total = NONCE_BYTES + plain.len() + TAG_BYTES;
        if out.len() < total {
            return Err(ERR_NO_SPACE);
        }
        let (nonce, rest) = out[..total].split_at_mut(NONCE_BYTES);
        if !crate::host::random(nonce) {
            return Err(ERR_IO);
        }
        let (body, tag) = rest.split_at_mut(plain.len());
        body.copy_from_slice(plain);
        let made = self
            .cipher
            .encrypt_in_place_detached(Nonce::from_slice(nonce), aad, body)
            .map_err(|_| ERR_IO)?;
        tag.copy_from_slice(&made);
        Ok(total)
    }

    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let body_len = sealed
            .len()
            .checked_sub(NONCE_BYTES + TAG_BYTES)
            .ok_or(ERR_INVALID)?;
        if out.len() < body_len {
            return Err(ERR_NO_SPACE);
        }
        let (nonce, rest) = sealed.split_at(NONCE_BYTES);
        let (body, tag) = rest.split_at(body_len);
        let out = &mut out[..body_len];
        out.copy_from_slice(body);
        self.cipher
            .decrypt_in_place_detached(Nonce::from_slice(nonce), aad, out, Tag::from_slice(tag))
            .map_err(|_| {
                // Nothing of a value that did not open stays in the buffer.
                out.fill(0);
                ERR_INVALID
            })?;
        Ok(body_len)
    }
}

/// The computer's random source, for `random` (always the system's: strong).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRng;

impl Rng for SystemRng {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        if crate::host::random(out) {
            Ok(true)
        } else {
            Err(ERR_IO)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;

    fn sealer() -> KeySealer {
        KeySealer::for_the_process().unwrap()
    }

    #[test]
    fn a_sealed_value_opens_only_with_its_line() {
        let s = sealer();
        let mut out = [0u8; 1_100];
        let n = s.seal(b"wallabag\0password", b"hunter2", &mut out).unwrap();
        assert_eq!(n, NONCE_BYTES + 7 + TAG_BYTES);
        // Not in the clear.
        assert!(!out[..n].windows(7).any(|w| w == b"hunter2"));
        let mut plain = [0u8; 64];
        let k = s
            .open(b"wallabag\0password", &out[..n], &mut plain)
            .unwrap();
        assert_eq!(&plain[..k], b"hunter2");
        // Copied to another line: it does not open, and nothing is left.
        let mut plain = [1u8; 64];
        assert_eq!(
            s.open(b"other\0password", &out[..n], &mut plain),
            Err(ERR_INVALID)
        );
        assert!(plain[..7].iter().all(|b| *b == 0));
        // A damaged value does not open.
        out[NONCE_BYTES] ^= 1;
        assert_eq!(
            s.open(b"wallabag\0password", &out[..n], &mut plain),
            Err(ERR_INVALID)
        );
    }

    #[test]
    fn every_seal_has_its_own_nonce() {
        let s = sealer();
        let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
        s.seal(b"x", b"same", &mut a).unwrap();
        s.seal(b"x", b"same", &mut b).unwrap();
        assert_ne!(a[..NONCE_BYTES], b[..NONCE_BYTES]);
    }

    #[test]
    fn short_buffers_and_short_values_are_refused() {
        let s = sealer();
        let mut small = [0u8; 10];
        assert_eq!(s.seal(b"x", b"value", &mut small), Err(ERR_NO_SPACE));
        let mut out = [0u8; 8];
        assert_eq!(s.open(b"x", &[0u8; 20], &mut out), Err(ERR_INVALID));
    }

    #[test]
    fn the_key_is_made_once_kept_private_and_never_replaced() {
        let dir = crate::private::dir("paperpaw-sealing-test").unwrap();
        let first = KeySealer::in_folder(&dir).unwrap();
        let mut out = [0u8; 64];
        let n = first.seal(b"a", b"v", &mut out).unwrap();
        // The same key the next time.
        let again = KeySealer::in_folder(&dir).unwrap();
        let mut plain = [0u8; 8];
        assert_eq!(again.open(b"a", &out[..n], &mut plain), Ok(1));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            // A key anyone can read is refused, not used.
            std::fs::set_permissions(dir.join(FILE), std::fs::Permissions::from_mode(0o644))
                .unwrap();
            assert!(KeySealer::in_folder(&dir).is_err());
        }
        // A key of the wrong size is refused, not replaced.
        std::fs::write(dir.join(FILE), b"short").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join(FILE), std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        assert!(KeySealer::in_folder(&dir).is_err());
        assert_eq!(std::fs::read(dir.join(FILE)).unwrap(), b"short");
        let _ = std::fs::remove_dir_all(dir);
    }
}
