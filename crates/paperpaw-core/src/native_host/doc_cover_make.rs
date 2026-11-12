//! The library list's covers are made in the background (Kernel's bisect on
//! #248: a cover made inside the list's `cover` call decodes the picture in
//! the list's frame, +34.7 KiB on the X4 for a folder of pictures).
//!
//! A `cover` (`library.md` 2.1, op 7) answers inline only for a book whose
//! cover the host made since the start ([`CoverAt::Made`]): the format then
//! finds the picture in its cache and decodes nothing. Any other book:
//!
//! * **ready** at its open: its document stays open as the host's one **cover
//!   to make** (`DocumentTable::start_cover_make`), beside the cover job, so a
//!   job the list holds back (a DOCX it must not convert now) never keeps a
//!   ready book waiting. The pump's next look ([`DocState::cover_make_next`])
//!   asks the format for the cover (op 17) outside every frame, notes the
//!   book, closes the document and tells the library (bit 6). While one waits,
//!   any other unmade book is [`CoverAt::Wait`]: nothing is opened for it in
//!   the list's frame; it is asked for again after the bit;
//! * **preparing**: its document becomes the cover job (`doc_cover_job`),
//!   which makes the cover the same way once it is ready.
//!
//! A book is noted once the format answered its cover call, with the picture
//! or with none (`ERR_NOT_FOUND`: a book without a cover is not sent again and
//! again). A call that cannot be made now (`ERR_BUSY`, no memory) notes
//! nothing and fails nothing: the book is asked for again after the bit, the
//! library's cap of failed jobs in a row bounding the retries. A call that
//! failed otherwise notes nothing and counts the book as failed
//! (`library.md`: `ERR_NOT_FOUND` until the next start).
//!
//! **The memo:** a key per book, [`COVERS_MADE`] of them, the most recently
//! asked first (a hit moves to the front; the least recently asked is written
//! over): 256 bytes and a mask, no allocation, RAM only (a card change, move
//! or delete only means an inline decode again, never a wrong picture: the
//! format's cache follows the book). A mask marks the books asked for since
//! the last cover was made: the ones on screen. A cover whose noting writes
//! over such a book (more than [`COVERS_MADE`] covers on screen) would start
//! an endless make/ask cycle; it is told to the library as a failed job of no
//! book, so after the library's cap of those in a row no more is raised.
use super::{BookKind, DocState};
use crate::native_host::{
    doc_service::{Document, JobKind, JobStep, fmt},
    fmt_call::Route,
    host_services::HostServices,
    lib_cover::path_key,
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_NOT_FOUND};

/// How many books' covers are noted as made (a page of a list, twice over).
pub(super) const COVERS_MADE: usize = 32;

/// The reply room of the cover call: one `fmt.image` at its caps (#248).
const REPLY_BYTES: usize = crate::native_host::doc_service::cover::COVER_REPLY;

/// Where a book's cover stands for the library's `cover`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoverAt {
    /// The cover job, or the cover to make, is this book now: asked for
    /// again later.
    Job,
    /// Made since the start: answered inline from the cache.
    Made,
    /// Not made, and another book's cover to make waits: asked for again
    /// when that ends (nothing is opened for it now).
    Wait,
    /// Not made yet: its document becomes the cover to make or the job.
    Unmade,
}

/// What came of a cover call of the host's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cover {
    /// Noted as made; `crowded`: it wrote over a book on screen.
    Noted { crowded: bool },
    /// The call failed (not noted): the book counts as failed.
    Failed,
    /// Not now (the book table held, no memory): not noted, the book not
    /// failed; asked for again after the bit.
    Later,
}

impl Cover {
    /// The step that tells the library (bit 6) of the book of `key`.
    pub(super) fn step(self, key: u64) -> JobStep {
        match self {
            Self::Noted { crowded: false } => JobStep::Ended { ready: true, key },
            // A failed job of no book: counted towards the library's cap of
            // raises, no book remembered.
            Self::Noted { crowded: true } | Self::Later => JobStep::Ended {
                ready: false,
                key: 0,
            },
            Self::Failed => JobStep::Ended { ready: false, key },
        }
    }
}

/// A cover call that failed with `status`: not now for the passing ones (the
/// book table held, the format not started yet, no memory), else for good.
fn failed(status: i32) -> Cover {
    if status == ERR_BUSY || status == ERR_NO_MEMORY {
        Cover::Later
    } else {
        Cover::Failed
    }
}

