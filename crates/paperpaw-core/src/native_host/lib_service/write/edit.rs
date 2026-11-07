//! Staged annotation transactions (`library.md` 2.2): ops 17-21 and 26.
//!
//! The table below is the service's whole memory of a transaction: who owns
//! it, which book, the generation the caller read, the staged byte count and
//! the (at most three) edits of the latest `edit_add` that still wait for
//! parts. The edits themselves and their parts go to the host's temp file on
//! the card through `LibWrite::stage`; nothing of them stays in memory.
use super::{
    super::{
        Caller, LibHost, ServiceOp, args, encode,
        wire::{self, Record},
    },
    require,
};
use crate::abi7::context::RowsEnd;
use alloc::vec::Vec;
use core::{
    cell::{Cell, RefCell},
    ops::ControlFlow,
};
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND,
};

/// A transaction that sees no call for this long is aborted.
const IDLE_MS: u64 = 600_000;
/// The annotation file's cap (interface 6, `annotations.rs:45`): what one
/// transaction may stage in all.
pub(crate) const STAGED_CAP: u32 = 1 << 20;
/// A note's cap (interface 6).
const NOTE_CAP: u32 = 3 * 1024;
/// An XPointer's cap (`doc_xpointer`).
const XPOINTER_CAP: u32 = 1024;
/// Edits of one `edit_add`.
const EDITS_PER_CALL: usize = 3;
/// `edit_text.field`: 0 text, 1 note, 2 start XPointer, 3 end XPointer.
const FIELDS: usize = 4;
/// `lib_edit.flags`: bit 0 noted, bit 3 has a conversion, bit 4 XPointers follow.
const EDIT_FLAGS: u32 = 0b1_1001;
const HAS_CONVERSION: u32 = 1 << 3;
const HAS_XPOINTERS: u32 = 1 << 4;
/// Device-wide and per-plugin limits (`ERR_FULL`).
const DEVICE: usize = 2;
const PER_OWNER: usize = 1;

/// The host's identity of a book while a transaction on it lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BookId(pub(crate) u64);

/// What a transaction asks the host to keep on the card.
#[derive(Clone, Copy, Debug)]
pub(crate) enum StageItem<'a> {
    Edit {
        index: u32,
        edit: &'a wire::ServiceLibEdit<'a>,
    },
    Part {
        edit: u32,
        field: u32,
        offset: u32,
        data: &'a str,
        last: bool,
    },
}

/// Apply every staged edit or none.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CommitRequest {
    pub(crate) transaction: u32,
    pub(crate) owner: u64,
    pub(crate) book: BookId,
    /// The annotation generation the caller read at `edit_begin`.
    pub(crate) base_generation: u32,
    pub(crate) edits: u32,
}

/// The outcome of a commit: one bounded record whatever the size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Committed {
    pub(crate) annotation_generation: u32,
    pub(crate) edits: u32,
    pub(crate) added: u32,
}

/// One text, note or XPointer still arriving in parts.
#[derive(Clone, Copy, Debug)]
struct Part {
    /// Whole length for a text or note; 0 (unknown) for an XPointer.
    total: u32,
    next: u32,
}

/// An edit of the latest `edit_add` that waits for parts.
#[derive(Clone, Copy, Debug)]
struct Pending {
    edit: u32,
    fields: [Option<Part>; FIELDS],
}

#[derive(Clone, Copy, Debug)]
struct Transaction {
    id: u32,
    owner: u64,
    book: BookId,
    base_generation: u32,
    touched_ms: u64,
    edits: u32,
    staged: u32,
    pending: [Option<Pending>; EDITS_PER_CALL],
}

/// The open transactions: one per plugin and two device-wide. Allocated once.
#[derive(Debug)]
pub(crate) struct Transactions {
    next_id: Cell<u32>,
    rows: RefCell<Vec<Transaction>>,
}

