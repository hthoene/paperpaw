//! Firmware updates, on devices whose firmware PaperPaw is (the X4):
//! [`Env::firmware`](crate::Env) is `Some` there. Elsewhere PaperPaw is
//! an app, updated by the system's store.
//!
//! The shell does what it is told ([`Command::Firmware`](crate::Command)
//! with a [`FirmwareCommand`]: install an update file, restart) and
//! reports how it goes in [`Firmware::state`], with
//! [`events::FIRMWARE_CHANGED`](crate::builtin::events::FIRMWARE_CHANGED).
//! Finding updates and asking the reader is a plugin's (the `update`
//! plugin, which another can replace).
//!
//! Update files are signed (the format is `paperpaw-update`'s, its parts
//! are here too). The shell installs those signed with one of
//! [`Firmware::keys`] (PaperPaw's); one signed with another key (the
//! reader's own builds) only with that key in the command, which the
//! reader accepted for this install, shown its fingerprint; and one not
//! newer than the running firmware only when the reader chose it
//! (`Update::may_install`). Nothing on the card makes a key trusted or
//! an older firmware welcome, so a card changed by someone else cannot
//! install firmware unasked. The shell writes the new firmware beside the
//! running one and starts it at the next start; if the new one does not
//! start cleanly, the old one comes back ([`Firmware::undone`]). Books,
//! settings and plugins on the card stay as they are.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_platform::{Storage, SystemStatus};
pub use paperpaw_update::{
    APP_HEAD_LEN, AppInfo, CHIP_ESP32_C3, CHIP_ESP32_S3, Check, PLUGINS_AT, PLUGINS_HEAD_LEN,
    PluginCrate, Plugins, Problem, TRAILER_LEN, Trailer, fingerprint, key_hex, parse_key,
};

#[cfg(any(test, feature = "kernel"))]
mod foreign;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use foreign::{ForeignCheck, ForeignImage};

/// The folder on the card where update files are looked for.
pub const UPDATE_DIR: &str = "updates";

/// The battery's charge in percent below which firmware is not installed
/// unless the device is charging: a battery run flat half way would leave
/// it to the firmware before (it comes back), but the reader without one.
pub const INSTALL_BATTERY: u8 = 20;

/// Whether the battery lets firmware be installed now: charged to
/// [`INSTALL_BATTERY`] or more, charging (on USB), or not measured (no
/// battery reading: the emulator, Android). The kernel refuses an install
/// otherwise ([`InstallFailure::LowBattery`]).
pub fn battery_allows_install(status: &SystemStatus) -> bool {
    status
        .battery
        .is_none_or(|b| b.charging || b.percent >= INSTALL_BATTERY)
}

/// What the shell is told to do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FirmwareCommand {
    /// Installs the update file at this storage path: checks it
    /// (`Update::may_install`), writes it beside the running firmware
    /// and makes it the one to start next. Asked for by
    /// `FirmwareCommand::install` (the kernel's and its update plugin's).
    #[non_exhaustive]
    Install {
        path: String,
        /// The key of an update not signed by PaperPaw, which the reader
        /// accepted for this install, shown its fingerprint; without it
        /// such an update is [`InstallFailure::Untrusted`].
        key: Option<[u8; 32]>,
        /// The reader chose an update not newer than the running firmware
        /// (an older one, or the same again); without it such an update
        /// is [`InstallFailure::NotNewer`].
        older: bool,
    },
    /// Restarts the device, e.g. into the new firmware once
    /// [`FirmwareState::Installed`]. What plugins keep is saved first.
    Restart,
    /// Installs another firmware's app image at this storage path (the
    /// way back to CrossPoint, say), which is not signed: checked as the
    /// bootloader would start it (`ForeignImage`, `ForeignCheck`),
    /// written beside the running firmware and made the one to start
    /// next. Sent only for a file the reader chose as such, told that it
    /// is not PaperPaw. Asked for by `FirmwareCommand::install_foreign`.
    #[non_exhaustive]
    InstallForeign { path: String },
}

impl FirmwareCommand {
    /// Installs the update file at `path`; `key` the key of an update not
    /// signed by PaperPaw that the reader accepted, `older` whether they
    /// chose one not newer than the running firmware.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn install(path: impl Into<String>, key: Option<[u8; 32]>, older: bool) -> Self {
        FirmwareCommand::Install {
            path: path.into(),
            key,
            older,
        }
    }

    /// Installs another firmware's image at `path`, which the reader chose
    /// as such.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn install_foreign(path: impl Into<String>) -> Self {
        FirmwareCommand::InstallForeign { path: path.into() }
    }
}

