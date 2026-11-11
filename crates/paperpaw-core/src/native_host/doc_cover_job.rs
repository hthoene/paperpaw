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
use paperpaw_native::host_glue::{ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_TIMEOUT};

/// The steps in a row that could not be made (the format is not started, no
/// memory for the buffer) after which the job's book counts as failed: only
/// that book, nothing else, whatever the cause (`ERR_NO_MEMORY`).
const LATER_MAX: u8 = 8;

/// The most a job may keep the device awake for one book this start
/// (`ERR_TIMEOUT`): the kernel's clock while the job's book prepares, summed
/// over its jobs (only the last book's sum is kept).
const AWAKE_MAX_MS: u64 = 10 * 60 * 1000;

/// The reply buffer of the format's close (as a step's).
const SCRATCH: usize = 4096;

impl DocState {
    /// Whether the job is a job of `kind` and the document of the book at
    /// `path`.
    pub(super) fn job_book_is(&self, path: &str, kind: JobKind) -> bool {
        let Some(job) = self.table.cover_job() else {
            return false;
        };
        job.job_kind == kind
            && self
                .books
                .try_borrow()
                .is_ok_and(|books| books.iter().any(|b| b.document == job.id && b.path == path))
    }

    /// Whether a job of either kind may begin now: the kernel's start
    /// returned, no job runs and no book is open for reading (else `Err
    /// (None)`), and the quiet moment after a displacement is over (else
    /// `Err(Some(at))`, its end).
    pub(super) fn job_may_begin(&self) -> Result<(), Option<u64>> {
        if !self.jobs_allowed.get()
            || self.table.cover_job().is_some()
            || self.table.reading_besides(0)
        {
            return Err(None);
        }
        let at = self.job_resume_at.get();
        if self.now.get() < at {
            return Err(Some(at));
        }
        Ok(())
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

    /// The job, of `kind`, was displaced (`DocHost::cover_job_displaced`).
    /// A details job is not held back: nothing waits for it (`cover_jobs_
    /// released` raises bit 6 for a held-back cover), it waits the quiet
    /// moment only (`DetailsJob::start`).
    pub(super) fn displaced(&self, kind: JobKind) {
        let key = self.job_key.get();
        let (last, times) = self.job_displaced.get();
        let times = if last == key {
            times.saturating_add(1)
        } else {
            1
        };
        self.job_displaced.set((key, times));
        if times >= super::DISPLACED_MAX {
            // Displaced again and again: its conversion would start over
            // for ever. No job for it until the next start.
            self.job_given_up.set(key);
            return;
        }
        if kind == JobKind::Cover {
            self.job_held_back.set(true);
        }
        self.job_resume_at
            .set(self.now.get().saturating_add(super::RESUME_AFTER_MS));
    }

    /// Writes the marker for the job of the book at `path`
    /// (`DocHost::cover_job_guard`).
    pub(super) fn guard(&self, path: &str) -> Result<(), i32> {
        let storage = self.storage.as_deref().ok_or(ERR_NOT_FOUND)?;
        let key = crate::native_host::lib_cover::path_key(path);
        crate::native_host::cover_job_marker::write(storage, key)?;
        self.job_key.set(key);
        self.job_guarded.set(true);
        // The awake time is counted per book: another book starts at 0.
        if self.job_awake.get().0 != key {
            self.job_awake.set((key, 0));
        }
        self.job_seen.set(self.now.get());
        Ok(())
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
        // A sleep that came inside a callback: the job ends now, and nothing
        // restarts it.
        if self.job_suspend.replace(false) {
            self.cover_job_end(hosts, route);
            return JobStep::None;
        }
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
        // It comes to an end by itself: displacements are not in a row.
        self.job_displaced.set((0, 0));
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

    /// Notes what a preparing step did for the job: a book whose job has
    /// been awake for [`AWAKE_MAX_MS`] this start fails (a conversion that
    /// outlasts every awake session would start over after each wake, for
    /// ever); steps that cannot be made, [`LATER_MAX`] in a row while only the
    /// job prepares, fail it too (its book alone counts as failed, the format
    /// plugin is left as it is).
    pub(super) fn cover_job_stepped(&self, step: PrepareStep) {
        if self.awake_over() {
            self.job_later.set(0);
            self.fail_job(ERR_TIMEOUT);
            return;
        }
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
        self.fail_job(ERR_NO_MEMORY);
    }

    /// Counts the job's awake time to now (the kernel's clock, from its start
    /// or its last count): `true` once its book's reached [`AWAKE_MAX_MS`].
    /// The device sleeps only after the job ended, so it is awake all along.
    fn awake_over(&self) -> bool {
        if self
            .table
            .cover_job()
            .is_none_or(|job| job.state != PREPARING)
        {
            return false;
        }
        let now = self.now.get();
        let (key, spent) = self.job_awake.get();
        let spent = spent.saturating_add(now.saturating_sub(self.job_seen.replace(now)));
        self.job_awake.set((key, spent));
        spent >= AWAKE_MAX_MS
    }

    /// The job's preparation fails with `status` (the pump's next look ends
    /// it, and its book counts as failed).
    fn fail_job(&self, status: i32) {
        if let Some(job) = self.table.cover_job()
            && job.state == PREPARING
            && self
                .table
                .set_prepared(job.id, FAILED, job.progress_ppm, status, job.flags)
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
