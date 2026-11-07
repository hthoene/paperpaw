//! The host's one store (`UiStore`) of hand-overs, their texts and the checked
//! id allocator (`docs/abi/ui.md` 1.1 "The host's hand-over seam", 8.2).
//!
//! The screen stack (H1) owns commands and their staging and only calls the
//! store: `handover_make` at op 16, `handover_check` while an `open` is checked,
//! `claim` once the whole batch is checked, then `rollback` (a refused submit),
//! or `release` and `attach` as each deferred `open` drains, `end_callback`
//! when the callback returns, `instance_closed` and `owner_stopped` when an
//! instance closes or a plugin stops. Every call completes or leaves
//! the store unchanged; no call runs plugin code and no borrow of the store
//! outlives a call (a [`Claim`] is a move-only token, not a reference).
//!
//! A hand-over is **made** (by a caller, in one callback epoch), **claimed**
//! (named by an accepted `open`, waiting in the stage) and then **owned** by the
//! instance it was attached to, until the instance closes or a reopen gives it
//! another. Its params are kept encoded as `ui_param` records. Texts are
//! written in parts (op 9) by the maker in the callback that made it and read
//! (op 10) only by the owning instance; they are wiped when freed and never in
//! a `Debug` or a log.
use super::registrations::{self as wire, Record};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::fmt;
use paperpaw_native::host_glue::{
    ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};

/// Most params of one hand-over.
pub(crate) const PARAMS_MAX: usize = 32;
/// Most bytes of keys and values of one hand-over.
pub(crate) const PARAM_BYTES_MAX: usize = 1_024;
/// Longest key.
const KEY_MAX: usize = 32;
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
}

impl Limits {
    /// The X4: params 1,536 bytes, 4 texts, 2 KiB of text in memory.
    pub(crate) const X4: Self = Self {
        params_bytes: 1_536,
        texts: 4,
        text_memory: 2_048,
    };
    /// Elsewhere: 16 texts, each in memory.
    pub(crate) const ELSEWHERE: Self = Self {
        params_bytes: 16 * 1_313,
        texts: 16,
        text_memory: usize::MAX,
    };
}

/// Who calls and in which callback. The epoch is the host's counter, bumped
/// at every callback entry; it is never 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    owner: u64,
    epoch: u64,
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

/// One param, as op 16 brought it.
#[derive(Clone, Copy)]
pub(crate) struct Param<'a> {
    pub(crate) key: &'a str,
    pub(crate) value: &'a str,
}

/// The hand-overs one accepted submit named: move-only, kept by the command
/// stage until its deferred `open`s drain. [`UiStore::rollback`] frees all that
/// are still claimed (a refused submit, a callback whose result drops its
/// commands); an `open` ends its own with [`UiStore::attach`] or
/// [`UiStore::release`]. A batch of no ids is fine.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "claims end in attach, release or rollback"]
pub(crate) struct Claims {
    owner: u64,
    batch: u64,
}

/// What the screen opened event says about an instance's hand-over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Info {
    pub(crate) handover: u32,
    pub(crate) params: u32,
    pub(crate) text_bytes: u32,
}

/// Bytes wiped when they are freed.
struct Wiped(Vec<u8>);

impl Drop for Wiped {
    fn drop(&mut self) {
        self.0.fill(0);
        core::hint::black_box(&mut self.0);
    }
}

impl Wiped {
    fn copy(bytes: &[u8]) -> Result<Self, i32> {
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        data.extend_from_slice(bytes);
        Ok(Self(data))
    }
}

enum State {
    Made { owner: u64, epoch: u64 },
    Claimed { owner: u64, batch: u64 },
    Owned { instance: u32 },
}

#[derive(Default)]
struct Text {
    parts: Vec<Wiped>,
    bytes: usize,
    committed: bool,
}

struct Entry {
    id: u32,
    state: State,
    /// The params as `ui_param` records, framed.
    params: Wiped,
    count: u32,
    text: Text,
}

struct Inner {
    entries: Vec<Entry>,
    params_bytes: usize,
    text_memory: usize,
}

