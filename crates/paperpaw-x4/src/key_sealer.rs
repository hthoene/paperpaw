//! `core.secrets` on the X4 apart from the chip (decision 138, 157): values are
//! sealed with AES-256-GCM under the device key ([`crate::device_key`]), which
//! stays in the device's flash and never goes on the card. The firmware gives
//! the flash ([`KeyStore`]), the cipher ([`Aead`]: mbedTLS) and the random
//! source; the rules are here, tested on a computer.
//!
//! - A key and every nonce come from the random source only while it is
//!   strong (a radio is on); otherwise a seal is `ERR_STATE` (the host asks to
//!   turn Wi-Fi on) and nothing is written. Never a weak fallback.
//! - Every seal draws a fresh 12-byte nonce. One that repeats the previous
//!   nonce of this start, or is all zeros or all ones, is refused
//!   (`ERR_IO`): the generator has stopped (Senior, #2465). Not `ERR_STATE`,
//!   which only weak numbers answer: a radio does not mend it (Senior M1, #254).
//! - A key that reads is never replaced; a flash that fails to read is never
//!   erased; a sector that is not ours ([`Sector::Foreign`]) is never erased
//!   or written. A new key is made at the first seal only, read again just
//!   before the erase, and counts once it reads back as written.
//! - No place for a key (no `nvs`, a sector that is not ours, decision 141's
//!   table) is `ERR_UNSUPPORTED` for a seal, as decision 141 has it: the host's
//!   "reinstall once" hint, and the start-up re-save gives up for this start
//!   (Senior M1, #254). For an open it is "absent" (`ERR_NOT_FOUND`).
//! - The key is read for each seal or open and wiped after: it is not held
//!   between calls.
//! - A value that does not open (another device's card, a lost key, changed
//!   bytes) is `ERR_NOT_FOUND`, with the output wiped: core counts it absent.
//! - Nothing here allocates, logs or panics on what the card holds.

use alloc::rc::Rc;
use core::cell::Cell;

use paperpaw_core::native_host::shell_wiring::{Rng, Sealer};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};

use crate::device_key::{self as dk, KEY, NONCE, Sector, TAG};

/// The device's key sector, as the firmware reaches it.
pub trait KeyStore {
    /// Reads the first [`dk::LEN`] bytes of the key sector.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND`: this device keeps no key (no place in its partition
    /// table); `ERR_UNSUPPORTED`: an X4 Classic on a table that gives no place
    /// (decision 141: the user is told to reinstall once with the web
    /// flasher); `ERR_IO`, `ERR_BUSY`, `ERR_NO_MEMORY`.
    fn read(&self, out: &mut [u8; dk::LEN]) -> Result<(), i32>;
    /// Erases the key sector and writes `record` at its start.
    ///
    /// # Errors
    /// `ERR_IO`, `ERR_BUSY`.
    fn replace(&self, record: &[u8; dk::LEN]) -> Result<(), i32>;
}

/// AES-256-GCM with a 12-byte nonce and a 16-byte tag.
pub trait Aead {
    /// Seals `plain` into `cipher` (as long) and `tag`.
    ///
    /// # Errors
    /// Any status: nothing sealed.
    fn seal(
        &self,
        key: &[u8; KEY],
        nonce: &[u8; NONCE],
        aad: &[u8],
        plain: &[u8],
        cipher: &mut [u8],
        tag: &mut [u8; TAG],
    ) -> Result<(), i32>;
    /// Opens `cipher` into `plain` (as long) when `tag` matches.
    ///
    /// # Errors
    /// Any status: the value does not open.
    fn open(
        &self,
        key: &[u8; KEY],
        nonce: &[u8; NONCE],
        aad: &[u8],
        cipher: &[u8],
        tag: &[u8; TAG],
        plain: &mut [u8],
    ) -> Result<(), i32>;
}

/// Overwrites `bytes` with zeros in a way the compiler keeps.
pub fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    core::hint::black_box(&*bytes);
}

/// A key that wipes itself.
struct Key([u8; KEY]);

impl Drop for Key {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// Seals secrets with the device key.
pub struct KeySealer<S, A> {
    store: S,
    aead: A,
    random: Rc<dyn Rng>,
    /// The previous nonce drawn in this start (all zeros: none yet, and never
    /// a nonce that is used).
    last: Cell<[u8; NONCE]>,
}

impl<S, A> core::fmt::Debug for KeySealer<S, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("KeySealer").finish_non_exhaustive()
    }
}

