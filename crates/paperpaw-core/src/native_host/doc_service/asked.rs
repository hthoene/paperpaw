//! A format that asks before it reads a book (`format.spec.flags` bit 5,
//! `document.md` 1, #242): before it opens a book of its extensions the host
//! asks it (op 1 with `flags` bit 0 `claimed`, root 3 the book, read only).
//! `fmt.yes` takes the book; no answer (or none at all: a format that fails
//! is not the one that reads it) hands it to the next format of the
//! extension, of the highest priority and the first loaded among equals,
//! at most [`PASSES`] times. A format plugin not made yet is asked at depth
//! 0 by the pump (`DocState::ask_waiting`), the open waiting meanwhile as a
//! pending one does. Nothing of it is kept: the next open asks again.
use super::{
    DETAILS, DocHost, Document, FormatCall, FormatRef, Options, args, fmt, pending, table::PASSES,
    wire,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_CANCELLED, ERR_UNSUPPORTED};

/// Which format reads a book, once asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Settled {
    /// This one: it does not ask, or it said yes.
    Reads(FormatRef),
    /// This one is to be asked, and its plugin is not made yet.
    Waits(FormatRef),
    /// Every format of the extension passed it on.
    Nobody,
}

/// Which format reads `document`, from `format` on: each that asks is asked,
/// and the document's row moves to the next on a pass (its flags and
/// conversion those of the next, `open` the open as asked, with its sides).
///
/// # Errors
/// The table's, the conversion key's.
pub(super) fn settle(
    host: &dyn DocHost,
    document: u32,
    mut format: FormatRef,
    open: &wire::ServiceDocOpen<'_>,
    options: Options<'_>,
) -> Result<Settled, i32> {
    for _ in 0..=PASSES {
        if !host.format_asks(format) {
            return Ok(Settled::Reads(format));
        }
        match host.ask_format(document, format) {
            Ok(true) => return Ok(Settled::Reads(format)),
            Err(ERR_BUSY) => return Ok(Settled::Waits(format)),
            Ok(false) | Err(_) => {}
        }
        match pass(host, document, format, open, options)? {
            Some(next) => format = next,
            None => return Ok(Settled::Nobody),
        }
    }
    Ok(Settled::Nobody)
}

/// `format` passed `document` on: the row moves to the next format, if any
/// (none past [`PASSES`]).
fn pass(
    host: &dyn DocHost,
    document: u32,
    format: FormatRef,
    open: &wire::ServiceDocOpen<'_>,
    options: Options<'_>,
) -> Result<Option<FormatRef>, i32> {
    let mut passed = [FormatRef(0); PASSES + 1];
    passed[..PASSES].copy_from_slice(&host.table().passed(document)?);
    passed[PASSES] = format;
    let Some(next) = host.next_format(document, &passed) else {
        return Ok(None);
    };
    // As the open does: `details` is for a `prepared` format's book only.
    let flags = if host.format_prepared(next) {
        open.flags
    } else {
        open.flags & !DETAILS
    };
    let conversion =
        host.conversion_key(&wire::ServiceDocOpen { flags, ..*open }, next, options)?;
    Ok(host
        .table()
        .retarget(document, next, flags, conversion)
        .ok()
        .map(|()| next))
}

/// The open's call of the format that reads `document` now (its row).
fn call_of<'a>(
    document: &Document,
    open: &wire::ServiceDocOpen<'a>,
    options: Options<'a>,
) -> FormatCall<'a> {
    let mut call = FormatCall::new(document.format, fmt::OPEN, document.id);
    call.flags = document.flags;
    call.image_side = document.image_side;
    call.cover_side = document.cover_side;
    call.conversion = document.conversion;
    call.options = options;
    call.css_root = open.css_root;
    call.css_path = open.css_path;
    call
}

/// The open (op 1) of `document` from `format` on, the format asked first
/// where it asks: the open's call as before, a wait for a format not made
/// yet (the cache made, the batch kept), or a failed document where nobody
/// reads it (`ERR_UNSUPPORTED`).
///
/// # Errors
/// As [`pending::open`].
pub(super) fn open(
    host: &dyn DocHost,
    document: u32,
    format: FormatRef,
    open: &wire::ServiceDocOpen<'_>,
    batch: &[u8],
    out: &mut [u8],
) -> Result<(), i32> {
    let options = Options { batch };
    let settled = settle(host, document, format, open, options)?;
    let table = host.table();
    let row = table.lease(host.owner(), document)?;
    let call = call_of(&row, open, options);
    match settled {
        Settled::Reads(_) => pending::open(host, &call, batch, out),
        Settled::Waits(_) => {
            host.open_cache(&call)?;
            host.keep_open(document, batch)?;
            table.set_open_pending(document, call.conversion)?;
            table.set_asking(document, true)
        }
        Settled::Nobody => table.set_state(
            document,
            super::FAILED,
            0,
            ERR_UNSUPPORTED,
            0,
            call.conversion,
        ),
    }
}

/// The pump's step for `document` whose format passed it on when asked at
/// depth 0 (`batch` the open's kept batch, `css` the card path of its CSS
/// file): the next format is settled and its cache made; the open is sent by
/// the preparing loop as a pending one. `Ok(false)` while the next one's
/// plugin is not made yet.
///
/// # Errors
/// `ERR_UNSUPPORTED` where nobody reads it; the cache's, the table's.
pub(crate) fn passed_on(
    host: &dyn DocHost,
    document: &Document,
    batch: &[u8],
    css: &str,
) -> Result<bool, i32> {
    let mut open = args::parse_open(batch)?.open;
    open.image_side = document.image_side;
    open.cover_side = document.cover_side;
    let options = Options { batch };
    let next = pass(host, document.id, document.format, &open, options)?.ok_or(ERR_UNSUPPORTED)?;
    let settled = settle(host, document.id, next, &open, options)?;
    if settled == Settled::Nobody {
        return Err(ERR_UNSUPPORTED);
    }
    let table = host.table();
    let row = table
        .lease(document.owner, document.id)
        .map_err(|_| ERR_CANCELLED)?;
    // The CSS file as the open named it, on the card.
    let css_open = wire::ServiceDocOpen {
        css_root: 1,
        css_path: css,
        ..open
    };
    host.open_cache(&call_of(&row, &css_open, options))?;
    let reads = matches!(settled, Settled::Reads(_));
    table.set_asking(document.id, !reads)?;
    Ok(reads)
}
