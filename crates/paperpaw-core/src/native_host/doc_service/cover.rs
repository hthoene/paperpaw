//! A book's cover for the library's `cover` (op 7, `docs/abi/library.md` 2.1)
//! made by `core.document` (op 15, `document.md` 5): the book is opened as the
//! caller (root 1, whatever the caller's features: the caller gets a picture,
//! never the file), the format is asked for its cover and the document is
//! closed again. The format keeps the picture in the book's document cache
//! (`document.md` 3), so a later call finds it there and decodes nothing.
//!
//! The picture's image id is the caller's own and does not end with the
//! document that made it ([`ImageTable::adopt_cover`]); the same book's cover
//! has one id, however often it is asked for.
//!
//! Memory: one scratch of 4,096 bytes (what the dispatcher lends the
//! document service), taken fallibly and freed on return, and the encoded
//! `doc_open` (the path and a few bytes). Nothing is kept between calls. The
//! format call is the document service's own, through the caller's frame
//! (a nested callback like the reader's own `doc_open`).
use super::{Caller, Cx, DocHost, Frame, READY, Record, close, codec, op, open, read, wire};
use crate::abi7::services_wire;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE};

/// The scratch a document call needs (the dispatcher's own).
const SCRATCH: usize = 4_096;

fn bytes(size: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(size, 0);
    Ok(bytes)
}

/// What a `cover` made.
pub(crate) enum Made {
    /// The picture, as an image id of the caller.
    Image(wire::ServiceDocImage),
    /// The book is still preparing: its document is the library's cover job
    /// (or waits behind the one that runs); the library tells when to ask
    /// again (`library_changed` bit 6).
    Queued,
}

/// The cover of the library book at `path` as an image id of `caller`
/// (`side` as `doc_open.cover_side`: 0 for the default). `ERR_NOT_FOUND` for
/// a book without one, [`Made::Queued`] for one whose format is still
/// preparing (`doc_cover_job`), `ERR_BUSY` when
/// the format is not running yet (its start is requested), `ERR_FULL` when
/// the document table has no place for the caller now; the format's and the
/// card's own statuses otherwise.
pub(crate) fn make(
    frame: &dyn Frame,
    caller: &Caller<'_>,
    path: &str,
    side: u32,
) -> Result<Made, i32> {
    let services = caller.hosts.documents().ok_or(ERR_STATE)?;
    // The book the cover job is converting: nothing to open again.
    if services.0.cover_job_path_is(path) {
        return Ok(Made::Queued);
    }
    let mut scratch = bytes(SCRATCH)?;
    let mut result = Err(ERR_STATE);
    services.0.serve(frame, caller, &mut |bound| {
        let host = Cx {
            host: bound,
            owner: caller.owner,
            library: true,
        };
        // Its id is the caller's and must outlive the document made for it:
        // adopted before the document is closed, which ends the document's
        // own pictures (an id that is not adopted ends with it).
        let adopt = |image: wire::ServiceDocImage| -> Result<wire::ServiceDocImage, i32> {
            let id = caller
                .hosts
                .images()
                .adopt_cover(caller.owner, image.image)?;
            Ok(wire::ServiceDocImage { image: id, ..image })
        };
        result = covered(&host, path, side, &mut scratch, &adopt);
    });
    result
}

/// Opens, asks for the cover, closes: the document is closed whatever
/// happened in between.
fn covered(
    host: &dyn DocHost,
    path: &str,
    side: u32,
    out: &mut [u8],
    adopt: &dyn Fn(wire::ServiceDocImage) -> Result<wire::ServiceDocImage, i32>,
) -> Result<Made, i32> {
    let mut input = bytes(path.len().saturating_add(96))?;
    // `doc_open` is a counted batch (its options follow the request).
    let used = codec::encode_batch(
        &[Record::ServiceDocOpen(wire::ServiceDocOpen {
            root: 1,
            path,
            format: "",
            flags: 0,
            image_side: 0,
            cover_side: side,
            css_root: 0,
            css_path: "",
        })],
        &mut input,
    )?;
    // `ERR_FULL`: no place in the caller's table (the library host answers
    // that `ERR_BUSY` and raises nothing: it does not clear by itself).
    let length = open(host, &input[..used], out)?;
    // Replies are bare records.
    let (state, _): (wire::ServiceDocState<'_>, usize) =
        codec::prefix(out.get(..length).ok_or(ERR_INVALID)?)?;
    let document = state.document;
    let ready = state.state == READY;
    let failed = state.state == super::table::FAILED;
    let asked = if ready {
        // A picture whose id cannot be adopted is not handed out: the close
        // below ends it with the document.
        cover_of(host, document, out)
            .and_then(adopt)
            .map(Made::Image)
    } else if failed {
        Err(ERR_NOT_FOUND)
    } else if !host.cover_jobs_allowed() {
        // The kernel is still starting: a crash of the format in a job there
        // would count as a failed start. Not now; the host raises the change
        // once start has returned and the book is asked for again.
        host.cover_job_held_back();
        Ok(Made::Queued)
    } else {
        // Preparing: the document stays open as the cover job, which the
        // preparing loop converts and the host ends (`doc_cover_job`); where
        // one runs already (another book's), this one waits and is asked for
        // again when that ends.
        // The marker is written only where a job really begins, before its
        // first preparing step; where it cannot be written, the job does not
        // begin (busy, nothing raised).
        if host.table().cover_job().is_none() && host.cover_job_guard(path).is_ok() {
            if host.table().start_cover_job(host.owner(), document) {
                return Ok(Made::Queued);
            }
            host.cover_job_unguard();
        }
        Ok(Made::Queued)
    };
    let reference = codec::encode_batch(
        &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        &mut input,
    )?;
    let _ = close(host, &input[..reference]);
    asked
}

/// Op 15 of the open document: the one `doc_image` the format answers.
fn cover_of(
    host: &dyn DocHost,
    document: u32,
    out: &mut [u8],
) -> Result<wire::ServiceDocImage, i32> {
    let op = services_wire::service_schema("core.document", 1)
        .and_then(|schema| schema.op(op::COVER))
        .ok_or(ERR_STATE)?;
    let mut input = [0_u8; 32];
    let used = codec::encode_batch(
        &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        &mut input,
    )?;
    let length = read(host, op, &input[..used], out)?;
    let (image, _) = codec::prefix(out.get(..length).ok_or(ERR_INVALID)?)?;
    Ok(image)
}
