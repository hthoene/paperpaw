//! The host's one store (`UiStore`) of hand-overs, their texts and the checked
//! id allocator (`docs/abi/ui.md` 1.1 "The host's hand-over seam", 8.2).
//!
//! The screen stack (H1) owns commands and their staging and only calls the
//! store (the seam of ui.md 1.1): `handover_make` at op 16, `handover_check`
//! while an `open` is checked (it changes nothing), `claim` once a submit is
//! checked, `rollback` / `release` for claims that do not apply, `attach` when
//! the `open` applies, `end_callback` when the maker's callback returns,
//! `instance_closed` when an instance closes, `owner_stopped` when a plugin
//! stops. Every call completes or leaves the store unchanged; no call runs
//! plugin code and no borrow of the store outlives a call.
//!
//! A hand-over is **made** (by a caller, in one callback epoch), **claimed**
//! (by a submit that passed `claim`, held by a [`Claims`] token) and then
//! **owned** by the instance `attach` gave it to, until the instance closes or
//! a reopen gives it another. Its params are kept encoded as `ui_param`
//! records. Texts are written in parts (op 9) by the maker in the callback that
//! made it and read (op 10) only by the owning instance; they are wiped when
//! freed and never in a `Debug` or a log.
use super::registrations::{self as wire, Record};
use crate::native_host::codec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::fmt;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE,
    ERR_NOT_FOUND, ERR_STATE,
};
use paperpaw_platform::MemoryClass;

/// Most params of one hand-over.
pub(crate) const PARAMS_MAX: usize = 32;
/// Most bytes of keys and values of one hand-over.
pub(crate) const PARAM_BYTES_MAX: usize = 1_024;
/// Longest key.
const KEY_MAX: usize = 32;
/// Whether the device declares the small memory class (a 256 KiB heap: the X4
/// and the X3): the one place the X4 profile of `core.ui` is decided, for the
/// store's limits (`HostWiring::ui`), for the admission rules
/// (`Frame::ui_x4`) and for op 14 (`native_status`). It follows the declared
/// class, not an id, so a device that declares `Large` (the X4 Pro until its
/// memory is measured) keeps the large limits. A class added later gets the
/// small limits until someone decides otherwise (fail closed).
pub(crate) fn is_x4(memory: MemoryClass) -> bool {
    !matches!(memory, MemoryClass::Large)
}

/// Largest text of a hand-over or a request.
pub(crate) const TEXT_MAX: usize = 24_576;
/// Largest part one write brings.
pub(crate) const PART_MAX: usize = 3_072;

/// What a device allows (`ui.md` 8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Encoded bytes of the params of all hand-overs together.
    pub(crate) params_bytes: usize,
    /// Texts alive at once (hand-overs and requests).
    pub(crate) texts: usize,
    /// Bytes of text kept in memory, all texts together (`ERR_IO` beyond: the
    /// card holds the rest where there is one).
    pub(crate) text_memory: usize,
    /// Bytes a live-typing answer may hold of a text that is not secret.
    pub(crate) typing_bytes: usize,
    /// Open requests (not counting a nested one).
    pub(crate) requests: u8,
    /// Hand-overs alive at once, whatever their params (an empty one still
    /// costs a record).
    pub(crate) handovers: usize,
    /// Request rows kept at once: open ones, nested ones and those done and
    /// waiting to be delivered (a cancelled request is one until its event).
    pub(crate) requests_kept: usize,
    /// Bytes of secret text kept in memory, all secret texts together (their
    /// own pool: a secret text never uses the ordinary one).
    pub(crate) secret_memory: usize,
    /// Bytes of the questions kept (title, text, choices of every request row
    /// kept, a question being up to 3.3 KiB): `ERR_FULL` at `ask` beyond.
    pub(crate) question_memory: usize,
}

impl Limits {
    /// The X4: params 1,536 bytes, 4 texts, 2 KiB of text in memory.
    pub(crate) const X4: Self = Self {
        params_bytes: 1_536,
        texts: 4,
        text_memory: 2_048,
        typing_bytes: 512,
        requests: 1,
        handovers: 12,
        requests_kept: 3,
        // 8,192 bytes of password in 32 chunks of 256 and their descriptors.
        secret_memory: 8_192 + 32 * 24,
        question_memory: 4_096,
    };
    /// Elsewhere: 16 texts, each in memory.
    pub(crate) const ELSEWHERE: Self = Self {
        params_bytes: 16 * 1_313,
        texts: 16,
        text_memory: usize::MAX,
        typing_bytes: usize::MAX,
        requests: 4,
        handovers: 64,
        requests_kept: 16,
        secret_memory: 40_960,
        question_memory: 65_536,
    };
}

/// Who calls and in which callback. The epoch is the host's counter, bumped
/// at every callback entry; it is never 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(super) owner: u64,
    pub(super) epoch: u64,
}

