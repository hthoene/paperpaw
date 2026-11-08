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
//!   replaced. Where the folder has no hard links (a FAT folder), the same
//!   comes from a rename that never replaces (`renameat2` with
//!   `RENAME_NOREPLACE` on Linux, `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`);
//!   only where the system has no such rename, from a lock file made
//!   exclusively around the check and a plain rename (a lock that stood for ten
//!   seconds is a leftover and is taken away, and that is the one case two
//!   creators can still both pass).
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
    /// Whether the folder takes hard links (tests turn it off to stand for a
    /// FAT folder, which has none).
    links: bool,
    /// Whether the rename that never replaces is tried (tests turn it off to
    /// reach the lock file, which is only for where it is unsupported).
    rename_new: bool,
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
        Self {
            path: path.into(),
            links: true,
            rename_new: true,
        }
    }

    /// A sealer over a folder with no hard links (what a FAT folder is).
    #[cfg(test)]
    fn without_hard_links(mut self) -> Self {
        self.links = false;
        self
    }

    /// A sealer whose system has no rename that never replaces (the lock file
    /// is then the last resort).
    #[cfg(test)]
    fn without_rename_new(mut self) -> Self {
        self.rename_new = false;
        self
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
                self.publish_new(&temp)?;
            }
            sync_dir(&self.path)
        })();
        record.zeroize();
        // Whatever the outcome, THIS call's temporary name goes (after a rename
        // it is already gone). Nothing else is ever deleted here: a leftover
        // temporary file of an earlier power cut may hold a key that was written
        // and not yet published, and is not swept without looking at the key.
        let _ = fs::remove_file(&temp);
        result.map_err(|_| ERR_IO)
    }
}

/// How long a lock file may stand before it is taken for a leftover of a
/// process that ended with it, and how long and how often a creator waits.
const STALE: std::time::Duration = std::time::Duration::from_secs(10);
const WAIT_STEP: std::time::Duration = std::time::Duration::from_millis(20);
const WAIT_STEPS: u32 = 100;

/// A lock file held: removed when it goes out of use.
struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl FileSealer {
    /// Puts `temp` at the key file's path where no file is, leaving one that is
    /// there alone (no error). A hard link does it in one step; where the
    /// folder has none (a FAT folder) a rename that never replaces does it
    /// (`renameat2` with `RENAME_NOREPLACE`, `MoveFileExW` without
    /// `MOVEFILE_REPLACE_EXISTING`: both are atomic and both work on FAT); only
    /// where the system has neither is a lock file the last resort.
    fn publish_new(&self, temp: &Path) -> std::io::Result<()> {
        let linked = if self.links {
            fs::hard_link(temp, &self.path)
        } else {
            Err(std::io::ErrorKind::Unsupported.into())
        };
        match linked {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            Err(_) => {}
        }
        if self.rename_new {
            match rename_new(temp, &self.path) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {}
                Err(e) => return Err(e),
            }
        }
        self.publish_locked(temp)
    }

    /// The last resort: an exclusive lock file (`create_new`) held across "is a
    /// file there?" and the rename. A lock that stood for [`STALE`] is the
    /// leftover of a creator that was cut off and is taken away; two who do it
    /// at once (or a clock that jumps) can break a live lock, which is why this
    /// is only for a system with no rename that never replaces.
    fn publish_locked(&self, temp: &Path) -> std::io::Result<()> {
        let mut name = self.path.clone().into_os_string();
        name.push(".lock");
        let lock_path = PathBuf::from(name);
        let mut tries = 0;
        let _lock = loop {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&lock_path) {
                Ok(_) => break Lock(lock_path.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Every round counts, so that no lock can hold this for
                    // ever: a lock that cannot be taken away (a folder, a
                    // refused removal) ends it with that error.
                    tries += 1;
                    if tries >= WAIT_STEPS {
                        return Err(std::io::ErrorKind::WouldBlock.into());
                    }
                    let old = fs::symlink_metadata(&lock_path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > STALE);
                    if old {
                        match fs::remove_file(&lock_path) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => return Err(e),
                        }
                    } else {
                        std::thread::sleep(WAIT_STEP);
                    }
                }
                Err(e) => return Err(e),
            }
        };
        // Under the lock: a file there is left alone (the first creator won).
        if fs::symlink_metadata(&self.path).is_ok() {
            return Ok(());
        }
        fs::rename(temp, &self.path)
    }
}

/// Moves `from` to `to` only where `to` is not: `Ok`, `AlreadyExists` (it
/// was there, nothing is changed) or `Unsupported` (this system or file system
/// has no such rename).
#[cfg(target_os = "linux")]
fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    /// `RENAME_NOREPLACE` of `renameat2` (linux/fs.h).
    const RENAME_NOREPLACE: libc::c_uint = 1;
    let c = |p: &Path| {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))
    };
    let (from, to) = (c(from)?, c(to)?);
    // SAFETY: both are NUL-terminated strings that live through the call; the
    // call writes nothing through them. `syscall` rather than the libc
    // wrapper, which old Androids and libcs lack.
    let r = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if r == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EEXIST) => Err(std::io::ErrorKind::AlreadyExists.into()),
        // The kernel or the file system does not know the flag.
        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP | libc::EPERM) => {
            Err(std::io::ErrorKind::Unsupported.into())
        }
        _ => Err(e),
    }
}

#[cfg(windows)]
fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both are NUL-terminated wide strings that live through the call.
    // Flags 0: no `MOVEFILE_REPLACE_EXISTING`, so an existing file is an error
    // (ERROR_ALREADY_EXISTS or ERROR_FILE_EXISTS, which std maps to
    // `AlreadyExists`).
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

// Android too, on purpose: its apps' seccomp filter may kill the process on
// `renameat2` (older releases). The app-private folder (ext4/f2fs) takes hard
// links, so the link is what publishes the key there; and the app is one
// process, so the lock of `publish_locked` is only a last resort that no second
// creator can race (the two-remover case needs a second process).
#[cfg(not(any(target_os = "linux", windows)))]
fn rename_new(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
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
