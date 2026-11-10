//! Op 22 `report` (`docs/abi/document.md` 2.4): the reader tells the host
//! where the user is. This file owns the checks of the report; the session and
//! the events it raises are the host's (`doc_reading.rs`).
//!
//! A report of a book the format has not counted yet asks the format for its
//! `meta` once (op 3, which keeps the section count in the table): the call
//! of a page turn after that costs no plugin call.
use super::{Ask, DocHost, Document, op, prepare, run, settle, verify};
use crate::native_host::{codec, registrations as wire};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE};

/// `doc_report.flags` bit 0.
const FINISHED: u32 = 1;
/// `progress_ppm` at the end of the book.
const PPM_MAX: u32 = 1_000_000;

/// Op 22. The reply is empty.
pub(super) fn report(host: &dyn DocHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let report: wire::ServiceDocReport<'_> = codec::single(input)?;
    // Another plugin's document is `ERR_DENIED`, a closed one `ERR_CANCELLED`:
    // before any field is looked at.
    let document = host.table().ready(host.owner(), report.document)?;
    fields(&report)?;
    let sections = sections_of(host, &document, out)?;
    if report.section >= sections {
        return Err(ERR_INVALID);
    }
    host.reading_report(&document, &report)?;
    Ok(0)
}

/// The rules of the fields that need nothing from the book (`ERR_INVALID`).
fn fields(report: &wire::ServiceDocReport<'_>) -> Result<(), i32> {
    let counters = match (report.page, report.pages) {
        (None, None) => None,
        (Some(page), Some(pages)) if (1..=pages).contains(&page) => Some(pages - page),
        _ => return Err(ERR_INVALID),
    };
    let chapter_left_fits = match (report.chapter_left, counters) {
        (None, _) => true,
        // A count down needs the counters and the chapter it counts in.
        (Some(left), Some(after)) => !report.chapter.is_empty() && left <= after,
        (Some(_), None) => false,
    };
    if report.progress_ppm > PPM_MAX || report.flags & !FINISHED != 0 || !chapter_left_fits {
        return Err(ERR_INVALID);
    }
    Ok(())
}

/// The document's section count: kept from an earlier `meta`, or asked of the
/// format now.
fn sections_of(host: &dyn DocHost, document: &Document, out: &mut [u8]) -> Result<u32, i32> {
    if let Some(sections) = document.sections {
        return Ok(sections);
    }
    let ask = Ask::Ref(wire::ServiceDocRef {
        document: document.id,
    });
    let (call, check) = prepare(op::META, document, &ask)?;
    let length = run(host, &call, out)?;
    let verdict = verify(host, document, &check, out, length);
    settle(host, document, verdict.map(|_| ()))?;
    host.table()
        .lease(host.owner(), document.id)?
        .sections
        .ok_or(ERR_STATE)
}
