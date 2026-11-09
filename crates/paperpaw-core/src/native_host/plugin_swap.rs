//! The journalled swap of a plugin install (`docs/abi/device.md` 5.4) and its
//! replay at start.
//!
//! The stage `plugins/.<id>.part` is complete, checked and confirmed. The swap:
//!
//! 1. writes the journal `plugins/.journal` (temp file, sync, rename): the id,
//!    whether an old folder is there, the pins and rotations to take, a
//!    checksum;
//! 2. moves the old folder `plugins/<id>` to `plugins/.old-<id>`;
//! 3. moves the stage to `plugins/<id>`;
//! 4. takes the pins (`plugin_rules::Pins`: author and recovery at a first
//!    install, the rotations, the code named as installed) and settles the
//!    marks the stage held (`install::marks`);
//! 5. removes `.old-<id>`, then the journal.
//!
//! Every move is `Storage::rename`, which on the card is the journalled
//! `move_entry` (`library.md` 6): a cut leaves the entry in one place or the
//! other, never two entries on one chain.
//!
//! **One state machine for the live swap and for the replay.** Each step looks
//! at the card (stage, old, target) and does the next thing, so a cut at any
//! point is finished by the same code at the next start: from the journal on, a
//! swap goes *forward* (the user confirmed this stage; the pins were checked
//! before the journal was written). Pins are taken idempotently: a rotation
//! whose key is already the pinned one is skipped. A failure of the live swap
//! before the stage is in place rolls back (the old folder is restored, the
//! stage and the journal removed); a failure after it keeps the journal, so
//! the next start takes the pins and removes the old folder. Code that is not
//! yet named as installed does not load, so a plugin is never half-trusted.
//!
//! A journal that cannot be read (a damaged record, an id no plugin has) is
//! renamed to `.journal.bad` and counted, never trusted.
use alloc::string::String;
use paperpaw_platform::{Storage, StorageError, firmware::Text};
use paperpaw_sdk::{
    digest::fnv1a64,
    install::{self, marks},
    native::{self, keys},
};

use super::plugin_job::{ID_MAX, reason};
use super::plugin_rules::{Pins, ROTATIONS_MAX, Rotation};

/// The journal, its temporary file and where a bad one is set aside.
pub(crate) const JOURNAL: &str = "plugins/.journal";
const TMP: &str = "plugins/.journal.tmp";
pub(crate) const BAD: &str = "plugins/.journal.bad";
const MAGIC: [u8; 4] = *b"PPIJ";
const VERSION: u8 = 1;
/// The longest record: the fixed part, the id, the keys, the rotations, the checksum.
const RECORD_MAX: usize = 8 + ID_MAX + 32 * 3 + ROTATIONS_MAX * 96 + 8;

const FLAG_HAD_OLD: u8 = 1;
const FLAG_FIRST: u8 = 1 << 1;
const FLAG_FIRST_RECOVERY: u8 = 1 << 2;
const FLAG_CODE: u8 = 1 << 3;

/// What the swap is: one plugin id, whether an old folder is replaced, and
/// the pins to take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: Text<ID_MAX>,
    pub(crate) had_old: bool,
    pub(crate) pins: Pins,
}

/// `plugins/<id>`, `plugins/.<id>.part` and `plugins/.old-<id>` of a valid id.
pub(crate) fn target_path(id: &str) -> Option<String> {
    named(id, "", "")
}
pub(crate) fn stage_path(id: &str) -> Option<String> {
    named(id, ".", ".part")
}
pub(crate) fn old_path(id: &str) -> Option<String> {
    named(id, ".old-", "")
}

fn named(id: &str, before: &str, after: &str) -> Option<String> {
    if !install::is_plugin_name(id) || id == "core" {
        return None;
    }
    let dir = paperpaw_sdk::builtin::PLUGIN_DIR;
    let mut path = String::new();
    path.try_reserve_exact(dir.len() + 1 + before.len() + id.len() + after.len())
        .ok()?;
    path.push_str(dir);
    path.push('/');
    path.push_str(before);
    path.push_str(id);
    path.push_str(after);
    Some(path)
}

