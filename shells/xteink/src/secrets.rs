//! `core.secrets` on this device (decisions 138, 140, 157): values are sealed
//! with AES-256-GCM under a per-device key kept in the internal flash, never
//! on the card. The rules are `paperpaw_x4::key_sealer`'s (tested on a
//! computer); this is the chip's part of it: the key's sector in the flash
//! ([`FlashKey`]), mbedTLS's AES-GCM ([`MbedGcm`], already in the firmware for
//! TLS; the AES peripheral would need a GCM of our own, Senior #2465) and the
//! chip's random numbers ([`ChipRandom`], strong only while a radio is on).
//!
//! The key's sector (`paperpaw_x4::device_key::place`): the fourth sector of
//! the `nvs` partition on the X4, X3 and X4 Pro; on the X4 Classic the last
//! sector of the flash, outside every partition. A table that gives no place
//! (a Classic still on CrossPoint's table, decision 141) is `ERR_UNSUPPORTED`:
//! nothing is sealed, values already there stay readable.
//!
//! Memory: nothing held but the sector's address; per seal or open the GCM
//! context (about 450 bytes) on the heap, the key and its record on the
//! stack, all wiped after. The partition table is read once, at the first
//! use, into a 3 KiB block taken fallibly and freed at once.

use alloc::vec::Vec;
use core::cell::Cell;
use core::mem::MaybeUninit;

use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, PARTITION_TABLE_MAX_LEN, PartitionType,
};
use mbedtls_rs::sys::{
    MBEDTLS_GCM_ENCRYPT, mbedtls_cipher_id_t_MBEDTLS_CIPHER_ID_AES, mbedtls_gcm_auth_decrypt,
    mbedtls_gcm_context, mbedtls_gcm_crypt_and_tag, mbedtls_gcm_free, mbedtls_gcm_init,
    mbedtls_gcm_setkey,
};
use paperpaw_core::native_host::shell_wiring::{
    Rng, SecretsInputs, SecretsLifecycle, WiredHosts,
};
use paperpaw_platform::Storage;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_x4::device_key::{self as dk, KEY, NONCE, TAG};
use paperpaw_x4::key_sealer::{Aead, KeySealer, KeyStore};

use crate::ota::Flash;

/// The sealer `core.secrets` is wired with.
pub type DeviceSealer = KeySealer<FlashKey, MbedGcm>;

/// The sealer of this device, reading its key from `flash`.
pub fn sealer(flash: Flash, random: alloc::rc::Rc<dyn Rng>) -> DeviceSealer {
    KeySealer::new(
        FlashKey {
            flash,
            at: Cell::new(None),
        },
        MbedGcm,
        random,
    )
}

/// The chip's generator, strong while a radio is on ([`crate::entropy`]).
pub struct ChipRandom;

impl Rng for ChipRandom {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        let strong = crate::entropy::strong();
        esp_hal::rng::Rng::new().read(out);
        Ok(strong)
    }
}

/// The key's sector in the internal flash.
pub struct FlashKey {
    flash: Flash,
    /// Where the sector is, found at the first use.
    at: Cell<Option<u32>>,
}

impl FlashKey {
    /// Where the key's sector is, from the partition table (read once).
    fn sector(&self) -> Result<u32, i32> {
        if let Some(at) = self.at.get() {
            return Ok(at);
        }
        let mut flash = self.flash.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let mut table = Vec::new();
        table
            .try_reserve_exact(PARTITION_TABLE_MAX_LEN)
            .map_err(|_| ERR_NO_MEMORY)?;
        table.resize(PARTITION_TABLE_MAX_LEN, 0u8);
        let pt =
            partitions::read_partition_table(&mut flash, &mut table[..]).map_err(|_| ERR_IO)?;
        let nvs = pt
            .find_partition(PartitionType::Data(DataPartitionSubType::Nvs))
            .ok()
            .flatten()
            .map(|p| (p.offset(), p.len()));
        let at = dk::locate(
            dk::place(cfg!(feature = "x4-classic")),
            nvs,
            pt.iter().map(|p| (p.offset(), p.len())),
        )
        .ok_or(ERR_UNSUPPORTED)?;
        self.at.set(Some(at));
        Ok(at)
    }
}

impl KeyStore for FlashKey {
    fn read(&self, out: &mut [u8; dk::LEN]) -> Result<(), i32> {
        let at = self.sector()?;
        self.flash
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .read(at, out)
            .map_err(|_| ERR_IO)
    }

    fn replace(&self, record: &[u8; dk::LEN]) -> Result<(), i32> {
        let at = self.sector()?;
        let mut flash = self.flash.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        flash
            .erase(at, at + dk::SECTOR as u32)
            .and_then(|()| flash.write_nor(at, record))
            .map_err(|_| ERR_IO)
    }
}

/// mbedTLS's AES-256-GCM.
pub struct MbedGcm;

/// The GCM context, on the heap (about 450 bytes), freed (and so wiped) at
/// the end.
struct Gcm(Vec<MaybeUninit<mbedtls_gcm_context>>);

