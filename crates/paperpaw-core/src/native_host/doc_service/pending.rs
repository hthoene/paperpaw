//! An open whose format plugin is not made yet (#248 D2: a format plugin may
//! be parked). The format's call answers `ERR_BUSY` and its start is asked
//! for; the opener is not told "busy": the document is opened preparing
//! (`document.md` 2.1, the reader's rules for a preparing book, #1214) with
//! its `open` pending, and the host's preparing loop sends that open once,
//! at depth 0, where the format is made (`doc_open_pending`). Until then the
//! format has nothing of the document, so no `close` is sent for it. A format
//! that cannot be made (`ERR_CANCELLED`) fails the document as a preparation
//! fails: the opener hears it with `event.document_changed`.
//!
//! The details and cover jobs open as any opener does, so their documents
//! take the same path; a `details` document with its open pending is sent
//! by the loop too (it is never prepared after its open).
use super::{DocHost, Document, DocumentTable, FormatCall, READY, check, run, wire};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID};

/// The open's call of the format (`call`, op 2, the opener's `batch`): its
/// cache is made ready, the format is called, and its `doc_state` checked and
/// recorded. A format plugin that is not made yet leaves the document
/// preparing with its open pending ([`keep`]).
///
/// # Errors
/// The cache's, the format's (but `ERR_BUSY`), `ERR_INVALID` for a malformed
/// answer.
pub(super) fn open(
    host: &dyn DocHost,
    call: &FormatCall<'_>,
    batch: &[u8],
    out: &mut [u8],
) -> Result<(), i32> {
    let table = host.table();
    host.open_cache(call)?;
    let length = match run(host, call, out) {
        Ok(length) => length,
        Err(error) => return keep(host, call.document, error, batch, call.conversion),
    };
    let answer: wire::ServiceDocState<'_> = check::only(out.get(..length).ok_or(ERR_INVALID)?)?;
    check::state(&answer)?;
    table.set_state(
        call.document,
        answer.state,
        answer.progress_ppm,
        answer.status,
        answer.flags,
        call.conversion,
    )?;
    if answer.state == READY {
        host.document_ready(call.document);
    }
    Ok(())
}

/// The open's call answered `error`: where that is `ERR_BUSY` (the format
/// plugin is not made yet and its start is asked for), the open's batch is
/// kept and document `id` waits preparing with its open pending. Any other
/// error is the open's answer, as before.
///
/// # Errors
/// `error` where it is not `ERR_BUSY`; `ERR_NO_MEMORY` where the batch
/// cannot be kept.
fn keep(host: &dyn DocHost, id: u32, error: i32, batch: &[u8], conversion: u64) -> Result<(), i32> {
    if error != ERR_BUSY {
        return Err(error);
    }
    host.keep_open(id, batch)?;
    host.table().set_open_pending(id, conversion)
}

/// The format's answer to the pending open of `document` (a `doc_state`),
/// checked as the open's is and recorded: the state, whether the opener's
/// view moved (it hears `event.document_changed`), and the status.
///
/// # Errors
/// `ERR_INVALID` for a malformed answer; the table's.
pub(crate) fn opened(
    table: &DocumentTable,
    document: &Document,
    answer: &[u8],
) -> Result<(u32, i32, bool), i32> {
    let state: wire::ServiceDocState<'_> = check::only(answer)?;
    check::state(&state)?;
    table.set_state(
        document.id,
        state.state,
        state.progress_ppm,
        state.status,
        state.flags,
        document.conversion,
    )?;
    let moved = state.state != document.state || state.progress_ppm != document.progress_ppm;
    if moved {
        table.note_changed(document.id);
    }
    Ok((state.state, state.status, moved))
}
