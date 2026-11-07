//! The host's one store (`UiStore`) of hand-overs, their texts and the checked
//! id allocator (`docs/abi/ui.md` 1.1 "The host's hand-over seam", 8.2).
//!
//! The screen stack (H1) owns commands and their staging and only calls the
//! store (the seam of ui.md 1.1): `make` at op 16, `check` while an `open` is
//! submitted (it marks the id **named**), `claim` when the batch applies the
//! open, `release_callback` when the maker's callback returns, `release_instance`
//! when an instance closes, `owner_stopped` when a plugin stops. Every call
//! completes or leaves the store unchanged; no call runs plugin code and no
//! borrow of the store outlives a call.
//!
//! A hand-over is **made** (by a caller, in one callback epoch), **named** (by
//! an `open` that passed `check`) and then **owned** by the instance the open
//! claimed it for, until the instance closes or a reopen gives it another. Its
//! params are kept encoded as `ui_param` records. Texts are
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
    /// Bytes a live-typing answer may hold of a text that is not secret.
    pub(crate) typing_bytes: usize,
    /// Open requests (not counting a nested one).
    pub(crate) requests: u8,
}

impl Limits {
    /// The X4: params 1,536 bytes, 4 texts, 2 KiB of text in memory.
    pub(crate) const X4: Self = Self {
        params_bytes: 1_536,
        texts: 4,
        text_memory: 2_048,
        typing_bytes: 512,
        requests: 1,
    };
    /// Elsewhere: 16 texts, each in memory.
    pub(crate) const ELSEWHERE: Self = Self {
        params_bytes: 16 * 1_313,
        texts: 16,
        text_memory: usize::MAX,
        typing_bytes: usize::MAX,
        requests: 4,
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

/// One param, as op 16 brought it.
#[derive(Clone, Copy)]
pub(crate) struct Param<'a> {
    pub(crate) key: &'a str,
    pub(crate) value: &'a str,
}

/// What the screen opened event says about an instance's hand-over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Info {
    pub(crate) handover: u32,
    pub(crate) params: u32,
    pub(crate) text_bytes: u32,
}

/// Bytes wiped when they are freed.
pub(super) struct Wiped(pub(super) Vec<u8>);