/// The store.
pub(crate) struct UiStore {
    limits: Limits,
    next: Cell<u32>,
    batches: Cell<u64>,
    inner: RefCell<Inner>,
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

impl UiStore {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            next: Cell::new(1),
            batches: Cell::new(1),
            inner: RefCell::new(Inner {
                entries: Vec::new(),
                params_bytes: 0,
                text_memory: 0,
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

    fn inner(&self) -> Result<core::cell::RefMut<'_, Inner>, i32> {
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
        // Encode once into a block of exactly the size.
        let mut framed = 0_usize;
        for param in params {
            framed += wire::record_framed_len(&Record::ServiceUiParam(wire::ServiceUiParam {
                key: param.key,
                value: param.value,
            }))?;
        }
        let mut inner = self.inner()?;
        if inner.params_bytes + framed > self.limits.params_bytes {
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
            used += wire::record_encode_one(&record, &mut block.0[used..])?;
        }
        inner.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let id = self.alloc_id()?;
        inner.params_bytes += framed;
        inner.entries.push(Entry {
            id,
            state: State::Made {
                owner: who.owner,
                epoch: who.epoch,
            },
            params: block,
            count: u32::try_from(params.len()).map_err(|_| ERR_INVALID)?,
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
        let had = inner.entries[at].text.bytes;
        let restart = offset == 0;
        if !restart && offset != had {
            return Err(ERR_INVALID);
        }
        let kept = if restart { 0 } else { had };
        if kept + data.len() > TEXT_MAX {
            return Err(ERR_NO_SPACE);
        }
        // A text counts once it has bytes; the slot is the first write's.
        let had_slot = inner.entries[at].text.bytes > 0 || inner.entries[at].text.committed;
        let slots = inner
            .entries
            .iter()
            .filter(|entry| entry.text.bytes > 0 || entry.text.committed)
            .count();
        let needs_slot = !had_slot && (!data.is_empty() || last);
        if needs_slot && slots >= self.limits.texts {
            return Err(ERR_FULL);
        }
        let dropped = if restart { had } else { 0 };
        let memory_after = inner.text_memory - dropped + data.len();
        if memory_after > self.limits.text_memory {
            return Err(ERR_IO);
        }
        let part = Wiped::copy(data)?;
        // The room for the part is reserved before anything changes.
        let mut fresh = Vec::new();
        if restart {
            fresh.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        } else {
            inner.entries[at]
                .text
                .parts
                .try_reserve(1)
                .map_err(|_| ERR_NO_MEMORY)?;
        }
        // Nothing fails past this point.
        if restart {
            inner.entries[at].text = Text {
                parts: fresh,
                bytes: 0,
                committed: false,
            };
            inner.text_memory -= had;
        }
        let text = &mut inner.entries[at].text;
        if !data.is_empty() {
            text.parts.push(part);
        }
        text.bytes += data.len();
        text.committed = last;
        inner.text_memory += data.len();
        Ok(())
    }

    /// While `command.open` is checked: the hand-over exists, `who` made it in
    /// this epoch, nothing claimed it and its text, if begun, is committed.
    /// Changes nothing.
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
            State::Made { owner, .. } if owner == who.owner => Err(ERR_STATE),
            State::Made { .. } => Err(ERR_NOT_FOUND),
            State::Claimed { owner, .. } if owner == who.owner => Err(ERR_STATE),
            State::Claimed { .. } => Err(ERR_NOT_FOUND),
            State::Owned { .. } => Err(ERR_NOT_FOUND),
        }
    }

    /// Once the whole submit batch is checked and every fallible reservation of
    /// the stage is done: every id **made** to **claimed**, all or none, with
    /// no allocation. An id twice (in the batch or claimed by an earlier
    /// accepted submit) is `ERR_STATE` and nothing is claimed.
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
    /// everything of the batch still claimed is freed. Other batches stay.
    pub(crate) fn rollback(&self, claims: Claims) {
        let Ok(mut inner) = self.inner() else {
            return;
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
    }

    /// An `open` that applies as a no-op, names a stale target or fails:
    /// **claimed** to freed. The instance's old hand-over stays.
    pub(crate) fn release(&self, id: u32) {
        let Ok(mut inner) = self.inner() else {
            return;
        };
        if let Some(at) = Self::position(&inner, id)
            && matches!(inner.entries[at].state, State::Claimed { .. })
        {
            Self::free(&mut inner, at);
        }
    }

    /// The `open` applies: **claimed** to **owned** by `instance`, allocation
    /// free. The instance's previous hand-over is freed only after that.
    ///
    /// # Errors
    /// `ERR_STATE` when `id` is not claimed (nothing changes).
    pub(crate) fn attach(&self, id: u32, instance: u32) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let at = Self::position(&inner, id).ok_or(ERR_STATE)?;
        if !matches!(inner.entries[at].state, State::Claimed { .. }) {
            return Err(ERR_STATE);
        }
        let old = inner
            .entries
            .iter()
            .position(|entry| matches!(entry.state, State::Owned { instance: i } if i == instance));
        inner.entries[at].state = State::Owned { instance };
        if let Some(old) = old {
            Self::free(&mut inner, old);
        }
        Ok(())
    }

    /// A successful reopen with hand-over 0: the instance's old params and text
    /// go.
    pub(crate) fn attach_none(&self, instance: u32) {
        self.instance_closed(instance);
    }

    /// The callback of `who` returned: what it made and did not name is freed.
    /// Claimed ones stay until their `open` drains.
    pub(crate) fn end_callback(&self, who: Stamp) {
        let Ok(mut inner) = self.inner() else {
            return;
        };
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
    }

    /// An instance closed: what it owns is freed.
    pub(crate) fn instance_closed(&self, instance: u32) {
        let Ok(mut inner) = self.inner() else {
            return;
        };
        let owned = inner
            .entries
            .iter()
            .position(|entry| matches!(entry.state, State::Owned { instance: i } if i == instance));
        if let Some(at) = owned {
            Self::free(&mut inner, at);
        }
    }

    /// A plugin stopped: what it made or has claimed is freed; what it handed
    /// to another live instance stays with that instance. No plugin is called.
    pub(crate) fn owner_stopped(&self, owner: u64) {
        let Ok(mut inner) = self.inner() else {
            return;
        };
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
    }

    fn free(inner: &mut Inner, at: usize) {
        let entry = inner.entries.remove(at);
        inner.params_bytes -= entry.params.0.len();
        inner.text_memory -= entry.text.bytes.min(inner.text_memory);
    }

    /// What the opened event says about `instance`'s hand-over, if it has one.
    pub(crate) fn info(&self, instance: u32) -> Option<Info> {
        let inner = self.inner.try_borrow().ok()?;
        inner
            .entries
            .iter()
            .find(|entry| matches!(entry.state, State::Owned { instance: i } if i == instance))
            .map(|entry| Info {
                handover: entry.id,
                params: entry.count,
                text_bytes: u32::try_from(entry.text.bytes).unwrap_or(u32::MAX),
            })
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
            .filter(|entry| matches!(entry.state, State::Owned { instance: i } if i == instance))
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
