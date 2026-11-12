//! The host's details job, the document host's side (`docs/abi/library.md`
//! 2.1): a native format's book whose details were never read (its catalog
//! line has no size) is opened by the host as its own background document, in
//! the device's one background job slot (`doc_cover_job`, kind `Details`),
//! converted by the preparing loop where its format prepares, and read here
//! once it is ready, before it is closed:
//!
//! * op 5 `meta`: the title, the series and its number, the counts;
//! * the whole title (op 6 kind 0, by offsets) where `meta` cut it, the series
//!   (kind 4) likewise, the first author (kind 1) and at most
//!   [`SUBJECTS_MAX`] subjects (kind 2), each cut at its cap.
//!
//! The calls go through the pump's route (`HostRoute`), checked as the
//! document service checks a plugin's (`doc_service::details`), inside the
//! book's scope (roots 3, 4, 6) as a preparing step's. What was read is kept by
//! the kernel (`library::keep_native_details`); where nothing was read nothing
//! is kept, and a book the format failed is the library's failed book.
//!
//! Memory: one reply buffer of 4 KiB, taken fallibly when the book is read and
//! freed with it, and what was read (at most three texts of 1 KiB and 16 of
//! 256 bytes; a few hundred bytes for a usual book), also fallible.
use super::{BookKind, Bound, DocState, NoImages};
use crate::abi7::context::Frame;
use crate::native_host::{
    doc_service::{
        DocHost, Document, FormatCall, HOST_JOB, JobKind, JobStep, PREPARING, READY,
        details::{self, DetailsJob, DetailsRun},
        fmt,
    },
    fmt_call::{Opener, Route, Stage},
    format_call::Target,
    host_services::{Caller, HostServices},
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE,
};
use paperpaw_sdk::document::Metadata;

/// The reply room of the reads: one `doc_meta` (1,133 bytes at its caps) or
/// one `doc_text` (1,044), with the byte a tag may grow by when the host
/// translates the format's record (tested; `document.md` 8.1). Not the 4 KiB
/// of a plugin's call (#248).
pub(super) const READ_REPLY: usize = 1_152;
/// The most of a title, an author or a series kept: a value of the book's
/// details (the SDK's `MAX_VALUE`).
const TEXT_CAP: usize = 1024;
/// The most of one subject kept, so that the details stay well within their
/// 16 KiB.
const SUBJECT_CAP: usize = 256;
/// The most subjects read.
const SUBJECTS_MAX: u32 = 16;
/// `doc_meta.flags`: the title (bit 0) or the series (bit 1) was cut.
const TITLE_CUT: u32 = 1;
const SERIES_CUT: u32 = 1 << 1;
/// `doc_text.kind`.
const TITLE: u32 = 0;
const AUTHOR: u32 = 1;
const SUBJECT: u32 = 2;
const SERIES: u32 = 4;

impl DetailsJob for DocState {
    fn run(&self) -> &DetailsRun {
        &self.details
    }

    fn may_begin(&self) -> Result<(), Option<u64>> {
        self.job_may_begin()
    }

    fn preparing(&self, path: &str) -> bool {
        self.table
            .cover_job()
            .is_some_and(|job| job.state == PREPARING)
            && self.job_book_is(path, JobKind::Details)
    }

    fn gave_way(&self) {
        self.job_later.set(0);
        // Its time counts for its book, as a displaced job's does.
        self.add_used();
        self.unguard();
        self.details.gone();
    }

    fn start(
        &self,
        frame: &dyn Frame,
        who: &Caller<'_>,
        route: &dyn Route,
        path: &str,
    ) -> Result<(), i32> {
        if !self.jobs_allowed.get() {
            return Err(ERR_STATE);
        }
        if self.job_may_begin().is_err() || self.job_guarded.get() {
            return Err(ERR_BUSY);
        }
        // Before the open: a format that crashes in it leaves the marker.
        self.cover_job_guard(path)?;
        let bound = Bound {
            state: self,
            frame,
            who,
            route: Some(route),
        };
        let started = details::open_book(&bound, path).and_then(|document| {
            if self.table.start_job(HOST_JOB, document, JobKind::Details) {
                Ok(())
            } else {
                details::close_book(&bound, document);
                Err(ERR_BUSY)
            }
        });
        if started.is_err() {
            self.unguard();
        }
        started
    }
}

/// Whether a status is no fault of the book (the card, the memory, a format
/// that is not there now): its key is not remembered as failed.
const fn not_the_books(status: i32) -> bool {
    matches!(status, ERR_IO | ERR_NO_MEMORY | ERR_BUSY | ERR_CANCELLED)
}

/// `text` cut at a character boundary to at most `room` bytes.
fn cut(text: &str, room: usize) -> &str {
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A copy of `text`, `None` for an empty one.
fn some(text: &str) -> Result<Option<String>, i32> {
    if text.is_empty() {
        return Ok(None);
    }
    super::copy(text).map(Some)
}

/// Whether nothing was read.
fn empty(metadata: &Metadata) -> bool {
    metadata.title.is_none()
        && metadata.authors.is_empty()
        && metadata.series.is_none()
        && metadata.subjects.is_empty()
}

/// The format's calls for one document, through the pump's route.
struct Reader<'a> {
    state: &'a DocState,
    stage: Stage<'a>,
    target: Target,
    kind: BookKind,
    document: &'a Document,
}