impl Drop for Wiped {
    fn drop(&mut self) {
        self.0.fill(0);
        core::hint::black_box(&mut self.0);
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

enum State {
    Made { owner: u64, epoch: u64 },
    Named { owner: u64, epoch: u64 },
    Owned { instance: u32 },
}

#[derive(Default)]
pub(super) struct Text {
    pub(super) parts: Vec<Wiped>,
    pub(super) bytes: usize,
    pub(super) committed: bool,
}

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
    pub(super) text_memory: usize,
}

/// The store.
pub(crate) struct UiStore {
    pub(super) limits: Limits,
    next: Cell<u32>,
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
/// at the parts before it.
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
    let dropped = if restart { had } else { 0 };
    if *memory - dropped + data.len() > rules.memory_cap {
        return Err(ERR_IO);
    }
    let part = Wiped::copy(data)?;
    // The room for the part is reserved before anything changes.
    let mut fresh = Vec::new();
    if restart {
        fresh.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    } else {
        text.parts.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    }
    // Nothing fails past this point.
    if restart {
        *text = Text {
            parts: fresh,
            bytes: 0,
            committed: false,
        };
        *memory -= had;
    }
    if !data.is_empty() {
        text.parts.push(part);
    }
    text.bytes += data.len();
    text.committed = last;
    *memory += data.len();
    Ok(())
}

impl UiStore {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            next: Cell::new(1),
            inner: RefCell::new(Inner {
                entries: Vec::new(),
                requests: Vec::new(),
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

    pub(super) fn inner(&self) -> Result<core::cell::RefMut<'_, Inner>, i32> {
        self.inner.try_borrow_mut().map_err(|_| ERR_STATE)
    }

    /// Op 16: a hand-over of `params` made by `who`. `ERR_INVALID` for a form
    /// that is not allowed, `ERR_NO_SPACE` over the byte cap, `ERR_FULL` when
    /// the device's params are full, `ERR_NO_MEMORY`.
    pub(crate) fn make(&self, who: Stamp, params: &[Param<'_>]) -> Result<u32, i32> {
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

    /// While `command.open` is submitted: the id exists, `who` made it in this
    /// callback, no other open of the batch (or an earlier accepted submit)
    /// named it and its text, if begun, is committed. On `Ok` it is **named**;
    /// on any error nothing changed (`ERR_NOT_FOUND` for an id that is not
    /// `who`'s, `ERR_STATE` for the rest).
    pub(crate) fn check(&self, who: Stamp, id: u32) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let at = Self::position(&inner, id).ok_or(ERR_NOT_FOUND)?;
        let entry = &mut inner.entries[at];
        match entry.state {
            State::Made { owner, epoch } if owner == who.owner && epoch == who.epoch => {
                if entry.text.bytes > 0 && !entry.text.committed {
                    return Err(ERR_STATE);
                }
                entry.state = State::Named { owner, epoch };
                Ok(())
            }
            State::Made { owner, .. } | State::Named { owner, .. } if owner == who.owner => {
                Err(ERR_STATE)
            }
            _ => Err(ERR_NOT_FOUND),
        }
    }

    /// A submit that is refused after some of its opens passed `check`: those
    /// ids go back to **made** (a refused batch names nothing). Ids named by an
    /// earlier accepted submit are not in `ids`.
    pub(crate) fn unname(&self, who: Stamp, ids: &[u32]) {
        let Ok(mut inner) = self.inner() else {
            return;
        };
        for id in ids {
            if let Some(at) = Self::position(&inner, *id)
                && matches!(
                    inner.entries[at].state,
                    State::Named { owner, epoch } if owner == who.owner && epoch == who.epoch
                )
            {
                inner.entries[at].state = State::Made {
                    owner: who.owner,
                    epoch: who.epoch,
                };
            }
        }
    }

    /// The batch applies the open: **named** to **owned** by `instance`,
    /// atomically and at most once, allocating nothing. The instance's previous
    /// hand-over is freed only after that (a failure before leaves the old
    /// params and text in place).
    ///
    /// # Errors
    /// `ERR_STATE` when `id` is not named (a host bug; nothing changes).
    pub(crate) fn claim(&self, id: u32, instance: u32) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let at = Self::position(&inner, id).ok_or(ERR_STATE)?;
        if !matches!(inner.entries[at].state, State::Named { .. }) {
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

    /// The maker's callback returned, after its batch applied or was dropped:
    /// every hand-over of `who` still **made** or **named** in that callback is
    /// freed. Owned ones stay with their instance.
    pub(crate) fn release_callback(&self, who: Stamp) {
        let Ok(mut inner) = self.inner() else {
            return;
        };
        let mut at = 0;
        while at < inner.entries.len() {
            let ours = matches!(
                inner.entries[at].state,
                State::Made { owner, epoch } | State::Named { owner, epoch }
                    if owner == who.owner && epoch == who.epoch
            );
            if ours {
                Self::free(&mut inner, at);
            } else {
                at += 1;
            }
        }
    }

    /// An instance closed: what it owns is freed.
    pub(crate) fn release_instance(&self, instance: u32) {
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

    /// A plugin stopped: what it made or named is freed; what it handed
    /// to another live instance stays with that instance. No plugin is called.
    pub(crate) fn owner_stopped(&self, owner: u64) {
        self.requests_owner_stopped(owner);
        let Ok(mut inner) = self.inner() else {
            return;
        };
        let mut at = 0;
        while at < inner.entries.len() {
            let theirs = match inner.entries[at].state {
                State::Made { owner: o, .. } | State::Named { owner: o, .. } => o == owner,
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