impl Stamp {
    /// `ERR_STATE` for epoch 0 (no callback is running).
    pub(crate) fn new(owner: u64, epoch: u64) -> Result<Self, i32> {
        if epoch == 0 {
            return Err(ERR_STATE);
        }
        Ok(Self { owner, epoch })
    }
}

/// The hand-overs one accepted submit claimed: move-only, kept by the command
/// stage until its deferred `open`s drain. [`UiStore::rollback`] frees all that
/// are still claimed; an `open` ends its own with [`UiStore::attach`] or
/// [`UiStore::release`]. A batch of no ids is fine.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "claims end in attach, release or rollback"]
pub(crate) struct Claims {
    owner: u64,
    batch: u64,
}

/// One param, as op 16 brought it.
#[derive(Clone, Copy)]
pub(crate) struct Param<'a> {
    pub(crate) key: &'a str,
    pub(crate) value: &'a str,
}

/// The file an instance's hand-over names (see [`UiStore::handed_file`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Handed<'a> {
    /// The plugin that made the hand-over, as its owner key, not its id. The
    /// host hands owner keys out once (`HostResources::try_owner`, a counter
    /// that is never reused and refuses at its end), so a key that outlives
    /// its plugin can never name another one. The reader resolves it with
    /// `native_instances.id(key)` when it opens the file and answers
    /// `ERR_NOT_FOUND` when there is no such installed plugin any more.
    pub(crate) maker: u64,
    /// Where the file is in the maker's folders: `0` its data folder, `2`
    /// its install folder.
    pub(crate) root: u32,
    /// The path as the hand-over carries it, not yet checked.
    pub(crate) path: &'a str,
}

/// What the screen opened event says about an instance's hand-over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Info {
    pub(crate) handover: u32,
    pub(crate) params: u32,
    pub(crate) text_bytes: u32,
}

/// Whether `ok` holds for every param of the framed block `block`.
fn params_in(block: &[u8], mut ok: impl FnMut(&str, &str) -> bool) -> bool {
    let mut rest = block;
    while !rest.is_empty() {
        let Ok((param, used)) = codec::prefix::<wire::ServiceUiParam<'_>>(rest) else {
            return false;
        };
        if !ok(param.key, param.value) {
            return false;
        }
        rest = &rest[used..];
    }
    true
}

/// The value of param `key` in the framed block, if it has one.
fn value_of<'a>(block: &'a [u8], key: &str) -> Option<&'a str> {
    let mut rest = block;
    while !rest.is_empty() {
        let (param, used) = codec::prefix::<wire::ServiceUiParam<'_>>(rest).ok()?;
        if param.key == key {
            return Some(param.value);
        }
        rest = &rest[used..];
    }
    None
}

impl Info {
    /// No hand-over: what the event says of an instance opened without one.
    pub(crate) const NONE: Self = Self {
        handover: 0,
        params: 0,
        text_bytes: 0,
    };
}

/// Bytes wiped when they are freed (with the shared volatile wipe). Room
/// beyond `len` was never written, so it holds nothing to wipe.
pub(super) struct Wiped(pub(super) Vec<u8>);

impl Drop for Wiped {
    fn drop(&mut self) {
        super::secret_bytes::wipe(&mut self.0);
    }
}

impl Wiped {
    pub(super) fn copy(bytes: &[u8]) -> Result<Self, i32> {
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        data.extend_from_slice(bytes);
        Ok(Self(data))
    }
}

/// The owner of what the host made itself (no plugin has this key).
const HOST_OWNER: u64 = u64::MAX;
/// Hand-overs beyond the limit the host may make for its dialogs, so that a
/// plugin that fills the table (`Limits::handovers`) cannot make every text
/// question fail.
const HOST_ROOM: usize = 2;
/// The bytes of params the host may make for them beyond the limit of the
/// plugins' ([`HOST_ROOM`] of the two params `core.request` and `core.kind`,
/// framed, are under 100 each), so that a plugin that fills the pool
/// (`Limits::params_bytes`) cannot make every text question fail either.
const HOST_BYTES: usize = HOST_ROOM * 128;

enum State {
    Made {
        owner: u64,
        epoch: u64,
    },
    Claimed {
        owner: u64,
        batch: u64,
    },
    /// `maker` is the plugin that made and claimed it (kept when it stops).
    Owned {
        instance: u32,
        maker: u64,
    },
}

/// A text is kept in chunks: a chunk is one allocation, filled in place by
/// later writes until it is full, so a text written a byte at a time costs a
/// chunk per [`CHUNK`] bytes and no more.
#[derive(Default)]
pub(super) struct Text {
    pub(super) parts: Vec<Wiped>,
    pub(super) bytes: usize,
    pub(super) committed: bool,
    /// What the text is charged to its pool: the capacity of its chunks and a
    /// descriptor for each (not just the bytes written).
    pub(super) charged: usize,
}

