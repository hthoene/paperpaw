//! Writes of `core.library`: ops 8-12, 14, 17-21, 23, 24, 26.
//!
//! The service owns the contract's rules (record order and counts, flag bits,
//! names, the position policy, the transaction lifecycle and its caps); the
//! host owns the catalog and the card. Every `LibWrite` method has a default
//! that refuses with `ERR_UNSUPPORTED`, so a host or a test fake implements
//! only what it backs.
use super::{
    BookRef, Caller, ServiceOp, args, encode,
    wire::{self, Record},
};
use crate::abi7::context::RowsEnd;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED};

mod edit;
mod files;
mod position;
pub(crate) use edit::{BookId, CommitRequest, Committed, StageItem, Transactions};
pub(crate) use files::ImportRequest;
pub(crate) use position::StoredPosition;

/// A field the plugin declared (`book_field.spec`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldKind {
    /// A short value (at most 64 bytes) kept in the catalog.
    pub(crate) in_catalog: bool,
}

/// Status, files, positions, staged annotation transactions, fields.
pub(crate) trait LibWrite {
    /// Resolve a named book to its catalog identity for as long as a
    /// transaction lives; `ERR_NOT_FOUND` for an unknown book.
    fn resolve_book(&self, _book: BookRef<'_>) -> Result<BookId, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The book's root-1 path, for the `lib_result` of an op.
    fn book_path(&self, _book: BookRef<'_>, _visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Both masks applied together (bit 0 finished, 1 favourite, 2 opened).
    fn set_status(&self, _book: BookRef<'_>, _set: u32, _clear: u32) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The delete preflight of one path: the status (`OK` deletable,
    /// `ERR_INVALID` not a book, `ERR_NOT_FOUND` gone already, `ERR_IO`) and
    /// the facts (bit 0 has annotations, 1 has a position, 2 has plugins'
    /// book data). It changes nothing.
    fn can_delete(&self, _path: &str) -> (i32, u32) {
        (ERR_UNSUPPORTED, 0)
    }
    /// Remove a book and everything kept for it. The service has applied the
    /// annotation rule already. A book that is gone is only forgotten.
    fn delete(&self, _path: &str) -> (i32, u32) {
        (ERR_UNSUPPORTED, 0)
    }
    /// Move a book and everything kept for it (`replace`: an existing `to`).
    fn move_book(&self, _from: &str, _to: &str, _replace: bool) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The library folder setting: a root-1 path, empty for the root.
    fn library_folder(&self, _visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Put the caller's file into the library; `visit` gets the path taken.
    fn import(
        &self,
        _who: &Caller<'_>,
        _request: &ImportRequest<'_>,
        _visit: &mut dyn FnMut(&str),
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The stored position's provenance, `None` when the book has none.
    fn stored_position(&self, _book: BookRef<'_>) -> Result<Option<StoredPosition>, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Write a position atomically (`from_sync` is stored with it).
    fn write_position(
        &self,
        _book: BookRef<'_>,
        _position: &wire::ServiceLibPosition<'_>,
        _from_sync: bool,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The declared field `field`, `ERR_NOT_FOUND` for an undeclared one.
    fn field_kind(&self, _field: &str) -> Result<FieldKind, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Set a field of the caller (empty `value` removes it).
    fn set_field(&self, _book: BookRef<'_>, _field: &str, _value: &str) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Provided metadata (the fields flagged in `provided.flags`).
    fn provide(
        &self,
        _book: BookRef<'_>,
        _provided: &wire::ServiceLibProvided<'_>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The host's clock in milliseconds (transaction expiry).
    fn now_ms(&self) -> u64 {
        0
    }
    /// The open transactions; `None` where the host has none.
    fn transactions(&self) -> Option<&Transactions> {
        None
    }
    /// Keep a staged item of transaction `transaction` on the card.
    fn stage(&self, _transaction: u32, _item: &StageItem<'_>) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Drop what a transaction staged (abort, expiry, a failed stage, a
    /// failed commit).
    fn abort_staged(&self, _transaction: u32) {}
    /// Apply all the staged edits or none (compare-and-swap on the book's
    /// annotation generation: `ERR_STATE`, `ERR_NOT_FOUND` for an unknown id,
    /// `ERR_INVALID` for a range outside the sections).
    fn commit(&self, _request: &CommitRequest) -> Result<Committed, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The ids of a committed transaction, paged; `ERR_NOT_FOUND` once the
    /// receipt is gone (the caller's next `edit_begin`, 10 minutes, stop).
    fn receipt(
        &self,
        _who: &Caller<'_>,
        _transaction: u32,
        _cursor: u32,
        _visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
}

pub(super) fn run(
    host: &dyn super::LibHost,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    match op.id {
        8 => set_status(host, input),
        9 | 10 => files::paths(host, who, op.id == 10, input, out),
        11 => files::move_book(host, who, input, out),
        12 => files::import(host, who, input, out),
        14 => position::set(host, input, out),
        23 => set_field(host, who, input),
        24 => provide(host, input),
        _ => edit::run(host, who, op, input, out),
    }
}

pub(super) fn require(ok: bool) -> Result<(), i32> {
    if ok { Ok(()) } else { Err(ERR_INVALID) }
}

fn set_status(host: &dyn super::LibHost, input: &[u8]) -> Result<usize, i32> {
    let parsed = args::with_ref::<1>(input)?;
    let book = args::book_ref(host, &parsed.reference)?;
    let [Some(Record::ServiceLibStatus(status))] = parsed.rest else {
        return Err(ERR_INVALID);
    };
    // Bit 0 finished, 1 favourite, 2 opened; a bit in both masks has no meaning.
    require((status.set | status.clear) & !0b111 == 0 && status.set & status.clear == 0)?;
    host.set_status(book, status.set, status.clear)?;
    Ok(0)
}

fn set_field(host: &dyn super::LibHost, who: &Caller<'_>, input: &[u8]) -> Result<usize, i32> {
    let parsed = args::with_ref::<1>(input)?;
    let book = args::book_ref(host, &parsed.reference)?;
    let [Some(Record::ServiceLibFieldSet(set))] = parsed.rest else {
        return Err(ERR_INVALID);
    };
    // `<plugin>.<name>`: a field of the caller's, else `ERR_DENIED`.
    require(set.field.contains('.'))?;
    let name = set
        .field
        .strip_prefix(who.plugin)
        .and_then(|rest| rest.strip_prefix('.'))
        .ok_or(ERR_DENIED)?;
    require(!name.is_empty())?;
    let kind = host.field_kind(set.field)?;
    // A short value of a catalog field; longer is refused, never cut.
    if kind.in_catalog && set.value.len() > 64 {
        return Err(ERR_NO_SPACE);
    }
    host.set_field(book, set.field, set.value)?;
    Ok(0)
}

fn provide(host: &dyn super::LibHost, input: &[u8]) -> Result<usize, i32> {
    let parsed = args::with_ref::<1>(input)?;
    let book = args::book_ref(host, &parsed.reference)?;
    let [Some(Record::ServiceLibProvided(provided))] = parsed.rest else {
        return Err(ERR_INVALID);
    };
    // Bit n: field n is provided (title, author, series, number, tags); a
    // value for a field that is not provided is a mistake, not ignored.
    let flags = provided.flags;
    require(flags & !0b1_1111 == 0)?;
    let fields = [
        provided.title,
        provided.author,
        provided.series,
        provided.number,
        provided.tags,
    ];
    require(
        fields
            .iter()
            .enumerate()
            .all(|(bit, value)| flags & (1 << bit) != 0 || value.is_empty()),
    )?;
    // Tags one per line: at most 32, each 1..=128 bytes.
    if flags & 0b1_0000 != 0 && !provided.tags.is_empty() {
        let mut count = 0;
        for tag in provided.tags.split('\n') {
            count += 1;
            require(!tag.is_empty() && tag.len() <= 128)?;
        }
        if count > 32 {
            return Err(ERR_NO_SPACE);
        }
    }
    host.provide(book, &provided)?;
    Ok(0)
}

/// Ops that remove or move the user's files need `uses.storage` too
/// (`ERR_UNSUPPORTED` when the host has not wired it, `ERR_DENIED` when the
/// plugin did not declare it).
pub(super) fn need_storage(who: &Caller<'_>) -> Result<(), i32> {
    who.storage
}

/// A `lib_result` record for one path.
pub(super) fn result_row(
    path: &str,
    status: i32,
    flags: u32,
    out: &mut [u8],
) -> Result<usize, i32> {
    encode(
        &Record::ServiceLibResult(wire::ServiceLibResult {
            path,
            status,
            flags,
        }),
        out,
    )
}
