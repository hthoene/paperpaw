//! [`FirmwareHost`] for the firmware's updates: the check, the write, the
//! read-back and the commit as the kernel drives them, one bounded step at a
//! time (`docs/abi/device.md` 5.3 and 5.4). The image stays on the card; the
//! flash, the OTA data and the trial are this module's parent's.
//!
//! - **Check:** a PaperPaw update's trailer and head, then the plugins it has
//!   built in (streamed to the caller, a line at a time: the shell keeps no
//!   list), then every byte of the image hashed (SHA-256) and verified (the
//!   signature, or another firmware's image the checks the boot loader would
//!   make). Four pieces of 4 KiB a step.
//! - **Write:** the same image, bound to the digest the user confirmed,
//!   block by block into the partition not running, hashed again while it is
//!   written. A digest other than the confirmed one is reason 18 and the
//!   partition keeps no first word, so no boot loader would start it.
//! - **Read-back:** the slot hashed against the digest.
//! - **Commit:** the first word, then the slot chosen in one entry: on
//!   trial for a PaperPaw image, valid for another firmware's, which never
//!   runs PaperPaw's trial.

use alloc::boxed::Box;
use alloc::string::String;

use paperpaw_platform::Storage;
use paperpaw_platform::firmware::{
    CheckFacts, FirmwareHost, KEY_NAME_MAX, PROJECT_MAX, Reason, Running, Signer, Step, Text,
    VERSION_MAX, WRITE_BLOCK,
};
use paperpaw_sdk::digest::sha256::Sha256;
use paperpaw_sdk::firmware::{
    APP_HEAD_LEN, AppInfo, Check, ForeignCheck, ForeignImage, InstallFailure, PLUGINS_AT,
    PLUGINS_HEAD_LEN, PluginCrate, Plugins, Problem, TRAILER_LEN, Trailer,
};

use super::{PIECE, Partition, Updates, owned, zeroed};
use crate::ota::{Flash, Image, OtaData, State};

/// Pieces a check or a read-back handles in one step.
const STEP_PIECES: usize = 4;
/// The longest line of an image's plugin list that is read (a crate's name
/// and version); a longer one is skipped.
const LINE: usize = 128;
/// What a PaperPaw key is called in a check's answer.
const KEY_NAME: &str = "PaperPaw";

/// The job open, one at a time. Its state is held on the heap only while it
/// runs (a job is about a kilobyte, which `Updates` would otherwise carry on
/// every stack it is passed over): what a check or write needs and cannot
/// have is a refusal, never a crash.
pub(super) enum Job {
    Check(Held<CheckJob>),
    Write(Held<WriteJob>),
}

/// One value on the heap, got fallibly.
pub(super) struct Held<T>(Box<[T; 1]>);

impl<T> Held<T> {
    /// `value` moved to the heap; no room is `Reason::OTHER`.
    fn new(value: T) -> Result<Self, Reason> {
        let mut one = alloc::vec::Vec::new();
        one.try_reserve_exact(1).map_err(|_| Reason::OTHER)?;
        one.push(value);
        one.into_boxed_slice()
            .try_into()
            .map(Self)
            .map_err(|_| Reason::OTHER)
    }
}

impl<T> core::ops::Deref for Held<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0[0]
    }
}

impl<T> core::ops::DerefMut for Held<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0[0]
    }
}

/// The firmware's own answer to why a step could not go on.
fn reason_of(failure: InstallFailure) -> Reason {
    match failure {
        InstallFailure::Unreadable => Reason::CARD,
        InstallFailure::NotAnUpdate | InstallFailure::Damaged => Reason::DAMAGED,
        InstallFailure::NewerFormat => Reason(10),
        InstallFailure::OtherDevice => Reason::OTHER_CHIP,
        InstallFailure::TooLarge => Reason::NO_ROOM,
        InstallFailure::Untrusted => Reason::UNKNOWN_KEY,
        InstallFailure::NotNewer => Reason(14),
        InstallFailure::Other(code) if code == crate::dual_boot::REFUSED => Reason::NOT_HERE,
        _ => Reason::OTHER,
    }
}