/// Where an install is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FirmwareState {
    #[default]
    Idle,
    /// Writing: `done` of `total` bytes.
    Installing { done: u64, total: u64 },
    /// Written and checked: the new firmware starts at the next start.
    Installed,
    /// Nothing was changed: the running firmware stays.
    Failed(InstallFailure),
}

/// Why an update cannot be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallFailure {
    /// The file could not be read.
    Unreadable,
    /// It is no update file (e.g. a whole flash image for the web
    /// flasher).
    NotAnUpdate,
    /// It is made for a newer PaperPaw than this one can install.
    NewerFormat,
    /// It is firmware for another device.
    OtherDevice,
    /// It is larger than the room for firmware.
    TooLarge,
    /// It is signed with a key that is not PaperPaw's, and the reader did
    /// not accept it.
    Untrusted,
    /// It is not newer than the running firmware, and the reader did not
    /// choose it.
    NotNewer,
    /// It was changed or damaged: its signature does not hold.
    Damaged,
    /// Writing it failed.
    Write,
    /// The battery is below [`INSTALL_BATTERY`] percent and the device is
    /// not charging: charged, or plugged in, it may be installed.
    LowBattery,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl From<Problem> for InstallFailure {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::NotAnUpdate | Problem::NotAnImage => InstallFailure::NotAnUpdate,
            Problem::NewerFormat => InstallFailure::NewerFormat,
            _ => InstallFailure::Damaged,
        }
    }
}

/// Firmware as the kernel knows it ([`Env::firmware`](crate::Env)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Firmware {
    /// The running firmware's version, e.g. `0.4.0`.
    pub version: String,
    /// Which firmware it is, e.g. `paperpaw-xteink`: updates must be the
    /// same.
    pub project: String,
    /// The chip it runs on, as update images name it.
    pub chip: u16,
    /// The keys built in, PaperPaw's (Ed25519 public keys).
    pub keys: Vec<[u8; 32]>,
    /// The room for firmware, in bytes.
    pub room: u64,
    /// An update installed before did not start cleanly, and the firmware
    /// before it came back at this start.
    pub undone: bool,
    pub state: FirmwareState,
    /// The code plugins built in, as the image lists them (empty if it
    /// does not).
    pub plugins: Plugins,
}

impl Firmware {
    /// The firmware `project` of `version` on `chip`; nothing else known.
    pub fn new(version: impl Into<String>, project: impl Into<String>, chip: u16) -> Self {
        Self {
            version: version.into(),
            project: project.into(),
            chip,
            ..Self::default()
        }
    }

    /// With PaperPaw's keys built in.
    pub fn keys(mut self, keys: Vec<[u8; 32]>) -> Self {
        self.keys = keys;
        self
    }

    /// With `room` bytes for an update.
    pub fn room(mut self, room: u64) -> Self {
        self.room = room;
        self
    }

    /// An update before did not start cleanly, if `undone`.
    pub fn undone(mut self, undone: bool) -> Self {
        self.undone = undone;
        self
    }

    pub fn state(mut self, state: FirmwareState) -> Self {
        self.state = state;
        self
    }

    /// With the code plugins built in.
    pub fn plugins(mut self, plugins: Plugins) -> Self {
        self.plugins = plugins;
        self
    }
}

/// Who signed an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Signer {
    /// PaperPaw ([`Firmware::keys`]).
    PaperPaw,
    /// Another key, e.g. the reader's own for their builds: installed only
    /// when the reader accepts it, shown its fingerprint, at each install.
    Unknown,
}

/// What an update file holds, from its start and its end (its signature
/// is checked as it is installed).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[non_exhaustive]
pub struct Update {
    pub path: String,
    pub info: AppInfo,
    pub trailer: Trailer,
    /// The code plugins it has built in; `None` for an image without the
    /// list (older ones).
    pub plugins: Option<Plugins>,
}

