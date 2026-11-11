//! `core.secrets` on the X4 (decision 138): values are sealed with AES-256-GCM
//! under a per-device key kept in the `nvs` partition of the internal flash
//! (`paperpaw_x4::device_key`), never on the card. A card from another device
//! opens nothing: its values count as absent here, never a crash.
//!
//! - The key is read for each seal or open and wiped after: it is not held in
//!   RAM between calls.
//! - A new key and every nonce come from the chip's generator only while the
//!   radio is on ([`crate::entropy`]); otherwise a seal is refused and the
//!   host does not replace the file. Nothing falls back to weak numbers.
//! - A key that reads is never replaced. A flash that fails to read is not
//!   erased. A sector without a valid key (erased, damaged) gets a new key
//!   only at a seal.
//! - A key sector that holds data that is not ours (an NVS page), and the X4
//!   Classic's (its `nvs` is the factory's), are never erased or written:
//!   nothing is sealed there.
//! - Statuses: `ERR_STATE` means "the random numbers are weak now" (the host
//!   then asks to turn Wi-Fi on) or "this device keeps no key now" (above); a failing cipher or flash is `ERR_IO`, a flash that
//!   somebody else holds is `ERR_BUSY`.
//! - Fallible throughout: the GCM context is made with `try_reserve`, a
//!   panic is never the answer to a value that does not open.

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::Cell;
use core::mem::MaybeUninit;

use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, PARTITION_TABLE_MAX_LEN, PartitionType,
};
use log::warn;
use mbedtls_rs::sys::{
    MBEDTLS_GCM_ENCRYPT, mbedtls_cipher_id_t_MBEDTLS_CIPHER_ID_AES, mbedtls_gcm_auth_decrypt,
    mbedtls_gcm_context, mbedtls_gcm_crypt_and_tag, mbedtls_gcm_free, mbedtls_gcm_init,
    mbedtls_gcm_setkey, mbedtls_platform_zeroize,
};
use paperpaw_core::native_host::shell_wiring::{Rng, Sealer};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};
use paperpaw_x4::device_key::{self as dk, KEY, NONCE, Sector, TAG};

use crate::ota::Flash;

/// The chip's generator, strong while a radio is on.
pub struct ChipRandom;

impl Rng for ChipRandom {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        let strong = crate::entropy::strong();
        esp_hal::rng::Rng::new().read(out);
        Ok(strong)
    }
}

/// Overwrites `bytes` so the compiler cannot drop it.
fn wipe(bytes: &mut [u8]) {
    // SAFETY: a valid, exclusively borrowed slice of the length given.
    unsafe { mbedtls_platform_zeroize(bytes.as_mut_ptr().cast(), bytes.len()) };
}

/// A key that wipes itself.
struct Key([u8; KEY]);

impl Drop for Key {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// The GCM context, on the heap (it is about 450 bytes), freed (and so
/// wiped) at the end.
struct Gcm(Vec<MaybeUninit<mbedtls_gcm_context>>);

impl Gcm {
    fn new(key: &Key) -> Result<Self, i32> {
        let mut cell = Vec::new();
        cell.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
        cell.push(MaybeUninit::zeroed());
        let mut gcm = Self(cell);
        // SAFETY: the context is zeroed memory of the right size; `init` makes it one.
        unsafe { mbedtls_gcm_init(gcm.ptr()) };
        // SAFETY: as above; a key of 256 bits is `KEY` bytes.
        let set = unsafe {
            mbedtls_gcm_setkey(
                gcm.ptr(),
                mbedtls_cipher_id_t_MBEDTLS_CIPHER_ID_AES,
                key.0.as_ptr(),
                (KEY * 8) as u32,
            )
        };
        if set != 0 {
            return Err(ERR_IO);
        }
        Ok(gcm)
    }

    fn ptr(&mut self) -> *mut mbedtls_gcm_context {
        self.0[0].as_mut_ptr()
    }
}

impl Drop for Gcm {
    fn drop(&mut self) {
        // SAFETY: made by `mbedtls_gcm_init` in `new`, once.
        unsafe { mbedtls_gcm_free(self.ptr()) };
    }
}

/// Seals secrets with the device key.
pub struct DeviceSealer {
    flash: Flash,
    /// Where the key is in the flash, found at the first use.
    at: Cell<Option<u32>>,
    random: Rc<dyn Rng>,
}

impl DeviceSealer {
    pub fn new(flash: Flash, random: Rc<dyn Rng>) -> Self {
        Self {
            flash,
            at: Cell::new(None),
            random,
        }
    }

