//! What the kernel needs of a device to check, install and restart into a
//! firmware image (`docs/abi/device.md` 5.3 and 5.4).
//!
//! The kernel owns every request, the order of jobs, the user's
//! confirmation and what the plugins see of it; the shell owns the flash,
//! the trial of a new image and what a good start is. Between them is this
//! trait: **one bounded step per call**, made from the main loop and never
//! inside a plugin's callback, each with a buffer that is taken fallibly
//! when the step's job begins (a refusal for no memory, nothing touched).
//!
//! An image is not copied: it stays at its path on the card and is bound by
//! its digest. Pass 1, [`FirmwareHost::check_step`], reads it in pieces,
//! checks what the device can (chip, board, size, version, project, the
//! signature or, for another firmware's image, the checks the boot loader
//! would make), hashes it and reads the plugins it has built in. The kernel
//! asks the user. Pass 2, [`FirmwareHost::write_step`], writes it block by
//! block into the slot not running, hashing it again; a digest other than
//! the one confirmed ends with [`Reason::CHANGED`]. Pass 3,
//! [`FirmwareHost::readback_step`], reads the slot back and hashes it.
//! [`FirmwareHost::commit`] then writes the image's first word and chooses
//! the slot in one entry.
//!
//! Only one job is open at a time: a second `begin_*` before the open one
//! ended or was abandoned is a failure ([`Reason::OTHER`]); the kernel's
//! queue never does it.
//!
//! **A job's life.** It is closed by a check's `Done` or `Failed`, by a write
//! stage's `Failed`, and by `commit` whether it returns `Ok` or `Err`: the
//! kernel then does not call [`FirmwareHost::abandon`], and a `begin_*` after
//! a `Failed` works. `abandon` is for an open job (a shutdown, a card swap,
//! a confirmation that ends while a check is under way) and does nothing
//! where none is open. [`FirmwareHost::running`] keeps reporting the firmware
//! that runs, the old one, after `commit` until the restart. A step may return
//! `Progress` with `done == total`; only `Done` and `Failed` end a sequence
//! of steps. The built-ins a check reports are plugin ids
//! (`paperpaw_update::PluginCrate::plugin_id`), the key op 4 lists them by.

use crate::Storage;

/// Longest version text a check reports.
pub const VERSION_MAX: usize = 32;
/// Longest project name a check reports.
pub const PROJECT_MAX: usize = 64;
/// Longest name of a signing key a check reports.
pub const KEY_NAME_MAX: usize = 32;
/// Longest id of a built-in plugin a check reports, as the image names it.
pub const BUILT_IN_MAX: usize = 64;

/// The most a step may be asked to do before it returns: one flash erase
/// block of an image's write (`write_step`), a few card reads of a check or
/// a read-back.
pub const WRITE_BLOCK: u32 = 64 * 1024;

/// Text of at most `N` bytes, in place (no allocation: a check's answer
/// must not fail for lack of memory).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Text<const N: usize> {
    bytes: [u8; N],
    len: u8,
}

impl<const N: usize> Text<N> {
    /// `text` cut at a character boundary to `N` bytes (at most 255).
    pub fn new(text: &str) -> Self {
        let mut end = text.len().min(N).min(usize::from(u8::MAX));
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut bytes = [0; N];
        bytes[..end].copy_from_slice(&text.as_bytes()[..end]);
        Self {
            bytes,
            len: end as u8,
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        // Cut at a character boundary in `new`, so this is whole.
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

impl<const N: usize> core::fmt::Debug for Text<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_str(), f)
    }
}

/// The firmware running now, as the shell knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Running<'a> {
    pub version: &'a str,
    pub project: &'a str,
    /// The chip, as an app image's header names it (`CHIP_ESP32_C3` 5,
    /// `CHIP_ESP32_S3` 9).
    pub chip: u16,
    /// Bytes of the slot an image goes to; 0 where nothing is installed here
    /// (the device is shared with another firmware).
    pub room: u64,
    /// An update did not start cleanly and this firmware came back.
    pub undone: bool,
}