impl<S: KeyStore, A: Aead> KeySealer<S, A> {
    pub fn new(store: S, aead: A, random: Rc<dyn Rng>) -> Self {
        Self {
            store,
            aead,
            random,
            last: Cell::new([0; NONCE]),
        }
    }

    /// What the key's sector holds. A flash that cannot be read is an error,
    /// not an empty sector.
    fn sector(&self) -> Result<Sector, i32> {
        let mut record = [0u8; dk::LEN];
        let read = self.store.read(&mut record).map(|()| dk::read(&record));
        wipe(&mut record);
        read
    }

    /// The device key; with `make`, a new one where the sector has none.
    fn key(&self, make: bool) -> Result<Key, i32> {
        match self.sector().map_err(|e| no_place(make, e))? {
            Sector::Key(key) => return Ok(Key(key)),
            // Not ours: never erased, never written over; nothing is sealed.
            Sector::Foreign => return Err(if make { ERR_UNSUPPORTED } else { ERR_NOT_FOUND }),
            // Erased or damaged: nothing sealed under it opens.
            _ if !make => return Err(ERR_NOT_FOUND),
            _ => {}
        }
        // A key from the generator when it is random, and from nothing else.
        let mut key = Key([0; KEY]);
        if !self.random.fill(&mut key.0)? {
            return Err(ERR_STATE);
        }
        // Read again before a key is replaced: one bad read must not destroy
        // a key that is there.
        match self.sector().map_err(|e| no_place(make, e))? {
            Sector::Key(there) => return Ok(Key(there)),
            Sector::Foreign => return Err(ERR_UNSUPPORTED),
            _ => {}
        }
        let mut record = dk::record(&key.0);
        let written = self.store.replace(&record);
        wipe(&mut record);
        written?;
        // It counts once it reads back as written.
        match self.sector()? {
            Sector::Key(back) if Key(back).0 == key.0 => Ok(key),
            _ => Err(ERR_IO),
        }
    }

    /// A fresh nonce from a strong source, never the previous one of this
    /// start nor all zeros or all ones.
    fn nonce(&self, out: &mut [u8; NONCE]) -> Result<(), i32> {
        if !self.random.fill(out)? {
            return Err(ERR_STATE);
        }
        let stuck = *out == self.last.get()
            || out.iter().all(|&b| b == 0)
            || out.iter().all(|&b| b == 0xFF);
        self.last.set(*out);
        if stuck { Err(ERR_IO) } else { Ok(()) }
    }
}

/// No place for the key is `ERR_UNSUPPORTED` where a key is to be made (a
/// seal: the host tells the user to reinstall once, decision 141) and "absent"
/// where one is only read. Never `ERR_STATE`, which asks for a radio that does
/// not help here (Senior M1, #254).
fn no_place(make: bool, error: i32) -> i32 {
    match (make, error) {
        (true, ERR_NOT_FOUND) => ERR_UNSUPPORTED,
        (false, ERR_UNSUPPORTED) => ERR_NOT_FOUND,
        _ => error,
    }
}

impl<S: KeyStore, A: Aead> Sealer for KeySealer<S, A> {
    fn seals(&self) -> bool {
        true
    }

    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        if plain.len() > dk::MAX_PLAIN {
            return Err(ERR_INVALID);
        }
        let total = NONCE + plain.len() + TAG;
        let out = out.get_mut(..total).ok_or(ERR_NO_SPACE)?;
        let (nonce, rest) = out.split_at_mut(NONCE);
        let (cipher, tag) = rest.split_at_mut(plain.len());
        let (Ok(nonce), Ok(tag)) = (
            <&mut [u8; NONCE]>::try_from(nonce),
            <&mut [u8; TAG]>::try_from(tag),
        ) else {
            return Err(ERR_INVALID);
        };
        self.nonce(nonce)?;
        let key = self.key(true)?;
        let sealed = self.aead.seal(&key.0, nonce, aad, plain, cipher, tag);
        drop(key);
        if let Err(error) = sealed {
            wipe(out);
            return Err(error);
        }
        Ok(total)
    }

    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let (nonce, cipher, tag) = dk::split(sealed).ok_or(ERR_INVALID)?;
        let out = out.get_mut(..cipher.len()).ok_or(ERR_NO_SPACE)?;
        // No key (another device, an erased sector): the value is absent.
        let opened = self.key(false).and_then(|key| {
            self.aead
                .open(&key.0, nonce, aad, cipher, tag, out)
                .map_err(|_| ERR_NOT_FOUND)
        });
        if let Err(error) = opened {
            wipe(out);
            return Err(error);
        }
        Ok(cipher.len())
    }
}

#[cfg(test)]
#[path = "key_sealer_tests.rs"]
mod tests;