/// How the image is verified as it is read.
#[expect(
    clippy::large_enum_variant,
    reason = "one at a time; the signature's state stays where it was"
)]
enum Verify {
    Signed(Check),
    Foreign(ForeignCheck),
}

impl Verify {
    fn absorb(&mut self, piece: &[u8]) {
        match self {
            Verify::Signed(check) => check.absorb(piece),
            Verify::Foreign(check) => check.absorb(piece),
        }
    }

    fn finish(self) -> Result<(), Reason> {
        match self {
            Verify::Signed(check) => check.finish().map_err(|_| Reason::DAMAGED),
            Verify::Foreign(check) => check.finish().map_err(reason_of),
        }
    }
}

/// What the head of an image says, for the check's answer and the write's
/// gates.
struct Meta {
    chip_ok: bool,
    /// The image's bytes, those that are hashed.
    size: u32,
    version: Text<VERSION_MAX>,
    project: Text<PROJECT_MAX>,
    signer: Signer,
    foreign: bool,
}

/// A check going on.
pub(super) struct CheckJob {
    path: String,
    /// The one I/O buffer (`PIECE` bytes), taken in `begin_check`.
    buf: Box<[u8]>,
    phase: Phase,
}

enum Phase {
    /// Nothing read yet.
    Head,
    /// The plugins the image has built in, being read.
    List(Listing),
    /// The image hashed and verified.
    Hash(Hashing),
}

/// An image's plugin list being streamed: its text at `at..end`.
struct Listing {
    meta: Meta,
    verify: Verify,
    at: u64,
    end: u64,
    line: [u8; LINE],
    len: usize,
    /// The current line is longer than [`LINE`]: skipped to its end.
    skipping: bool,
}

struct Hashing {
    meta: Meta,
    verify: Verify,
    sha: Sha256,
    at: u64,
    len: u64,
}

/// A write going on: its stages.
pub(super) struct WriteJob {
    path: String,
    digest: [u8; 32],
    key: Option<[u8; 32]>,
    foreign: bool,
    buf: Box<[u8]>,
    /// Where in the flash the image goes, and the partition.
    offset: u32,
    slot: u32,
    stage: Stage,
}

#[expect(
    clippy::large_enum_variant,
    reason = "one at a time, held in place so that nothing is allocated for it"
)]
enum Stage {
    /// The image's head not read yet (the first step does: the card is
    /// not asked in `begin_write`).
    Start,
    Writing {
        image: Image,
        verify: Verify,
        sha: Sha256,
    },
    Readback {
        image: Image,
        sha: Sha256,
        at: u32,
    },
    Written {
        image: Image,
    },
}

impl<F: Flash> Updates<F> {
    /// Sets what `restart` does: the firmware's own restart, which does not
    /// return on a device.
    pub fn set_restarter(&mut self, restarter: fn() -> Result<(), Reason>) {
        self.restarter = Some(restarter);
    }

    /// Whether the OTA data chooses an image that is not the running one (or
    /// cannot be read: then it counts as waiting).
    fn image_waits(&mut self) -> bool {
        let running = self.running;
        match OtaData::read(&mut self.data_partition()) {
            Ok(data) => data.chosen().is_some_and(|chosen| chosen != running),
            Err(_) => true,
        }
    }

    /// Whether another job may begin.
    fn free_for_a_job(&self) -> bool {
        self.job.is_none() && self.install.is_none()
    }