impl<'a> Running<'a> {
    pub const fn new(
        version: &'a str,
        project: &'a str,
        chip: u16,
        room: u64,
        undone: bool,
    ) -> Self {
        Self {
            version,
            project,
            chip,
            room,
            undone,
        }
    }
}

/// Why a step ended without doing its work: a reason of `device.md` 5.5 by
/// its number. The kernel maps it to its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reason(pub u8);

impl Reason {
    /// Another cause (and a `begin_*` while a job is open).
    pub const OTHER: Self = Self(1);
    /// Damaged, or not a file this firmware reads (a signature that does
    /// not hold).
    pub const DAMAGED: Self = Self(4);
    /// Unsigned.
    pub const UNSIGNED: Self = Self(8);
    /// Signed with a key not PaperPaw's that the user did not accept.
    pub const UNKNOWN_KEY: Self = Self(9);
    /// Made for another chip.
    pub const OTHER_CHIP: Self = Self(12);
    /// No room in the slot.
    pub const NO_ROOM: Self = Self(13);
    /// A card error.
    pub const CARD: Self = Self(16);
    /// The content changed since it was checked or asked about.
    pub const CHANGED: Self = Self(18);
    /// Not on this device (nothing is installed here, or it cannot restart).
    pub const NOT_HERE: Self = Self(19);
    /// The host stopped it.
    pub const STOPPED: Self = Self(20);
}

/// What one step of a job came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<T> {
    /// Not done: call again. `done` of `total` bytes (of the image).
    Progress { done: u32, total: u32 },
    /// Done.
    Done(T),
    /// Ended; nothing is changed that a later start would trust (an image
    /// partly written is not bootable).
    Failed(Reason),
}

/// Who signed an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Signer {
    /// A key PaperPaw's firmware has built in.
    PaperPaw {
        /// What the key is called.
        name: Text<KEY_NAME_MAX>,
        key: [u8; 32],
    },
    /// Another key (e.g. a user's own builds): it installs only where the
    /// user accepted this very key.
    Other { key: [u8; 32] },
    /// No signature (another firmware's image).
    Unsigned,
}

/// What a finished check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckFacts {
    /// The image is for this chip, and (another firmware's) for this board.
    pub chip_ok: bool,
    /// The image's bytes (a signed update's, without its trailer).
    pub size: u32,
    /// The version and project its maker gave it.
    pub version: Text<VERSION_MAX>,
    pub project: Text<PROJECT_MAX>,
    pub signer: Signer,
    /// Another firmware than PaperPaw (found from the image, not asked):
    /// unsigned, checked as the boot loader would start it.
    pub foreign: bool,
    /// SHA-256 of the image's bytes: what the user's yes is bound to.
    pub sha256: [u8; 32],
}

impl CheckFacts {
    pub const fn new(
        chip_ok: bool,
        size: u32,
        version: Text<VERSION_MAX>,
        project: Text<PROJECT_MAX>,
        signer: Signer,
        foreign: bool,
        sha256: [u8; 32],
    ) -> Self {
        Self {
            chip_ok,
            size,
            version,
            project,
            signer,
            foreign,
            sha256,
        }
    }
}

/// A device's firmware: checks, writes and restarts into an image.
pub trait FirmwareHost {
    /// The running firmware.
    fn running(&self) -> Running<'_>;

    /// Begins pass 1 on the image at `path` on the card. The check finds out
    /// for itself whether the image is another firmware's (its project is
    /// not PaperPaw's: its checks are the boot loader's, there is no
    /// signature) and says so in [`CheckFacts::foreign`]. The step's buffer
    /// is taken here: no room is
    /// `Err(Reason::OTHER)` and nothing is touched. Where the device shares
    /// its flash with another firmware a check still works.
    fn begin_check(&mut self, path: &str) -> Result<(), Reason>;

