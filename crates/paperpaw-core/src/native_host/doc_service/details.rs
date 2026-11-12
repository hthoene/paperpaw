//! The host's details job, the document service's side (`doc_details_job`,
//! `docs/abi/library.md` 2.1): a native format's book of the catalog whose
//! details were never read is opened by the host itself, as its own background
//! document (owner [`HOST_JOB`], root 1, the defaults of a reader's open), in
//! the background job's one slot (`DocumentTable::start_job`), and read through
//! ops 5 `meta` and 6 `meta_text` once it is ready.
//!
//! Here: the open and the checks of the two answers (the same the service
//! makes of a plugin's call), and the run's few numbers ([`DetailsRun`]),
//! which the kernel's pump keeps and reads. No list, nothing that grows.
use super::{
    Cx, DETAILS, DocHost, HOST_JOB, JobKind, Record, check, close, codec, open, table::BACKGROUND,
    wire,
};
use crate::abi7::context::Frame;
use crate::native_host::{fmt_call::Route, host_services::Caller};
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};

/// What keeps a book's details: the book's path and what was read; `true`
/// where they were kept (`library::keep_native_details`).
pub(crate) type Keep<'a> = dyn FnMut(&str, &paperpaw_sdk::document::Metadata) -> bool + 'a;

/// The reply buffer of the open (the dispatcher's own scratch).
const SCRATCH: usize = 4_096;

/// Opens of the same book in a row that could not be made (the format not
/// started, no place, no memory) before the job goes on to the next book.
const LATER_MAX: u8 = 8;

/// The least time between two `core.catalog-wanted` of a running job, ms.
pub(crate) const WANTED_MS: u64 = 120_000;

/// The details job as the kernel's pump sees it (the production host).
pub(crate) trait DetailsJob {
    /// The run's numbers.
    fn run(&self) -> &DetailsRun;
    /// Whether a job may begin now, as for a cover (`doc_cover_job`): `Err
    /// (Some(at))` not before `at` (the quiet moment after a displacement),
    /// `Err(None)` not now (the kernel's start has not returned, a job runs,
    /// a book is open for reading).
    fn may_begin(&self) -> Result<(), Option<u64>>;
    /// Whether the details job prepares the book at `path`.
    fn preparing(&self, path: &str) -> bool;
    /// The details job left the slot to a cover that needs it ([`give_way`]):
    /// its marker goes, and its book is looked at again later.
    fn gave_way(&self);
    /// Opens the book at `path` (root 1) as the host's own document and makes
    /// it the details job: its marker is written first, so that a format that
    /// crashes in the open counts the book as failed at the next start.
    ///
    /// # Errors
    /// `ERR_STATE` before the kernel's start returned, `ERR_BUSY` where a job
    /// runs, the marker's, and what the open says.
    fn start(
        &self,
        frame: &dyn Frame,
        who: &Caller<'_>,
        route: &dyn Route,
        path: &str,
    ) -> Result<(), i32>;
}

/// The details job leaves the slot to a cover that needs it (`cover`): its
/// document ends as one whose place was taken ends, its book is looked at
/// again later and is not counted as failed or displaced. Nothing where the
/// job is a cover job.
pub(crate) fn give_way(host: &dyn DocHost) {
    let Some(details) = host.details_job() else {
        return;
    };
    let table = host.table();
    let Some(job) = table
        .cover_job()
        .filter(|job| job.job_kind == JobKind::Details)
    else {
        return;
    };
    if job.closed_by_host != 0 {
        // Closed by the host already: only its row is left.
        let _ = table.close(job.owner, job.id);
    } else if let Some(job) = table.take_cover_job() {
        host.end_images(&job);
        if !job.format_gone {
            host.close_format(&job);
        }
        if job.reader_held {
            host.reader_closed(job.id);
        }
    }
    details.gave_way();
}

fn buffer(size: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(size, 0);
    Ok(bytes)
}

/// The host's own open of the book at `path` (root 1, no options, the default
/// sides: the cache a reader's plain open makes): the document's id.
///
/// # Errors
/// What `open` says; `ERR_NO_MEMORY` for its buffers.
pub(crate) fn open_book(host: &dyn DocHost, path: &str) -> Result<u32, i32> {
    let cx = Cx {
        host,
        owner: HOST_JOB,
        library: true,
    };
    let mut input = buffer(path.len().saturating_add(96))?;
    let used = codec::encode_batch(
        &[Record::ServiceDocOpen(wire::ServiceDocOpen {
            root: 1,
            path,
            format: "",
            // A `prepared` format's book is read for its details alone, never
            // converted (`document.md` 2.1); the host drops the bit for others.
            flags: BACKGROUND | DETAILS,
            image_side: 0,
            cover_side: 0,
            css_root: 0,
            css_path: "",
        })],
        &mut input,
    )?;
    let mut out = buffer(SCRATCH)?;
    let length = open(&cx, &input[..used], &mut out)?;
    let (state, _): (wire::ServiceDocState<'_>, usize) =
        codec::prefix(out.get(..length).ok_or(ERR_INVALID)?)?;
    Ok(state.document)
}

/// Closes the host's own document `document` as `close` does (the job could
/// not begin after all).
pub(crate) fn close_book(host: &dyn DocHost, document: u32) {
    let cx = Cx {
        host,
        owner: HOST_JOB,
        library: true,
    };
    let mut input = [0_u8; 32];
    if let Ok(used) = codec::encode_batch(
        &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        &mut input,
    ) {
        let _ = close(&cx, &input[..used]);
    }
}

/// A format's answer to op 5, checked as the service checks it.
///
/// # Errors
/// `ERR_INVALID` for a malformed answer.
pub(crate) fn meta(bytes: &[u8]) -> Result<wire::ServiceDocMeta<'_>, i32> {
    let meta: wire::ServiceDocMeta<'_> = check::only(bytes)?;
    check::meta(&meta)?;
    Ok(meta)
}

