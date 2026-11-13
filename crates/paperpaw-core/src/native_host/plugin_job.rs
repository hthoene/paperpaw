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
    /// The user declined, or dismissed it.
    pub(crate) const DECLINED: u8 = 2;
    pub(crate) const DISMISSED: u8 = 3;
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
    /// The same or an older version, not allowed.
    pub(crate) const OLDER_NOT_ALLOWED: u8 = 14;
    /// Built for an older interface.
    pub(crate) const OLDER_BUILD: u8 = 11;
    /// Another chip or profile.
    pub(crate) const OTHER_CHIP: u8 = 12;
    pub(crate) const NO_ROOM: u8 = 13;
    /// The id is taken: built in, or the reserved `core`.
    pub(crate) const ID_TAKEN: u8 = 15;
    /// A card error.
    pub(crate) const CARD: u8 = 16;
    /// The content changed since it was checked or asked about.
    pub(crate) const CHANGED: u8 = 18;
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
    /// The chip, interface and entry length the loader reads; `None` on a
    /// device that loads no plugin code (data plugins only: a code file is
    /// reason 19, as code of another kind of device is).
    pub(crate) loader: Option<header5::Host>,
    /// What it offers a plugin's metadata record (services and features).
    pub(crate) offered: metadata::Host<'static>,
    /// The signing keys the firmware itself knows by name (PaperPaw's own);
    /// every other key is shown by its fingerprint.
    pub(crate) signers: &'static [(&'static str, [u8; 32])],
}

impl PluginHost {
    /// The host of a device that loads no plugin code: data plugins only.
    pub(crate) const DATA_ONLY: Self = Self {
        loader: None,
        // Never read: a check ends at the code file, before its metadata.
        offered: metadata::Host {
            abi_major: 0,
            minor: 0,
            profile: metadata::Profile {
                target: "",
                entry_layout: 0,
                entry_bytes: 0,
            },
            services: &[],
            features: &[],
        },
        signers: &[],
    };
}

/// What a step may use.
pub(crate) struct JobEnv<'a> {
    pub(crate) storage: &'a dyn Storage,
    pub(crate) host: &'a PluginHost,
    /// The plugins built into the running image: an id among them is taken.
    pub(crate) built_ins: &'a dyn BuiltIns,
    /// The asking plugin holds the trusted-installer grant
    /// (`core.grant.install.<plugin>`): it skips the prompt, never a sideload's
    /// or one that brings a new `uses.*` feature.
    pub(crate) granted: bool,
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
#[allow(
    clippy::large_enum_variant,
    reason = "returned by value once per step and kept in place; a box would be an allocation that can fail at the job's end"
)]
pub(crate) enum JobStep {
    /// Not done: call again.
    More,
    /// Waiting for the user's answer ([`PluginJob::answer`]); stepping it
    /// again does nothing.
    Wait,
    /// The host asks the user; the job waits for [`PluginJob::answer`].
    Ask(PluginQuestion),
    /// The host closes the instances of this plugin (a removal's first step
    /// after its journal); the job waits for [`PluginJob::unloaded`].
    Unload(Text<ID_MAX>),
    /// The host forgets the plugin's host-held state (its secrets and its
    /// grants), a removal with its data, after its folders moved; the job waits
    /// for [`PluginJob::forgotten`]. Safe to repeat.
    Forget(Text<ID_MAX>),
    /// Done: `reason` as in `device.md` 5.5, and for a check its facts (kept
    /// in the plugin's result slot).
    Done(Finished),
}

/// A job's end.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finished {
    pub(crate) reason: u8,
    pub(crate) facts: Option<PluginFacts>,
    /// `install_done.flags`: bit 0 data kept (a removal), bit 1 the change
    /// reaches code at the next start (a code plugin installed or removed).
    pub(crate) flags: u32,
    /// The plugin list changed (event 70): a data plugin to load now, or one
    /// gone.
    pub(crate) changed: Option<Text<ID_MAX>>,
    /// `dev_check.needs_draft` of a check that ended with reason 10: the
    /// interface revision the file needs (the header's `needs_minor`), 0 for
    /// every other end and where the file names no number (`device.md` 5.1).
    pub(crate) needs_draft: u32,
}

impl Finished {
    pub(crate) const fn refused(reason: u8) -> Self {
        Self {
            reason,
            facts: None,
            flags: 0,
            changed: None,
            needs_draft: 0,
        }
    }

