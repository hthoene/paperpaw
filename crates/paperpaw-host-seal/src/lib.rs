//! Sealing of saved passwords where the shell has a file system of its own:
//! the desktop emulator and Android (decision 140). Values are sealed with
//! AES-256-GCM under a per-device key kept in a file the shell names, which is
//! never the card's or the user's book folder: the emulator's own data folder,
//! Android's app-private folder. A folder, a card or a copy of the settings
//! moved to another device opens none of them (the values count as absent and
//! are entered again).
//!
//! The X4 does the same with a sector of its flash (`paperpaw_x4::device_key`);
//! the key file is that record: `MAGIC (8) | key (32) | CRC-32 (4)`, and a
//! sealed value is `nonce (12) | ciphertext | tag (16)`, at most 1,052 bytes.
//!
//! - The key is read for each seal or open and wiped after; it is not held
//!   between calls.
//! - A key that reads is never replaced. A file that cannot be read is not
//!   replaced either (`ERR_IO`). A missing, erased or damaged file gets a new key
//!   only at a seal, from the system's random numbers; opening answers "absent".
//! - The file is made private (mode 0600 on Unix, from its first byte),
//!   written beside itself and put in place without replacing a file that is
//!   there (the first creator wins, the others use its key), the folder is
//!   synced, and the key is read back. Only a file that is not a key record is
//!   replaced.
//! - A device that cannot make a key refuses the seal (`ERR_IO` or `ERR_STATE`)
//!   and the host writes nothing; there is no weaker fallback.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce, Tag};
use paperpaw_core::native_host::{ShellRandom, ShellSealer};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE};
use zeroize::{Zeroize, Zeroizing};

/// What the key record begins with: the X4's, so a record means the same
/// everywhere.
const MAGIC: [u8; 8] = *b"PPDEVKY1";
const KEY: usize = 32;
const LEN: usize = MAGIC.len() + KEY + 4;
const NONCE: usize = 12;
const TAG: usize = 16;
/// The longest plain value (`core.secrets`).
const MAX_PLAIN: usize = 1024;

/// The system's random numbers: always strong.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsRandom;

impl ShellRandom for OsRandom {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        getrandom::fill(out).map_err(|_| ERR_STATE)?;
        Ok(true)
    }
}

/// Seals secrets with the key in one file.
#[derive(Debug)]
pub struct FileSealer {
    path: PathBuf,
}

/// Key creators in this process, one at a time.
static MAKING: Mutex<()> = Mutex::new(());

/// What the key file holds.
enum Found {
    Key(Zeroizing<[u8; KEY]>),
    /// No file, an erased one, or something that is not a key record.
    None,
}