    /// Where the key's sector is: in the `nvs` partition, which must hold it.
    fn sector(&self) -> Result<u32, i32> {
        if let Some(at) = self.at.get() {
            return Ok(at);
        }
        // The X4 Classic's `nvs` is the factory's NVS (`0x9000` to `0xE000`,
        // "kept whole, never written": it says which panel the device has), and
        // the key's sector is inside it. No key is kept there until decision
        // 141 gives it a place of its own: nothing is sealed (`ERR_STATE`),
        // nothing is written.
        if cfg!(feature = "x4-classic") {
            return Err(ERR_STATE);
        }
        let mut flash = self.flash.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let mut table = Vec::new();
        table
            .try_reserve_exact(PARTITION_TABLE_MAX_LEN)
            .map_err(|_| ERR_NO_MEMORY)?;
        table.resize(PARTITION_TABLE_MAX_LEN, 0u8);
        let found = partitions::read_partition_table(&mut flash, &mut table[..])
            .ok()
            .and_then(|pt| {
                pt.find_partition(PartitionType::Data(DataPartitionSubType::Nvs))
                    .ok()
                    .flatten()
                    .map(|p| (p.offset(), p.len()))
            });
        let (offset, len) = found.ok_or(ERR_NOT_FOUND)?;
        if (len as usize) < dk::OFFSET as usize + dk::SECTOR {
            return Err(ERR_NOT_FOUND);
        }
        let at = offset + dk::OFFSET;
        self.at.set(Some(at));
        Ok(at)
    }

    /// What the key's sector holds. A flash that cannot be read is an error,
    /// not an empty sector.
    fn read(&self) -> Result<Sector, i32> {
        let at = self.sector()?;
        let mut record = [0u8; dk::LEN];
        self.flash
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .read(at, &mut record)
            .map_err(|_| ERR_IO)?;
        let sector = dk::read(&record);
        wipe(&mut record);
        Ok(sector)
    }

    /// The device key; with `make`, a new one where there is none.
    fn key(&self, make: bool) -> Result<Key, i32> {
        match self.read()? {
            Sector::Key(key) => return Ok(Key(key)),
            // Not ours: never erased, never written over (a page of the
            // factory's or the stock firmware's NVS); nothing is sealed.
            Sector::Foreign => return Err(if make { ERR_STATE } else { ERR_NOT_FOUND }),
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
        match self.read()? {
            Sector::Key(there) => return Ok(Key(there)),
            Sector::Foreign => return Err(ERR_STATE),
            _ => {}
        }
        let at = self.sector()?;
        let mut record = dk::record(&key.0);
        {
            let mut flash = self.flash.try_borrow_mut().map_err(|_| ERR_BUSY)?;
            let written = flash
                .erase(at, at + dk::SECTOR as u32)
                .and_then(|()| flash.write_nor(at, &record));
            wipe(&mut record);
            written.map_err(|_| ERR_IO)?;
        }
        // It counts once it reads back as written.
        match self.read()? {
            Sector::Key(back) if Key(back).0 == key.0 => Ok(key),
            _ => {
                warn!("secrets: the device key was not written");
                Err(ERR_IO)
            }
        }
    }
}

impl Sealer for DeviceSealer {
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
        // A fresh nonce for every seal, from the radio's noise: not a counter.
        if !self.random.fill(nonce)? {
            return Err(ERR_STATE);
        }
        let key = self.key(true)?;
        let mut gcm = Gcm::new(&key)?;
        // SAFETY: every pointer is to a slice of the length given beside it;
        // `plain` and `cipher` do not overlap; the context is made.
        let done = unsafe {
            mbedtls_gcm_crypt_and_tag(
                gcm.ptr(),
                MBEDTLS_GCM_ENCRYPT as i32,
                plain.len(),
                nonce.as_ptr(),
                NONCE,
                aad.as_ptr(),
                aad.len(),
                plain.as_ptr(),
                cipher.as_mut_ptr(),
                TAG,
                tag.as_mut_ptr(),
            )
        };
        if done != 0 {
            out.fill(0);
            return Err(ERR_IO);
        }
        Ok(total)
    }

    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let (nonce, cipher, tag) = dk::split(sealed).ok_or(ERR_INVALID)?;
        let out = out.get_mut(..cipher.len()).ok_or(ERR_NO_SPACE)?;
        // No key (another device, an erased sector): the value is absent.
        let key = self.key(false)?;
        let mut gcm = Gcm::new(&key)?;
        // SAFETY: as in `seal`; `cipher` and `out` are different slices.
        let done = unsafe {
            mbedtls_gcm_auth_decrypt(
                gcm.ptr(),
                cipher.len(),
                nonce.as_ptr(),
                NONCE,
                aad.as_ptr(),
                aad.len(),
                tag.as_ptr(),
                TAG,
                cipher.as_ptr(),
                out.as_mut_ptr(),
            )
        };
        if done != 0 {
            // mbedTLS wipes the output of a failed check; so do we.
            out.fill(0);
            return Err(ERR_NOT_FOUND);
        }
        Ok(cipher.len())
    }
}
