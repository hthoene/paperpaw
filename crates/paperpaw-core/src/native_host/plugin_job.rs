//! What a plugin job (check, install, removal: `docs/abi/device.md` 5.1,
//! 5.2) is to the job queue, and what the device tells it.
//!
//! The queue (`firmware_driver`) keeps the request table, the one-job-at-a-time
//! rule and the outcomes; a plugin job is a boxed [`PluginJob`] it starts when
//! the machine is idle and steps from the main loop, never inside a callback.
//!
//! * `step` is bounded (a few KiB of card reads, one hash block run or one
//!   signature verification) and never panics on card content. A step that
//!   cannot get memory ends the job with reason 1, it does not abort.
//! * `abort` is infallible and allocates nothing; a job that was aborted ends
//!   with the reason it was given on its next `step`, and `Drop` releases
//!   everything it holds (the I/O buffer, an evidence slot).
use alloc::vec::Vec;
use paperpaw_native::{header5, metadata};
use paperpaw_platform::{Storage, firmware::Text};
use paperpaw_sdk::Version;

use super::firmware_install::BuiltIns;

/// `install_done.kind` of a plugin install, a removal and a check.
pub(crate) const KIND_INSTALL: u32 = 0;
pub(crate) const KIND_REMOVAL: u32 = 2;
pub(crate) const KIND_CHECK: u32 = 3;

/// The reasons of `device.md` 5.5 a plugin job ends with.
pub(crate) mod reason {
    pub(crate) const OK: u8 = 0;
    /// Another cause, and no memory for the job's buffer.
    pub(crate) const OTHER: u8 = 1;
    /// Damaged, or not a plugin this firmware reads.
    pub(crate) const DAMAGED: u8 = 4;
    pub(crate) const NOT_THE_AUTHOR: u8 = 5;
    pub(crate) const KEY_CHANGED: u8 = 6;
    pub(crate) const REVOKED: u8 = 7;
    pub(crate) const UNSIGNED: u8 = 8;
    /// Needs a newer interface, or a service or feature this one lacks.
    pub(crate) const NEEDS_NEWER: u8 = 10;
    /// Built for an older interface.
    pub(crate) const OLDER_BUILD: u8 = 11;
    /// Another chip or profile.
    pub(crate) const OTHER_CHIP: u8 = 12;
    pub(crate) const NO_ROOM: u8 = 13;
    /// The id is taken: built in, or the reserved `core`.
    pub(crate) const ID_TAKEN: u8 = 15;
    /// A card error.
    pub(crate) const CARD: u8 = 16;
    /// Not on this device: the code is of another kind (a computer's).
    pub(crate) const NOT_HERE: u8 = 19;
    /// The host stopped it (shutdown, card swap, suspend).
    pub(crate) const STOPPED: u8 = 20;
}

/// Longest id, name and signer name a check tells (the wire's limits).
pub(crate) const ID_MAX: usize = 64;
pub(crate) const NAME_MAX: usize = 64;
pub(crate) const VERSION_MAX: usize = 32;
pub(crate) const SIGNER_MAX: usize = 32;

/// `dev_check.flags` bits of a plugin check (`device.md` 5.1).
pub(crate) const FLAG_DATA_PLUGIN: u32 = 1;
/// This id's author key is pinned here and is the signing key.
pub(crate) const FLAG_PINNED_HERE: u32 = 1 << 1;
/// This id is pinned to another key: an install needs a rotation.
pub(crate) const FLAG_PINNED_OTHER: u32 = 1 << 2;

/// What the device tells a plugin job about itself: what its loader reads
/// and which signing keys it knows by name. The shell gives it once, with the
/// kernel's wiring.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginHost {
    /// The chip, interface and entry length the loader reads.
    pub(crate) loader: header5::Host,
    /// What it offers a plugin's metadata record (services and features).
    pub(crate) offered: metadata::Host<'static>,
    /// The signing keys the firmware itself knows by name (PaperPaw's own);
    /// every other key is shown by its fingerprint.
    pub(crate) signers: &'static [(&'static str, [u8; 32])],
}

/// What a step may use.
pub(crate) struct JobEnv<'a> {
    pub(crate) storage: &'a dyn Storage,
    pub(crate) host: &'a PluginHost,
    /// The plugins built into the running image: an id among them is taken.
    pub(crate) built_ins: &'a dyn BuiltIns,
}

/// The facts of a finished plugin check (`dev_check`), in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PluginFacts {
    pub(crate) id: Text<ID_MAX>,
    pub(crate) version: Text<VERSION_MAX>,
    /// The sum of the sizes of the files an install would copy.
    pub(crate) bytes: u64,
    /// The signing key's name, empty where the device does not know the key.
    pub(crate) signer: Text<SIGNER_MAX>,
    /// The signing key and the code's digest, `None` for a data plugin.
    pub(crate) key: Option<[u8; 32]>,
    pub(crate) sha256: Option<[u8; 32]>,
    /// `dev_check.flags` bits 0 to 2.
    pub(crate) flags: u32,
}

/// What one step came to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JobStep {
    /// Not done: call again.
    More,
    /// Done: `reason` as in `device.md` 5.5, and for a check its facts (kept
    /// in the plugin's result slot).
    Done(Finished),
}

/// A job's end.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finished {
    pub(crate) reason: u8,
    pub(crate) facts: Option<PluginFacts>,
}

impl Finished {
    pub(crate) const fn refused(reason: u8) -> Self {
        Self {
            reason,
            facts: None,
        }
    }
}

/// One plugin job in the queue.
pub(crate) trait PluginJob {
    /// The plugin that asked.
    fn owner(&self) -> u64;
    /// Its request number.
    fn request(&self) -> u32;
    /// The `install_done` kind: [`KIND_INSTALL`], [`KIND_REMOVAL`] or
    /// [`KIND_CHECK`].
    fn kind(&self) -> u32;
    /// One bounded step.
    fn step(&mut self, env: &JobEnv<'_>) -> JobStep;
    /// Stops the job with `reason` (20 for a suspend or a stop). Infallible
    /// and allocation-free; the next `step` ends with it.
    fn abort(&mut self, reason: u8);
}

/// The buffer a job reads through: 4 KiB, taken fallibly when the job's
/// first step runs (`device.md` 6.2).
pub(crate) const IO_BYTES: usize = 4096;

/// A zeroed buffer of [`IO_BYTES`], or `None` where the heap cannot take it.
pub(crate) fn io_buffer() -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(IO_BYTES).ok()?;
    buf.resize(IO_BYTES, 0);
    Some(buf)
}

/// A plugin's version, shown as the manifest wrote it.
pub(crate) fn version_text(text: &str) -> Option<(Version, Text<VERSION_MAX>)> {
    let version = Version::try_parse(text.trim()).ok()?;
    Some((version, Text::new(text.trim())))
}