impl FileSealer {
    /// A sealer whose key file is `path` (its folder must exist and be the
    /// shell's own, never the card's folder).
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The key file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<Found, i32> {
        let mut bytes = Zeroizing::new(Vec::new());
        match fs::read(&self.path) {
            Ok(read) => *bytes = read,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Found::None),
            Err(_) => return Err(ERR_IO),
        }
        Ok(parse(&bytes).map_or(Found::None, Found::Key))
    }

    /// The device key; with `make`, a new one where there is none.
    fn key(&self, make: bool) -> Result<Zeroizing<[u8; KEY]>, i32> {
        if let Found::Key(key) = self.read()? {
            return Ok(key);
        }
        if !make {
            return Err(ERR_NOT_FOUND);
        }
        // One creator at a time in this process; another process is handled
        // by the exclusive publish below.
        let _making = MAKING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut key = Zeroizing::new([0u8; KEY]);
        if !OsRandom.fill(&mut key[..])? {
            return Err(ERR_STATE);
        }
        // Read again before a key is made: one bad read must not destroy a key
        // that is there, and a creator that was first has won.
        if let Found::Key(there) = self.read()? {
            return Ok(there);
        }
        // A key is published without replacing anything (hard link: fails when
        // the file exists). Whoever publishes first wins; the others read the
        // winner's key.
        self.write(&key, false)?;
        if let Found::Key(won) = self.read()? {
            return Ok(won);
        }
        // The file is there but is not a key record (damaged, erased): no key
        // is lost by replacing it.
        self.write(&key, true)?;
        match self.read()? {
            Found::Key(back) => Ok(back),
            Found::None => Err(ERR_IO),
        }
    }

    /// Writes the record to a temporary file made private from the start,
    /// then puts it in place: without `replace` only where no file is (an
    /// existing one is left alone and is not an error), with `replace` over
    /// whatever is there. The folder is synced, so a power cut cannot lose the
    /// key (and with it every saved password).
    fn write(&self, key: &[u8; KEY], replace: bool) -> Result<(), i32> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut record = Zeroizing::new(record(key));
        let mut temp = self.path.clone().into_os_string();
        temp.push(format!(
            ".new-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let temp = PathBuf::from(temp);
        let result = (|| -> std::io::Result<()> {
            // `create_new`: never a file that was there (a leftover, a link),
            // and mode 0600 from the first byte.
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(&record[..])?;
            file.sync_all()?;
            drop(file);
            if replace {
                fs::rename(&temp, &self.path)?;
            } else {
                match fs::hard_link(&temp, &self.path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
            sync_dir(&self.path)
        })();
        record.zeroize();
        // Whatever the outcome, the temporary name goes (after a rename it
        // is already gone).
        let _ = fs::remove_file(&temp);
        result.map_err(|_| ERR_IO)
    }
}

/// Makes the folder entry of `path` durable (Unix; elsewhere the file system
/// has no such call and the file's own sync is all there is).
fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let dir = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

impl ShellSealer for FileSealer {
    fn seals(&self) -> bool {
        true
    }

    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        if plain.len() > MAX_PLAIN {
            return Err(ERR_INVALID);
        }
        let total = NONCE + plain.len() + TAG;
        let out = out.get_mut(..total).ok_or(ERR_NO_SPACE)?;
        let (nonce, rest) = out.split_at_mut(NONCE);
        let (cipher, tag) = rest.split_at_mut(plain.len());
        // A fresh nonce for every seal, from the system: not a counter.
        if !OsRandom.fill(nonce)? {
            return Err(ERR_STATE);
        }
        let key = self.key(true)?;
        let aes = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key[..]));
        cipher.copy_from_slice(plain);
        match aes.encrypt_in_place_detached(Nonce::from_slice(nonce), aad, cipher) {
            Ok(computed) => {
                tag.copy_from_slice(&computed);
                Ok(total)
            }
            Err(_) => {
                out.fill(0);
                Err(ERR_IO)
            }
        }
    }

    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        if sealed.len() < NONCE + TAG || sealed.len() > NONCE + MAX_PLAIN + TAG {
            return Err(ERR_INVALID);
        }
        let (nonce, rest) = sealed.split_at(NONCE);
        let (cipher, tag) = rest.split_at(rest.len() - TAG);
        let out = out.get_mut(..cipher.len()).ok_or(ERR_NO_SPACE)?;
        // No key (another device, a missing file): the value is absent.
        let key = self.key(false)?;
        let aes = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key[..]));
        out.copy_from_slice(cipher);
        if aes
            .decrypt_in_place_detached(Nonce::from_slice(nonce), aad, out, Tag::from_slice(tag))
            .is_err()
        {
            // Wipe what the failed check left.
            out.fill(0);
            return Err(ERR_NOT_FOUND);
        }
        Ok(cipher.len())
    }
}

/// The key of a record, if `bytes` is exactly one (a file is not a sector: no more after it).
fn parse(bytes: &[u8]) -> Option<Zeroizing<[u8; KEY]>> {
    if bytes.len() != LEN {
        return None;
    }
    let (body, crc) = bytes.split_at(LEN - 4);
    if !body.starts_with(&MAGIC) || crc != crc32(body).to_le_bytes() {
        return None;
    }
    let mut key = Zeroizing::new([0u8; KEY]);
    key.copy_from_slice(&body[MAGIC.len()..]);
    Some(key)
}

/// The record that keeps `key`.
fn record(key: &[u8; KEY]) -> [u8; LEN] {
    let mut out = [0u8; LEN];
    out[..MAGIC.len()].copy_from_slice(&MAGIC);
    out[MAGIC.len()..LEN - 4].copy_from_slice(key);
    let crc = crc32(&out[..LEN - 4]);
    out[LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    out
}

/// CRC-32 (IEEE), as the X4's record has it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests;