#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
impl Update {
    /// Reads the update file at `path`.
    pub fn read(storage: &dyn Storage, path: &str) -> Result<Self, InstallFailure> {
        let size = storage.size(path).map_err(|_| InstallFailure::Unreadable)?;
        let mut tail = [0; TRAILER_LEN];
        let start = size
            .checked_sub(TRAILER_LEN as u64)
            .ok_or(InstallFailure::NotAnUpdate)?;
        read_exact(storage, path, start, &mut tail)?;
        let trailer = Trailer::read(&tail, size)?;
        let mut head = [0; APP_HEAD_LEN];
        read_exact(storage, path, 0, &mut head)?;
        let info = AppInfo::read(&head)?;
        Ok(Self {
            path: String::from(path),
            info,
            trailer,
            plugins: read_plugins(storage, path),
        })
    }

    /// The plugin crates the running firmware has that this update leaves
    /// out (none if either does not list them).
    pub fn leaves_out<'a>(&'a self, firmware: &'a Firmware) -> Vec<&'a str> {
        match &self.plugins {
            Some(plugins) => firmware.plugins.missing_from(plugins).collect(),
            None => Vec::new(),
        }
    }

    /// Whether it is for this device and has room.
    pub fn fits(&self, firmware: &Firmware) -> Result<(), InstallFailure> {
        if self.info.chip != firmware.chip || self.info.project != firmware.project {
            return Err(InstallFailure::OtherDevice);
        }
        if u64::from(self.trailer.image_len) > firmware.room {
            return Err(InstallFailure::TooLarge);
        }
        Ok(())
    }

    /// Who signed it, as far as this device knows.
    pub fn signer(&self, firmware: &Firmware) -> Signer {
        if firmware.keys.contains(&self.trailer.key) {
            Signer::PaperPaw
        } else {
            Signer::Unknown
        }
    }

    /// Whether it is newer than the running firmware.
    pub fn is_newer(&self, firmware: &Firmware) -> bool {
        compare_versions(&self.info.version, &firmware.version).is_gt()
    }

    /// Whether it may be installed as [`FirmwareCommand::Install`] asks,
    /// with the `key` the reader accepted and whether they chose an
    /// `older` one: it must be for this device and have room, be signed
    /// by PaperPaw or with `key`, and be newer unless `older`. Its
    /// signature is checked as it is written.
    pub fn may_install(
        &self,
        firmware: &Firmware,
        key: Option<&[u8; 32]>,
        older: bool,
    ) -> Result<(), InstallFailure> {
        self.fits(firmware)?;
        if self.signer(firmware) == Signer::Unknown && key != Some(&self.trailer.key) {
            return Err(InstallFailure::Untrusted);
        }
        if !older && !self.is_newer(firmware) {
            return Err(InstallFailure::NotNewer);
        }
        Ok(())
    }
}

/// The list of plugins in the image of the update file at `path`.
fn read_plugins(storage: &dyn Storage, path: &str) -> Option<Plugins> {
    let mut head = [0; PLUGINS_HEAD_LEN];
    read_exact(storage, path, PLUGINS_AT as u64, &mut head).ok()?;
    let len = Plugins::text_len(&head)?;
    let mut bytes = alloc::vec![0; PLUGINS_HEAD_LEN + len];
    read_exact(storage, path, PLUGINS_AT as u64, &mut bytes).ok()?;
    Plugins::read(&bytes)
}

fn read_exact(
    storage: &dyn Storage,
    path: &str,
    offset: u64,
    buf: &mut [u8],
) -> Result<(), InstallFailure> {
    match storage.read_at(path, offset, buf) {
        Ok(n) if n == buf.len() => Ok(()),
        _ => Err(InstallFailure::Unreadable),
    }
}

/// The release and the number of a beta (`0.5.0-beta.2` is `0.5.0` and
/// `2`), for showing it as one; none for any other version, a release or
/// another pre-release (`0.5.0-dev`).
pub fn beta(version: &str) -> Option<(&str, &str)> {
    let version = version.split('+').next()?;
    let (release, pre) = version.split_once('-')?;
    let number = pre.strip_prefix("beta.")?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some((release, number))
}