    /// What the image at `path` says about itself and how to verify it.
    /// `foreign` is what the write was confirmed for; `None` finds out from
    /// the image (a check): an app image whose project is not PaperPaw's is
    /// another firmware's. Anything else is looked at as PaperPaw's, so a
    /// damaged update is said to be damaged.
    fn open_image(
        &self,
        storage: &dyn Storage,
        path: &str,
        foreign: Option<bool>,
    ) -> Result<(Meta, Verify, u64), Reason> {
        let this = &self.this;
        let foreign = match foreign {
            Some(foreign) => foreign,
            None => {
                let mut head = [0; APP_HEAD_LEN];
                read_exact(storage, path, 0, &mut head).is_ok()
                    && AppInfo::read(&head).is_ok_and(|info| !info.project.starts_with("paperpaw-"))
            }
        };
        if foreign {
            let image = ForeignImage::read(storage, path).map_err(reason_of)?;
            let chip_ok = !matches!(image.fits(this), Err(InstallFailure::OtherDevice));
            let size = u32::try_from(image.len).map_err(|_| Reason::NO_ROOM)?;
            let meta = Meta {
                chip_ok,
                size,
                version: Text::new(&image.info.version),
                project: Text::new(&image.info.project),
                signer: Signer::Unsigned,
                foreign: true,
            };
            let verify = Verify::Foreign(ForeignCheck::new(this.chip, self.board, image.len));
            return Ok((meta, verify, image.len));
        }
        let size = storage.size(path).map_err(|_| Reason::CARD)?;
        let tail_at = size
            .checked_sub(TRAILER_LEN as u64)
            .ok_or(Reason::DAMAGED)?;
        let mut tail = [0; TRAILER_LEN];
        read_exact(storage, path, tail_at, &mut tail)?;
        let mut head = [0; APP_HEAD_LEN];
        read_exact(storage, path, 0, &mut head)?;
        let trailer = match Trailer::read(&tail, size) {
            Ok(trailer) => trailer,
            // An app image without PaperPaw's trailer: unsigned, not damaged.
            Err(Problem::NotAnUpdate) if AppInfo::read(&head).is_ok() => {
                return Err(Reason::UNSIGNED);
            }
            Err(Problem::NewerFormat) => return Err(Reason(10)),
            Err(_) => return Err(Reason::DAMAGED),
        };
        let info = AppInfo::read(&head).map_err(|_| Reason::DAMAGED)?;
        let signer = if this.keys.contains(&trailer.key) {
            Signer::PaperPaw {
                name: Text::<KEY_NAME_MAX>::new(KEY_NAME),
                key: trailer.key,
            }
        } else {
            Signer::Other { key: trailer.key }
        };
        let meta = Meta {
            chip_ok: info.chip == this.chip && info.project == this.project,
            size: trailer.image_len,
            version: Text::new(&info.version),
            project: Text::new(&info.project),
            signer,
            foreign: false,
        };
        let verify = Verify::Signed(Check::new(&trailer).map_err(|_| Reason::DAMAGED)?);
        Ok((meta, verify, u64::from(trailer.image_len)))
    }

