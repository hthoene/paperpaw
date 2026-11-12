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
//! Memory: one reply room of [`COVER_REPLY`] bytes (a `doc_state`, then the
//! format's `fmt.image` at its caps; not the 4 KiB the dispatcher lends a
//! plugin's call: it is held while the format opens the book, #248), taken
//! fallibly and freed on return, and the encoded `doc_open` (the path and a
//! few bytes). Nothing is kept between calls. The
//! format call is the document service's own, through the caller's frame
//! (a nested callback like the reader's own `doc_open`).
use super::{
    Caller, Cx, DocHost, Frame, READY, Record, close, codec, op, open, read, table::BACKGROUND,
    wire,
};
use crate::abi7::services_wire;
use crate::native_host::doc_host::CoverAt;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE};

/// The reply room of a cover: the open's `doc_state` (under
/// [`OPEN_REPLY`](super::details::OPEN_REPLY)), then op 17's one `fmt.image`
/// (`path` 256 bytes and four `u32`) with the byte a tag may grow by when the
/// host translates it to a `doc_image` (tested). The library's cover make
/// takes the same (`doc_cover_make`).
pub(crate) const COVER_REPLY: usize = 384;

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
    /// (or waits behind the cover job that runs; the host's details job gives
    /// way to it); the library tells when to ask again (`library_changed` bit
    /// 6).
    Queued,
    /// The book's job could not begin: its marker cannot be written (the
    /// card). The library counts the book as failed.
    Failed,
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
    // The book the cover job is converting: nothing to open again. A cover
    // the job has not made is not made here either: no picture is decoded in
    // the asking list's frame (`doc_cover_make`).
    let inline = match services.0.cover_at(path) {
        CoverAt::Job | CoverAt::Wait => return Ok(Made::Queued),
        CoverAt::Made => true,
        CoverAt::Unmade => false,
    };
    // Not now (the start, a book open for reading, the user's last input
    // not a second ago): nothing is opened in the list's frame, where the
    // zip and XML of the book would come beside the list's buffers. The host
    // raises the change when covers may be made, and the book is asked for
    // again.
    if !inline && services.0.cover_must_wait() {
        services.0.cover_job_held_back();
        return Ok(Made::Queued);
    }
    let mut scratch = bytes(COVER_REPLY)?;
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
        result = covered(&host, (path, inline), side, &mut scratch, &adopt);
    });
    result
}

/// Opens, asks for the cover, closes: the document is closed whatever
/// happened in between.
fn covered(
    host: &dyn DocHost,
    (path, inline): (&str, bool),
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
            // Not for showing: a format skips what only a shown book needs
            // (contents, positions, page labels), and the open takes no
            // place another opener's document needs (`document.md` 2.1).
            // `details` for a `prepared` format's book (DOCX, MOBI): its cover
            // without a conversion (`document.md` 2.1).
            flags: BACKGROUND | super::DETAILS,
            image_side: 0,
            cover_side: side,
            css_root: 0,
            css_path: "",
        })],
        &mut input,
    )?;
    // The host's details job gives way first where it converts this very
    // book (a second document would prepare the same cache beside it), or
    // holds the place the cover needs (no cover is busy for it).
    if host
        .details_job()
        .is_some_and(|details| details.preparing(path) || host.table().full())
    {
        super::details::give_way(host);
    }
    // `ERR_FULL`: no place in the caller's table (the library host answers
    // that `ERR_BUSY` and raises nothing: it does not clear by itself).
    let length = open(host, &input[..used], out)?;
    // Replies are bare records.
    let (state, _): (wire::ServiceDocState<'_>, usize) =
        codec::prefix(out.get(..length).ok_or(ERR_INVALID)?)?;
    let document = state.document;
    let ready = state.state == READY;
    let failed = state.state == super::table::FAILED;
    let asked = if ready && inline {
        // A picture whose id cannot be adopted is not handed out: the close
        // below ends it with the document.
        cover_of(host, document, out)
            .and_then(adopt)
            .map(Made::Image)
    } else if failed {
        Err(ERR_NOT_FOUND)
        // Below: preparing, or ready with a cover not made yet.
    } else if !host.cover_jobs_allowed() || host.table().reading_besides(document) {
        // The kernel is still starting (a crash of the format in a job there
        // would count as a failed start), or a book is open for reading (the
        // job takes no step then). Not now: the host raises the change when
        // jobs may begin, and the book is asked for again.
        host.cover_job_held_back();
        Ok(Made::Queued)
    } else if ready {
        // Ready, its cover not made yet: the document stays open as the
        // host's cover to make, beside any cover job (one the list holds back
        // never keeps it waiting); the pump's next look asks the format for
        // the cover and ends it (`doc_cover_make`). Where one waits already,
        // this book is asked for again when that ends.
        if host.table().start_cover_make(host.owner(), document) {
            return Ok(Made::Queued);
        }
        Ok(Made::Queued)
    } else if host
        .table()
        .lease(host.owner(), document)
        .is_ok_and(|d| d.details && !d.open_pending)
    {
        // A `details` open the format answered preparing is never prepared
        // (`document.md` 2.1): no cover until the next start, no conversion.
        Ok(Made::Failed)
    } else {
        // Preparing: the document stays open as the cover job, which the
        // preparing loop converts and the host ends (`doc_cover_job`); where
        // one runs already (another book's), this one waits and is asked for
        // again when that ends.
        // The marker is written only where a job really begins, before its
        // first preparing step; where it cannot be written, the job does not
        // begin and the book counts as failed ([`Made::Failed`]: no loop of
        // asking again). The host's details job is no cover: it gives way.
        super::details::give_way(host);
        if host.table().cover_job().is_some() {
            Ok(Made::Queued)
        } else if host.cover_job_guard(path).is_err() {
            Ok(Made::Failed)
        } else if host.table().start_cover_job(host.owner(), document) {
            return Ok(Made::Queued);
        } else {
            host.cover_job_unguard();
            Ok(Made::Queued)
        }
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