impl Gcm {
    fn new(key: &[u8; KEY]) -> Result<Self, i32> {
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
                key.as_ptr(),
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
        // SAFETY: made by `mbedtls_gcm_init` in `new`, once; `free` zeroizes it.
        unsafe { mbedtls_gcm_free(self.ptr()) };
    }
}

impl Aead for MbedGcm {
    fn seal(
        &self,
        key: &[u8; KEY],
        nonce: &[u8; NONCE],
        aad: &[u8],
        plain: &[u8],
        cipher: &mut [u8],
        tag: &mut [u8; TAG],
    ) -> Result<(), i32> {
        if cipher.len() != plain.len() {
            return Err(ERR_INVALID);
        }
        let mut gcm = Gcm::new(key)?;
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
        if done == 0 { Ok(()) } else { Err(ERR_IO) }
    }

    fn open(
        &self,
        key: &[u8; KEY],
        nonce: &[u8; NONCE],
        aad: &[u8],
        cipher: &[u8],
        tag: &[u8; TAG],
        plain: &mut [u8],
    ) -> Result<(), i32> {
        if cipher.len() != plain.len() {
            return Err(ERR_INVALID);
        }
        let mut gcm = Gcm::new(key)?;
        // SAFETY: as in `seal`; `cipher` and `plain` are different slices.
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
                plain.as_mut_ptr(),
            )
        };
        if done == 0 { Ok(()) } else { Err(ERR_IO) }
    }
}

/// What the loader wires `core.secrets` with, and where it leaves what the
/// firmware keeps of it.
pub struct Wiring {
    flash: Flash,
    kept: Kept,
}

impl Wiring {
    pub fn new(flash: Flash, kept: Kept) -> Self {
        Self { flash, kept }
    }

    /// The card's secrets file, this device's sealer and its random numbers.
    pub fn inputs(&self, storage: &alloc::rc::Rc<dyn Storage>) -> SecretsInputs {
        let random: alloc::rc::Rc<dyn Rng> = alloc::rc::Rc::new(ChipRandom);
        let sealer = alloc::rc::Rc::new(sealer(self.flash.clone(), random.clone()));
        SecretsInputs::new(storage.clone(), sealer).with_rng(random)
    }

    /// Hands the firmware what it calls outside a plugin's call.
    pub fn keep(&self, wired: &mut WiredHosts) {
        if let Some(secrets) = wired.take_secrets() {
            self.kept.keep(secrets);
        }
    }
}

/// What the loader hands over of `core.secrets` once it is wired: the calls
/// the firmware makes outside a plugin's call. Shared between the loader,
/// which fills it, and the firmware's [`Secrets`].
#[derive(Clone, Default)]
pub struct Kept(alloc::rc::Rc<core::cell::RefCell<Option<SecretsLifecycle>>>);

impl Kept {
    /// Keeps what the wiring gave.
    pub fn keep(&self, secrets: SecretsLifecycle) {
        if let Ok(mut slot) = self.0.try_borrow_mut() {
            *slot = Some(secrets);
        }
    }
}

/// `core.secrets` in the firmware's main loop.
pub struct Secrets {
    kept: Kept,
    /// Beta 3's veiled values were sealed this start, or there is nothing to
    /// seal, or it failed for another reason than weak numbers.
    done: bool,
}

impl Secrets {
    pub fn new(kept: Kept) -> Self {
        Self { kept, done: false }
    }

    /// Before the kernel starts: interface 6's passwords go through the same
    /// sealer (`Kernel::wire_secrets`), and Beta 3's veiled values are sealed
    /// if the numbers are strong already.
    pub fn started(&mut self, kernel: &mut paperpaw_core::Kernel) {
        let Ok(slot) = self.kept.0.try_borrow() else {
            return;
        };
        let Some(secrets) = slot.as_ref() else {
            self.done = true;
            return;
        };
        kernel.wire_secrets(secrets);
        self.done = upgrade(secrets);
    }

    /// Once per main-loop turn until done (one flag and one atomic read): at
    /// start the radio is off and the numbers weak, so Beta 3's veiled values
    /// are sealed the first time a radio is on (Senior R1, #2465).
    #[inline(never)]
    pub fn poll(&mut self) {
        if self.done || !crate::entropy::strong() {
            return;
        }
        if let Ok(slot) = self.kept.0.try_borrow()
            && let Some(secrets) = slot.as_ref()
        {
            self.done = upgrade(secrets);
        }
    }
}

/// Seals Beta 3's veiled values; whether this start is done with it (not
/// when the numbers were weak: then again when a radio is on).
fn upgrade(secrets: &SecretsLifecycle) -> bool {
    match dk::startup_upgrade(secrets.upgrade(), ERR_UNSUPPORTED) {
        dk::StartupUpgrade::Rewritten => {
            log::info!("secrets: veiled values sealed");
            true
        }
        dk::StartupUpgrade::Nothing | dk::StartupUpgrade::NoKeyPlace => true,
        dk::StartupUpgrade::Failed(paperpaw_native::host_glue::ERR_STATE) => false,
        dk::StartupUpgrade::Failed(status) => {
            log::warn!("secrets: veiled values not sealed ({status})");
            true
        }
    }
}
