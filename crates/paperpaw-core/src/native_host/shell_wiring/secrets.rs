//! Shell-owned objects behind the crate-private secrets service.
use super::*;
use crate::native_host::{
    secrets_file::{Sealer, Veiling},
    secrets_service::{Rng, SecretsHost},
};

/// The shell's sealing of secret values, with no allocation or retained plaintext.
/// On the X4 this uses AES-256-GCM and the device key kept in internal flash.
/// The emulator and Android can use [`SecretsInputs::veiled`] instead.
pub trait ShellSealer {
    /// Whether new values are written sealed (`s1:`) and veiled ones found are
    /// rewritten sealed at the next save.
    fn seals(&self) -> bool;
    /// Seal `plain` into `out`: nonce, ciphertext and tag, at most 1,052 bytes.
    /// `aad` is the line's owner, a zero byte and its name; it authenticates the
    /// value's owner and name. Nothing may log or retain the plaintext.
    ///
    /// Every seal draws a fresh 12-byte nonce from a strong random source,
    /// never a counter that restarts and never one reused under the device key.
    /// A repeated nonce under AES-GCM leaks the key stream and authentication
    /// key. Refuse when a strong nonce is unavailable, without a weak fallback.
    ///
    /// # Errors
    /// Return the failure status for missing entropy, storage/key failure or
    /// insufficient output; the host refuses the save without replacing the file.
    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32>;
    /// Open `sealed` into `out`, returning the plaintext length. The value may
    /// belong to another device or have been damaged; never log its bytes.
    ///
    /// # Errors
    /// Any error means the value is absent here. The host clears its plaintext
    /// scratch even if an implementation wrote beyond the returned length.
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32>;
}

/// The shell's random source, without a weaker stand-in on failure.
pub trait ShellRandom {
    /// Fill `out` and report whether the source is fully random now (`strong`).
    /// ESP32-C3 hardware randomness is strong only while the radio or the
    /// bootloader's entropy source is on. A key, token or nonce must not rely
    /// on weak bytes. A device without a source leaves this input absent.
    ///
    /// # Errors
    /// Return the source's refusal; the host clears its random scratch on error.
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32>;
}

struct Sealing(Rc<dyn ShellSealer>);
impl Sealer for Sealing {
    fn seals(&self) -> bool {
        self.0.seals()
    }
    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        self.0.seal(aad, plain, out)
    }
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        self.0.open(aad, sealed, out)
    }
}
struct Random(Rc<dyn ShellRandom>);
impl Rng for Random {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        self.0.fill(out)
    }
}

/// Shared shell-owned objects for `core.secrets`. No key or plaintext is copied
/// into this adapter. The shell controls each object's lifetime through its Rc.
#[non_exhaustive]
pub struct SecretsInputs {
    storage: Rc<dyn Storage>,
    sealer: Option<Sealing>,
    random: Option<Random>,
    unreadable: Option<Rc<dyn Fn()>>,
}
impl core::fmt::Debug for SecretsInputs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SecretsInputs")
            .field("custom_sealer", &self.sealer.is_some())
            .field("random", &self.random.is_some())
            .field("unreadable", &self.unreadable.is_some())
            .finish()
    }
}
impl SecretsInputs {
    /// Wire shell-provided storage and sealing. The X4 shell supplies its
    /// device-key sealer; core never creates, persists or exports that key.
    #[must_use]
    pub fn new(storage: Rc<dyn Storage>, sealer: Rc<dyn ShellSealer>) -> Self {
        Self {
            storage,
            sealer: Some(Sealing(sealer)),
            random: None,
            unreadable: None,
        }
    }
    /// Keep interface 6's veiling on the emulator or in Android's private
    /// folder. Veiling hides values from casual viewing; it is not encryption.
    /// Do not use this constructor for the X4.
    #[must_use]
    pub fn veiled(storage: Rc<dyn Storage>) -> Self {
        Self {
            storage,
            sealer: None,
            random: None,
            unreadable: None,
        }
    }
    /// Set the random source. Without one, random requests are unsupported.
    #[must_use]
    pub fn random(mut self, source: Rc<dyn ShellRandom>) -> Self {
        self.random = Some(Random(source));
        self
    }
    /// Notify the shell once when reading the file fails with an I/O error.
    /// The core's secrets service owns the once-only check.
    #[must_use]
    pub fn unreadable(mut self, notice: Rc<dyn Fn()>) -> Self {
        self.unreadable = Some(notice);
        self
    }
}
impl SecretsHost for SecretsInputs {
    fn storage(&self) -> &dyn Storage {
        self.storage.as_ref()
    }
    fn sealer(&self) -> &dyn Sealer {
        self.sealer
            .as_ref()
            .map_or(&Veiling, |sealer| sealer as &dyn Sealer)
    }
    fn rng(&self) -> Option<&dyn Rng> {
        self.random.as_ref().map(|source| source as &dyn Rng)
    }
    fn unreadable(&self) {
        if let Some(notice) = &self.unreadable {
            notice();
        }
    }
}