impl Reader<'_> {
    /// One call of `op` (`flags` the text's kind); the answer is at the front
    /// of `out`.
    fn ask(
        &self,
        op: u32,
        flags: u32,
        index: u32,
        offset: u32,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let mut call = FormatCall::new(self.document.format, op, self.document.id);
        call.conversion = self.document.conversion;
        call.flags = flags;
        call.index = index;
        call.offset = offset;
        let mut result = Err(ERR_NOT_FOUND);
        let mut run = |sibling: &str| {
            result = self.stage.call(
                Opener {
                    owner: self.document.owner,
                    sibling,
                },
                self.target,
                &call,
                out,
            );
        };
        if self.kind == BookKind::Siblings {
            self.state.sibling(self.document.id, &mut |name| run(name));
        } else {
            run("");
        }
        result
    }

    /// The text of `kind` and `index`, read by offsets up to `cap` bytes.
    fn text(&self, kind: u32, index: u32, cap: usize, out: &mut [u8]) -> Result<String, i32> {
        let mut text = String::new();
        let mut offset = 0_u32;
        loop {
            let length = self.ask(fmt::META_TEXT, kind, index, offset, out)?;
            let answer = details::text(&out[..length], kind, index, offset)?;
            if text.capacity() == 0 {
                let whole = usize::try_from(answer.total).unwrap_or(usize::MAX);
                text.try_reserve_exact(whole.min(cap))
                    .map_err(|_| ERR_NO_MEMORY)?;
            }
            let piece = cut(answer.text, cap - text.len());
            text.try_reserve(piece.len()).map_err(|_| ERR_NO_MEMORY)?;
            text.push_str(piece);
            let read = u32::try_from(answer.text.len()).map_err(|_| ERR_NO_MEMORY)?;
            offset = offset.saturating_add(read);
            let whole = piece.len() < answer.text.len() || text.len() >= cap;
            if whole || read == 0 || offset >= answer.total {
                return Ok(text);
            }
        }
    }

    /// The book's details (see the module).
    fn details(&self, out: &mut [u8]) -> Result<Metadata, i32> {
        let mut metadata = Metadata::default();
        let length = self.ask(fmt::META, 0, 0, 0, out)?;
        let (flags, authors, subjects) = {
            let meta = details::meta(&out[..length])?;
            metadata.title = some(cut(meta.title, TEXT_CAP))?;
            metadata.series = some(cut(meta.series, TEXT_CAP))?;
            metadata.series_index = some(meta.series_index)?;
            (meta.flags, meta.authors, meta.subjects)
        };
        if flags & TITLE_CUT != 0 {
            metadata.title = some(&self.text(TITLE, 0, TEXT_CAP, out)?)?;
        }
        if flags & SERIES_CUT != 0 {
            metadata.series = some(&self.text(SERIES, 0, TEXT_CAP, out)?)?;
        }
        if authors > 0 {
            let author = self.text(AUTHOR, 0, TEXT_CAP, out)?;
            if !author.is_empty() {
                metadata
                    .authors
                    .try_reserve_exact(1)
                    .map_err(|_| ERR_NO_MEMORY)?;
                metadata.authors.push(author);
            }
        }
        let subjects = subjects.min(SUBJECTS_MAX);
        let wanted = usize::try_from(subjects).unwrap_or_default();
        metadata
            .subjects
            .try_reserve_exact(wanted)
            .map_err(|_| ERR_NO_MEMORY)?;
        for index in 0..subjects {
            let subject = self.text(SUBJECT, index, SUBJECT_CAP, out)?;
            if !subject.is_empty() {
                metadata.subjects.push(subject);
            }
        }
        Ok(metadata)
    }
}

impl DocState {
    /// The details of the ready `document`, read through `route`.
    fn read_details(
        &self,
        hosts: &HostServices,
        route: &dyn Route,
        document: &Document,
    ) -> Result<Metadata, i32> {
        let (target, kind) = self.format_target(document.format)?;
        // The format's roots 3, 4 and 6 are this book's, as in a preparing step.
        let _scope = self.enter_scope(hosts, target.plugin, document.id, kind, "")?;
        let mut out = Vec::new();
        out.try_reserve_exact(READ_REPLY)
            .map_err(|_| ERR_NO_MEMORY)?;
        out.resize(READ_REPLY, 0);
        Reader {
            state: self,
            stage: Stage {
                route,
                images: &NoImages,
            },
            target,
            kind,
            document,
        }
        .details(&mut out)
    }

    /// `keep` with the path of `document`'s book; `false` where it is not
    /// held.
    fn keep_for(&self, document: u32, metadata: &Metadata, keep: &mut details::Keep<'_>) -> bool {
        let Ok(books) = self.books.try_borrow() else {
            return false;
        };
        books
            .iter()
            .find(|book| book.document == document)
            .is_some_and(|book| keep(&book.path, metadata))
    }

    /// The details job is over (ready or failed): its book is read where it is
    /// ready, what was read is kept, and the document is closed as the cover
    /// job's is.
    pub(super) fn details_end(
        &self,
        hosts: &HostServices,
        route: &dyn Route,
        job: &Document,
        keep: &mut details::Keep<'_>,
    ) -> JobStep {
        let read = if job.state == READY {
            self.read_details(hosts, route, job)
        } else {
            // A failed preparation: a card that failed is no fault of the book.
            Err(if job.status == ERR_IO {
                ERR_IO
            } else {
                ERR_STATE
            })
        };
        let key = match &read {
            Err(status) if !not_the_books(*status) => self.book_key(job.id),
            _ => 0,
        };
        let Ok(document) = self.table.close(job.owner, job.id) else {
            return JobStep::Wait;
        };
        let filled = match &read {
            Ok(metadata) if !empty(metadata) => self.keep_for(job.id, metadata, keep),
            _ => false,
        };
        self.end_job(hosts, route, &document);
        self.unguard();
        JobStep::DetailsEnded { filled, key }
    }
}
