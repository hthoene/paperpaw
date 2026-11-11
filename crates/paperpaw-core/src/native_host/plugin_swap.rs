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
//! A journal that cannot be read (a damaged record, an id no plugin has), or
//! one the start can never finish (a state no step of this host makes), is
//! renamed to `.journal.bad` and counted, never trusted: it would otherwise
//! block every later install with reason 1.
//!
//! The removal of a plugin (`plugin_remove`, record kind 1) shares the journal
//! file: one change of the plugin folder at a time.
use alloc::vec::Vec;
use alloc::{boxed::Box, string::String};
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
pub(crate) const TMP: &str = "plugins/.journal.tmp";
pub(crate) const BAD: &str = "plugins/.journal.bad";
/// One byte for each start at which the host could not forget a removed
/// plugin's secrets: [`FORGET_TRIES`] of them end the waiting.
/// The count is appended, so a card without room for one byte (full) never gives
/// up: the removal waits on until there is room, as every install does.
pub(crate) const TRIES: &str = "plugins/.journal.tries";
/// Starts a removal waits for the host to forget what it holds of the plugin
/// before it is finished without (the secrets stay, the user is told).
pub(crate) const FORGET_TRIES: u64 = 3;
const MAGIC: [u8; 4] = *b"PPIJ";
const VERSION: u8 = 1;
/// The longest record: the fixed part, the id, the keys, the rotations, the checksum.
const RECORD_MAX: usize = 8 + ID_MAX + 32 * 3 + ROTATIONS_MAX * 96 + 8;

const KIND_ENTRY: u8 = 0;
const KIND_REMOVAL: u8 = 1;
const FLAG_DATA: u8 = 1;
const FLAG_HAD_OLD: u8 = 1;
const FLAG_FIRST: u8 = 1 << 1;
const FLAG_FIRST_RECOVERY: u8 = 1 << 2;
const FLAG_CODE: u8 = 1 << 3;

/// What a removal is: one plugin id, and whether its data goes too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Removal {
    pub(crate) id: Text<ID_MAX>,
    pub(crate) data: bool,
}

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
/// Where a removal moves the plugin folder, `plugins/.removing-<id>`.
pub(crate) fn removing_path(id: &str) -> Option<String> {
    named(id, ".removing-", "")
}
/// The plugin's data folder `.paperpaw/plugins/<id>` and where a removal moves
/// it, built from a valid id and nothing else.
pub(crate) fn data_path(id: &str) -> Option<String> {
    data_named(id, "")
}
pub(crate) fn data_removing_path(id: &str) -> Option<String> {
    data_named(id, ".removing-")
}