/// A chunk that takes small writes in place.
const CHUNK: usize = 256;
/// The charge of a chunk's descriptor (its `Vec` header).
const DESCRIPTOR: usize = core::mem::size_of::<Wiped>();

pub(super) struct Entry {
    pub(super) id: u32,
    state: State,
    /// The params as `ui_param` records, framed.
    params: Wiped,
    count: u32,
    pub(super) text: Text,
}

pub(super) struct Inner {
    pub(super) entries: Vec<Entry>,
    pub(super) requests: Vec<super::ui_requests::Request>,
    pub(super) params_bytes: usize,
    /// Ordinary texts' charge; secret texts have their own pool.
    pub(super) text_memory: usize,
    pub(super) secret_memory: usize,
    /// The questions kept.
    pub(super) question_memory: usize,
    /// Answers of a host dialog the store did not take, not yet logged.
    pub(super) refused: u32,
    /// The clipboard (`core.ui` ops 18 and 19).
    pub(super) clipboard: super::ui_clipboard::ClipboardState,
}

/// The store.
pub(crate) struct UiStore {
    pub(super) limits: Limits,
    next: Cell<u32>,
    batches: Cell<u64>,
    pub(super) inner: RefCell<Inner>,
}

impl fmt::Debug for UiStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts only: a text or a param may be secret.
        f.debug_struct("UiStore")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

fn key_fine(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= KEY_MAX
        && key.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-')
        })
}

/// What a text write is held to.
pub(super) struct WriteRules {
    /// A text without bytes may take a slot (`ERR_FULL` when not).
    pub(super) slots_free: bool,
    /// Bytes of text in memory, all texts together (`ERR_IO` beyond).
    pub(super) memory_cap: usize,
    /// This text's cap (`ERR_NO_SPACE` beyond; never cut).
    pub(super) text_cap: usize,
}

/// A part of a text (ui.md 3.1): an offset 0 starts the text over, any other
/// must be the bytes written so far (`ERR_INVALID`); `last` commits. Every
/// fallible step comes before the first change, so a refusal leaves the text
/// at the parts before it. Bytes go into the spare room of the last chunk; the
/// rest into one new chunk (so many small parts never mean many allocations),
/// and the pool is charged the chunks' capacity and descriptors, not only the
/// bytes.
pub(super) fn write_part(
    text: &mut Text,
    memory: &mut usize,
    rules: &WriteRules,
    offset: usize,
    data: &[u8],
    last: bool,
) -> Result<(), i32> {
    if data.len() > PART_MAX {
        return Err(ERR_NO_SPACE);
    }
    let had = text.bytes;
    let restart = offset == 0;
    if !restart && offset != had {
        return Err(ERR_INVALID);
    }
    let kept = if restart { 0 } else { had };
    if kept + data.len() > rules.text_cap {
        return Err(ERR_NO_SPACE);
    }
    let had_slot = had > 0 || text.committed;
    if !had_slot && (!data.is_empty() || last) && !rules.slots_free {
        return Err(ERR_FULL);
    }
    let spare = if restart {
        0
    } else {
        text.parts
            .last()
            .map_or(0, |part| part.0.capacity() - part.0.len())
    };
    let into_last = data.len().min(spare);
    let rest = data.len() - into_last;
    // A new chunk is as big as what does not fit, rounded up to CHUNK (never
    // past the text's own cap).
    let room_after = rules.text_cap - (kept + data.len());
    let chunk = if rest == 0 {
        0
    } else if rest >= CHUNK {
        rest
    } else {
        rest + room_after.min(CHUNK - rest)
    };
    let charge = if chunk == 0 { 0 } else { chunk + DESCRIPTOR };
    let mut dropped = if restart { text.charged } else { 0 };
    if (*memory - dropped).saturating_add(charge) > rules.memory_cap {
        return Err(ERR_IO);
    }
    // A restart holds the old text while the new chunk is allocated: that
    // peak (old + new) must stay inside the pool. When both do not fit, the
    // old text is freed first, so the peak is the larger of the two and a
    // refusal after that leaves the text started over and empty (offset 0
    // said so).
    if restart && dropped > 0 && memory.saturating_add(charge) > rules.memory_cap {
        *text = Text::default();
        *memory -= dropped;
        dropped = 0;
    }
    let fresh_chunk = if chunk == 0 {
        None
    } else {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(chunk).map_err(|_| ERR_NO_MEMORY)?;
        Some(Wiped(bytes))
    };
    // The room for the descriptor is reserved before anything changes.
    let mut fresh = Vec::new();
    if restart {
        fresh.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
    } else if fresh_chunk.is_some() {
        text.parts.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
    }
    // Nothing fails past this point.
    if restart {
        *text = Text {
            parts: fresh,
            bytes: 0,
            committed: false,
            charged: 0,
        };
        *memory -= dropped;
    }
    if into_last > 0
        && let Some(part) = text.parts.last_mut()
    {
        part.0.extend_from_slice(&data[..into_last]);
    }
    if let Some(mut chunk) = fresh_chunk {
        chunk.0.extend_from_slice(&data[into_last..]);
        text.parts.push(chunk);
        text.charged += charge;
        *memory += charge;
    }
    text.bytes += data.len();
    text.committed = last;
    Ok(())
}