/// A format's answer to op 6 of `kind`, `index` from `offset`, checked as
/// the service checks it.
///
/// # Errors
/// `ERR_INVALID` for a malformed answer.
pub(crate) fn text(
    bytes: &[u8],
    kind: u32,
    index: u32,
    offset: u32,
) -> Result<wire::ServiceDocText<'_>, i32> {
    let text: wire::ServiceDocText<'_> = check::only(bytes)?;
    check::text(&text, kind, index, offset)?;
    Ok(text)
}

/// The details job's run, a few numbers (`doc_details_job`): whether it is
/// armed (a walk published, `core.catalog-changed`), where in the catalog the
/// next look begins, the line of the book whose job runs, and what was kept
/// since the last `core.catalog-wanted`.
#[derive(Debug, Default)]
pub(crate) struct DetailsRun {
    armed: Cell<bool>,
    /// The byte of the catalog the next look begins at.
    cursor: Cell<u64>,
    /// The line of the running job's book (looked at again where an open
    /// took its place).
    line: Cell<u64>,
    running: Cell<bool>,
    /// Opens of the line at the cursor that could not be made, in a row.
    later: Cell<u8>,
    /// Books kept since the last `core.catalog-wanted`.
    filled: Cell<u32>,
    /// The kernel's clock (ms) at the last `core.catalog-wanted`, or where
    /// the job was armed from rest.
    since: Cell<u64>,
    /// The timed wake of the next look (the kernel's clock, ms), where one
    /// waits (`details_job_pump`).
    wake: Cell<Option<u64>>,
    /// The last wait while a first walk was under way (ms; 0: none).
    walk_wait: Cell<u64>,
}

impl DetailsRun {
    /// A walk published, or the catalog changed: the catalog is looked at from
    /// its start (its lines may have moved).
    pub(crate) fn arm(&self, now_ms: u64) {
        if !self.armed.replace(true) && self.filled.get() == 0 {
            self.since.set(now_ms);
        }
        self.cursor.set(0);
        self.line.set(0);
        self.later.set(0);
        self.wake.set(None);
        self.walk_wait.set(0);
    }
    /// Nothing more to read in the catalog.
    pub(crate) fn disarm(&self) {
        self.armed.set(false);
    }
    /// The device sleeps or stops: the job ended and nothing starts one until
    /// the next arm.
    pub(crate) fn suspend(&self) {
        self.armed.set(false);
        self.running.set(false);
        self.wake.set(None);
    }
    pub(crate) fn armed(&self) -> bool {
        self.armed.get()
    }
    pub(crate) fn running(&self) -> bool {
        self.running.get()
    }
    pub(crate) fn cursor(&self) -> u64 {
        self.cursor.get()
    }
    pub(crate) fn set_cursor(&self, at: u64) {
        self.cursor.set(at);
    }
    /// The job of the book at `line` began; the next look begins at `next`.
    pub(crate) fn started(&self, line: u64, next: u64) {
        self.running.set(true);
        self.line.set(line);
        self.cursor.set(next);
        self.later.set(0);
    }
    /// The job ended (read, or failed).
    pub(crate) fn ended(&self) {
        self.running.set(false);
    }
    /// The job left the table without an end (an open took its place): its
    /// book is looked at again.
    pub(crate) fn gone(&self) {
        self.running.set(false);
        self.cursor.set(self.line.get());
    }
    /// The book at `line` could not be opened now: looked at again later,
    /// [`LATER_MAX`] times in a row at most, then passed by (`next`). The
    /// tries in a row so far.
    pub(crate) fn later(&self, line: u64, next: u64) -> u8 {
        let later = self.later.get().saturating_add(1);
        if later >= LATER_MAX {
            self.later.set(0);
            self.cursor.set(next);
        } else {
            self.later.set(later);
            self.cursor.set(line);
        }
        later
    }
    /// A first walk is under way: how long until the next look, `first` ms
    /// the first time, then twice the last, `most` at most.
    pub(crate) fn walk_waits(&self, first: u64, most: u64) -> u64 {
        let wait = match self.walk_wait.get() {
            0 => first,
            last => last.saturating_mul(2).min(most),
        };
        self.walk_wait.set(wait);
        wait
    }
    /// No first walk is under way (any more): the next one waits from `first`.
    pub(crate) fn walk_ended(&self) {
        self.walk_wait.set(0);
    }
    /// The next look is due at `at` (a timed wake of the pump).
    pub(crate) fn wake_at(&self, at: u64) {
        self.wake.set(Some(at));
    }
    /// The wake where the run is armed and one waits.
    pub(crate) fn wake(&self) -> Option<u64> {
        self.wake.get().filter(|_| self.armed.get())
    }
    /// Whether the look may be made at `now_ms`: not before its wake, which
    /// is spent then.
    pub(crate) fn due(&self, now_ms: u64) -> bool {
        if self.wake.get().is_some_and(|at| now_ms < at) {
            return false;
        }
        self.wake.set(None);
        true
    }
    /// A book's details were kept.
    pub(crate) fn filled(&self) {
        self.filled.set(self.filled.get().saturating_add(1));
    }
    /// Whether `core.catalog-wanted` is sent now: something was kept since the
    /// last, and the catalog is at its `end`, or [`WANTED_MS`] went by. It is
    /// counted as sent.
    pub(crate) fn wanted(&self, now_ms: u64, end: bool) -> bool {
        let due = end || now_ms.saturating_sub(self.since.get()) >= WANTED_MS;
        if self.filled.get() == 0 || !due {
            return false;
        }
        self.filled.set(0);
        self.since.set(now_ms);
        true
    }
}
