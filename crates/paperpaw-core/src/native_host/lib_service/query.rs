//! Reads of `core.library`: ops 1-7, 13, 15, 16, 22, 25.
//!
//! `run` decodes and validates the input (record order, value sets, bit
//! sets, the paging rules), asks the [`LibQuery`] with typed arguments, checks
//! what the host answers (`check`) and builds the reply in the scratch the
//! dispatcher lends (`page`): nothing is allocated, no call keeps anything.
//! An unknown book is the host's `ERR_NOT_FOUND`, a stale generation its
//! `ERR_STATE`; both pass through.
use super::{BookRef, Caller, ServiceOp, args};
use crate::abi7::context::RowsEnd;
use crate::native_host::lib_cover::CoverCx;
use crate::native_host::registrations::{self as wire, Record};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED};

mod check;
mod page;

/// A visitor of one record the host answers with.
pub(crate) type One<'a> = &'a mut dyn FnMut(Record<'_>);
/// A visitor of the rows of a page; `Break` ends the page (it is full).
pub(crate) type Rows<'a> = &'a mut dyn FnMut(Record<'_>) -> ControlFlow<()>;

/// The catalog, positions, annotations and fields as the service reads them.
/// Every method has the arguments the service validated; a method that
/// answers one record calls its visitor exactly once, a list calls it once per
/// row in order (stopping at `Break`). Unknown book: `ERR_NOT_FOUND`; a
/// stale generation (a handle's, a page's): `ERR_STATE`. The defaults refuse
/// with `ERR_UNSUPPORTED`, so a host implements what it has.
pub(crate) trait LibQuery {
    /// The screen's long side in pixels (the bound of a cover's `side`).
    fn screen_long_side(&self) -> Result<u32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 1: one `lib_state`.
    fn state(&self, _visit: One<'_>) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 2: `lib_entry` rows of the query from its cursor under its
    /// generation (0: the current one). `total` is the number found; the
    /// generation is the catalog's, never 0.
    fn query(&self, _query: &wire::ServiceLibQuery<'_>, _visit: Rows<'_>) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 3: one `lib_book`.
    fn book(&self, _book: BookRef<'_>, _visit: One<'_>) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 3's lookup for `lib_book.flags` bit 8 `missing`: whether the book's
    /// file (`path`, a library path, already checked) is on the card now. A
    /// card failure is `ERR_IO`, as every op that reads the card.
    fn book_file_present(&self, _path: &str) -> Result<bool, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 4: one `lib_text` of `at.kind` (0-6), `at.index` (0 unless the kind
    /// is a list: tag, field, annotation text or note) from `at.offset`.
    /// `ERR_NOT_FOUND` for an index beyond the list.
    fn text(
        &self,
        _book: BookRef<'_>,
        _at: &wire::ServiceLibTextAt,
        _visit: One<'_>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 5: `lib_facet` rows of `query.facet` (1-4 bounded), the other fields
    /// filter; the paging of op 2.
    fn facets(&self, _query: &wire::ServiceLibQuery<'_>, _visit: Rows<'_>) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 6: the `lib_entry` after the book in its series, `ERR_NOT_FOUND`
    /// for none.
    fn next_in_series(&self, _book: BookRef<'_>, _visit: One<'_>) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 7: the `doc_image` of the book's cover for `who`, `side` 0 for the
    /// list's size, else at most the screen's long side. `ERR_NOT_FOUND` for
    /// none, `ERR_BUSY` while it is made. `cx` is the call's frame and
    /// services: the format that makes it is called through them.
    fn cover(
        &self,
        _who: &Caller<'_>,
        _book: BookRef<'_>,
        _side: u32,
        _cx: &CoverCx<'_>,
        _visit: One<'_>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 13: one `lib_position`, `ERR_NOT_FOUND` for none stored.
    fn position(&self, _book: BookRef<'_>, _visit: One<'_>) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 15: `lib_annotation` rows in reading order. `cursor` counts
    /// annotations; `generation` is the book's annotation generation.
    fn annotations(
        &self,
        _book: BookRef<'_>,
        _cursor: u32,
        _generation: u32,
        _visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 16: `lib_anchor` rows of the annotations that have anchors, in
    /// reading order. `cursor` and `total` count anchors, `generation` is the
    /// book's annotation generation.
    fn anchors(
        &self,
        _book: BookRef<'_>,
        _cursor: u32,
        _generation: u32,
        _visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 22: `lib_field` rows of the book, all plugins', ordered by owning
    /// plugin id then field name. `cursor` counts fields; `generation` is the
    /// book's field generation (every `set_field` and every move of its data
    /// advances it), never 0 in the answer. The host keeps the same order on
    /// every page of one generation: the service checks a page's rows, not
    /// the order across pages.
    fn fields(
        &self,
        _book: BookRef<'_>,
        _cursor: u32,
        _generation: u32,
        _visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// Op 25: one `lib_key` (the book key and the fingerprint).
    fn book_key(&self, _book: BookRef<'_>, _visit: One<'_>) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
}

mod op {
    pub(super) const STATE: u32 = 1;
    pub(super) const QUERY: u32 = 2;
    pub(super) const BOOK: u32 = 3;
    pub(super) const TEXT: u32 = 4;
    pub(super) const FACETS: u32 = 5;
    pub(super) const NEXT_IN_SERIES: u32 = 6;
    pub(super) const COVER: u32 = 7;
    pub(super) const POSITION: u32 = 13;
    pub(super) const ANNOTATIONS: u32 = 15;
    pub(super) const ANCHORS: u32 = 16;
    pub(super) const FIELDS: u32 = 22;
    pub(super) const BOOK_KEY: u32 = 25;
}

pub(super) fn run(
    host: &dyn super::LibHost,
    who: &Caller<'_>,
    cx: &CoverCx<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let rows = usize::from(op.reply_rows);
    match op.id {
        op::STATE => state(host, input, out),
        op::QUERY | op::FACETS => catalog(host, op.id, rows, input, out),
        op::BOOK | op::NEXT_IN_SERIES | op::POSITION | op::BOOK_KEY => {
            book_alone(host, op.id, input, out)
        }
        op::TEXT => text(host, input, out),
        op::COVER => cover(host, who, cx, input, out),
        op::ANNOTATIONS | op::ANCHORS => annotated(host, op.id, rows, input, out),
        op::FIELDS => fields(host, rows, input, out),
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn require(ok: bool) -> Result<(), i32> {
    if ok { Ok(()) } else { Err(ERR_INVALID) }
}

/// A non-zero cursor needs a generation, as REG pages.
fn page_state(cursor: u32, generation: u32) -> Result<(), i32> {
    if cursor != 0 && generation == 0 {
        Err(ERR_STATE)
    } else {
        Ok(())
    }
}

/// `lib_ref` first and exactly `own` more records (0 or 1) of the op's own.
/// A missing, repeated or out-of-order record is `ERR_INVALID`.
fn named<'a>(
    host: &dyn super::LibHost,
    input: &'a [u8],
    own: usize,
) -> Result<(BookRef<'a>, Option<Record<'a>>), i32> {
    let parsed = args::with_ref::<2>(input)?;
    require(parsed.count == own)?;
    Ok((args::book_ref(host, &parsed.reference)?, parsed.rest[0]))
}

/// Op 1 takes no record.
fn state(host: &dyn super::LibHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    wire::host_records_visit(input, 1, &mut |_| Err(ERR_INVALID))?;
    page::one(
        out,
        |visit| host.state(visit),
        |record| match record {
            Record::ServiceLibState(state) => check::state(state),
            _ => Err(ERR_INVALID),
        },
    )
}

/// The values of a `lib_query`; op 5 alone names a facet. A search over 256
/// bytes is `ERR_NO_SPACE` from the decoder (no field is ever shortened).
fn check_query(query: &wire::ServiceLibQuery<'_>, facets: bool) -> Result<(), i32> {
    require(
        query.show <= 5
            && query.sort <= 4
            && query.flags & !0b11 == 0
            && query.facet <= if facets { 4 } else { 0 },
    )?;
    page_state(query.cursor, query.generation)
}

/// Ops 2 and 5: one `lib_query`.
fn catalog(
    host: &dyn super::LibHost,
    id: u32,
    rows: usize,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let query: wire::ServiceLibQuery<'_> = args::single(input)?;
    let facets = id == op::FACETS;
    check_query(&query, facets)?;
    let request = page::Request {
        cursor: query.cursor,
        generation: query.generation,
    };
    if facets {
        page::list(out, rows, check::Row::Facet, request, |visit| {
            host.facets(&query, visit)
        })
    } else {
        page::list(out, rows, check::Row::Entry, request, |visit| {
            host.query(&query, visit)
        })
    }
}

/// Ops 3, 6, 13, 25: `lib_ref` alone.
fn book_alone(
    host: &dyn super::LibHost,
    id: u32,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let (book, _) = named(host, input, 0)?;
    match id {
        op::BOOK => lib_book(host, book, out),
        op::NEXT_IN_SERIES => page::one(
            out,
            |visit| host.next_in_series(book, visit),
            |record| match record {
                // The next book is another one.
                Record::ServiceLibEntry(row) => {
                    check::entry(row)?;
                    require(!matches!(book, BookRef::Key { key, .. } if key == row.key))
                }
                _ => Err(ERR_INVALID),
            },
        ),
        op::POSITION => page::one(
            out,
            |visit| host.position(book, visit),
            |record| match record {
                Record::ServiceLibPosition(row) => check::position(row),
                _ => Err(ERR_INVALID),
            },
        ),
        _ => page::one(
            out,
            |visit| host.book_key(book, visit),
            |record| match record {
                Record::ServiceLibKey(row) => check::key(row),
                _ => Err(ERR_INVALID),
            },
        ),
    }
}

/// Op 4: kinds 0 title, 1 author, 2 series are whole texts; 3 tag, 4 field, 5
/// annotation text, 6 annotation note are lists, read by `index`.
fn text(host: &dyn super::LibHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let (book, Some(Record::ServiceLibTextAt(at))) = named(host, input, 1)? else {
        return Err(ERR_INVALID);
    };
    require(at.kind <= 6)?;
    if at.kind < 3 && at.index != 0 {
        return Err(ERR_NOT_FOUND);
    }
    page::one(
        out,
        |visit| host.text(book, &at, visit),
        |record| match record {
            Record::ServiceLibText(row) => check::text(row, at.kind, at.index, at.offset),
            _ => Err(ERR_INVALID),
        },
    )
}

/// Op 7: `side` 0 for the list's size, else at most the screen's long side.
fn cover(
    host: &dyn super::LibHost,
    who: &Caller<'_>,
    cx: &CoverCx<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let (book, Some(Record::ServiceLibCover(cover))) = named(host, input, 1)? else {
        return Err(ERR_INVALID);
    };
    require(cover.side <= host.screen_long_side()?)?;
    page::one(
        out,
        |visit| host.cover(who, book, cover.side, cx, visit),
        |record| match record {
            Record::ServiceDocImage(image) => check::cover(image),
            _ => Err(ERR_INVALID),
        },
    )
}

/// Ops 15 and 16: `lib_ref`, `lib_page`.
fn annotated(
    host: &dyn super::LibHost,
    id: u32,
    rows: usize,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let (book, Some(Record::ServiceLibPage(ask))) = named(host, input, 1)? else {
        return Err(ERR_INVALID);
    };
    page_state(ask.cursor, ask.annotation_generation)?;
    let request = page::Request {
        cursor: ask.cursor,
        generation: ask.annotation_generation,
    };
    if id == op::ANNOTATIONS {
        page::list(out, rows, check::Row::Annotation, request, |visit| {
            host.annotations(book, ask.cursor, ask.annotation_generation, visit)
        })
    } else {
        page::list(out, rows, check::Row::Anchor, request, |visit| {
            host.anchors(book, ask.cursor, ask.annotation_generation, visit)
        })
    }
}

/// Op 22: `lib_ref`, `lib_field_page`; the field generation pages as the
/// annotation generation does (`ERR_STATE` for a stale one).
fn fields(
    host: &dyn super::LibHost,
    rows: usize,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let (book, Some(Record::ServiceLibFieldPage(ask))) = named(host, input, 1)? else {
        return Err(ERR_INVALID);
    };
    page_state(ask.cursor, ask.generation)?;
    let request = page::Request {
        cursor: ask.cursor,
        generation: ask.generation,
    };
    page::list(out, rows, check::Row::Field, request, |visit| {
        host.fields(book, ask.cursor, ask.generation, visit)
    })
}

/// Bit 8 of `lib_book.flags`: the book's file is not on the card now.
const BOOK_MISSING: u32 = 1 << 8;

/// Op 3: the catalog's `lib_book` with bit 8 `missing` set by the host's own
/// lookup (the catalog does not decide it; a plugin needs no `uses.storage`
/// to tell a gone book).
fn lib_book(host: &dyn super::LibHost, book: BookRef<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let mut failed = None;
    let result = page::one(
        out,
        |visit| {
            host.book(book, &mut |record| match record {
                Record::ServiceLibBook(mut row) => match host.book_file_present(row.path) {
                    Ok(present) => {
                        row.flags &= !BOOK_MISSING;
                        if !present {
                            row.flags |= BOOK_MISSING;
                        }
                        visit(Record::ServiceLibBook(row));
                    }
                    // The reply is the lookup's failure, not a record.
                    Err(status) => failed = Some(status),
                },
                other => visit(other),
            })
        },
        |record| match record {
            Record::ServiceLibBook(row) => check::book(row, book),
            _ => Err(ERR_INVALID),
        },
    );
    failed.map_or(result, Err)
}
