//! The library list's covers are made in the background (Kernel's bisect on
//! #248: a cover made inside the list's `cover` call decodes the picture in
//! the list's frame, +34.7 KiB on the X4 for a folder of pictures).
//!
//! A `cover` (`library.md` 2.1, op 7) answers inline only for a book whose
//! cover the cover job made since the start ([`CoverAt::Made`]): the format
//! then finds the picture in its cache and decodes nothing. Any other book's
//! document becomes the cover job ([`doc_cover_job`](super::cover_job)) even
//! where it is ready: the job asks the format for the cover (op 17) from the
//! pump, outside every frame, notes the book here and ends; the library raises
//! `library_changed` bit 6, and the list asks again and gets it.
//!
//! A book is noted whatever the format answered (a cover, none, an error), so
//! a book without a cover is not sent to the job again and again: its next
//! `cover` answers as the format does, inline. What is noted is a key per
//! book, [`COVERS_MADE`] of them (the oldest written over): 256 bytes, no
//! allocation.
use super::{BookKind, DocState};
use crate::native_host::{
    doc_service::{Document, fmt},
    fmt_call::Route,
    host_services::HostServices,
    lib_cover::path_key,
};
use alloc::vec::Vec;

/// How many books' covers are noted as made (a page of a list, twice over).
pub(super) const COVERS_MADE: usize = 32;

/// The reply buffer of the job's cover call (as a preparing step's).
const REPLY_BYTES: usize = 4096;

/// Where a book's cover stands for the library's `cover`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoverAt {
    /// The cover job is converting or making it now: asked for again later.
    Job,
    /// The cover job made it since the start: answered inline from the cache.
    Made,
    /// Not made yet: its document becomes the cover job.
    Unmade,
}

impl DocState {
    /// Where the cover of the book at `path` stands.
    pub(super) fn cover_at(&self, path: &str) -> CoverAt {
        if self.cover_job_path_is(path) {
            return CoverAt::Job;
        }
        let key = path_key(path);
        if self.covers_made.get().contains(&key) {
            CoverAt::Made
        } else {
            CoverAt::Unmade
        }
    }

    /// Notes the book of `key` as made (no-op for 0, a book not held).
    pub(super) fn note_cover_made(&self, key: u64) {
        if key == 0 {
            return;
        }
        let mut made = self.covers_made.get();
        if made.contains(&key) {
            return;
        }
        made.rotate_right(1);
        made[0] = key;
        self.covers_made.set(made);
    }

    /// The job's book is ready: the format makes its cover now (op 17, in its
    /// cache), from the pump and outside every frame; the answer is dropped
    /// (the list's next `cover` gets the picture from the cache). The book is
    /// noted whatever came of it.
    pub(super) fn make_job_cover(&self, hosts: &HostServices, route: &dyn Route, job: &Document) {
        self.note_cover_made(self.book_key(job.id));
        if job.format_gone {
            return;
        }
        let Ok(found) = self.format_target(job.format) else {
            return;
        };
        let mut out = Vec::new();
        if out.try_reserve_exact(REPLY_BYTES).is_err() {
            return;
        }
        out.resize(REPLY_BYTES, 0);
        let found: (_, BookKind) = found;
        let _answer = self.call_in_scope(hosts, route, job, found, fmt::COVER, &mut out);
    }
}