/// The record's bytes, in `out` (at least [`RECORD_MAX`]); their number.
#[inline(never)]
fn encode(entry: &Entry, out: &mut [u8]) -> usize {
    let id = entry.id.as_str().as_bytes();
    let pins = &entry.pins;
    let mut flags = 0;
    if entry.had_old {
        flags |= FLAG_HAD_OLD;
    }
    if let Some((_, recovery)) = pins.first {
        flags |= FLAG_FIRST;
        if recovery.is_some() {
            flags |= FLAG_FIRST_RECOVERY;
        }
    }
    if pins.code.is_some() {
        flags |= FLAG_CODE;
    }
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    // Kind 0: an install. Kind 1 is the removal's (B2c-3).
    out[5] = 0;
    out[6] = flags;
    out[7] = pins.rotations().len() as u8;
    out[8] = id.len() as u8;
    let mut at = 9;
    out[at..at + id.len()].copy_from_slice(id);
    at += id.len();
    let (author, recovery) = pins
        .first
        .map_or(([0; 32], [0; 32]), |(a, r)| (a, r.unwrap_or([0; 32])));
    for key in [author, recovery, pins.code.unwrap_or([0; 32])] {
        out[at..at + 32].copy_from_slice(&key);
        at += 32;
    }
    for step in pins.rotations() {
        out[at..at + 32].copy_from_slice(&step.to);
        out[at + 32..at + 96].copy_from_slice(&step.signature);
        at += 96;
    }
    let sum = fnv1a64(&out[..at]);
    out[at..at + 8].copy_from_slice(&sum.to_le_bytes());
    at + 8
}

/// The entry a record holds; `None` for anything that is not one.
#[inline(never)]
fn decode(bytes: &[u8]) -> Option<Entry> {
    let (body, sum) = bytes.split_at(bytes.len().checked_sub(8)?);
    if fnv1a64(body).to_le_bytes() != sum || body.len() < 9 || body[..4] != MAGIC {
        return None;
    }
    let (version, kind, flags, count, len) = (
        body[4],
        body[5],
        body[6],
        usize::from(body[7]),
        usize::from(body[8]),
    );
    if version != VERSION || kind != 0 || flags & !0b1111 != 0 || count > ROTATIONS_MAX {
        return None;
    }
    if body.len() != 9 + len + 96 + count * 96 {
        return None;
    }
    let id = core::str::from_utf8(&body[9..9 + len]).ok()?;
    if len > ID_MAX || !install::is_plugin_name(id) || id == "core" {
        return None;
    }
    let key = |at: usize| -> [u8; 32] {
        let mut key = [0; 32];
        key.copy_from_slice(&body[at..at + 32]);
        key
    };
    let mut at = 9 + len;
    let (author, recovery, code) = (key(at), key(at + 32), key(at + 64));
    at += 96;
    let mut steps = [Rotation {
        to: [0; 32],
        signature: [0; 64],
    }; ROTATIONS_MAX];
    for step in steps.iter_mut().take(count) {
        step.to = key(at);
        step.signature.copy_from_slice(&body[at + 32..at + 96]);
        at += 96;
    }
    let first = (flags & FLAG_FIRST != 0).then(|| {
        (
            author,
            (flags & FLAG_FIRST_RECOVERY != 0).then_some(recovery),
        )
    });
    let pins = Pins::with(
        first,
        &steps[..count],
        (flags & FLAG_CODE != 0).then_some(code),
    )?;
    Some(Entry {
        id: Text::new(id),
        had_old: flags & FLAG_HAD_OLD != 0,
        pins,
    })
}

/// Whether an entry named like the last part of `path` is in its folder
/// (`Storage::exists` asks for files; a plugin folder is a directory).
///
/// # Errors
/// 16 for a card error; a missing parent is none there.
pub(crate) fn present(storage: &dyn Storage, path: &str) -> Result<bool, u8> {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let mut found = false;
    match storage.list_from(dir, 0, &mut |entry| {
        found = found || entry.name.eq_ignore_ascii_case(name);
        !found
    }) {
        Ok(_) => Ok(found),
        Err(StorageError::NotFound) => Ok(false),
        Err(error) => Err(io(error)),
    }
}

pub(crate) fn io(error: StorageError) -> u8 {
    match error {
        StorageError::Full => 17,
        StorageError::OutOfMemory => reason::OTHER,
        _ => reason::CARD,
    }
}

/// Writes the journal whole: a temporary file, synced, renamed into place.
///
/// # Errors
/// A reason of 5.5 (16 a card error, 17 a full card); the journal is then
/// either not there or the old one, never half written.
pub(crate) fn begin(storage: &dyn Storage, entry: &Entry) -> Result<(), u8> {
    let mut buf = [0_u8; RECORD_MAX];
    let len = encode(entry, &mut buf);
    storage
        .create_dir(paperpaw_sdk::builtin::PLUGIN_DIR)
        .map_err(io)?;
    storage.write(TMP, &buf[..len]).map_err(io)?;
    storage.sync().map_err(io)?;
    storage.rename(TMP, JOURNAL).map_err(io)?;
    storage.sync().map_err(io)
}