/// Compares versions such as `0.4.0` and `0.10.1` number by number; a
/// pre-release such as `0.4.0-dev` comes before its release, and
/// pre-releases are ordered as SemVer orders them (`rc.9` before
/// `rc.10`, `beta.2` before `beta.10`).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    // Versions a `Version` holds are ordered as it orders them; for those
    // it does not (a firmware's tag with another pre-release), by text.
    if let (Some(a), Some(b)) = (crate::Version::parse(a), crate::Version::parse(b)) {
        return a.cmp(&b);
    }
    /// The numbers, and the pre-release if there is one; build metadata
    /// left out.
    fn parts(v: &str) -> (Vec<u64>, Option<&str>) {
        let v = v.split('+').next().unwrap_or_default();
        let (numbers, pre) = v.split_once('-').map_or((v, None), |(n, p)| (n, Some(p)));
        let numbers = numbers
            .split('.')
            .map_while(|part| part.parse().ok())
            .collect();
        (numbers, pre)
    }
    let ((a_numbers, a_pre), (b_numbers, b_pre)) = (parts(a), parts(b));
    a_numbers
        .cmp(&b_numbers)
        .then_with(|| match (a_pre, b_pre) {
            (None, None) => Ordering::Equal,
            // No pre-release is the release itself, after its pre-releases.
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => compare_pre_releases(a, b),
        })
}

