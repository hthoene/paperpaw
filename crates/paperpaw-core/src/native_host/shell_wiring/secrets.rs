//! Shell-owned objects behind the crate-private secrets service.
use super::*;
use crate::native_host::{
    secrets_file::{Sealer, Veiling},
    secrets_service::{Rng, SecretsHost, SecretsServices, check_owner},
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
    needs_radio: Option<Rc<dyn Fn()>>,
}
impl core::fmt::Debug for SecretsInputs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SecretsInputs")
            .field("custom_sealer", &self.sealer.is_some())
            .field("random", &self.random.is_some())
            .field("unreadable", &self.unreadable.is_some())
            .field("needs_radio", &self.needs_radio.is_some())
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
            needs_radio: None,
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
            needs_radio: None,
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
impl SecretsInputs {
    /// Notify the shell when a save was refused because the sealer could not
    /// draw a strong nonce now ([`ShellSealer::seal`] or [`ShellRandom::fill`]
    /// answered `ERR_STATE`; the X4 with its radio off). The call fails with
    /// `ERR_STATE` and the file is unchanged. The shell shows the host text
    /// `core.host-secrets.needs-wifi` ("Turn on Wi-Fi to save this."). Called at
    /// every refused `keep`, `remove` or uninstall, never for
    /// [`SecretsMaintenance::migrate`]; the shell rate-limits its toast.
    #[must_use]
    pub fn needs_radio(mut self, notice: Rc<dyn Fn()>) -> Self {
        self.needs_radio = Some(notice);
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
    fn needs_radio(&self) {
        if let Some(notice) = &self.needs_radio {
            notice();
        }
    }
}

/// Where a startup migration step goes on; start with [`Self::default`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MigrationCursor(u32);

/// How far a [`SecretsMaintenance::migrate`] step got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Migration {
    /// No veiled value is left to seal from the cursor on.
    Done,
    /// A bounded batch was sealed; call again with this cursor.
    More(MigrationCursor),
}

/// What the user chose for the data folder of a plugin being removed
/// (decision 124).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataChoice {
    /// "Remove and keep data": its secrets stay, found again on reinstall.
    Keep,
    /// "Remove and delete data": its secrets go with the folder.
    Delete,
}

/// The shell's maintenance of the secrets file, from [`WiredHosts::secrets`].
/// It shares the wired service's inputs and holds no key or plaintext.
#[derive(Clone)]
pub struct SecretsMaintenance(SecretsServices);

impl core::fmt::Debug for SecretsMaintenance {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecretsMaintenance")
    }
}

impl SecretsMaintenance {
    pub(super) fn new(service: SecretsServices) -> Self {
        Self(service)
    }

    /// Startup migration from interface 6 (`docs/abi/secrets.md` 3.1): seals
    /// veiled values, a bounded batch per call (at most eight values; one pass
    /// over a file of at most 64 KiB), and replaces the file by rename, so a
    /// power cut leaves the old file, still readable, or the new one. Call it
    /// once after [`HostResources::wire`], one step per turn, with the cursor
    /// from the last step until it answers [`Migration::Done`]. A device that
    /// only veils is `Done` at once. A veiled value that does not open is
    /// passed over, and a step that finds none writes nothing.
    ///
    /// Any save also seals every veiled value it finds, so the migration is
    /// an eager start, not a precondition.
    ///
    /// # Errors
    /// `ERR_IO` (card away; nothing changed, the unreadable notice follows once),
    /// `ERR_STATE` (the sealer has no strong nonce now, the X4 with its radio
    /// off; nothing changed, try again later; no notice is raised),
    /// `ERR_NO_MEMORY`.
    pub fn migrate(&self, cursor: MigrationCursor) -> Result<Migration, i32> {
        let step = self.0.migrate(cursor.0)?;
        Ok(step.next.map_or(Migration::Done, |next| {
            Migration::More(MigrationCursor(next))
        }))
    }

    /// A plugin was removed by the user's choice (decision 124). With
    /// [`DataChoice::Delete`] every secret of `plugin` is removed from the file,
    /// also those that do not open here, in one pass and replaced by rename;
    /// call it after the data folder was moved away. With [`DataChoice::Keep`]
    /// the secrets stay and nothing is read or written. `plugin` is the validated
    /// installed id, never a name a plugin gave.
    ///
    /// # Errors
    /// `ERR_INVALID` (not a usable id), `ERR_IO` (nothing changed; the secrets
    /// remain: retry), `ERR_STATE` (a veiled value was due for sealing and no
    /// strong nonce is available; nothing changed; the `needs_radio` notice
    /// follows), `ERR_NO_MEMORY`.
    pub fn plugin_removed(&self, plugin: &str, data: DataChoice) -> Result<(), i32> {
        match data {
            DataChoice::Keep => check_owner(plugin),
            DataChoice::Delete => self.0.forget_plugin(plugin),
        }
    }
}