    /// One step of a check.
    fn advance_check(
        &mut self,
        storage: &dyn Storage,
        job: &mut CheckJob,
        built_in: &mut dyn FnMut(&str),
    ) -> Result<Option<CheckFacts>, Reason> {
        let phase = core::mem::replace(&mut job.phase, Phase::Head);
        match phase {
            Phase::Head => {
                let (meta, verify, len) = self.open_image(storage, &job.path, None)?;
                // The list, where the image has one (an older one does not).
                let mut head = [0; PLUGINS_HEAD_LEN];
                let listed = !meta.foreign
                    && read_exact(storage, &job.path, PLUGINS_AT as u64, &mut head).is_ok();
                let text_len = if listed {
                    Plugins::text_len(&head).map(|n| n as u64)
                } else {
                    None
                };
                let list_at = (PLUGINS_AT + PLUGINS_HEAD_LEN) as u64;
                job.phase = match text_len {
                    Some(n) if list_at + n <= len => Phase::List(Listing {
                        meta,
                        verify,
                        at: list_at,
                        end: list_at + n,
                        line: [0; LINE],
                        len: 0,
                        skipping: false,
                    }),
                    _ => Phase::Hash(Hashing {
                        meta,
                        verify,
                        sha: Sha256::new(),
                        at: 0,
                        len,
                    }),
                };
                Ok(None)
            }
            Phase::List(mut list) => {
                let n = (list.end - list.at).min(PIECE as u64) as usize;
                if n > 0 {
                    let piece = &mut job.buf[..n];
                    read_exact(storage, &job.path, list.at, piece)?;
                    for &byte in piece.iter() {
                        list.byte(byte, built_in);
                    }
                    list.at += n as u64;
                }
                if list.at < list.end {
                    job.phase = Phase::List(list);
                    return Ok(None);
                }
                list.flush(built_in);
                let Listing { meta, verify, .. } = list;
                job.phase = Phase::Hash(Hashing {
                    len: u64::from(meta.size),
                    meta,
                    verify,
                    sha: Sha256::new(),
                    at: 0,
                });
                Ok(None)
            }
            Phase::Hash(mut hashing) => {
                for _ in 0..STEP_PIECES {
                    let n = (hashing.len - hashing.at).min(PIECE as u64) as usize;
                    if n == 0 {
                        break;
                    }
                    let piece = &mut job.buf[..n];
                    read_exact(storage, &job.path, hashing.at, piece)?;
                    hashing.sha.update(piece);
                    hashing.verify.absorb(piece);
                    hashing.at += n as u64;
                }
                if hashing.at < hashing.len {
                    job.phase = Phase::Hash(hashing);
                    return Ok(None);
                }
                let Hashing {
                    meta, verify, sha, ..
                } = hashing;
                verify.finish()?;
                Ok(Some(CheckFacts::new(
                    meta.chip_ok,
                    meta.size,
                    meta.version,
                    meta.project,
                    meta.signer,
                    meta.foreign,
                    sha.finish(),
                )))
            }
        }
    }

    /// One step of a write (and its end, the check against the digest).
    fn advance_write(
        &mut self,
        storage: &dyn Storage,
        job: &mut WriteJob,
    ) -> Result<(u32, u32, bool), Reason> {
        let stage = core::mem::replace(&mut job.stage, Stage::Start);
        match stage {
            Stage::Start => {
                let (meta, verify, len) = self.open_image(storage, &job.path, Some(job.foreign))?;
                if !meta.chip_ok {
                    return Err(Reason::OTHER_CHIP);
                }
                if len > u64::from(self.apps[job.slot as usize].1) {
                    return Err(Reason::NO_ROOM);
                }
                if let Signer::Other { key } = meta.signer
                    && job.key != Some(key)
                {
                    return Err(Reason::UNKNOWN_KEY);
                }
                job.stage = Stage::Writing {
                    image: Image::new(meta.size),
                    verify,
                    sha: Sha256::new(),
                };
                Ok((0, meta.size, false))
            }
            Stage::Writing {
                mut image,
                mut verify,
                mut sha,
            } => {
                let end = image.done().saturating_add(WRITE_BLOCK).min(image.total());
                let partition = &mut Partition {
                    flash: &mut self.flash,
                    offset: job.offset,
                };
                while image.done() < end {
                    let len = (end - image.done()).min(PIECE as u32) as usize;
                    let piece = &mut job.buf[..len];
                    read_exact(storage, &job.path, u64::from(image.done()), piece)?;
                    sha.update(piece);
                    verify.absorb(piece);
                    image
                        .write(partition, &mut job.buf[..], len)
                        .map_err(|_| Reason::OTHER)?;
                }
                let (done, total) = (image.done(), image.total());
                if done < total {
                    job.stage = Stage::Writing { image, verify, sha };
                    return Ok((done, total, false));
                }
                // All read: the content is the one confirmed, and verifies.
                if sha.finish() != job.digest {
                    return Err(Reason::CHANGED);
                }
                verify.finish()?;
                job.stage = Stage::Readback {
                    image,
                    sha: Sha256::new(),
                    at: 0,
                };
                Ok((total, total, true))
            }
            other => {
                job.stage = other;
                Err(Reason::OTHER)
            }
        }
    }