/// SemVer's order of pre-releases: part by part (between dots), numbers
/// as numbers and before words, words as text; where all parts of one
/// are those the other begins with, the shorter first.
fn compare_pre_releases(a: &str, b: &str) -> Ordering {
    let is_number = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let (mut a, mut b) = (a.split('.'), b.split('.'));
    loop {
        let order = match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match (is_number(x), is_number(y)) {
                // Of any length: the one with more digits is larger.
                (true, true) => {
                    let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                    x.len().cmp(&y.len()).then_with(|| x.cmp(y))
                }
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => x.cmp(y),
            },
        };
        if order != Ordering::Equal {
            return order;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_storage::MemoryStorage;

    const SEED: [u8; 32] = [3; 32];

    fn firmware() -> Firmware {
        Firmware {
            version: "0.3.0".into(),
            project: "paperpaw-xteink".into(),
            chip: paperpaw_update::CHIP_ESP32_C3,
            keys: alloc::vec![paperpaw_update::public_key(&[1; 32])],
            room: 100_000,
            ..Firmware::default()
        }
    }

    fn file(project: &[u8], len: usize, seed: &[u8; 32]) -> Vec<u8> {
        file_for(CHIP_ESP32_C3, project, len, seed)
    }

    fn file_for(chip: u16, project: &[u8], len: usize, seed: &[u8; 32]) -> Vec<u8> {
        let mut image = alloc::vec![0u8; len];
        image[0] = 0xE9;
        image[12..14].copy_from_slice(&chip.to_le_bytes());
        image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
        image[48..53].copy_from_slice(b"0.4.0");
        image[80..80 + project.len()].copy_from_slice(project);
        let trailer = paperpaw_update::sign(&image, seed).unwrap();
        image.extend_from_slice(&trailer.to_bytes());
        image
    }

    #[test]
    fn an_update_says_what_it_is_and_who_signed_it() {
        let storage = MemoryStorage::new()
            .with("updates/a.bin", file(b"paperpaw-xteink", 4000, &SEED))
            .with("updates/b.bin", file(b"other-firmware", 4000, &SEED))
            .with("updates/c.bin", file(b"paperpaw-xteink", 200_000, &SEED))
            .with("updates/d.bin", alloc::vec![0xE9; 5000]);
        let fw = firmware();
        let a = Update::read(&storage, "updates/a.bin").unwrap();
        assert_eq!(a.info.version, "0.4.0");
        assert_eq!(a.fits(&fw), Ok(()));
        assert_eq!(a.signer(&fw), Signer::Unknown);
        let b = Update::read(&storage, "updates/b.bin").unwrap();
        assert_eq!(b.fits(&fw), Err(InstallFailure::OtherDevice));
        let c = Update::read(&storage, "updates/c.bin").unwrap();
        assert_eq!(c.fits(&fw), Err(InstallFailure::TooLarge));
        assert_eq!(
            Update::read(&storage, "updates/d.bin"),
            Err(InstallFailure::NotAnUpdate)
        );
        assert_eq!(
            Update::read(&storage, "updates/none.bin"),
            Err(InstallFailure::Unreadable)
        );
    }

    #[test]
    fn only_what_the_reader_accepted_is_installed() {
        let paperpaw = [1; 32];
        let storage = MemoryStorage::new()
            .with("updates/own.bin", file(b"paperpaw-xteink", 4000, &SEED))
            .with(
                "updates/paperpaw.bin",
                file(b"paperpaw-xteink", 4000, &paperpaw),
            );
        let fw = firmware();
        let own = Update::read(&storage, "updates/own.bin").unwrap();
        let release = Update::read(&storage, "updates/paperpaw.bin").unwrap();
        // PaperPaw's, newer: at once.
        assert_eq!(release.signer(&fw), Signer::PaperPaw);
        assert_eq!(release.may_install(&fw, None, false), Ok(()));
        // Another key: only the one the reader accepted.
        let untrusted = Err(InstallFailure::Untrusted);
        assert_eq!(own.may_install(&fw, None, false), untrusted);
        assert_eq!(own.may_install(&fw, Some(&[7; 32]), false), untrusted);
        assert_eq!(own.may_install(&fw, Some(&own.trailer.key), false), Ok(()));
        // The same version again, or an older one: only when chosen.
        for running in ["0.4.0", "0.5.0"] {
            let fw = Firmware {
                version: running.into(),
                ..firmware()
            };
            assert!(!release.is_newer(&fw));
            let not_newer = Err(InstallFailure::NotNewer);
            assert_eq!(release.may_install(&fw, None, false), not_newer);
            assert_eq!(release.may_install(&fw, None, true), Ok(()));
        }
    }

    #[test]
    fn an_update_for_the_other_chip_is_refused() {
        let storage = MemoryStorage::new()
            .with(
                "updates/c3.bin",
                file_for(CHIP_ESP32_C3, b"paperpaw-xteink", 4000, &SEED),
            )
            .with(
                "updates/s3.bin",
                file_for(CHIP_ESP32_S3, b"paperpaw-xteink", 4000, &SEED),
            );
        let c3 = Update::read(&storage, "updates/c3.bin").unwrap();
        let s3 = Update::read(&storage, "updates/s3.bin").unwrap();
        assert_eq!(s3.info.chip, CHIP_ESP32_S3);
        // The X4 (ESP32-C3) and an ESP32-S3 device of the same firmware.
        let x4 = firmware();
        let pro = Firmware {
            chip: CHIP_ESP32_S3,
            ..firmware()
        };
        assert_eq!(c3.fits(&x4), Ok(()));
        assert_eq!(s3.fits(&x4), Err(InstallFailure::OtherDevice));
        assert_eq!(s3.fits(&pro), Ok(()));
        assert_eq!(c3.fits(&pro), Err(InstallFailure::OtherDevice));
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(compare_versions("0.10.0", "0.9.1"), Ordering::Greater);
        assert_eq!(compare_versions("0.4.0", "0.4.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.4.0-dev", "0.4.1"), Ordering::Less);
        assert_eq!(compare_versions("0.4.0-dev", "0.4.0"), Ordering::Less);
        assert_eq!(compare_versions("0.4.0-dev", "0.3.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.4.0+x4", "0.4.0"), Ordering::Equal);
        assert_eq!(
            compare_versions("0.5.0-alpha", "0.5.0-alpha.1"),
            Ordering::Less
        );
        assert_eq!(compare_versions("0.5.0-2", "0.5.0-alpha"), Ordering::Less);
        assert_eq!(
            compare_versions("0.5.0-rc.007", "0.5.0-rc.7"),
            Ordering::Equal
        );
    }

    /// The betas before a release: each after the one before, by number,
    /// and the release after all of them; never one back.
    #[test]
    fn betas_come_in_order_and_before_their_release() {
        let order = [
            "0.4.0",
            "0.5.0-beta.1",
            "0.5.0-beta.2",
            "0.5.0-beta.10",
            "0.5.0",
            "0.5.1-beta.1",
        ];
        for (i, a) in order.iter().enumerate() {
            for (j, b) in order.iter().enumerate() {
                assert_eq!(compare_versions(a, b), i.cmp(&j), "{a} and {b}");
            }
        }
        // As SemVer orders words: a build in work named `-dev` comes after
        // the betas of its release, before the release.
        assert_eq!(
            compare_versions("0.5.0-dev", "0.5.0-beta.10"),
            Ordering::Greater
        );
        assert_eq!(compare_versions("0.5.0-dev", "0.5.0"), Ordering::Less);
        assert_eq!(beta("0.5.0-beta.10"), Some(("0.5.0", "10")));
        assert_eq!(beta("0.5.0-beta.2+x4"), Some(("0.5.0", "2")));
        for none in [
            "0.5.0",
            "0.5.0-dev",
            "0.5.0-beta",
            "0.5.0-beta.x",
            "0.5.0-rc.1",
        ] {
            assert_eq!(beta(none), None, "{none}");
        }
    }
}