fn data_named(id: &str, before: &str) -> Option<String> {
    if !install::is_plugin_name(id) || id == "core" {
        return None;
    }
    let dir = super::storage::PREFIX;
    let mut path = String::new();
    path.try_reserve_exact(dir.len() + before.len() + id.len())
        .ok()?;
    path.push_str(dir);
    path.push_str(before);
    path.push_str(id);
    Some(path)
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
    // Kind 0: an install. Kind 1 is the removal's ([`encode_removal`]).
    out[5] = KIND_ENTRY;
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
    if version != VERSION || kind != KIND_ENTRY || flags & !0b1111 != 0 || count > ROTATIONS_MAX {
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

/// The removal record's bytes, in `out` (at least [`RECORD_MAX`]); their number.
fn encode_removal(removal: &Removal, out: &mut [u8]) -> usize {
    let id = removal.id.as_str().as_bytes();
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5] = KIND_REMOVAL;
    out[6] = if removal.data { FLAG_DATA } else { 0 };
    out[7] = 0;
    out[8] = id.len() as u8;
    out[9..9 + id.len()].copy_from_slice(id);
    let at = 9 + id.len();
    let sum = fnv1a64(&out[..at]);
    out[at..at + 8].copy_from_slice(&sum.to_le_bytes());
    at + 8
}

/// The removal a record holds; `None` for anything that is not one.
fn decode_removal(bytes: &[u8]) -> Option<Removal> {
    let (body, sum) = bytes.split_at(bytes.len().checked_sub(8)?);
    if fnv1a64(body).to_le_bytes() != sum || body.len() < 9 || body[..4] != MAGIC {
        return None;
    }
    let (version, kind, flags, count, len) =
        (body[4], body[5], body[6], body[7], usize::from(body[8]));
    if version != VERSION
        || kind != KIND_REMOVAL
        || flags & !FLAG_DATA != 0
        || count != 0
        || body.len() != 9 + len
        || len > ID_MAX
    {
        return None;
    }
    let id = core::str::from_utf8(&body[9..]).ok()?;
    if !install::is_plugin_name(id) || id == "core" {
        return None;
    }
    Some(Removal {
        id: Text::new(id),
        data: flags & FLAG_DATA != 0,
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
/// A reason of 5.5 (16 a card error, 17 a full card; 1 where another journal
/// is still there); the journal is then either not there or the old one, never
/// half written.
pub(crate) fn begin(storage: &dyn Storage, entry: &Entry) -> Result<(), u8> {
    let mut buf = [0_u8; RECORD_MAX];
    let len = encode(entry, &mut buf);
    write_journal(storage, &buf[..len])
}

/// Writes the journal of a removal, as [`begin`] does an install's.
///
/// # Errors
/// As [`begin`].
pub(crate) fn begin_removal(storage: &dyn Storage, removal: &Removal) -> Result<(), u8> {
    let mut buf = [0_u8; RECORD_MAX];
    let len = encode_removal(removal, &mut buf);
    write_journal(storage, &buf[..len])
}

fn write_journal(storage: &dyn Storage, record: &[u8]) -> Result<(), u8> {
    // The journal of another install, one that could not be finished, is not
    // ours to replace: the start finishes it.
    match storage.size(JOURNAL) {
        Ok(_) => return Err(reason::OTHER),
        Err(StorageError::NotFound) => {}
        Err(error) => return Err(io(error)),
    }
    storage
        .create_dir(paperpaw_sdk::builtin::PLUGIN_DIR)
        .map_err(io)?;
    storage.write(TMP, record).map_err(io)?;
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
    Removal(Removal),
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
    let found = if size > RECORD_MAX as u64 {
        None
    } else {
        let len = size as usize;
        // On the heap, at the size of the file: the record's own room
        // (`RECORD_MAX`) and the entry it decodes to would not both fit a
        // frame of the limit the firmware keeps for the host's code.
        let mut buf = Vec::new();
        buf.try_reserve_exact(len).map_err(|_| reason::OTHER)?;
        buf.resize(len, 0_u8);
        let mut done = 0;
        while done < len {
            match storage.read_at(JOURNAL, done as u64, &mut buf[done..len]) {
                Ok(0) | Err(_) => return Err(reason::CARD),
                Ok(n) => done += n,
            }
        }
        decode(&buf[..len])
            .map(Found::Entry)
            .or_else(|| decode_removal(&buf[..len]).map(Found::Removal))
    };
    match found {
        Some(found) => Ok(found),
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
    /// A state no step of this swap makes: it can never be finished.
    Stuck,
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

    /// [`Self::new`] of a live swap of the install `id` with `pins`, in a box
    /// of its own made here (the swap is 800 bytes: not built on the caller's
    /// frame, nor copied to the box from it). `None` for no memory.
    #[inline(never)]
    pub(crate) fn live_box(id: Text<ID_MAX>, had_old: bool, pins: &Pins) -> Option<Box<Self>> {
        let (stage, old, target) = (
            stage_path(id.as_str())?,
            old_path(id.as_str())?,
            target_path(id.as_str())?,
        );
        super::shared::try_box_with(|| Self {
            entry: Entry {
                id,
                had_old,
                pins: *pins,
            },
            stage,
            old,
            target,
            phase: Phase::Moving,
            live: true,
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
        // The journal goes before the stage: a cut while the stage is half
        // removed must not let the replay install what is left of it.
        match storage.remove(JOURNAL) {
            Ok(()) | Err(StorageError::NotFound) => {}
            Err(_) => return,
        }
        if stage {
            let _ = storage.remove_dir_all(&self.stage);
        }
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
            // The old folder goes first, and the stage follows in the same
            // step: a stop between the two would leave the plugin without its
            // folder until the next start.
            (true, false, true) if self.entry.had_old => {
                match storage.rename(&self.target, &self.old) {
                    Ok(()) => self.moving(storage),
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
            (true, true, true) => SwapStep::Stuck,
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
            // Or a key it was rotated to since: a replay after a cut.
            Some(known) => {
                kept &= known == author || pins.rotations().iter().any(|step| step.to == known);
            }
            None => {
                kept &= match recovery {
                    Some(recovery) => keys::remember_first_install(storage, id, &author, &recovery),
                    None => native::remember_author(storage, id, &author),
                };
            }
        }
    }
    // Only the steps after the key pinned now: a replay after a cut finds the
    // chain taken as far as it got, and a step before it would not hold.
    let known = native::known_author(storage, id);
    let taken = pins
        .rotations()
        .iter()
        .rposition(|step| Some(step.to) == known)
        .map_or(0, |at| at + 1);
    for step in pins.rotations().iter().skip(taken) {
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
    /// A removal finished, for this plugin.
    Removed { id: Text<ID_MAX>, data: bool },
    /// A removal stopped where the host must forget what it holds of the plugin
    /// (its secrets and grants); the journal stays, [`resume`] finishes it.
    Forgetting,
    /// A removal finished after the host failed [`FORGET_TRIES`] starts in a
    /// row to forget the plugin's secrets and grants: they stay on the card.
    Unforgotten { id: Text<ID_MAX> },
    /// A journal this host does not read, or can never finish, was set aside.
    SetAside,
    /// Stopped with this reason; the journal stays for the next start.
    Failed(u8),
}

/// Renames the journal to [`BAD`]: what the start can never finish must not
/// block every later change of the plugin folder.
fn set_aside(storage: &dyn Storage) -> Replayed {
    match storage.rename(JOURNAL, BAD) {
        Ok(()) => Replayed::SetAside,
        Err(error) => Replayed::Failed(io(error)),
    }
}

/// The start's replay: finishes a journalled swap or removal. Bounded: at most
/// 16 steps; a journal that is not finished by then is set aside.
pub(crate) fn replay(storage: &dyn Storage) -> Replayed {
    let entry = match read(storage) {
        Ok(Found::None) => return Replayed::Nothing,
        Ok(Found::SetAside) => return Replayed::SetAside,
        Ok(Found::Entry(entry)) => entry,
        Ok(Found::Removal(removal)) => return replay_removal(storage, &removal, None),
        Err(why) => return Replayed::Failed(why),
    };
    let Some(mut swap) = Swap::new(entry, false) else {
        return Replayed::Failed(reason::OTHER);
    };
    for _ in 0..16 {
        match swap.step(storage) {
            SwapStep::More => {}
            SwapStep::Done { rolled_back } => {
                return Replayed::Finished {
                    id: entry.id,
                    rolled_back,
                };
            }
            SwapStep::Failed(why) => return Replayed::Failed(why),
            SwapStep::Stuck => return set_aside(storage),
        }
    }
    set_aside(storage)
}

/// Finishes a removal journal the start left at its forget step, with the
/// host's help: `forget` forgets the plugin's secrets and grants (`false`: it
/// could not, the journal stays for the next start). Nothing else is touched.
pub(crate) fn resume(storage: &dyn Storage, forget: &mut dyn FnMut(&str) -> bool) -> Replayed {
    let done = match read(storage) {
        Ok(Found::Removal(removal)) => replay_removal(storage, &removal, Some(forget)),
        Ok(_) => Replayed::Nothing,
        Err(why) => Replayed::Failed(why),
    };
    // The count belongs to the journal that is gone.
    if matches!(
        done,
        Replayed::Removed { .. } | Replayed::Unforgotten { .. }
    ) {
        let _ = storage.remove(TRIES);
    }
    done
}

/// Counts a start at which the host could not forget; `true` once
/// [`FORGET_TRIES`] are counted. A card that cannot count never gives up.
fn tired_of_waiting(storage: &dyn Storage) -> bool {
    storage.append(TRIES, b"x").is_ok() && storage.size(TRIES).is_ok_and(|n| n >= FORGET_TRIES)
}

#[inline(never)]
fn replay_removal(
    storage: &dyn Storage,
    removal: &Removal,
    mut forget: Option<&mut dyn FnMut(&str) -> bool>,
) -> Replayed {
    use super::plugin_remove::{RemoveStep, Removing};
    let Some(mut removing) = Removing::new(*removal) else {
        return Replayed::Failed(reason::OTHER);
    };
    let mut unforgotten = false;
    for _ in 0..16 {
        match removing.step(storage) {
            // The journal is gone: what is left is cleaned by the sweep, in
            // bounded pieces.
            RemoveStep::More if removing.settled() && unforgotten => {
                return Replayed::Unforgotten { id: removal.id };
            }
            RemoveStep::More if removing.settled() => {
                return Replayed::Removed {
                    id: removal.id,
                    data: removal.data,
                };
            }
            RemoveStep::More => {}
            // No host at the start: the journal stays for the first pass.
            RemoveStep::Forget => match forget.as_mut() {
                None => return Replayed::Forgetting,
                Some(forget) => {
                    let ok = forget(removal.id.as_str());
                    // A host that fails start after start must not keep every
                    // later install waiting behind this journal.
                    // ...but not before the secrets that stay are on the list
                    // (`plugin_unforgotten`): they are retried at every start,
                    // and the id is refused for an install until they are gone.
                    unforgotten = !ok
                        && tired_of_waiting(storage)
                        && super::plugin_unforgotten::add(storage, removal.id.as_str());
                    removing.forgotten(ok || unforgotten);
                }
            },
            RemoveStep::Done if unforgotten => return Replayed::Unforgotten { id: removal.id },
            RemoveStep::Done => {
                return Replayed::Removed {
                    id: removal.id,
                    data: removal.data,
                };
            }
            RemoveStep::Failed(why) => return Replayed::Failed(why),
            RemoveStep::Stuck => return set_aside(storage),
        }
    }
    set_aside(storage)
}

#[cfg(test)]
#[path = "plugin_swap_tests.rs"]
pub(in crate::native_host) mod tests;