impl UiStore {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            next: Cell::new(1),
            batches: Cell::new(1),
            inner: RefCell::new(Inner {
                entries: Vec::new(),
                requests: Vec::new(),
                params_bytes: 0,
                text_memory: 0,
                secret_memory: 0,
                question_memory: 0,
                refused: 0,
                clipboard: super::ui_clipboard::ClipboardState::default(),
            }),
        }
    }

    /// The next id for an instance, a request or a hand-over: nonzero, never
    /// reused; `ERR_FULL` at `u32::MAX` until restart.
    pub(crate) fn alloc_id(&self) -> Result<u32, i32> {
        let id = self.next.get();
        if id == 0 || id == u32::MAX {
            return Err(ERR_FULL);
        }
        self.next.set(id + 1);
        Ok(id)
    }

    /// The store for a cleanup that must not be skipped: `ERR_BUSY` when it is
    /// borrowed, so the caller retries.
    pub(super) fn inner_busy(&self) -> Result<core::cell::RefMut<'_, Inner>, i32> {
        self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)
    }

    pub(super) fn inner(&self) -> Result<core::cell::RefMut<'_, Inner>, i32> {
        self.inner.try_borrow_mut().map_err(|_| ERR_STATE)
    }

    /// Op 16: a hand-over of `params` made by `who`. `ERR_INVALID` for a form
    /// that is not allowed, `ERR_NO_SPACE` over the byte cap, `ERR_FULL` when
    /// the device's params are full, `ERR_NO_MEMORY`.
    pub(crate) fn handover_make(&self, who: Stamp, params: &[Param<'_>]) -> Result<u32, i32> {
        if params.len() > PARAMS_MAX {
            return Err(ERR_NO_SPACE);
        }
        let mut content = 0_usize;
        for (index, param) in params.iter().enumerate() {
            if !key_fine(param.key)
                || param.key.starts_with("core.")
                || params[..index].iter().any(|before| before.key == param.key)
            {
                return Err(ERR_INVALID);
            }
            content += param.key.len() + param.value.len();
        }
        if content > PARAM_BYTES_MAX {
            return Err(ERR_NO_SPACE);
        }
        self.insert(
            params.iter().copied(),
            State::Made {
                owner: who.owner,
                epoch: who.epoch,
            },
            (0, 0),
        )
    }

    /// The hand-over of the host's own for the dialog of `request` of `kind`
    /// (both ASCII decimal): the params `core.request` and `core.kind`
    /// (`docs/abi/ui.md` 3.2), which op 16 refuses a plugin. No plugin made
    /// it: it is made **claimed**, so that the stack's `attach` gives it to
    /// the dialog's instance as it gives one a plugin's `open` names, and
    /// [`Self::release`] frees it where the screen opens without it.
    ///
    /// # Errors
    /// As [`Self::handover_make`].
    pub(crate) fn handover_dialog(&self, request: &str, kind: &str) -> Result<u32, i32> {
        self.insert(
            [
                Param {
                    key: "core.request",
                    value: request,
                },
                Param {
                    key: "core.kind",
                    value: kind,
                },
            ]
            .into_iter(),
            // `batch` is a number no `claim` makes (they count up from 0), so
            // that no plugin's rollback frees it.
            State::Claimed {
                owner: HOST_OWNER,
                batch: u64::MAX,
            },
            (HOST_ROOM, HOST_BYTES),
        )
    }

    /// The hand-over of the host's own for a native screen opened without
    /// one: by `Kernel::open` with interface-6 `Params` (a shell's script or
    /// request, an interface-6 plugin, the kernel), never for a native
    /// plugin's `command.open` (that one names its own, or none). Its params
    /// are those of `params` a plugin's op 16 could name (other keys, and a key
    /// again, are left out), under op 16's caps; `text` (interface 6
    /// `params::TEXT`) is also its text (op 10), and stays a param only where
    /// it fits those caps. Made **claimed** by the host as
    /// [`Self::handover_dialog`]'s, so the stack's `attach` gives it to the
    /// instance (freed when it closes) and [`Self::release`] frees it where it
    /// does not open.
    ///
    /// # Errors
    /// `ERR_NO_SPACE` when the params beyond `text` exceed op 16's caps or the
    /// text is longer than a hand-over's; as [`Self::text_write`] for the
    /// text; `ERR_FULL`, `ERR_NO_MEMORY`. Nothing is kept then.
    pub(crate) fn handover_params(&self, params: &[Param<'_>]) -> Result<u32, i32> {
        // Interface 6 names the long text so; it is also the hand-over's text.
        const TEXT: &str = "text";
        if params.len() > 2 * PARAMS_MAX {
            return Err(ERR_NO_SPACE);
        }
        let named = |at: usize, param: &Param<'_>| {
            key_fine(param.key)
                && !param.key.starts_with("core.")
                && !params[..at].iter().any(|before| before.key == param.key)
        };
        let others = params
            .iter()
            .enumerate()
            .filter(move |(at, param)| param.key != TEXT && named(*at, param))
            .map(|(_, param)| *param);
        let (count, content) = others
            .clone()
            .fold((0_usize, 0_usize), |(count, bytes), param| {
                (count + 1, bytes + param.key.len() + param.value.len())
            });
        if count > PARAMS_MAX || content > PARAM_BYTES_MAX {
            return Err(ERR_NO_SPACE);
        }
        let text = params
            .iter()
            .find(|param| param.key == TEXT)
            .map_or("", |param| param.value);
        if text.len() > TEXT_MAX {
            return Err(ERR_NO_SPACE);
        }
        let as_param = !text.is_empty()
            && count < PARAMS_MAX
            && content + TEXT.len() + text.len() <= PARAM_BYTES_MAX;
        let text_param = as_param.then_some(Param {
            key: TEXT,
            value: text,
        });
        let id = self.insert(
            others.chain(text_param),
            State::Claimed {
                owner: HOST_OWNER,
                batch: u64::MAX,
            },
            // The room of the host's own, as a dialog's.
            (HOST_ROOM, HOST_BYTES),
        )?;
        if let Err(error) = self.host_text(id, text.as_bytes()) {
            let mut inner = self.inner()?;
            if let Some(at) = Self::position(&inner, id) {
                Self::free(&mut inner, at);
            }
            return Err(error);
        }
        Ok(id)
    }

    /// The host's own text of its hand-over `id`, in parts as op 9 writes
    /// them, committed (nothing for none).
    fn host_text(&self, id: u32, text: &[u8]) -> Result<(), i32> {
        let mut offset = 0;
        for part in text.chunks(PART_MAX) {
            let mut inner = self.inner()?;
            let at = Self::position(&inner, id).ok_or(ERR_NOT_FOUND)?;
            let slots_free = Self::text_slots(&inner) < self.limits.texts;
            let Inner {
                entries,
                text_memory,
                ..
            } = &mut *inner;
            write_part(
                &mut entries[at].text,
                text_memory,
                &WriteRules {
                    slots_free,
                    memory_cap: self.limits.text_memory,
                    text_cap: TEXT_MAX,
                },
                offset,
                part,
                offset + part.len() == text.len(),
            )?;
            offset += part.len();
        }
        Ok(())
    }

    /// Keeps a hand-over of `params` (checked) in `state`, `room` (hand-overs,
    /// bytes) beyond the limits allowed.
    fn insert<'p>(
        &self,
        params: impl Iterator<Item = Param<'p>> + Clone,
        state: State,
        room: (usize, usize),
    ) -> Result<u32, i32> {
        // Encode once into a block of exactly the size.
        let mut framed = 0_usize;
        let mut count = 0_u32;
        for param in params.clone() {
            count += 1;
            framed += codec::framed_len(&Record::ServiceUiParam(wire::ServiceUiParam {
                key: param.key,
                value: param.value,
            }))?;
        }
        let mut inner = self.inner()?;
        if inner.params_bytes + framed > self.limits.params_bytes + room.1
            || inner.entries.len() >= self.limits.handovers + room.0
        {
            return Err(ERR_FULL);
        }
        let mut block = Vec::new();
        block.try_reserve_exact(framed).map_err(|_| ERR_NO_MEMORY)?;
        block.resize(framed, 0);
        let mut block = Wiped(block);
        let mut used = 0;
        for param in params {
            let record = Record::ServiceUiParam(wire::ServiceUiParam {
                key: param.key,
                value: param.value,
            });
            used += codec::encode_one(&record, &mut block.0[used..])?;
        }
        inner.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let id = self.alloc_id()?;
        inner.params_bytes += framed;
        inner.entries.push(Entry {
            id,
            state,
            params: block,
            count,
            text: Text::default(),
        });
        Ok(id)
    }

    fn position(inner: &Inner, id: u32) -> Option<usize> {
        inner.entries.iter().position(|entry| entry.id == id)
    }

    /// Op 9 on a hand-over: `data` at `offset` of its text, by its maker in the
    /// callback that made it. An offset 0 always starts the text over; any
    /// other must be the bytes written so far (`ERR_INVALID`). `last` commits.
    /// A failed part leaves the text uncommitted at the parts before it.
    pub(crate) fn text_write(
        &self,
        who: Stamp,
        id: u32,
        offset: usize,
        data: &[u8],
        last: bool,
    ) -> Result<(), i32> {
        if data.len() > PART_MAX {
            return Err(ERR_NO_SPACE);
        }
        let mut inner = self.inner()?;
        let at = Self::position(&inner, id).ok_or(ERR_NOT_FOUND)?;
        match inner.entries[at].state {
            State::Made { owner, epoch } if owner == who.owner => {
                if epoch != who.epoch {
                    return Err(ERR_STATE);
                }
            }
            State::Made { .. } => return Err(ERR_NOT_FOUND),
            _ => return Err(ERR_STATE),
        }
        let slots_free = Self::text_slots(&inner) < self.limits.texts;
        let Inner {
            entries,
            text_memory,
            ..
        } = &mut *inner;
        write_part(
            &mut entries[at].text,
            text_memory,
            &WriteRules {
                slots_free,
                memory_cap: self.limits.text_memory,
                text_cap: TEXT_MAX,
            },
            offset,
            data,
            last,
        )
    }

    /// Texts alive now (hand-overs and requests): one slot per text with bytes
    /// or a commit.
    pub(super) fn text_slots(inner: &Inner) -> usize {
        let held = |text: &Text| text.bytes > 0 || text.committed;
        inner.entries.iter().filter(|e| held(&e.text)).count()
            + inner.requests.iter().filter(|r| held(&r.text)).count()
    }

    /// While `command.open` is checked: the id exists, `who` made it in this
    /// callback, no submit claimed it and its text, if begun, is committed.
    /// Changes nothing. `ERR_NOT_FOUND` for an id that is not `who`'s, `ERR_STATE`
    /// for the rest.
    pub(crate) fn handover_check(&self, who: Stamp, id: u32) -> Result<(), i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let entry = Self::position(&inner, id)
            .map(|at| &inner.entries[at])
            .ok_or(ERR_NOT_FOUND)?;
        match entry.state {
            State::Made { owner, epoch } if owner == who.owner && epoch == who.epoch => {
                if entry.text.bytes > 0 && !entry.text.committed {
                    return Err(ERR_STATE);
                }
                Ok(())
            }
            State::Made { owner, .. } | State::Claimed { owner, .. } if owner == who.owner => {
                Err(ERR_STATE)
            }
            _ => Err(ERR_NOT_FOUND),
        }
    }

    /// Once the whole submit batch is checked and every fallible reservation of
    /// the stage is done: every id **made** to **claimed**, all or none, with no
    /// allocation. An id twice (in the batch or claimed by an earlier accepted
    /// submit) is `ERR_STATE` and nothing is claimed. The instance's own
    /// hand-over is not touched here (that is [`Self::attach`]).
    pub(crate) fn claim(&self, who: Stamp, ids: &[u32]) -> Result<Claims, i32> {
        for (index, id) in ids.iter().enumerate() {
            if ids[..index].contains(id) {
                return Err(ERR_STATE);
            }
            self.handover_check(who, *id)?;
        }
        let mut inner = self.inner()?;
        let batch = self.batches.get();
        self.batches.set(batch.checked_add(1).ok_or(ERR_FULL)?);
        for id in ids {
            let at = Self::position(&inner, *id).ok_or(ERR_NOT_FOUND)?;
            inner.entries[at].state = State::Claimed {
                owner: who.owner,
                batch,
            };
        }
        Ok(Claims {
            owner: who.owner,
            batch,
        })
    }

    /// A refused submit or a callback whose result drops its staged commands:
    /// everything of exactly this batch still claimed is freed. Other batches
    /// stay.
    ///
    /// # Errors
    /// `ERR_BUSY` while the store is borrowed: the claims are handed back
    /// unchanged to roll back again, never dropped.
    pub(crate) fn rollback(&self, claims: Claims) -> Result<(), (i32, Claims)> {
        let Ok(mut inner) = self.inner() else {
            return Err((ERR_BUSY, claims));
        };
        let mut at = 0;
        while at < inner.entries.len() {
            let ours = matches!(
                inner.entries[at].state,
                State::Claimed { owner, batch } if owner == claims.owner && batch == claims.batch
            );
            if ours {
                Self::free(&mut inner, at);
            } else {
                at += 1;
            }
        }
        Ok(())
    }

    /// An `open` that applies as a no-op (a guard refused), names a stale
    /// target or fails: that one **claimed** hand-over is freed. The instance's
    /// old hand-over stays.
    pub(crate) fn release(&self, id: u32) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        if let Some(at) = Self::position(&inner, id)
            && matches!(inner.entries[at].state, State::Claimed { .. })
        {
            Self::free(&mut inner, at);
        }
        Ok(())
    }

    /// The `open` applies: **claimed** to **owned** by `instance`, allocation
    /// free; the instance's previous hand-over is freed only after that, so a
    /// failure before leaves the old params and text. `None` (an `open` with
    /// hand-over 0 that reuses an instance) clears its params and text here.
    ///
    /// # Errors
    /// `ERR_STATE` when `id` is not claimed (nothing changes).
    pub(crate) fn attach(&self, id: Option<u32>, instance: u32) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let new = match id {
            Some(id) => {
                let at = Self::position(&inner, id).ok_or(ERR_STATE)?;
                let State::Claimed { owner, .. } = inner.entries[at].state else {
                    return Err(ERR_STATE);
                };
                Some((at, owner))
            }
            None => None,
        };
        let old = inner.entries.iter().position(
            |entry| matches!(entry.state, State::Owned { instance: i, .. } if i == instance),
        );
        if let Some((at, maker)) = new {
            inner.entries[at].state = State::Owned { instance, maker };
        }
        if let Some(old) = old {
            Self::free(&mut inner, old);
        }
        Ok(())
    }

    /// The callback of `who` returned: what it made and no submit claimed is
    /// freed. Claimed ones stay until their `open` drains.
    pub(crate) fn end_callback(&self, who: Stamp) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        let mut at = 0;
        while at < inner.entries.len() {
            let ours = matches!(
                inner.entries[at].state,
                State::Made { owner, epoch } if owner == who.owner && epoch == who.epoch
            );
            if ours {
                Self::free(&mut inner, at);
            } else {
                at += 1;
            }
        }
        Ok(())
    }

    /// Find the next retained instance, including request ties and dialogs.
    /// The kernel compares these ids against its actual stack during recovery.
    pub(crate) fn next_retained_instance(&self, after: u32) -> Result<Option<u32>, i32> {
        let inner = self.inner_busy()?;
        Ok(inner
            .entries
            .iter()
            .filter_map(|entry| match entry.state {
                State::Owned { instance, .. } => Some(instance),
                _ => None,
            })
            .chain(
                inner
                    .requests
                    .iter()
                    .flat_map(|request| request.instances())
                    .flatten(),
            )
            .filter(|instance| *instance > after)
            .min())
    }

    /// An instance closed: what it owns is freed.
    pub(crate) fn instance_closed(&self, instance: u32) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        let owned = inner.entries.iter().position(
            |entry| matches!(entry.state, State::Owned { instance: i, .. } if i == instance),
        );
        if let Some(at) = owned {
            Self::free(&mut inner, at);
        }
        Ok(())
    }

    /// A plugin stopped: what it made or has claimed is freed; what it handed
    /// to another live instance stays with that instance. No plugin is called.
    ///
    /// # Errors
    /// `ERR_BUSY` while the store is borrowed: nothing was freed and the caller
    /// retries (the plugin's retirement waits).
    pub(crate) fn owner_stopped(&self, owner: u64) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        Self::requests_owner_stopped(&mut inner, owner);
        let mut at = 0;
        while at < inner.entries.len() {
            let theirs = match inner.entries[at].state {
                State::Made { owner: o, .. } | State::Claimed { owner: o, .. } => o == owner,
                State::Owned { .. } => false,
            };
            if theirs {
                Self::free(&mut inner, at);
            } else {
                at += 1;
            }
        }
        Ok(())
    }

    fn free(inner: &mut Inner, at: usize) {
        let entry = inner.entries.remove(at);
        inner.params_bytes -= entry.params.0.len();
        inner.text_memory -= entry.text.charged.min(inner.text_memory);
    }

    /// Whether `made` (a hand-over of an `open` being applied, or none) has
    /// the params `instance` was given: the compare of `open` modes 1 and 3 and
    /// of instances per params (`ui.md` 1.1). Texts are not compared.
    pub(crate) fn same_params(&self, made: Option<u32>, instance: u32) -> bool {
        let Ok(inner) = self.inner.try_borrow() else {
            return false;
        };
        let framed = |id: Option<u32>, owned: bool| -> (&[u8], u32) {
            inner
                .entries
                .iter()
                .find(|entry| match id {
                    Some(id) => entry.id == id,
                    None => {
                        owned
                            && matches!(entry.state, State::Owned { instance: i, .. } if i == instance)
                    }
                })
                .map_or((&[][..], 0), |entry| (&entry.params.0[..], entry.count))
        };
        let ((new, new_count), (old, old_count)) = (framed(made, false), framed(None, true));
        // The same keys with the same values, in any order (keys are unique).
        new_count == old_count && params_in(new, |key, value| value_of(old, key) == Some(value))
    }

    /// What the opened event says about `instance`'s hand-over, if it has one.
    pub(crate) fn info(&self, instance: u32) -> Option<Info> {
        let inner = self.inner.try_borrow().ok()?;
        inner
            .entries
            .iter()
            .find(|entry| matches!(entry.state, State::Owned { instance: i, .. } if i == instance))
            .map(|entry| Info {
                handover: entry.id,
                params: entry.count,
                text_bytes: u32::try_from(entry.text.bytes).unwrap_or(u32::MAX),
            })
    }

    /// The one file `instance`'s hand-over names: its params `root` (`0` or
    /// `2`) and `path`, and the plugin that made it, for root 5 of
    /// `doc_open` (`document.md` 2.1). Nothing is copied: `visit` sees them
    /// in place and must not reenter the store; the store stays borrowed for
    /// the length of `visit`, so a caller copies the path out and does its
    /// card I/O afterwards. `root` is exactly `"0"` or `"2"`: any other text
    /// (`"02"`, `" 2"`, `"+2"`) is not a root.
    ///
    /// # Errors
    /// `ERR_DENIED` for an instance without a hand-over, a hand-over without
    /// both params, a root other than `0` or `2`, or a repeated param;
    /// `ERR_STATE` while the store is busy.
    pub(crate) fn handed_file(
        &self,
        instance: u32,
        visit: &mut dyn FnMut(Handed<'_>),
    ) -> Result<(), i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let entry = inner
            .entries
            .iter()
            .find(|entry| matches!(entry.state, State::Owned { instance: i, .. } if i == instance))
            .ok_or(ERR_DENIED)?;
        let State::Owned { maker, .. } = entry.state else {
            return Err(ERR_DENIED);
        };
        let (mut root, mut path) = (None, None);
        let mut rest = &entry.params.0[..];
        while !rest.is_empty() {
            let Ok((param, used)) = codec::prefix::<wire::ServiceUiParam<'_>>(rest) else {
                return Err(ERR_DENIED);
            };
            let slot = match param.key {
                "root" => &mut root,
                "path" => &mut path,
                _ => {
                    rest = &rest[used..];
                    continue;
                }
            };
            if slot.replace(param.value).is_some() {
                return Err(ERR_DENIED);
            }
            rest = &rest[used..];
        }
        let root = match root {
            Some("0") => 0,
            Some("2") => 2,
            _ => return Err(ERR_DENIED),
        };
        let path = path.ok_or(ERR_DENIED)?;
        visit(Handed { maker, root, path });
        Ok(())
    }

    /// Op 17: the params of `instance`'s hand-over as `ui_param` records into
    /// `out`; the instance must own it (`ERR_STATE` when `handover` is not the
    /// instance's). Returns the bytes written.
    pub(crate) fn params(
        &self,
        instance: u32,
        handover: u32,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let entry = Self::owned(&inner, instance, handover)?;
        let bytes = &entry.params.0;
        out.get_mut(..bytes.len())
            .ok_or(ERR_NO_SPACE)?
            .copy_from_slice(bytes);
        Ok(bytes.len())
    }

    fn owned(inner: &Inner, instance: u32, handover: u32) -> Result<&Entry, i32> {
        inner
            .entries
            .iter()
            .find(|entry| entry.id == handover)
            .filter(
                |entry| matches!(entry.state, State::Owned { instance: i, .. } if i == instance),
            )
            .ok_or(ERR_STATE)
    }

    /// Op 10 on a hand-over: reads from `offset` into `out`, cut only at a UTF-8
    /// boundary; returns the text's total and the bytes written. Only the
    /// owning instance reads, and only a committed text.
    pub(crate) fn text_read(
        &self,
        instance: u32,
        handover: u32,
        offset: usize,
        out: &mut [u8],
    ) -> Result<(usize, usize), i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let text = &Self::owned(&inner, instance, handover)?.text;
        if !text.committed && text.bytes > 0 {
            return Err(ERR_STATE);
        }
        Self::read_text(text, offset, out)
    }

    /// Reads `text` from `offset` into `out`, cut only at a UTF-8 boundary;
    /// returns the text's total and the bytes written.
    pub(super) fn read_text(
        text: &Text,
        offset: usize,
        out: &mut [u8],
    ) -> Result<(usize, usize), i32> {
        let total = text.bytes;
        if offset > total {
            return Err(ERR_INVALID);
        }
        let want = out.len().min(total - offset);
        let byte_at = |position: usize| -> Option<u8> {
            let mut skip = position;
            for part in &text.parts {
                if skip < part.0.len() {
                    return part.0.get(skip).copied();
                }
                skip -= part.0.len();
            }
            None
        };
        let mut cut = want;
        while cut > 0 && byte_at(offset + cut).is_some_and(|b| b & 0xC0 == 0x80) {
            cut -= 1;
        }
        if cut == 0 && want > 0 {
            return Err(ERR_NO_SPACE);
        }
        let (mut written, mut skip) = (0, offset);
        for part in &text.parts {
            if skip >= part.0.len() {
                skip -= part.0.len();
                continue;
            }
            let take = (part.0.len() - skip).min(cut - written);
            out[written..written + take].copy_from_slice(&part.0[skip..skip + take]);
            written += take;
            skip = 0;
            if written == cut {
                break;
            }
        }
        Ok((total, written))
    }

    /// Bytes of heap held now (params and text), for the budget tests.
    #[cfg(test)]
    pub(crate) fn held(&self) -> (usize, usize) {
        let inner = self.inner.borrow();
        (inner.params_bytes, inner.text_memory)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.inner.borrow().entries.len()
    }
}

#[cfg(test)]
#[path = "ui_store_tests.rs"]
mod tests;