/// `mask` with bit `at` moved to bit 0 and the bits below it one up (the
/// memo's entry `at` moved to the front).
fn to_front(mask: u32, at: usize) -> u32 {
    let below = (1_u64 << at) - 1;
    let mask = u64::from(mask);
    let above = mask & !((below << 1) | 1);
    // Within 32 bits: `at` is below `COVERS_MADE`.
    u32::try_from(above | ((mask & below) << 1) | 1).unwrap_or(u32::MAX)
}

impl DocState {
    /// Whether the cover job or the cover to make is the book at `path`.
    /// (A details job's book is not the cover's: a cover that needs the slot
    /// takes it, `doc_service::cover`.)
    fn host_row_path_is(&self, path: &str) -> bool {
        if self.job_book_is(path, JobKind::Cover) {
            return true;
        }
        let Some(row) = self.table.cover_make() else {
            return false;
        };
        self.books
            .try_borrow()
            .is_ok_and(|books| books.iter().any(|b| b.path == path && b.document == row.id))
    }

    /// Where the cover of the book at `path` stands; a book found made moves
    /// to the memo's front and is marked as on screen.
    pub(super) fn cover_at(&self, path: &str) -> CoverAt {
        if self.host_row_path_is(path) {
            return CoverAt::Job;
        }
        let key = path_key(path);
        let mut made = self.covers_made.get();
        if let Some(at) = made.iter().position(|&k| k == key && k != 0) {
            made[..=at].rotate_right(1);
            self.covers_made.set(made);
            self.covers_shown.set(to_front(self.covers_shown.get(), at));
            return CoverAt::Made;
        }
        if self.table.cover_make().is_some() {
            CoverAt::Wait
        } else {
            CoverAt::Unmade
        }
    }

    /// Notes the book of `key` as made, at the memo's front (no-op for 0, a
    /// book not held); `true` where it wrote over a book on screen. A new
    /// round of asks begins: no book is on screen until asked again.
    pub(super) fn note_cover_made(&self, key: u64) -> bool {
        if key == 0 {
            return false;
        }
        let mut made = self.covers_made.get();
        let shown = self.covers_shown.replace(0);
        let at = made.iter().position(|&k| k == key);
        let last = COVERS_MADE - 1;
        let crowded = at.is_none() && made[last] != 0 && shown & (1 << last) != 0;
        made[..=at.unwrap_or(last)].rotate_right(1);
        made[0] = key;
        self.covers_made.set(made);
        crowded
    }

    /// The format of `document` (ready) makes its cover now (op 17, in its
    /// cache), from the pump and outside every frame; the answer is dropped
    /// (the list's next `cover` gets the picture from the cache). Noted only
    /// once the format answered.
    pub(super) fn make_cover(
        &self,
        hosts: &HostServices,
        route: &dyn Route,
        document: &Document,
    ) -> Cover {
        if document.format_gone {
            return Cover::Failed;
        }
        let found = match self.format_target(document.format) {
            Ok(found) => found,
            Err(status) => return failed(status),
        };
        let mut out = Vec::new();
        if out.try_reserve_exact(REPLY_BYTES).is_err() {
            return Cover::Later;
        }
        out.resize(REPLY_BYTES, 0);
        let found: (_, BookKind) = found;
        match self.call_in_scope(hosts, route, document, found, fmt::COVER, &mut out) {
            Ok(_) | Err(ERR_NOT_FOUND) => Cover::Noted {
                crowded: self.note_cover_made(self.book_key(document.id)),
            },
            Err(status) => failed(status),
        }
    }

    /// One look at the cover to make, where one waits: its cover is made
    /// (not while a book is open for reading: then it only ends, and the list
    /// asks again when it is shown), its document closed, the library told.
    pub(super) fn cover_make_next(
        &self,
        hosts: &HostServices,
        route: &dyn Route,
        row: &Document,
    ) -> JobStep {
        if row.closed_by_host != 0 {
            let _ = self.table.close(row.owner, row.id);
            return JobStep::None;
        }
        let made = if self.table.reading_besides(0) {
            None
        } else {
            Some(self.make_cover(hosts, route, row))
        };
        let key = self.book_key(row.id);
        let Ok(document) = self.table.close(row.owner, row.id) else {
            return JobStep::Wait;
        };
        self.end_job(hosts, route, &document);
        made.map_or(JobStep::None, |made| made.step(key))
    }

    /// The cover to make ends now (sleep): its row goes, the format gets its
    /// `close`; nothing is noted or told.
    pub(super) fn cover_make_end(&self, hosts: &HostServices, route: &dyn Route) {
        if let Some(row) = self.table.take_cover_make() {
            self.end_job(hosts, route, &row);
        }
    }
}