impl Transactions {
    /// # Errors
    /// `ERR_NO_MEMORY`.
    pub(crate) fn try_new() -> Result<Self, i32> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(DEVICE).map_err(|_| ERR_NO_MEMORY)?;
        Ok(Self {
            next_id: Cell::new(1),
            rows: RefCell::new(rows),
        })
    }

    fn rows(&self) -> Result<core::cell::RefMut<'_, Vec<Transaction>>, i32> {
        self.rows
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)
    }

    /// Drop the transactions idle for 10 minutes; `ended` gets their ids.
    fn expire(&self, now_ms: u64, mut ended: impl FnMut(u32)) -> Result<(), i32> {
        let mut dropped = [0_u32; DEVICE];
        let mut count = 0;
        self.rows()?.retain(|t| {
            let stale = now_ms.saturating_sub(t.touched_ms) > IDLE_MS;
            if stale && let Some(slot) = dropped.get_mut(count) {
                *slot = t.id;
                count += 1;
            }
            !stale
        });
        dropped.iter().take(count).for_each(|id| ended(*id));
        Ok(())
    }

    fn begin(&self, owner: u64, book: BookId, base: u32, now_ms: u64) -> Result<u32, i32> {
        let mut rows = self.rows()?;
        if rows.len() >= DEVICE || rows.iter().filter(|t| t.owner == owner).count() >= PER_OWNER {
            return Err(ERR_FULL);
        }
        let id = self.next_id.get();
        let next = id.checked_add(1).ok_or(ERR_NO_SPACE)?;
        rows.push(Transaction {
            id,
            owner,
            book,
            base_generation: base,
            touched_ms: now_ms,
            edits: 0,
            staged: 0,
            pending: [None; EDITS_PER_CALL],
        });
        self.next_id.set(next);
        Ok(id)
    }

    /// A copy of the caller's transaction, touched now.
    fn lease(&self, owner: u64, id: u32, now_ms: u64) -> Result<Transaction, i32> {
        let mut rows = self.rows()?;
        let found = rows.iter_mut().find(|t| t.id == id).ok_or(ERR_NOT_FOUND)?;
        if found.owner != owner {
            return Err(ERR_DENIED);
        }
        found.touched_ms = now_ms;
        Ok(*found)
    }

    fn store(&self, updated: Transaction) -> Result<(), i32> {
        let mut rows = self.rows()?;
        let slot = rows
            .iter_mut()
            .find(|t| t.id == updated.id)
            .ok_or(ERR_NOT_FOUND)?;
        *slot = updated;
        Ok(())
    }

    fn finish(&self, id: u32) {
        if let Ok(mut rows) = self.rows.try_borrow_mut() {
            rows.retain(|t| t.id != id);
        }
    }

    /// The plugin stopped: its transaction ends; `ended` gets the id.
    pub(crate) fn close_owner(&self, owner: u64, mut ended: impl FnMut(u32)) {
        if let Ok(mut rows) = self.rows.try_borrow_mut() {
            rows.retain(|t| {
                let mine = t.owner == owner;
                if mine {
                    ended(t.id);
                }
                !mine
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn begin_for_test(&self, owner: u64, now_ms: u64) -> u32 {
        self.begin(owner, BookId(1), 1, now_ms).unwrap()
    }
    #[cfg(test)]
    pub(crate) fn open_count(&self) -> usize {
        self.rows.borrow().len()
    }
}

impl Transaction {
    fn complete(&self) -> bool {
        self.pending.iter().all(Option::is_none)
    }
}

pub(super) fn run(
    host: &dyn LibHost,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let table = host
        .transactions()
        .ok_or(paperpaw_native::host_glue::ERR_UNSUPPORTED)?;
    let now = host.now_ms();
    table.expire(now, |id| host.abort_staged(id))?;
    match op.id {
        17 => begin(host, table, who, input, out, now),
        18 => add(host, table, who, input, now),
        19 => text(host, table, who, input, now),
        20 => commit(host, table, who, input, out, now),
        21 => abort(host, table, who, input, now),
        _ => receipt(host, who, input, usize::from(op.reply_rows.max(32)), out),
    }
}

fn reference(input: &[u8]) -> Result<u32, i32> {
    let Record::ServiceLibTransactionRef(reference) = args::single(input)? else {
        return Err(ERR_INVALID);
    };
    Ok(reference.transaction)
}

fn begin(
    host: &dyn LibHost,
    table: &Transactions,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
    now: u64,
) -> Result<usize, i32> {
    let parsed = args::with_ref::<1>(input)?;
    let book = args::book_ref(host, &parsed.reference)?;
    let [Some(Record::ServiceLibEditHead(head))] = parsed.rest else {
        return Err(ERR_INVALID);
    };
    let book_id = host.resolve_book(book)?;
    let id = table.begin(who.owner, book_id, head.annotation_generation, now)?;
    encode(
        &Record::ServiceLibTransaction(wire::ServiceLibTransaction { transaction: id }),
        out,
    )
}

fn abort(
    host: &dyn LibHost,
    table: &Transactions,
    who: &Caller<'_>,
    input: &[u8],
    now: u64,
) -> Result<usize, i32> {
    let id = reference(input)?;
    table.lease(who.owner, id, now)?;
    table.finish(id);
    host.abort_staged(id);
    Ok(0)
}

fn commit(
    host: &dyn LibHost,
    table: &Transactions,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
    now: u64,
) -> Result<usize, i32> {
    let id = reference(input)?;
    let transaction = table.lease(who.owner, id, now)?;
    // Every part must have arrived whole before anything is applied.
    require(transaction.complete())?;
    let request = CommitRequest {
        transaction: id,
        owner: who.owner,
        book: transaction.book,
        base_generation: transaction.base_generation,
        edits: transaction.edits,
    };
    // The transaction ends whatever the host answers: a refused commit means
    // read again, merge, begin again.
    let result = host.commit(&request);
    table.finish(id);
    let committed = match result {
        Ok(committed) => committed,
        Err(error) => {
            host.abort_staged(id);
            return Err(error);
        }
    };
    require(committed.edits == transaction.edits && committed.added <= committed.edits)?;
    encode(
        &Record::ServiceLibCommitted(wire::ServiceLibCommitted {
            transaction: id,
            annotation_generation: committed.annotation_generation,
            edits: committed.edits,
            added: committed.added,
        }),
        out,
    )
}

/// What one `lib_edit` adds to the transaction: the bytes it stages and the
/// parts it still waits for.
type Fields = [Option<Part>; FIELDS];

fn plan(edit: &wire::ServiceLibEdit<'_>) -> Result<(u32, Option<Fields>), i32> {
    require(edit.action <= 3 && edit.flags & !EDIT_FLAGS == 0 && edit.percent <= 100)?;
    // Replace and remove name an annotation; add and upsert-by-key do not.
    require((edit.action == 1 || edit.action == 2) == (edit.id != 0))?;
    require(edit.action != 3 || !edit.client_key.is_empty())?;
    // The conversion is present iff its flag says so (absent and 0 differ).
    require(edit.flags & HAS_CONVERSION != 0 || edit.conversion == 0)?;
    // A highlight runs forward.
    require(
        edit.kind != 1
            || (edit.start_section, edit.start_offset) <= (edit.end_section, edit.end_offset),
    )?;
    // Parts mean the inline text is empty; a remove carries nothing.
    require(edit.text_bytes == 0 || edit.text.is_empty())?;
    require(edit.note_bytes == 0 || edit.note.is_empty())?;
    let xpointers = edit.flags & HAS_XPOINTERS != 0;
    if edit.action == 2 {
        require(edit.text_bytes == 0 && edit.note_bytes == 0 && !xpointers)?;
    }
    if edit.note_bytes > NOTE_CAP {
        return Err(ERR_NO_SPACE);
    }
    let inline = u32::try_from(edit.text.len() + edit.note.len() + edit.client_key.len())
        .map_err(|_| ERR_NO_SPACE)?;
    let staged = inline
        .checked_add(edit.text_bytes)
        .and_then(|sum| sum.checked_add(edit.note_bytes))
        .ok_or(ERR_NO_SPACE)?;
    let parts = [
        (edit.text_bytes > 0).then_some(Part {
            total: edit.text_bytes,
            next: 0,
        }),
        (edit.note_bytes > 0).then_some(Part {
            total: edit.note_bytes,
            next: 0,
        }),
        xpointers.then_some(Part { total: 0, next: 0 }),
        xpointers.then_some(Part { total: 0, next: 0 }),
    ];
    Ok((staged, parts.iter().any(Option::is_some).then_some(parts)))
}

fn add(
    host: &dyn LibHost,
    table: &Transactions,
    who: &Caller<'_>,
    input: &[u8],
    now: u64,
) -> Result<usize, i32> {
    let mut id = None;
    let mut edits = [None; EDITS_PER_CALL];
    let mut count = 0;
    wire::host_records_visit(input, EDITS_PER_CALL + 1, &mut |record| {
        match (id, record) {
            (None, Record::ServiceLibTransactionRef(reference)) => id = Some(reference.transaction),
            (Some(_), Record::ServiceLibEdit(edit)) if count < EDITS_PER_CALL => {
                edits[count] = Some(edit);
                count += 1;
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    let id = id.ok_or(ERR_INVALID)?;
    require(count > 0)?;
    let mut transaction = table.lease(who.owner, id, now)?;
    // The edits before these must have all their parts: parts follow their
    // edit, and only the latest call's edits may still wait for them.
    require(transaction.complete())?;
    let mut planned = [(0_u32, None); EDITS_PER_CALL];
    let mut total = transaction.staged;
    for (slot, edit) in planned.iter_mut().zip(edits.iter().flatten()) {
        *slot = plan(edit)?;
        total = total
            .checked_add(slot.0)
            .filter(|sum| *sum <= STAGED_CAP)
            .ok_or(ERR_NO_SPACE)?;
    }
    for (offset, edit) in edits.iter().flatten().enumerate() {
        let index = transaction.edits;
        let item = StageItem::Edit { index, edit };
        if let Err(error) = host.stage(id, &item) {
            // The temp file now holds some of this call: the transaction ends.
            table.finish(id);
            host.abort_staged(id);
            return Err(error);
        }
        if let Some(parts) = planned[offset].1 {
            transaction.pending[offset] = Some(Pending {
                edit: index,
                fields: parts,
            });
        }
        transaction.edits = index.checked_add(1).ok_or(ERR_NO_SPACE)?;
    }
    transaction.staged = total;
    table.store(transaction)?;
    Ok(0)
}

fn text(
    host: &dyn LibHost,
    table: &Transactions,
    who: &Caller<'_>,
    input: &[u8],
    now: u64,
) -> Result<usize, i32> {
    let Record::ServiceLibEditText(part) = args::single(input)? else {
        return Err(ERR_INVALID);
    };
    let mut transaction = table.lease(who.owner, part.transaction, now)?;
    let last = part.flags & 1 != 0;
    require(part.flags & !1 == 0 && !part.data.is_empty())?;
    let field = usize::try_from(part.field)
        .ok()
        .filter(|f| *f < FIELDS)
        .ok_or(ERR_INVALID)?;
    let length = u32::try_from(part.data.len()).map_err(|_| ERR_NO_SPACE)?;
    let slot = transaction
        .pending
        .iter_mut()
        .find(|p| p.is_some_and(|p| p.edit == part.edit))
        .ok_or(ERR_INVALID)?;
    let Some(pending) = slot else {
        return Err(ERR_INVALID);
    };
    let expected = pending.fields[field].as_mut().ok_or(ERR_INVALID)?;
    // No gap, no overlap: each part starts where the one before ended.
    require(part.offset == expected.next)?;
    let end = expected.next.checked_add(length).ok_or(ERR_NO_SPACE)?;
    // A text or note was counted whole when its edit was added; an XPointer
    // is counted as its parts arrive.
    let mut counted = 0;
    if expected.total == 0 {
        // An XPointer: at most 1,024 bytes, ended by `last`.
        if end > XPOINTER_CAP {
            return Err(ERR_NO_SPACE);
        }
        counted = length;
    } else {
        // A text or note: `last` exactly when the whole length has arrived.
        require(end <= expected.total && last == (end == expected.total))?;
    }
    let staged = transaction
        .staged
        .checked_add(counted)
        .filter(|sum| *sum <= STAGED_CAP)
        .ok_or(ERR_NO_SPACE)?;
    let item = StageItem::Part {
        edit: part.edit,
        field: part.field,
        offset: part.offset,
        data: part.data,
        last,
    };
    if let Err(error) = host.stage(part.transaction, &item) {
        table.finish(part.transaction);
        host.abort_staged(part.transaction);
        return Err(error);
    }
    expected.next = end;
    if last {
        pending.fields[field] = None;
        if pending.fields.iter().all(Option::is_none) {
            *slot = None;
        }
    }
    transaction.staged = staged;
    table.store(transaction)?;
    Ok(0)
}

fn receipt(
    host: &dyn LibHost,
    who: &Caller<'_>,
    input: &[u8],
    rows_cap: usize,
    out: &mut [u8],
) -> Result<usize, i32> {
    let Record::ServiceLibReceiptAsk(ask) = args::single(input)? else {
        return Err(ERR_INVALID);
    };
    rows(out, rows_cap, ask.transaction, |visit| {
        host.receipt(who, ask.transaction, ask.cursor, visit)
    })
}

/// Rows and the closing `end`: whole rows only. A receipt never changes, so
/// its transaction id is the generation.
fn rows(
    out: &mut [u8],
    rows_cap: usize,
    generation: u32,
    run: impl FnOnce(&mut dyn FnMut(Record<'_>) -> ControlFlow<()>) -> Result<RowsEnd, i32>,
) -> Result<usize, i32> {
    let mut end = wire::ServiceEnd {
        flags: 0,
        total: 0,
        generation,
        next: 0,
    };
    let reserved = wire::record_framed_len(&Record::ServiceEnd(end))?;
    let available = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
    let mut used = 0;
    let mut count = 0;
    let mut failure = None;
    let position = run(&mut |record| {
        if count == rows_cap {
            return ControlFlow::Break(());
        }
        match wire::record_encode_one(&record, &mut out[used..available]) {
            Ok(written) => {
                used += written;
                count += 1;
                ControlFlow::Continue(())
            }
            Err(ERR_NO_SPACE) => ControlFlow::Break(()),
            Err(error) => {
                failure = Some(error);
                ControlFlow::Break(())
            }
        }
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    end.total = position.total;
    end.next = position.next;
    if position.next >= position.total {
        end.flags = 1;
    } else if count == 0 {
        return Err(ERR_NO_SPACE);
    }
    Ok(used + wire::record_encode_one(&Record::ServiceEnd(end), &mut out[used..])?)
}