    /// One bounded step of the check. For a signed update it reads the
    /// plugins the image has built in first and calls `built_in` with each
    /// plugin id once (at most [`BUILT_IN_MAX`] bytes, in the image's order; none
    /// for another firmware's image, whose list PaperPaw cannot read; the
    /// shell keeps no list), then hashes and verifies the whole image.
    fn check_step(
        &mut self,
        storage: &dyn Storage,
        built_in: &mut dyn FnMut(&str),
    ) -> Step<CheckFacts>;

    /// Begins pass 2 for the image at `path`, bound to `digest` (the one
    /// confirmed). `key` is the signing key the user accepted, `None` for
    /// PaperPaw's keys only; `foreign` uses the boot loader's checks and no
    /// signature, and the image is final at the switch: another firmware
    /// never runs PaperPaw's trial, so nothing brings the old one back
    /// by itself (the confirmation says so). Nothing is installed where the device is shared with
    /// another firmware: `Err(Reason::NOT_HERE)`. An install waiting for its
    /// restart is given up before another is written.
    fn begin_write(
        &mut self,
        path: &str,
        digest: [u8; 32],
        key: Option<[u8; 32]>,
        foreign: bool,
    ) -> Result<(), Reason>;

    /// One step of the write: at most [`WRITE_BLOCK`] bytes (erase, then
    /// write), hashing and, at the end, comparing with the confirmed digest
    /// and checking the signature or the boot loader's checks. The first
    /// word of the image is not written yet.
    fn write_step(&mut self, storage: &dyn Storage) -> Step<()>;

    /// One step of pass 3: reads the written slot back and hashes it against
    /// the digest ([`Reason::CHANGED`] where it differs).
    fn readback_step(&mut self) -> Step<()>;

    /// Writes the image's first word and chooses the slot in one entry. A
    /// signed image then starts on trial; another firmware's is final at the
    /// switch (it never runs PaperPaw's trial).
    fn commit(&mut self) -> Result<(), Reason>;

    /// Ends the open job, a check or a write, as it is (a written image
    /// that was not committed stays unbootable).
    fn abandon(&mut self);

    /// Starts the image that was chosen. Returns only where it cannot
    /// (`Err(Reason::NOT_HERE)`).
    fn restart(&mut self) -> Result<(), Reason>;

    /// Whether the device has a second app partition to share with another
    /// firmware, so that `docs/dual-boot.md` applies (`command.firmware_dual_boot`
    /// is `Reason::NOT_HERE` where it does not). Default: no.
    fn dual_boot_capable(&self) -> bool {
        false
    }

    /// Whether the setup was declared dual boot at this start (the marker in
    /// the bootloader's window, or `dual-boot = on` in `paperpaw.ini`);
    /// `dev_firmware.flags` bit 0. Only read where [`Self::dual_boot_capable`].
    /// Default: no.
    fn dual_boot_declared(&self) -> bool {
        false
    }

    /// Restarts the running image, where no new one waits (the end of
    /// `command.firmware_dual_boot` with mode 1). Returns only where it cannot
    /// (`Err(Reason::NOT_HERE)`). Default: it cannot. A host that says
    /// [`Self::dual_boot_capable`] must implement it: else a yes writes the
    /// file and the device never restarts.
    fn restart_running(&mut self) -> Result<(), Reason> {
        Err(Reason::NOT_HERE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cut_at_a_character_boundary() {
        assert_eq!(Text::<4>::new("abcdef").as_str(), "abcd");
        assert_eq!(Text::<4>::new("abc").as_str(), "abc");
        // "ä" is two bytes: cut in the middle, it is left out whole.
        assert_eq!(Text::<4>::new("abcä").as_str(), "abc");
        assert_eq!(Text::<4>::new("").as_str(), "");
    }
}