    /// One step of the read-back.
    fn advance_readback(&mut self, job: &mut WriteJob) -> Result<(u32, u32, bool), Reason> {
        let Stage::Readback {
            image,
            mut sha,
            mut at,
        } = core::mem::replace(&mut job.stage, Stage::Start)
        else {
            return Err(Reason::OTHER);
        };
        let total = image.total();
        let partition = &mut Partition {
            flash: &mut self.flash,
            offset: job.offset,
        };
        for _ in 0..STEP_PIECES {
            let n = (total - at).min(PIECE as u32) as usize;
            if n == 0 {
                break;
            }
            let piece = &mut job.buf[..n];
            partition.read(at, piece).map_err(|_| Reason::OTHER)?;
            if at == 0 {
                // The first word is kept back until the commit.
                let head = image.head();
                let k = head.len().min(n);
                piece[..k].copy_from_slice(&head[..k]);
            }
            sha.update(piece);
            at += n as u32;
        }
        if at < total {
            job.stage = Stage::Readback { image, sha, at };
            return Ok((at, total, false));
        }
        if sha.finish() != job.digest {
            return Err(Reason::CHANGED);
        }
        job.stage = Stage::Written { image };
        Ok((total, total, true))
    }
}

impl Listing {
    /// The next byte of the list's text: a line is `name version`.
    fn byte(&mut self, byte: u8, built_in: &mut dyn FnMut(&str)) {
        if byte == b'\n' {
            self.flush(built_in);
        } else if self.skipping {
        } else if self.len < LINE {
            self.line[self.len] = byte;
            self.len += 1;
        } else {
            self.skipping = true;
        }
    }

    /// The line so far is whole: the plugin id of its crate goes to the
    /// caller (a crate that is not `paperpaw-plugin-<id>` is skipped).
    fn flush(&mut self, built_in: &mut dyn FnMut(&str)) {
        if !self.skipping
            && let Ok(line) = core::str::from_utf8(&self.line[..self.len])
            && let Some((name, version)) = line.split_once(' ')
            && let Some(id) = (PluginCrate { name, version }).plugin_id()
        {
            built_in(id);
        }
        self.len = 0;
        self.skipping = false;
    }
}

fn read_exact(storage: &dyn Storage, path: &str, at: u64, buf: &mut [u8]) -> Result<(), Reason> {
    match storage.read_at(path, at, buf) {
        Ok(n) if n == buf.len() => Ok(()),
        _ => Err(Reason::CARD),
    }
}