    /// A check that ends with reason 10 and says which revision it needs.
    pub(crate) const fn needs_newer(needs_draft: u32) -> Self {
        Self {
            needs_draft,
            ..Self::refused(reason::NEEDS_NEWER)
        }
    }

    /// A finished check: its verdict and facts.
    pub(crate) const fn checked(reason: u8, facts: Option<PluginFacts>) -> Self {
        Self {
            reason,
            facts,
            flags: 0,
            changed: None,
            needs_draft: 0,
        }
    }
}

/// `install_done.flags` bit 0: the data of a removed plugin was kept.
pub(crate) const DONE_DATA_KEPT: u32 = 1;
/// `install_done.flags` bit 1: the code changes at the next start.
pub(crate) const DONE_AT_NEXT_START: u32 = 2;

/// The words of a host confirmation, as plain data (`device.md` 5.0): Core
/// renders them in the user's language. Fixed-size, no heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PluginQuestion {
    /// The plugin that asked and its request.
    pub(crate) owner: u64,
    pub(crate) request: u32,
    pub(crate) kind: u32,
    pub(crate) id: Text<ID_MAX>,
    pub(crate) name: Text<NAME_MAX>,
    pub(crate) version: Text<VERSION_MAX>,
    /// The signing key's name; empty where the device does not know it, and
    /// then the host shows the fingerprint of `key`.
    pub(crate) signer: Text<SIGNER_MAX>,
    pub(crate) key: Option<[u8; 32]>,
    /// Not from a catalog: no evidence came with it.
    pub(crate) sideload: bool,
    /// What an update is compared with.
    pub(crate) relation: Relation,
    /// What the plugin declares (`uses.*`), as the store page shows it.
    pub(crate) uses: Uses,
}

/// How the new version stands to the installed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relation {
    /// A first install, or a newer version.
    Newer,
    /// The same version, allowed.
    Same,
    /// An older version, allowed.
    Older,
}

/// At most this many `uses.*` keys are shown; more is said by [`Uses::more`].
pub(crate) const USES_SHOWN: usize = 16;
pub(crate) const USE_KEY_MAX: usize = 24;

/// The features a plugin declares, as shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Uses {
    pub(crate) keys: [Text<USE_KEY_MAX>; USES_SHOWN],
    pub(crate) count: u8,
    /// More were declared than shown.
    pub(crate) more: bool,
}

/// The user's answer to a confirmation (the firmware's own).
pub(crate) use super::firmware_install::Answer;

/// The host's trusted-installer grants (`device.md` 5.2).
pub(crate) trait InstallGrants {
    /// Whether the plugin `owner` may install without being asked each time.
    fn granted(&self, owner: u64) -> bool;
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
    /// The user's answer to the question the job asked.
    fn answer(&mut self, answer: Answer);
    /// The host closed the instances the job asked for ([`JobStep::Unload`]).
    fn unloaded(&mut self) {}
    /// The host forgot the plugin's state ([`JobStep::Forget`]); `ok`: all of it.
    fn forgotten(&mut self, ok: bool) {
        let _ = ok;
    }
    /// Stops the job with `reason` (20 for a suspend or a stop). Infallible
    /// and allocation-free; the next `step` ends with it.
    fn abort(&mut self, reason: u8);
    /// Whether the job may not be abandoned where it stands (an install past
    /// the point where the card holds half of a change: its journal is
    /// written). The host then steps it on to its end, bounded, after
    /// `abort`, and the job ends with its own reason; any other job just
    /// ends with the reason `abort` was given. Default: it may be abandoned.
    fn must_finish(&self) -> bool {
        false
    }
    /// Whether the job runs on when the plugin that asked stops (an install
    /// the user confirmed): nobody is told its end. Default: it stops with
    /// its sender.
    fn keep_without_sender(&self) -> bool {
        false
    }
    /// Tells the job, before each step, whether an installed image waits for
    /// its restart (a dual-boot change then ends with reason 21). Default:
    /// nothing to know.
    fn image_waiting(&mut self, waiting: bool) {
        let _ = waiting;
    }
    /// What the host does once the job ended (a dual-boot change). Default:
    /// nothing.
    fn after(&self) -> super::dual_boot_job::After {
        super::dual_boot_job::After::Nothing
    }
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