/// What the start found of a journal.
// Read once at start and consumed at once: boxing the entry would only add an
// allocation that can fail.
#[allow(
    clippy::large_enum_variant,
    reason = "passed once and consumed at once; boxing would add an allocation that can fail"
)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Found {
    None,
    Entry(Entry),
    /// Not a record this host reads: set aside as `.journal.bad`.
    SetAside,
}

/// Reads the journal; a bad one is renamed to [`BAD`] (an older one there is
/// replaced). A short read of a good one (the card) is an error, not a bad
/// journal.
///
/// # Errors
/// 16 for a card error.
pub(crate) fn read(storage: &dyn Storage) -> Result<Found, u8> {
    let size = match storage.size(JOURNAL) {
        Ok(size) => size,
        Err(StorageError::NotFound) => return Ok(Found::None),
        Err(error) => return Err(io(error)),
    };
    let mut buf = [0_u8; RECORD_MAX];
    let entry = if size > RECORD_MAX as u64 {
        None
    } else {
        let len = size as usize;
        let mut done = 0;
        while done < len {
            match storage.read_at(JOURNAL, done as u64, &mut buf[done..len]) {
                Ok(0) | Err(_) => return Err(reason::CARD),
                Ok(n) => done += n,
            }
        }
        decode(&buf[..len])
    };
    match entry {
        Some(entry) => Ok(Found::Entry(entry)),
        None => {
            storage.rename(JOURNAL, BAD).map_err(io)?;
            Ok(Found::SetAside)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Moving,
    Pins,
    Settle,
    RemoveOld,
    RemoveJournal,
    Done,
}

/// What a step came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwapStep {
    More,
    /// Finished: the new plugin is in place (`rolled_back` false) or the old
    /// one is back.
    Done {
        rolled_back: bool,
    },
    /// Stopped with this reason; the journal stays where the plugin is
    /// half done (the next start finishes it), and is gone where the old
    /// plugin was restored.
    Failed(u8),
}

/// The swap of one entry, one card action a step.
pub(crate) struct Swap {
    entry: Entry,
    stage: String,
    old: String,
    target: String,
    phase: Phase,
    /// A live swap rolls back on a failure before the stage is in place; a
    /// replay goes on at the next start instead.
    live: bool,
    rolled_back: bool,
}

impl Swap {
    /// The swap of `entry`, its journal already written ([`begin`]) or read
    /// ([`read`]). `None` for no memory.
    pub(crate) fn new(entry: Entry, live: bool) -> Option<Self> {
        let id = entry.id.as_str();
        Some(Self {
            stage: stage_path(id)?,
            old: old_path(id)?,
            target: target_path(id)?,
            entry,
            phase: Phase::Moving,
            live,
            rolled_back: false,
        })
    }

    pub(crate) fn entry(&self) -> &Entry {
        &self.entry
    }

    fn fail(&mut self, storage: &dyn Storage, why: u8) -> SwapStep {
        if self.live && self.phase == Phase::Moving {
            self.roll_back(storage);
        }
        SwapStep::Failed(why)
    }

    /// Restores the old folder and removes the stage and the journal, as far
    /// as the card lets it (what is left, the next start finishes).
    fn roll_back(&mut self, storage: &dyn Storage) {
        let (Ok(stage), Ok(old), Ok(target)) = (
            present(storage, &self.stage),
            present(storage, &self.old),
            present(storage, &self.target),
        ) else {
            return;
        };
        if old && !target && storage.rename(&self.old, &self.target).is_err() {
            return;
        }
        if stage {
            let _ = storage.remove_dir_all(&self.stage);
        }
        let _ = storage.remove(JOURNAL);
        self.rolled_back = true;
    }

    /// One step.
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> SwapStep {
        match self.phase {
            Phase::Moving => self.moving(storage),
            Phase::Pins => {
                if take_pins(storage, &self.entry) {
                    self.phase = Phase::Settle;
                    SwapStep::More
                } else {
                    SwapStep::Failed(reason::CARD)
                }
            }
            Phase::Settle => {
                // What the stage held (its origin), kept now it is the plugin's.
                let _ = marks::settle(storage, self.entry.id.as_str(), &self.target);
                self.phase = Phase::RemoveOld;
                SwapStep::More
            }
            Phase::RemoveOld => {
                match present(storage, &self.old) {
                    Ok(true) if storage.remove_dir_all(&self.old).is_err() => {
                        return SwapStep::Failed(reason::CARD);
                    }
                    Err(why) => return SwapStep::Failed(why),
                    _ => {}
                }
                self.phase = Phase::RemoveJournal;
                SwapStep::More
            }
            Phase::RemoveJournal => match storage.remove(JOURNAL) {
                Ok(()) | Err(StorageError::NotFound) => {
                    self.phase = Phase::Done;
                    SwapStep::Done {
                        rolled_back: self.rolled_back,
                    }
                }
                Err(error) => SwapStep::Failed(io(error)),
            },
            Phase::Done => SwapStep::Done {
                rolled_back: self.rolled_back,
            },
        }
    }

    #[inline(never)]
    fn moving(&mut self, storage: &dyn Storage) -> SwapStep {
        let (Ok(stage), Ok(old), Ok(target)) = (
            present(storage, &self.stage),
            present(storage, &self.old),
            present(storage, &self.target),
        ) else {
            return SwapStep::Failed(reason::CARD);
        };
        match (stage, old, target) {
            // The stage is in place: the pins come next.
            (false, _, true) => {
                self.phase = Phase::Pins;
                SwapStep::More
            }
            // The old folder is back and the stage gone: rolled back.
            (false, true, false) => {
                if let Err(error) = storage.rename(&self.old, &self.target) {
                    return SwapStep::Failed(io(error));
                }
                self.rolled_back = true;
                self.phase = Phase::RemoveJournal;
                SwapStep::More
            }
            // Nothing there at all: nothing to finish.
            (false, false, false) => {
                self.rolled_back = true;
                self.phase = Phase::RemoveJournal;
                SwapStep::More
            }
            // The old folder goes first.
            (true, false, true) if self.entry.had_old => {
                match storage.rename(&self.target, &self.old) {
                    Ok(()) => SwapStep::More,
                    Err(error) => {
                        let why = io(error);
                        self.fail(storage, why)
                    }
                }
            }
            // A folder of the id appeared since the check: not what was
            // confirmed.
            (true, false, true) => self.fail(storage, reason::CHANGED),
            // The stage takes the place.
            (true, _, false) => match storage.rename(&self.stage, &self.target) {
                Ok(()) => {
                    self.phase = Phase::Pins;
                    SwapStep::More
                }
                Err(error) => {
                    let why = io(error);
                    self.fail(storage, why)
                }
            },
            // The stage, an old folder and a target at once: no step of this
            // swap makes it. Left for a person.
            (true, true, true) => SwapStep::Failed(reason::OTHER),
        }
    }
}

/// Takes the pins of `entry` idempotently; `false` where one could not be kept
/// (the next start tries again).
#[inline(never)]
pub(crate) fn take_pins(storage: &dyn Storage, entry: &Entry) -> bool {
    let id = entry.id.as_str();
    let pins = &entry.pins;
    let mut kept = true;
    if let Some((author, recovery)) = pins.first {
        match native::known_author(storage, id) {
            Some(known) => kept &= known == author,
            None => {
                kept &= match recovery {
                    Some(recovery) => keys::remember_first_install(storage, id, &author, &recovery),
                    None => native::remember_author(storage, id, &author),
                };
            }
        }
    }
    for step in pins.rotations() {
        if native::known_author(storage, id) == Some(step.to) {
            continue;
        }
        kept &= keys::accept_rotation(storage, id, &step.to, &step.signature).is_ok();
        if !kept {
            return false;
        }
    }
    if let Some(code) = pins.code {
        kept &= native::remember_code(storage, id, &code);
    }
    kept
}

/// What the start's replay did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Replayed {
    /// No journal.
    Nothing,
    /// A swap finished (`rolled_back` false) or undone, for this plugin.
    Finished { id: Text<ID_MAX>, rolled_back: bool },
    /// A journal this host does not read was set aside.
    SetAside,
    /// Stopped with this reason; the journal stays for the next start.
    Failed(u8),
}

/// The start's replay: finishes a journalled swap. Bounded: at most 8 steps.
pub(crate) fn replay(storage: &dyn Storage) -> Replayed {
    let entry = match read(storage) {
        Ok(Found::None) => return Replayed::Nothing,
        Ok(Found::SetAside) => return Replayed::SetAside,
        Ok(Found::Entry(entry)) => entry,
        Err(why) => return Replayed::Failed(why),
    };
    let Some(mut swap) = Swap::new(entry, false) else {
        return Replayed::Failed(reason::OTHER);
    };
    for _ in 0..8 {
        match swap.step(storage) {
            SwapStep::More => {}
            SwapStep::Done { rolled_back } => {
                return Replayed::Finished {
                    id: entry.id,
                    rolled_back,
                };
            }
            SwapStep::Failed(why) => return Replayed::Failed(why),
        }
    }
    Replayed::Failed(reason::OTHER)
}

#[cfg(test)]
#[path = "plugin_swap_tests.rs"]
pub(in crate::native_host) mod tests;