impl<F: Flash> FirmwareHost for Updates<F> {
    fn running(&self) -> Running<'_> {
        let target = (self.running + 1) % crate::ota::SLOTS;
        let room = if self.dual.is_some() {
            0
        } else {
            u64::from(self.apps[target as usize].1)
        };
        Running::new(
            &self.this.version,
            &self.this.project,
            self.this.chip,
            room,
            self.undone,
        )
    }

    fn begin_check(&mut self, path: &str) -> Result<(), Reason> {
        if !self.free_for_a_job() {
            return Err(Reason::OTHER);
        }
        let buf = zeroed(PIECE).map_err(|_| Reason::OTHER)?;
        let path = owned(path).map_err(|_| Reason::OTHER)?;
        let job = Held::new(CheckJob {
            path,
            buf,
            phase: Phase::Head,
        })?;
        self.job = Some(Job::Check(job));
        Ok(())
    }

    fn check_step(
        &mut self,
        storage: &dyn Storage,
        built_in: &mut dyn FnMut(&str),
    ) -> Step<CheckFacts> {
        let Some(Job::Check(mut job)) = self.job.take() else {
            return Step::Failed(Reason::OTHER);
        };
        match self.advance_check(storage, &mut job, built_in) {
            Err(reason) => Step::Failed(reason),
            Ok(Some(facts)) => Step::Done(facts),
            Ok(None) => {
                let (done, total) = match &job.phase {
                    Phase::Head => (0, 0),
                    Phase::List(list) => (0, list.meta.size),
                    Phase::Hash(hashing) => (
                        u32::try_from(hashing.at).unwrap_or(u32::MAX),
                        u32::try_from(hashing.len).unwrap_or(u32::MAX),
                    ),
                };
                self.job = Some(Job::Check(job));
                Step::Progress { done, total }
            }
        }
    }

    fn begin_write(
        &mut self,
        path: &str,
        digest: [u8; 32],
        key: Option<[u8; 32]>,
        foreign: bool,
    ) -> Result<(), Reason> {
        if self.dual.is_some() {
            return Err(Reason::NOT_HERE);
        }
        if !self.free_for_a_job() {
            return Err(Reason::OTHER);
        }
        // The buffers first: a refusal for no room leaves the flash and the
        // OTA data as they were.
        let buf = zeroed(PIECE).map_err(|_| Reason::OTHER)?;
        let path = owned(path).map_err(|_| Reason::OTHER)?;
        // An update chosen and not started is given up before this writes
        // over its partition.
        let slot = self.prepare_slot().map_err(|_| Reason::OTHER)?;
        let offset = self.apps[slot as usize].0;
        let job = Held::new(WriteJob {
            path,
            digest,
            key,
            foreign,
            buf,
            offset,
            slot,
            stage: Stage::Start,
        })?;
        self.job = Some(Job::Write(job));
        Ok(())
    }

    fn write_step(&mut self, storage: &dyn Storage) -> Step<()> {
        let Some(Job::Write(mut job)) = self.job.take() else {
            return Step::Failed(Reason::OTHER);
        };
        match self.advance_write(storage, &mut job) {
            Err(reason) => Step::Failed(reason),
            Ok((done, total, finished)) => {
                self.job = Some(Job::Write(job));
                if finished {
                    Step::Done(())
                } else {
                    Step::Progress { done, total }
                }
            }
        }
    }

    fn readback_step(&mut self) -> Step<()> {
        let Some(Job::Write(mut job)) = self.job.take() else {
            return Step::Failed(Reason::OTHER);
        };
        match self.advance_readback(&mut job) {
            Err(reason) => Step::Failed(reason),
            Ok((done, total, finished)) => {
                self.job = Some(Job::Write(job));
                if finished {
                    Step::Done(())
                } else {
                    Step::Progress { done, total }
                }
            }
        }
    }

    fn commit(&mut self) -> Result<(), Reason> {
        // Ok or Err, the job is over: a commit that fails leaves the slot
        // without its first word and nothing chosen.
        let Some(Job::Write(mut held)) = self.job.take() else {
            return Err(Reason::OTHER);
        };
        let job = &mut *held;
        let Stage::Written { image } = &job.stage else {
            return Err(Reason::OTHER);
        };
        // State and choice in one entry, written whole: on trial for a
        // PaperPaw image, valid for another firmware's (it never runs the
        // trial).
        let state = if job.foreign {
            State::Valid
        } else {
            State::New
        };
        self.make_whole_and_choose(image, job.offset, job.slot, state, &mut job.buf[..])
            .map_err(|_| Reason::OTHER)
    }

    fn abandon(&mut self) {
        self.job = None;
    }

    fn restart(&mut self) -> Result<(), Reason> {
        match self.restarter {
            Some(restart) => restart(),
            None => Err(Reason::NOT_HERE),
        }
    }

    /// The X4 always has two app partitions (`ota::SLOTS`), the one another
    /// firmware can share (`docs/dual-boot.md`).
    fn dual_boot_capable(&self) -> bool {
        true
    }

    /// The setup as last settled (`Updates::settle`, which an install's
    /// `declare` may change again): the marker in the bootloader or
    /// `paperpaw.ini`. The driver asks once, at its build, after the settle.
    fn dual_boot_declared(&self) -> bool {
        self.dual.is_some()
    }

    /// Restarts the image running, and only that: `restart` starts whatever
    /// the OTA data chooses, so it is refused (the restart of a waiting image
    /// is the reader's own to confirm) while a job or an install is open, while
    /// the OTA data chooses another partition than the running one, and where
    /// it cannot be read.
    fn restart_running(&mut self) -> Result<(), Reason> {
        if !self.free_for_a_job() || self.installing() || self.image_waits() {
            return Err(Reason::STOPPED);
        }
        self.restart()
    }
}
