//! What keeps the secrets of linked native plugins (`core.secrets`) on the
//! computer: values are sealed with AES-256-GCM under a key of this user's,
//! never written in the clear and never beside the card's files.
//!
//! The key is 32 random bytes in `device.key` in the user's configuration
//! folder (`$XDG_CONFIG_HOME/paperpaw-emulator`, or `~/.config/paperpaw-emulator`;
//! the folder and the file are this user's alone, 0700 and 0600, where the
//! system has such rights). It is made at the first use, atomically (a
//! synced temporary file linked into place, so another start sees no key or
//! the whole key, never a part), and is never replaced: a key file of another
//! size, or one this user cannot be the only reader of (open to group or
//! others, owned by another user, in a folder others can write to), is
//! refused. Two starts at once end with the same key. Without a key nothing is sealed and
//! `core.secrets` is not wired: a plugin that needs it is refused. A secrets
//! file copied to another computer (or kept with the card's files) does not
//! open there: its values are absent, as on a device.
//!
//! The store is fixed for the whole process: it is chosen once, when the linked
//! plugins are registered, and a key made or removed, or a card that cannot be
//! opened, afterwards changes nothing until the next start.
//!
//! A run on simulated time (tests, scripts) keeps its key **and its secrets
//! file** in memory for the process: it writes nothing outside its own folders
//! and cannot lose the secrets of a card it shares with a real run (`linked`).
//!
//! The key belongs to the user, not to a card: the same card folder opened
//! on another account, or after the key file was removed, has secrets that do
//! not open (they are absent, as on a device with another key), and a save
//! then writes beside them.
//!
//! Every `seal` draws a fresh 12-byte nonce from the system's random source
//! ([`crate::host::random`]); where that fails the seal fails and nothing is
//! written (the `Sealer` duties in `docs/abi/secrets.md`).

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use zeroize::Zeroizing;

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
                let mut key = Zeroizing::new([0u8; KEY_BYTES]);
                if !crate::host::random(&mut *key) {
                    return Err(io::Error::other("no random numbers for a key"));
                }
                match create_key(folder, &path, &key) {
                    Ok(()) => Ok(Self::from_key(&key)),
                    // Another start made it first: its key is the key.
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        let key = read_key(&path)?;
                        Ok(Self::from_key(&key))
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }
}

/// Where a user's settings live on this system: `%LOCALAPPDATA%` on Windows
/// (the machine's, not the roaming profile that a domain copies to a server),
/// `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config`
/// elsewhere. The owner and mode checks of the folder and key are Unix's;
/// Windows has none (the folder is the user's own by default).
fn config_folder() -> io::Result<PathBuf> {
    let home = || {
        std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("no configuration folder (HOME is not set)"))
    };
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("no configuration folder (LOCALAPPDATA is not set)"))?
    } else if cfg!(target_os = "macos") {
        home()?.join("Library").join("Application Support")
    } else {
        match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => home()?.join(".config"),
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
    builder.create(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = std::fs::metadata(folder)?;
        if meta.permissions().mode() & 0o022 != 0 {
            return Err(io::Error::other(format!(
                "{} can be written by others (mode {:o}); it is refused",
                folder.display(),
                meta.permissions().mode() & 0o777
            )));
        }
        if meta.uid() != me()? {
            return Err(io::Error::other(format!(
                "{} is owned by another user; it is refused",
                folder.display()
            )));
        }
    }
    Ok(())
}

/// This process's user id: the owner of its own `/proc` entry on Linux, the
/// effective user id elsewhere (macOS).
#[cfg(unix)]
fn me() -> io::Result<u32> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(std::fs::metadata("/proc/self")?.uid())
    }
    #[cfg(not(target_os = "linux"))]
    {
        // SAFETY: `geteuid` takes nothing and cannot fail.
        Ok(unsafe { libc::geteuid() })
    }
}

fn read_key(path: &Path) -> io::Result<Zeroizing<[u8; KEY_BYTES]>> {
    let file = std::fs::File::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = file.metadata()?;
        let mode = meta.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(io::Error::other(format!(
                "{} is open to other users (mode {:o}); it is refused",
                path.display(),
                mode & 0o777
            )));
        }
        if meta.uid() != me()? {
            return Err(io::Error::other(format!(
                "{} is owned by another user; it is refused",
                path.display()
            )));
        }
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(KEY_BYTES + 1));
    file.take(KEY_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    let mut key = Zeroizing::new([0u8; KEY_BYTES]);
    if bytes.len() != KEY_BYTES {
        return Err(io::Error::other(format!(
            "{} is not a 32-byte key",
            path.display()
        )));
    }
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Makes the key file whole or not at all: written and synced under a name of
/// its own, linked to `path` (which fails if there is one: the first start
/// wins), the temporary name removed and the folder synced.
fn create_key(folder: &Path, path: &Path, key: &[u8; KEY_BYTES]) -> io::Result<()> {
    let mut suffix = [0u8; 8];
    if !crate::host::random(&mut suffix) {
        return Err(io::Error::other("no random numbers for a name"));
    }
    let name: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    let temp = folder.join(format!("{FILE}.{name}.new"));
    let made = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        {
            // Closed before the link is made.
            let mut file = options.open(&temp)?;
            file.write_all(key)?;
            file.sync_all()?;
        }
        std::fs::hard_link(&temp, path)
    })();
    let _ = std::fs::remove_file(&temp);
    made?;
    // The link is on disk before the key is used for a secret. (Windows
    // cannot open a folder as a file; its file system journals the link.)
    #[cfg(unix)]
    std::fs::File::open(folder)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = folder;
    Ok(())
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

    #[test]
    fn two_first_starts_at_once_end_with_the_same_key() {
        let dir = crate::private::dir("paperpaw-sealing-race").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (dir, barrier) = (dir.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    let sealer = KeySealer::in_folder(&dir).unwrap();
                    // What each start can open of what another sealed.
                    let mut out = [0u8; 64];
                    let n = sealer.seal(b"a", b"v", &mut out).unwrap();
                    (out, n)
                })
            })
            .collect();
        let sealed: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let sealer = KeySealer::in_folder(&dir).unwrap();
        for (out, n) in &sealed {
            let mut plain = [0u8; 8];
            assert_eq!(sealer.open(b"a", &out[..*n], &mut plain), Ok(1));
        }
        // No temporary file is left.
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, [std::ffi::OsString::from(FILE)]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_others_can_write_to_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::private::dir("paperpaw-sealing-folder").unwrap();
        KeySealer::in_folder(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(KeySealer::in_folder(&dir).is_err());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(KeySealer::in_folder(&dir).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
