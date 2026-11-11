//! The library's cover job, the host's side (`docs/abi/library.md` 2.1): a
//! book whose format is still *preparing* (a DOCX or MOBI conversion not yet in
//! the cache) cannot give its cover inside the asking call. The first `cover`
//! of it keeps its document open as the **cover job** (`DocumentTable::
//! start_cover_job`): the preparing loop (`doc_prepare`) converts it, one step
//! a pass between inputs, and this step, run by the kernel's pump
//! (`cover_job_pump`), ends it when it is ready or failed and tells the library.
//!
//! * One job per device, one book at a time; the document is `background`, so
//!   any open that needs a place takes it first (the reader on the X4's two
//!   places always wins, `DocumentTable::open`), and an open that is not the
//!   library's own takes it at once (`DocumentTable::take_cover_job`).
//! * Nothing is kept here: the job is a document of the table, its book's path
//!   is the document's. A document the host closed (sleep, the card gone, its
//!   format left) leaves only its row, which this step frees.
//! * No frame: the format's `close` goes through the pump's route, as a
//!   preparing step's call does.
//! * The slot is the device's one background job, of either kind
//!   (`JobKind`): the library's cover, or the host's details job
//!   (`doc_details_job`), whose book is read here before it is closed. The
//!   marker, the start gate, the end at sleep, the place any other open takes
//!   and the failure after [`LATER_MAX`] steps are the same for both.
use super::{DocState, NoImages};
use crate::native_host::{
    doc_service::{Document, FAILED, JobKind, JobStep, PREPARING, PrepareStep, READY, details},
    fmt_call::{Route, Stage},
    host_services::HostServices,
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_IO, ERR_NO_MEMORY};

/// The steps in a row that could not be made (the format is not started, no
/// memory for the buffer) after which the job's book counts as failed: only
/// that book, nothing else, whatever the cause (`ERR_NO_MEMORY`).
const LATER_MAX: u8 = 8;

/// The reply buffer of the format's close (as a step's).
const SCRATCH: usize = 4096;

impl DocState {
    /// Whether the cover job is the document of the book at `path`.
    pub(super) fn cover_job_path_is(&self, path: &str) -> bool {
        let Some(job) = self.table.cover_job() else {
            return false;
        };
        self.books
            .try_borrow()
            .is_ok_and(|books| books.iter().any(|b| b.document == job.id && b.path == path))
    }

    /// The key of the path of the book of document `document` (what the
    /// library keeps of a failed book), 0 where the book is not held.
    pub(super) fn book_key(&self, document: u32) -> u64 {
        self.books.try_borrow().map_or(0, |books| {
            books
                .iter()
                .find(|b| b.document == document)
                .map_or(0, |b| crate::native_host::lib_cover::path_key(&b.path))
        })
    }

    /// The job has left the table, however it did: its marker goes.
    pub(super) fn unguard(&self) {
        if self.job_guarded.replace(false)
            && let Some(storage) = self.storage.as_deref()
        {
            crate::native_host::cover_job_marker::clear(storage);
        }
    }

    /// One look at the job; ends it when it is ready or failed (a details
    /// job's book is read first, and `keep` keeps what was read).
    #[inline(never)]
    pub(super) fn cover_job_next(
        &self,
        hosts: &HostServices,
        route: &dyn Route,
        keep: &mut details::Keep<'_>,
    ) -> JobStep {
        let Some(job) = self.table.cover_job() else {
            // A job that an open took the place of: its marker is left.
            self.unguard();
            return JobStep::None;
        };
        // The host closed it already (reason 1 to 3, the opener's `close` is
        // not coming): the row is all that is left of it.
        if job.closed_by_host != 0 {
            let _ = self.table.close(job.owner, job.id);
            self.unguard();
            return JobStep::None;
        }
        if job.state == PREPARING {
            return JobStep::Wait;
        }
        if job.job_kind == JobKind::Details {
            return self.details_end(hosts, route, &job, keep);
        }
        // A card that failed is no fault of the book: not remembered as failed.
        let key = if job.state == FAILED && job.status == ERR_IO {
            0
        } else {
            self.book_key(job.id)
        };
        let Ok(document) = self.table.close(job.owner, job.id) else {
            return JobStep::Wait;
        };
        self.end_job(hosts, route, &document);
        self.unguard();
        JobStep::Ended {
            ready: job.state == READY,
            key,
        }
    }

    /// Notes what a preparing step did for the cover job: steps that cannot be
    /// made, [`LATER_MAX`] in a row while only the job prepares, fail it (its
    /// book alone counts as failed, the format plugin is left as it is).
    pub(super) fn cover_job_stepped(&self, step: PrepareStep) {
        if step != PrepareStep::Later || !self.table.only_cover_job_preparing() {
            self.job_later.set(0);
            return;
        }
        let later = self.job_later.get().saturating_add(1);
        self.job_later.set(later);
        if later < LATER_MAX {
            return;
        }
        self.job_later.set(0);
        if let Some(job) = self.table.cover_job()
            && job.state == PREPARING
            && self
                .table
                .set_prepared(job.id, FAILED, job.progress_ppm, ERR_NO_MEMORY, job.flags)
                .is_ok_and(|(moved, ..)| moved)
        {
            self.fresh.set(true);
        }
    }

    /// The job ends now (a book opens elsewhere, the device sleeps): its row
    /// goes and the format gets its `close`. Nothing is restarted.
    pub(super) fn cover_job_end(&self, hosts: &HostServices, route: &dyn Route) {
        self.job_later.set(0);
        if let Some(job) = self.table.take_cover_job() {
            self.end_job(hosts, route, &job);
        }
        self.unguard();
    }

    /// Ends a job whose preparation is over, as `close` would: the cache is
    /// finished, the format gets op 4, the book's hold is given back.
    pub(super) fn end_job(&self, hosts: &HostServices, route: &dyn Route, document: &Document) {
        self.finish_cache(hosts, document.id);
        if !document.format_gone
            && let Ok((target, _)) = self.format_target(document.format)
        {
            let mut scratch = Vec::new();
            if scratch.try_reserve_exact(SCRATCH).is_ok() {
                scratch.extend(core::iter::repeat_n(0_u8, SCRATCH));
                Stage {
                    route,
                    images: &NoImages,
                }
                .close(target, document.id, &mut scratch);
            }
        }
        if document.reader_held {
            self.reader_closed_at(document.id, self.now.get());
            self.table.release_reader(document.id);
        }
    }
}
