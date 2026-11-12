//! Host side of the service `core.document` (`docs/abi/document.md` 2).
//!
//! The backend is the generic [`Backend`](super::resources) function: it holds
//! no state. Everything it needs comes from the frame's [`DocHost`]: the
//! caller, the [`DocumentTable`] and the broker that calls the format plugin's
//! `core.format` stage. This file and its parts own the contract: record
//! order and values of every input, the lease of a document (only its opener),
//! the state rules (preparing is `ERR_BUSY`), the paging rules and the
//! checks of a format's answer before it is passed on.
//!
//! Memory (X4): the table is allocated once at its device limit (about 40
//! bytes a document, 2 documents), open and close allocate nothing. A call
//! runs in the 4,096-byte scratch the generic dispatcher lends: the format's
//! answer is written into it, checked there and, for pictures, rewritten in
//! place. No call keeps anything. The peak of the DOC host side beyond the
//! scratch is the table: 80 bytes.
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
};
use crate::abi7::{
    context::{Frame, RowsEnd},
    services_wire::ServiceOp,
};
use crate::native_host::codec;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{
    ERR_CANCELLED, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};

mod args;
mod call;
mod check;
pub(crate) mod cover;
pub(crate) mod details;
pub(crate) mod pending;
mod report;
mod steps;
mod table;
#[allow(unused_imports, reason = "the shell builds the table with its limits")]
pub(crate) use args::ROOT_HANDED;
pub(crate) use call::{FormatCall, Options};
pub(crate) use steps::{JobStep, PrepareStep};
pub(crate) use table::{
    BACKGROUND, DETAILS, Document, DocumentTable, FAILED, FormatRef, HOST_JOB, JobKind, Limits,
    PREPARING, READY,
};

/// Operations of `core.format` the host calls (`document.md` 3).
pub(crate) mod fmt;

/// The file a hand-over names, resolved (`doc_open` root 5).
pub(crate) struct HandedBook {
    /// Its path on the card (the maker's data or install folder joined).
    pub(crate) card_path: alloc::string::String,
    /// The maker's owner key.
    pub(crate) maker: u64,
}

/// Everything the document service needs from the host. The shell implements
/// it over its declarations, storage and stage broker; tests over fakes.
pub(crate) trait DocHost {
    /// This host as a trait object (so [`serve`](Self::serve) can hand it on).
    fn as_dyn(&self) -> &dyn DocHost;
    /// Runs `run` with the host that serves one call of `who`, bound to its
    /// `frame` (the settings, the declarations, a directed stage call). A host
    /// that needs none of it serves as it is; the production host binds them.
    fn serve(&self, _frame: &dyn Frame, _who: &Caller<'_>, run: &mut dyn FnMut(&dyn DocHost)) {
        run(self.as_dyn());
    }
    /// The calling plugin: a never-reused host owner id (the backend wraps it).
    fn owner(&self) -> u64 {
        0
    }
    fn table(&self) -> &DocumentTable;
    /// The screen's long side in pixels.
    fn screen_long_side(&self) -> u32;
    /// Root 0 is the caller's Data; root 1 needs `uses.storage`
    /// (`ERR_DENIED` without).
    fn root_access(&self, root: u32) -> Result<(), i32>;
    /// `doc_open` root 5: the card path of the file the caller's hand-over
    /// names, if `path` is that file (`document.md` 1, draft 25). Read only,
    /// in the maker's data or install folder.
    ///
    /// # Errors
    /// `ERR_DENIED` (another path, no hand-over, no such instance),
    /// `ERR_NOT_FOUND` (the maker is not installed, or no such file),
    /// `ERR_STATE` (the store is busy), `ERR_UNSUPPORTED` until wired,
    /// `ERR_NO_MEMORY`.
    fn handed_open(&self, _path: &str) -> Result<HandedBook, i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The document `document` reads a file of `maker`'s folders (root 5):
    /// it ends when the maker goes (`document_of_maker`).
    fn handed_maker(&self, _document: u32, _maker: u64) {}
    /// A document that reads a file of `maker`'s folders and is not ended yet.
    fn document_of_maker(&self, _maker: u64) -> Option<u32> {
        None
    }
    /// Section 1: the format that reads `path` (the longest extension, the
    /// user's choice, priority); `requested` is a format id or empty.
    /// `ERR_NOT_FOUND` when none reads it.
    fn select_format(&self, root: u32, path: &str, requested: &str) -> Result<FormatRef, i32>;
    fn format_id(&self, format: FormatRef, visit: &mut dyn FnMut(&str)) -> Result<(), i32>;
    /// The conversion key of an open: the book, the format and what changes
    /// the output (`document.md` 2.1).
    fn conversion_key(
        &self,
        open: &wire::ServiceDocOpen<'_>,
        format: FormatRef,
        options: Options<'_>,
    ) -> Result<u64, i32>;
    /// Call the format plugin. On success the answer (service-tag records)
    /// is at the start of `out`; the count is at most `out.len()`.
    fn call_format(&self, call: &FormatCall<'_>, out: &mut [u8]) -> Result<usize, i32>;
    /// Before an open's `call_format`: the book's cache (root 6) is made
    /// ready (`document.md` 3). Its own step, so that a busy format is told
    /// apart from a busy cache (`pending`).
    fn open_cache(&self, _call: &FormatCall<'_>) -> Result<(), i32> {
        Ok(())
    }
    /// Keeps the open's batch (its CSS file and options) for the document
    /// whose open waits for its format plugin (`pending`).
    fn keep_open(&self, _document: u32, _batch: &[u8]) -> Result<(), i32> {
        Ok(())
    }
    /// Tell the format plugin to close a document (op 4 `close`). Best
    /// effort; nothing the format says can fail it. The service never calls
    /// it for a document whose format stopped (`format_gone`). The host
    /// refuses it itself for a plugin that is retired (its instance is
    /// revoked), whatever it is asked.
    fn close_format(&self, document: &Document);
    /// Every image id of the document ends now. It may be called twice for the
    /// same document (ids are generation-checked); the second call does
    /// nothing.
    fn end_images(&self, document: &Document);
    /// The image ids of documents the host ended out of reach of the image
    /// table (`end_images` kept them) end now in `images`. A host that ends
    /// them at once has none to flush.
    fn flush_images(&self, _images: &super::image_service::ImageTable) {}
    /// The host closed a document on its own (three malformed answers, the
    /// format plugin stopped): the opener gets `event.document_closed`. Only
    /// the event; the service ends the image ids (`end_images`) first and
    /// never calls the format here.
    fn document_closed(&self, document: &Document, reason: u32);
    /// The oldest `event.document_closed` of `owner` still to deliver,
    /// encoded into `out` as a one-record batch: its length, or `None`. A
    /// failure consumes nothing. A host without events has none.
    ///
    /// # Errors
    /// `ERR_BUSY` when the host's queue is borrowed; `ERR_NO_SPACE` when `out`
    /// is too small.
    fn next_event(&self, _owner: u64, _out: &mut [u8]) -> Result<Option<usize>, i32> {
        Ok(None)
    }
    /// Whether an event was raised since the pump's last pass ([`Self::set_now`]
    /// starts one): the kernel's pump looks again at once. Events left over
    /// for an owner that does not run are not asked for again.
    fn events_waiting(&self) -> bool {
        false
    }
    /// The kernel's clock, milliseconds, as the pump last read it: the time
    /// the events it delivers are stamped with.
    fn set_now(&self, _now_ms: u64) {}
    /// One step of the preparing loop (`document.md` 2.1): the format of the
    /// next preparing document gets op 3 `prepare`, between inputs, from the
    /// kernel's pump (no opener callback runs), and its answer is recorded.
    /// A host without a preparing loop has nothing to do.
    /// Whether a document is preparing (the pump asks before it makes a route).
    fn preparing(&self) -> bool {
        false
    }
    fn prepare_step(
        &self,
        _hosts: &super::host_services::HostServices,
        _route: &dyn super::fmt_call::Route,
    ) -> PrepareStep {
        PrepareStep::Idle
    }
    /// One look at the background job (`doc_cover_job`): ended when its
    /// preparation is over; a details job's book is read first and `keep`
    /// keeps what was read. Run by the kernel's pump, never inside a callback.
    fn cover_job_step(
        &self,
        _hosts: &super::host_services::HostServices,
        _route: &dyn super::fmt_call::Route,
        _keep: &mut details::Keep<'_>,
    ) -> JobStep {
        JobStep::None
    }
    /// The host's details job (`doc_details_job`), where the host has one.
    fn details_job(&self) -> Option<&dyn details::DetailsJob> {
        None
    }
    /// Where the cover of the book at `path` stands (`doc_cover_make`): a
    /// host without the cover job answers every cover inline.
    fn cover_at(&self, _path: &str) -> super::doc_host::CoverAt {
        super::doc_host::CoverAt::Made
    }
    /// Whether only the cover job prepares: the pump's "more" then does not
    /// hold the device awake.
    fn only_cover_job_preparing(&self) -> bool {
        false
    }
    /// The cover job ends now (a book opens elsewhere, the device sleeps): its
    /// document leaves the table and the format gets its `close`. Nothing is
    /// restarted; the next `cover` of the book starts it again.
    fn cover_job_abort(
        &self,
        _hosts: &super::host_services::HostServices,
        _route: &dyn super::fmt_call::Route,
    ) {
    }
    /// The device sleeps inside a callback, where the job cannot be ended: it
    /// ends at the pump's next look (`cover_job_pump`).
    fn cover_job_suspend_later(&self) {}
    /// Whether a cover job exists (the pump asks before it makes a route).
    fn cover_job_active(&self) -> bool {
        false
    }
    /// Whether a cover job may begin: not before the kernel's start has
    /// returned (a crash of the format there would count as a failed start).
    fn cover_jobs_allowed(&self) -> bool {
        true
    }
    /// A `cover` asked for a job where none may begin (before
    /// [`DocHost::cover_jobs_allowed`], or while a book is open for reading):
    /// it is remembered, and [`DocHost::cover_jobs_released`] says when to ask
    /// again.
    fn cover_job_held_back(&self) {}
    /// The job (of `kind`) was displaced by an open (its place was needed):
    /// a cover job is held back as above; either waits a quiet moment, so that
    /// what displaced it is done before it starts again from the beginning (a
    /// cache is whole or not used: nothing of the job's work is kept).
    fn cover_job_displaced(&self, _kind: JobKind) {}
    /// A library list screen is up (`up`) or not: no *preparing* step (a
    /// conversion) is taken while one is up and for a quiet moment after it
    /// closes. Said by the pump, with the end of the quiet time after the
    /// user's last input and the start (`quiet_until`, kernel ms): before it,
    /// no step, no cover and no start (#248). Held time does not count toward
    /// the awake time. Answers when a waiting job's wait ends: the pump wakes.
    fn cover_job_list(&self, _up: bool, _quiet_until: u64) -> Option<u64> {
        None
    }
    /// An input came: the quiet time begins anew, before its callbacks run.
    fn cover_job_input(&self, _quiet_until: u64) {}
    /// Whether the user's quiet time is over.
    fn cover_quiet(&self) -> bool {
        true
    }
    /// Whether a `cover` of an unmade book waits: nothing is opened for it in
    /// the list's frame; the library is told when to ask again.
    fn cover_must_wait(&self) -> bool {
        !self.cover_jobs_allowed() || !self.cover_quiet() || self.table().reading_besides(0)
    }
    /// The key of a book whose job was displaced too often (0: none): failed.
    fn cover_job_given_up(&self) -> u64 {
        0
    }
    /// Whether a held-back job may begin again now: allowed, no book open for
    /// reading, no job, and the quiet moment over. Said once; the library then
    /// raises bit 6 (the screens ask again).
    fn cover_jobs_released(&self) -> bool {
        false
    }
    /// The kernel's start has returned: jobs may begin. The key of the book a
    /// marker left from before names (`cover_job_marker`), 0 for none.
    fn allow_cover_jobs(&self) -> u64 {
        0
    }
    /// A job for the book at `path` is about to begin: its marker is written
    /// (`cover_job_marker`). An error: the job does not begin.
    fn cover_job_guard(&self, _path: &str) -> Result<(), i32> {
        Ok(())
    }
    /// The job has left the table, however: its marker goes.
    fn cover_job_unguard(&self) {}
    /// The call is the library's own `cover`: it does not displace the cover
    /// job.
    fn library_call(&self) -> bool {
        false
    }
    /// Whether `format`'s `format.spec` has bit 0 `prepared` (a conversion
    /// before reading): only its books are opened with bit 3 `details`.
    fn format_prepared(&self, _format: FormatRef) -> bool {
        false
    }
    /// An `open` is about to read the card path `path` of root `root`
    /// (document `document`): the host counts it so the file is not freed under
    /// the reader. `Err` refuses the open: nothing is opened and the error is
    /// the `open`'s. Called once per document, before the format.
    ///
    /// # Errors
    /// What the host's in-use table says (`ERR_FULL`, `ERR_BUSY`).
    fn reader_opened(&self, _document: u32, _root: u32, _path: &str) -> Result<(), i32> {
        Ok(())
    }
    /// Document `document` ended, whichever way (`close`, its opener or format
    /// stopped, the host closed it, its place was taken): called once per
    /// document that `reader_opened` accepted.
    fn reader_closed(&self, _document: u32) {}
    /// Whether the plugin `owner` shows a screen now. A document of an opener
    /// that shows none may be taken for a book to show (`document.md` 2.1).
    fn shows_screen(&self, _owner: u64) -> bool {
        false
    }
    /// The reading session (`document.md` 2.4): a checked report of
    /// `document`'s reader. `ERR_UNSUPPORTED` where the host keeps none.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, `ERR_BUSY` (see `Reading::report`).
    fn reading_report(
        &self,
        _document: &Document,
        _report: &wire::ServiceDocReport<'_>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    /// The oldest event of the reading session for the kernel's pump.
    fn take_reading_edge(&self) -> Option<super::doc_reading::Edge> {
        None
    }
    /// The device goes to sleep or off: a book that is open is closed with
    /// reason 1 (`core-events.md` 3.2).
    fn reading_sleep(&self, _now_ms: u64) {}
    /// Turn a format's picture into the opener's image id (`document.md`
    /// 5): `Some(id)`, or `Some(0)` where all ids are out and none is drawn
    /// (`in_block` only; else `ERR_BUSY`), or `None` for a picture that fails
    /// the cache-image check.
    fn map_image(
        &self,
        document: &Document,
        image: &wire::ServiceDocImage,
        in_block: bool,
    ) -> Result<Option<u32>, i32>;
    /// A picture of `document` was left out of a block stream (it failed the
    /// image check, or the format sent none): the block keeps its alt text.
    /// The host says so once per run of left-out pictures of the same
    /// document (it keeps the last document it named), never per picture.
    fn picture_left_out(&self, _document: u32) {}
    /// The format answered that the document is ready (the open, or the
    /// last preparing call): what the host keeps for it is complete now.
    fn document_ready(&self, _document: u32) {}
    /// The owner is a format plugin that stopped: it is removed from the
    /// format index and `visit` gets each format reference it had. Nothing
    /// is called on the plugin.
    fn formats_of_owner(&self, owner: u64, visit: &mut dyn FnMut(FormatRef));
    /// Each format reference of the plugin `owner`, left in the index: whether
    /// a document uses one is asked before the plugin is parked again (#248,
    /// D3), which must not take its formats away.
    fn formats_owned(&self, owner: u64, visit: &mut dyn FnMut(FormatRef));
    /// Section 1's formats in a stable order; `ERR_STATE` for a stale
    /// generation (never 0, as REG's).
    fn formats(
        &self,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32>;
    /// Reading apps, of one format id or of all (`scope` empty).
    fn readers(
        &self,
        scope: &str,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32>;
    /// Op 20: the format that reads a file; `sniff` reads its first 512
    /// bytes when no extension matches. Calls `visit` once, `ERR_NOT_FOUND`
    /// when none.
    fn format_for(
        &self,
        root: u32,
        path: &str,
        sniff: bool,
        visit: &mut dyn FnMut(Record<'_>),
    ) -> Result<(), i32>;
}

/// The format's answer to a `prepare` of `document`, checked and recorded:
/// the document's state, progress, status and flags are the format's, and
/// the opener hears of a change (`event.document_changed`). Returns the new
/// state, its status and whether the state or the progress moved. A
/// preparation that answers "more" 32 times in a row without rising above its
/// best progress fails with `ERR_TIMEOUT` (the returned state is then 2). A
/// malformed answer is `ERR_INVALID` and records nothing.
///
/// # Errors
/// `ERR_INVALID`, `ERR_BUSY` (the table is borrowed), `ERR_NOT_FOUND`.
pub(crate) fn prepared(
    table: &DocumentTable,
    document: &Document,
    answer: &[u8],
) -> Result<(u32, i32, bool), i32> {
    let state: wire::ServiceDocState<'_> = check::only(answer)?;
    check::state(&state)?;
    let (moved, now, status) = table.set_prepared(
        document.id,
        state.state,
        state.progress_ppm,
        state.status,
        state.flags,
    )?;
    Ok((now, status, moved))
}

/// The opener stopped: its documents end, and each format gets op 4 `close`
/// and the document's image ids end (`document.md` 2.1). The shell calls it
/// when a plugin stops.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed; a retry ends the rest.
pub(crate) fn opener_stopped(host: &dyn DocHost, owner: u64) -> Result<usize, i32> {
    host.table()
        .close_owner(owner, |document| end(host, document))
}

/// A document leaves the table (its opener closed it or stopped, or a `open`
/// failed): its image ids end (again, if the host's own close ended them; the
/// host makes that idempotent) and the format is told to close it unless the
/// format is gone. One place for every path, so no path calls a stopped format.
fn end(host: &dyn DocHost, document: &Document) {
    host.end_images(document);
    if !document.format_gone && !document.open_pending {
        host.close_format(document);
    }
    // One that the host closed earlier released its reader then; one whose
    // format retired with its opener (no event) still holds it here.
    if document.reader_held {
        host.reader_closed(document.id);
        host.table().release_reader(document.id);
    }
}

/// The host closes a document on its own: image ids end, the reader is
/// released, the opener hears it.
fn host_closed(host: &dyn DocHost, document: &Document, reason: u32) {
    host.end_images(document);
    if document.reader_held {
        host.reader_closed(document.id);
        host.table().release_reader(document.id);
    }
    host.document_closed(document, reason);
}

/// The host ends one document on its own: the card went (reason 2) or its book
/// was moved or removed (reason 3), or reason 1. The opener hears
/// `event.document_closed`, later calls but `close` are `ERR_CANCELLED`, and the
/// book's reader is released. A document already ended is left alone.
///
/// # Errors
/// `ERR_INVALID` for another reason, `ERR_NOT_FOUND`, `ERR_BUSY` (table
/// borrowed; a retry ends it).
///
/// A caller that has the host services ends a document with
/// `HostServices::end_document_by_host`, which also ends its image ids at
/// once (this function alone defers them to the next call).
pub(crate) fn end_by_host(host: &dyn DocHost, id: u32, reason: u32) -> Result<(), i32> {
    if let Some(document) = host.table().end_by_host(id, reason)? {
        host_closed(host, &document, reason);
    }
    Ok(())
}

/// A document taken for a book to show (`document.md` 2.1): closed as by
/// `close` (image ids end, the format gets op 4, the reader is released) and
/// its opener hears reason 4 `needed`.
fn taken(host: &dyn DocHost, document: &Document) {
    if document.cover_job {
        host.cover_job_displaced(document.job_kind);
    }
    host.end_images(document);
    if !document.format_gone && !document.open_pending {
        host.close_format(document);
    }
    if document.reader_held {
        host.reader_closed(document.id);
    }
    host.document_closed(document, table::NEEDED);
}

/// A format plugin stopped or was uninstalled: its documents are closed by the
/// host (reason 1), their image ids end, each opener hears
/// `event.document_closed`, and the format is not called again (not even by
/// the opener's later `close`). The shell calls it when a format plugin stops.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed; a retry finishes the rest.
pub(crate) fn format_stopped(host: &dyn DocHost, format: FormatRef) -> Result<usize, i32> {
    format_retired(host, format, None)
}

/// As [`format_stopped`], for a format plugin that is retiring: `owner`, if it
/// is also an opener, hears nothing and its documents are marked gone, so its
/// own opener cleanup (`opener_stopped`, right after) never calls the format.
///
/// # Errors
/// As [`format_stopped`].
pub(crate) fn format_retired(
    host: &dyn DocHost,
    format: FormatRef,
    owner: Option<u64>,
) -> Result<usize, i32> {
    host.table()
        .cancel_format(format, 1, owner, |document| host_closed(host, document, 1))
}

/// What the shell hands the frame under the key `core.document`.
pub(crate) struct DocServices(pub(crate) alloc::rc::Rc<dyn DocHost>);
impl DocServices {
    /// Whether any document, in any state (a pending open too), names a
    /// format of which `of` says yes: its plugin may not park again (#248 D3).
    pub(crate) fn uses_format(&self, of: &dyn Fn(FormatRef) -> bool) -> bool {
        self.0.table().uses_format(of)
    }
}

/// The host of this call with its caller: forwards everything but `owner`.
struct Cx<'a> {
    host: &'a dyn DocHost,
    owner: u64,
    /// The call is the library's `cover` (`cover`): the book is the library's
    /// own, so root 1 needs no `uses.storage` of the caller.
    library: bool,
}
impl DocHost for Cx<'_> {
    fn as_dyn(&self) -> &dyn DocHost {
        self
    }
    fn owner(&self) -> u64 {
        self.owner
    }
    fn table(&self) -> &DocumentTable {
        self.host.table()
    }
    fn screen_long_side(&self) -> u32 {
        self.host.screen_long_side()
    }
    fn root_access(&self, root: u32) -> Result<(), i32> {
        if self.library && root == 1 {
            return Ok(());
        }
        self.host.root_access(root)
    }
    fn handed_open(&self, path: &str) -> Result<HandedBook, i32> {
        self.host.handed_open(path)
    }
    fn handed_maker(&self, document: u32, maker: u64) {
        self.host.handed_maker(document, maker);
    }
    fn document_of_maker(&self, maker: u64) -> Option<u32> {
        self.host.document_of_maker(maker)
    }
    fn select_format(&self, root: u32, path: &str, requested: &str) -> Result<FormatRef, i32> {
        self.host.select_format(root, path, requested)
    }
    fn format_id(&self, format: FormatRef, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        self.host.format_id(format, visit)
    }
    fn conversion_key(
        &self,
        open: &wire::ServiceDocOpen<'_>,
        format: FormatRef,
        options: Options<'_>,
    ) -> Result<u64, i32> {
        self.host.conversion_key(open, format, options)
    }
    fn call_format(&self, call: &FormatCall<'_>, out: &mut [u8]) -> Result<usize, i32> {
        self.host.call_format(call, out)
    }
    fn open_cache(&self, call: &FormatCall<'_>) -> Result<(), i32> {
        self.host.open_cache(call)
    }
    fn keep_open(&self, document: u32, batch: &[u8]) -> Result<(), i32> {
        self.host.keep_open(document, batch)
    }
    fn close_format(&self, document: &Document) {
        self.host.close_format(document);
    }
    fn end_images(&self, document: &Document) {
        self.host.end_images(document);
    }
    fn document_closed(&self, document: &Document, reason: u32) {
        self.host.document_closed(document, reason);
    }
    fn reader_opened(&self, document: u32, root: u32, path: &str) -> Result<(), i32> {
        self.host.reader_opened(document, root, path)
    }
    fn details_job(&self) -> Option<&dyn details::DetailsJob> {
        self.host.details_job()
    }
    fn cover_at(&self, path: &str) -> super::doc_host::CoverAt {
        self.host.cover_at(path)
    }
    fn cover_jobs_allowed(&self) -> bool {
        self.host.cover_jobs_allowed()
    }
    fn cover_job_held_back(&self) {
        self.host.cover_job_held_back();
    }
    fn cover_quiet(&self) -> bool {
        self.host.cover_quiet()
    }
    fn cover_job_displaced(&self, kind: JobKind) {
        self.host.cover_job_displaced(kind);
    }
    fn cover_job_guard(&self, path: &str) -> Result<(), i32> {
        self.host.cover_job_guard(path)
    }
    fn cover_job_unguard(&self) {
        self.host.cover_job_unguard();
    }
    fn library_call(&self) -> bool {
        self.library
    }
    fn format_prepared(&self, format: FormatRef) -> bool {
        self.host.format_prepared(format)
    }
    fn document_ready(&self, document: u32) {
        self.host.document_ready(document);
    }
    fn picture_left_out(&self, document: u32) {
        self.host.picture_left_out(document);
    }
    fn reader_closed(&self, document: u32) {
        self.host.reader_closed(document);
    }
    fn shows_screen(&self, owner: u64) -> bool {
        self.host.shows_screen(owner)
    }
    fn reading_report(
        &self,
        document: &Document,
        report: &wire::ServiceDocReport<'_>,
    ) -> Result<(), i32> {
        self.host.reading_report(document, report)
    }
    fn take_reading_edge(&self) -> Option<super::doc_reading::Edge> {
        self.host.take_reading_edge()
    }
    fn reading_sleep(&self, now_ms: u64) {
        self.host.reading_sleep(now_ms);
    }
    fn map_image(
        &self,
        document: &Document,
        image: &wire::ServiceDocImage,
        in_block: bool,
    ) -> Result<Option<u32>, i32> {
        self.host.map_image(document, image, in_block)
    }
    fn formats_of_owner(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        self.host.formats_of_owner(owner, visit);
    }
    fn formats_owned(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        self.host.formats_owned(owner, visit);
    }
    fn formats(
        &self,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.host.formats(cursor, generation, visit)
    }
    fn readers(
        &self,
        scope: &str,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.host.readers(scope, cursor, generation, visit)
    }
    fn format_for(
        &self,
        root: u32,
        path: &str,
        sniff: bool,
        visit: &mut dyn FnMut(Record<'_>),
    ) -> Result<(), i32> {
        self.host.format_for(root, path, sniff, visit)
    }
}

mod op {
    pub(super) const OPEN: u32 = 1;
    pub(super) const STATE: u32 = 2;
    pub(super) const META: u32 = 3;
    pub(super) const META_TEXT: u32 = 4;
    pub(super) const SECTIONS: u32 = 5;
    pub(super) const BLOCK: u32 = 6;
    pub(super) const TOC: u32 = 7;
    pub(super) const TOC_TARGET: u32 = 8;
    pub(super) const PAGE_LABEL: u32 = 9;
    pub(super) const XPOINTER: u32 = 10;
    pub(super) const LOCATE: u32 = 11;
    pub(super) const LINKS: u32 = 12;
    pub(super) const LINK_TARGET: u32 = 13;
    pub(super) const FONTS: u32 = 14;
    pub(super) const COVER: u32 = 15;
    pub(super) const IMAGE: u32 = 16;
    pub(super) const PROGRESS: u32 = 17;
    pub(super) const CLOSE: u32 = 18;
    pub(super) const FORMATS: u32 = 19;
    pub(super) const FORMAT_FOR: u32 = 20;
    pub(super) const READERS: u32 = 21;
    pub(super) const REPORT: u32 = 22;
}

/// The `core.document` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let services = who.hosts.documents().ok_or(ERR_UNSUPPORTED)?;
    let mut result = Err(ERR_STATE);
    services.0.serve(frame, who, &mut |bound| {
        let host = &Cx {
            host: bound,
            owner: who.owner,
            library: false,
        };
        let host: &dyn DocHost = host;
        result = match op.id {
            op::OPEN => open(host, input, out),
            op::STATE => state(host, input, out),
            op::CLOSE => close(host, input),
            op::FORMATS => formats(host, input, usize::from(op.reply_rows.max(64)), out),
            op::READERS => readers(host, input, usize::from(op.reply_rows.max(64)), out),
            op::FORMAT_FOR => format_for(host, input, out),
            op::REPORT => report::report(host, input, out),
            op::META..=op::PROGRESS => read(host, op, input, out),
            _ => Err(ERR_UNSUPPORTED),
        };
    });
    result
}

fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    codec::encode_one(record, out)
}

/// `doc_state` of a document, from the table.
fn state_reply(host: &dyn DocHost, document: &Document, out: &mut [u8]) -> Result<usize, i32> {
    let mut written = Err(ERR_NOT_FOUND);
    host.format_id(document.format, &mut |format| {
        written = encode(
            &Record::ServiceDocState(wire::ServiceDocState {
                document: document.id,
                state: document.state,
                progress_ppm: document.progress_ppm,
                status: document.status,
                flags: document.flags,
                conversion: document.conversion,
                format,
            }),
            out,
        );
    })?;
    written
}

fn state(host: &dyn DocHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let reference: wire::ServiceDocRef = codec::single(input)?;
    let document = host.table().lease(host.owner(), reference.document)?;
    state_reply(host, &document, out)
}

fn close(host: &dyn DocHost, input: &[u8]) -> Result<usize, i32> {
    let reference: wire::ServiceDocRef = codec::single(input)?;
    let document = match host.table().close(host.owner(), reference.document) {
        Ok(document) => document,
        // Taken for a book to show: already ended, nothing left to do.
        Err(ERR_CANCELLED) => return Ok(0),
        Err(other) => return Err(other),
    };
    end(host, &document);
    Ok(0)
}

/// Op 1. The format's answer becomes `doc_state`; a failed open still has an
/// id until closed, a callback that fails has none.
fn open(host: &dyn DocHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let parsed = args::parse_open(input)?;
    let mut request = parsed.open;
    let long_side = host.screen_long_side();
    args::check_open(&request, long_side, host.library_call())?;
    // Root 5 names a file of another plugin: the host resolves it to its card
    // path (the format is chosen by it, the book is held under it).
    let handed = if request.root == args::ROOT_HANDED {
        Some(host.handed_open(request.path)?)
    } else {
        host.root_access(request.root)?;
        None
    };
    if !request.css_path.is_empty() {
        host.root_access(request.css_root)?;
    }
    let format = match handed.as_ref().map(|book| book.card_path.as_str()) {
        // Read as the card's own path: a sniff reads that file.
        Some(card) => host.select_format(1, card, request.format)?,
        None => host.select_format(request.root, request.path, request.format)?,
    };
    // Bit 3 `details` is for a `prepared` format's book only; any other
    // opens as before (`document.md` 2.1).
    if !host.format_prepared(format) {
        request.flags &= !table::DETAILS;
    }
    let options = Options { batch: input };
    // The host resolves the sides, a format never asks the screen and never
    // sees 0; before the conversion key, so an open with 0 and an explicit
    // open of the same size share one cache (`document.md` 3).
    let image_side = match request.image_side {
        0 => args::IMAGE_FIT,
        side => side,
    };
    let cover_side = match request.cover_side {
        0 => args::default_cover_side(long_side).min(long_side),
        side => side,
    };
    request.image_side = image_side;
    request.cover_side = cover_side;
    let conversion = host.conversion_key(&request, format, options)?;

    let table = host.table();
    // Any open that is not the library's own cover takes the cover job's
    // place at once, before its own work (`doc_cover_job`).
    if !host.library_call() {
        if let Some(job) = table.take_cover_job() {
            taken(host, &job);
        }
        // The host's cover to make too (`doc_cover_make`): a `details`
        // document is the job's alone, never handed to another open.
        if let Some(row) = table.take_cover_make() {
            taken(host, &row);
        }
    }
    let caller_shows = host.shows_screen(host.owner());
    let (id, took) = table.open(
        host.owner(),
        caller_shows,
        format,
        PREPARING,
        request.flags,
        &|owner| host.shows_screen(owner),
    )?;
    if let Some(victim) = took {
        taken(host, &victim);
    }
    let held = handed
        .as_ref()
        .map_or(request.path, |book| book.card_path.as_str());
    if let Err(error) = host.reader_opened(id, request.root, held) {
        table.discard(id);
        return Err(error);
    }
    if let Some(book) = &handed {
        host.handed_maker(id, book.maker);
    }
    table.hold_reader(id);
    let answered = (|| {
        table.set_sides(id, image_side, cover_side)?;
        let mut call = FormatCall::new(format, fmt::OPEN, id);
        call.flags = request.flags;
        call.image_side = image_side;
        call.cover_side = cover_side;
        call.conversion = conversion;
        call.options = options;
        call.css_root = request.css_root;
        call.css_path = request.css_path;
        pending::open(host, &call, input, out)
    })();
    if let Err(error) = answered {
        // The id never reached the opener; the format may hold state for it.
        if let Ok(document) = table.close(host.owner(), id) {
            end(host, &document);
        }
        return Err(error);
    }
    let document = table.lease(host.owner(), id)?;
    state_reply(host, &document, out)
}

/// Call the format; its answer is at the start of `out`.
fn run(host: &dyn DocHost, call: &FormatCall<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let length = host.call_format(call, out)?;
    if length > out.len() {
        // The adapter wrote more than it was lent: never trust the count.
        return Err(ERR_INVALID);
    }
    Ok(length)
}

/// What a call found: counts a malformed answer against the document.
fn settle(host: &dyn DocHost, document: &Document, verdict: Result<(), i32>) -> Result<(), i32> {
    match verdict {
        Ok(()) => {
            host.table().pardon(document.id);
            Ok(())
        }
        Err(ERR_INVALID) => {
            if host.table().strike(document.id) {
                host_closed(host, document, 1);
            }
            Err(ERR_INVALID)
        }
        Err(other) => Err(other),
    }
}

/// Where a block call reads: the address, the cursor and identity named, and
/// what the earlier fragments said.
#[derive(Clone, Copy)]
struct BlockAt {
    section: u32,
    offset: u32,
    cursor: u32,
    generation: u32,
    resume: Option<check::Fragment>,
}

/// What the host checks of a format's answer.
enum Check {
    Meta,
    Text { kind: u32, index: u32, offset: u32 },
    Sections { cursor: u32, generation: u32 },
    Toc { cursor: u32, generation: u32 },
    Fonts { cursor: u32, generation: u32 },
    Links { cursor: u32, generation: u32 },
    Block(BlockAt),
    At,
    Label,
    Xpointer,
    Image,
    Progress,
}

/// The request record of ops 3-17; another tag is `ERR_INVALID`.
#[derive(Clone, Copy)]
enum Ask<'a> {
    Ref(wire::ServiceDocRef),
    TextAt(wire::ServiceDocTextAt),
    Page(wire::ServiceDocPage<'a>),
    At(wire::ServiceDocAt),
    Index(wire::ServiceDocIndex),
    Xpointer(wire::ServiceDocXpointer<'a>),
    Span(wire::ServiceDocSpan),
    Href(wire::ServiceDocHref<'a>),
    Question(wire::ServiceDocImageAsk),
}

impl<'a> Ask<'a> {
    fn of(input: &'a [u8]) -> Result<Self, i32> {
        let raw = wire::RawRecord::single(input)?;
        Ok(match raw.tag() {
            wire::SERVICE_DOC_REF_TAG => Self::Ref(raw.decode()?),
            wire::SERVICE_DOC_TEXT_AT_TAG => Self::TextAt(raw.decode()?),
            wire::SERVICE_DOC_PAGE_TAG => Self::Page(raw.decode()?),
            wire::SERVICE_DOC_AT_TAG => Self::At(raw.decode()?),
            wire::SERVICE_DOC_INDEX_TAG => Self::Index(raw.decode()?),
            wire::SERVICE_DOC_XPOINTER_TAG => Self::Xpointer(raw.decode()?),
            wire::SERVICE_DOC_SPAN_TAG => Self::Span(raw.decode()?),
            wire::SERVICE_DOC_HREF_TAG => Self::Href(raw.decode()?),
            wire::SERVICE_DOC_IMAGE_ASK_TAG => Self::Question(raw.decode()?),
            _ => return Err(wire::unexpected(raw.tag())),
        })
    }

    fn document(&self) -> u32 {
        match self {
            Self::Ref(r) => r.document,
            Self::TextAt(r) => r.document,
            Self::Page(r) => r.document,
            Self::At(r) => r.document,
            Self::Index(r) => r.document,
            Self::Xpointer(r) => r.document,
            Self::Span(r) => r.document,
            Self::Href(r) => r.document,
            Self::Question(r) => r.document,
        }
    }
}

/// A continuation the host did not begin (or that changed) is `ERR_STATE`.
fn require_state(ok: bool) -> Result<(), i32> {
    if ok { Ok(()) } else { Err(ERR_STATE) }
}

fn require(ok: bool) -> Result<(), i32> {
    if ok { Ok(()) } else { Err(ERR_INVALID) }
}

/// A page of ops 5, 7, 14: `scope` is op 21's alone.
fn page(record: &Ask<'_>) -> Result<(u32, u32), i32> {
    let Ask::Page(page) = record else {
        return Err(ERR_INVALID);
    };
    require(page.scope.is_empty())?;
    args::page_state(page.cursor, page.generation)?;
    Ok((page.cursor, page.generation))
}

/// A plain position (ops 9, 10, 17): no insert, no paging.
fn position(at: &wire::ServiceDocAt) -> Result<(), i32> {
    require(at.insert == 0 && at.cursor == 0 && at.generation == 0)
}

/// Ops 5, 7, 14: the format op and what to check of its page.
fn listing(id: u32, cursor: u32, generation: u32) -> (u32, Check) {
    match id {
        op::SECTIONS => (fmt::SECTIONS, Check::Sections { cursor, generation }),
        op::TOC => (fmt::TOC, Check::Toc { cursor, generation }),
        _ => (fmt::FONTS, Check::Fonts { cursor, generation }),
    }
}

/// Ops 9, 10, 17: the format op and what to check of its answer.
fn locating(id: u32) -> (u32, Check) {
    match id {
        op::PAGE_LABEL => (fmt::PAGE_LABEL, Check::Label),
        op::XPOINTER => (fmt::XPOINTER, Check::Xpointer),
        _ => (fmt::PROGRESS, Check::Progress),
    }
}

/// Check the arguments of op `id` and make the format's call for it.
fn prepare<'a>(
    id: u32,
    document: &Document,
    record: &Ask<'a>,
) -> Result<(FormatCall<'a>, Check), i32> {
    let mut call = FormatCall::new(document.format, 0, document.id);
    call.conversion = document.conversion;
    let check = match (id, record) {
        (op::META, Ask::Ref(_)) => {
            call.op = fmt::META;
            Check::Meta
        }
        (op::COVER, Ask::Ref(_)) => {
            call.op = fmt::COVER;
            call.cover_side = document.cover_side;
            call.image_side = document.image_side;
            Check::Image
        }
        (op::META_TEXT, Ask::TextAt(r)) => {
            // 0 title, 1 author, 2 subject, 3 description, 4 series; only
            // authors and subjects are lists.
            require(r.kind <= 4)?;
            if !matches!(r.kind, 1 | 2) && r.index != 0 {
                return Err(ERR_NOT_FOUND);
            }
            call.op = fmt::META_TEXT;
            call.index = r.index;
            call.flags = r.kind;
            call.offset = r.offset;
            Check::Text {
                kind: r.kind,
                index: r.index,
                offset: r.offset,
            }
        }
        (op::SECTIONS | op::TOC | op::FONTS, page_record) => {
            let (cursor, generation) = page(page_record)?;
            call.cursor = cursor;
            call.generation = generation;
            let (format_op, check) = listing(id, cursor, generation);
            call.op = format_op;
            check
        }
        (op::BLOCK, Ask::At(r)) => block_ask(r, document, &mut call)?,
        (op::PAGE_LABEL | op::XPOINTER | op::PROGRESS, Ask::At(r)) => {
            position(r)?;
            call.section = r.section;
            call.offset = r.offset;
            let (format_op, check) = locating(id);
            call.op = format_op;
            check
        }
        (op::TOC_TARGET, Ask::Index(r)) => {
            call.op = fmt::TOC_TARGET;
            call.index = r.index;
            Check::At
        }
        (op::LOCATE, Ask::Xpointer(r)) => {
            require(!r.text.is_empty())?;
            call.op = fmt::LOCATE;
            call.text = r.text;
            Check::At
        }
        (op::LINKS, Ask::Span(r)) => {
            require(r.from <= r.to)?;
            args::page_state(r.cursor, r.generation)?;
            call.op = fmt::LINKS;
            call.section = r.section;
            call.offset = r.from;
            call.index = r.to;
            call.cursor = r.cursor;
            call.generation = r.generation;
            Check::Links {
                cursor: r.cursor,
                generation: r.generation,
            }
        }
        (op::LINK_TARGET, Ask::Href(r)) => {
            require(!r.href.is_empty())?;
            call.op = fmt::LINK_TARGET;
            call.text = r.href;
            Check::At
        }
        (op::IMAGE, Ask::Question(r)) => {
            if r.index == u32::MAX {
                // The cover is index `u32::MAX` (`document.md` 5): its own
                // size, larger when asked.
                call.op = fmt::COVER;
                call.cover_side = args::image_side(r.side, document.cover_side);
                call.image_side = document.image_side;
            } else {
                call.op = fmt::IMAGE;
                call.index = r.index;
                call.image_side = args::image_side(r.side, document.image_side);
                call.cover_side = document.cover_side;
            }
            Check::Image
        }
        _ => return Err(ERR_INVALID),
    };
    Ok((call, check))
}

/// Ops 3-17: ask the format, check its answer, pass it on.
fn read(host: &dyn DocHost, op: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let record = Ask::of(input)?;
    let id = record.document();
    let document = host.table().ready(host.owner(), id)?;
    let (call, check) = prepare(op.id, &document, &record)?;
    let length = run(host, &call, out)?;
    let verdict = verify(host, &document, &check, out, length);
    let kept = verdict.as_ref().map_or(0, |kept| *kept);
    settle(host, &document, verdict.map(|_| ()))?;
    Ok(kept)
}

/// Paging rows must move forward from the cursor.
struct Forward(Option<u32>, u32);
impl Forward {
    fn row(&mut self, index: u32) -> Result<(), i32> {
        require(self.0.map_or(index >= self.1, |last| index > last))?;
        self.0 = Some(index);
        Ok(())
    }
}

/// Check the answer in `out[..length]`; pictures are rewritten in place.
/// Returns the length to pass on.
fn verify(
    host: &dyn DocHost,
    document: &Document,
    check: &Check,
    out: &mut [u8],
    length: usize,
) -> Result<usize, i32> {
    let bytes = &out[..length];
    match *check {
        Check::Meta => {
            let meta: wire::ServiceDocMeta<'_> = check::only(bytes)?;
            check::meta(&meta)?;
            host.table().set_sections(document.id, meta.sections);
        }
        Check::Text {
            kind,
            index,
            offset,
        } => {
            let text: wire::ServiceDocText<'_> = check::only(bytes)?;
            check::text(&text, kind, index, offset)?;
        }
        Check::Sections { cursor, generation } => {
            let mut forward = Forward(None, cursor);
            let end = check::rows(bytes, PAGE_ROWS, |row: &wire::ServiceDocSection| {
                forward.row(row.index)?;
                check::section(row, cursor)
            })?;
            check::page_end(&end, cursor, generation)?;
        }
        Check::Toc { cursor, generation } => {
            let mut forward = Forward(None, cursor);
            let end = check::rows(bytes, PAGE_ROWS, |row: &wire::ServiceDocToc<'_>| {
                forward.row(row.index)?;
                check::toc(row, cursor)
            })?;
            check::page_end(&end, cursor, generation)?;
        }
        Check::Fonts { cursor, generation } => {
            let mut forward = Forward(None, cursor);
            let end = check::rows(bytes, PAGE_ROWS, |row: &wire::ServiceDocFont| {
                forward.row(row.index)?;
                check::font(row)
            })?;
            check::page_end(&end, cursor, generation)?;
        }
        Check::Links { cursor, generation } => {
            let end = check::rows(bytes, PAGE_ROWS, |row: &wire::ServiceDocLink<'_>| {
                check::link(row)
            })?;
            check::page_end(&end, cursor, generation)?;
        }
        Check::Block(at) => return block_verify(host, document, at, out, length),
        Check::At => {
            let at: wire::ServiceDocAt = check::only(bytes)?;
            check::at(&at, document.id)?;
        }
        Check::Label => {
            let _: wire::ServiceDocLabel<'_> = check::only(bytes)?;
        }
        Check::Xpointer => {
            let pointer: wire::ServiceDocXpointer<'_> = check::only(bytes)?;
            require(pointer.document == document.id && !pointer.text.is_empty())?;
        }
        Check::Image => {
            let _: wire::ServiceDocImage = check::only(bytes)?;
            let kept = images(host, document, out, length, false)?;
            // A picture that fails its check is not a picture at all: none
            // (`document.md` 1, op 15), and the format's file is at fault, not
            // the caller's request, so the document is not struck for it.
            if kept == 0 {
                return Err(ERR_NOT_FOUND);
            }
            return Ok(kept);
        }
        Check::Progress => {
            let progress: wire::ServiceDocProgress = check::only(bytes)?;
            check::progress(&progress)?;
        }
    }
    Ok(length)
}

/// Op 6: the block at `at`, or the next fragment of the one in progress.
fn block_ask(
    r: &wire::ServiceDocAt,
    document: &Document,
    call: &mut FormatCall<'_>,
) -> Result<Check, i32> {
    // `insert` is the reader's own (inserted content); never here.
    require(r.insert == 0)?;
    args::page_state(r.cursor, r.generation)?;
    // A later fragment continues the block the last call began: the table
    // keeps what the earlier fragments said (`document.md` 4).
    let resume = if r.cursor == 0 {
        None
    } else {
        let pending = document.block.ok_or(ERR_STATE)?;
        require_state(pending.is(r.section, r.offset, r.generation, r.cursor))?;
        Some(pending.fragment)
    };
    call.op = fmt::BLOCK;
    call.section = r.section;
    call.offset = r.offset;
    call.cursor = r.cursor;
    call.generation = r.generation;
    Ok(Check::Block(BlockAt {
        section: r.section,
        offset: r.offset,
        cursor: r.cursor,
        generation: r.generation,
        resume,
    }))
}

/// Op 6's answer: checked as the next fragment, then remembered when the block
/// goes on and forgotten when it ends. Only a fragment that passed moves it.
fn block_verify(
    host: &dyn DocHost,
    document: &Document,
    at: BlockAt,
    out: &mut [u8],
    length: usize,
) -> Result<usize, i32> {
    let (end, fragment) = check::block(
        &out[..length],
        at.cursor,
        at.generation,
        at.resume,
        |_, _| Ok(()),
    )?;
    if end.flags & 2 == 0 && fragment.picture_missing() {
        host.picture_left_out(document.id);
    }
    let kept = images(host, document, out, length, true)?;
    let pending = (end.flags & 2 != 0).then_some(table::Pending {
        section: at.section,
        offset: at.offset,
        generation: end.generation,
        next: end.next,
        fragment,
    });
    host.table().keep_block(document.id, pending);
    Ok(kept)
}

/// Rows per page of the document service's lists (`page_rows` 64).
const PAGE_ROWS: usize = 64;

/// Replace each format picture by the opener's image id, in place. A picture
/// the host cannot vouch for is left out of the stream (its block keeps its
/// alt text), never failing the stream. Returns the new length.
fn images(
    host: &dyn DocHost,
    document: &Document,
    out: &mut [u8],
    mut length: usize,
    in_block: bool,
) -> Result<usize, i32> {
    let mut at = 0;
    while at < length {
        let (raw, used) = wire::RawRecord::prefix(&out[at..length])?;
        if raw.tag() != wire::SERVICE_DOC_IMAGE_TAG {
            at += used;
            continue;
        }
        let image: wire::ServiceDocImage = raw.decode()?;
        match host.map_image(document, &image, in_block)? {
            Some(id) => {
                // The same fixed-width record: it fits where the old one was.
                let mut scratch = [0_u8; 32];
                let size = encode(
                    &Record::ServiceDocImage(wire::ServiceDocImage { image: id, ..image }),
                    &mut scratch,
                )?;
                require(size == used)?;
                out[at..at + used].copy_from_slice(&scratch[..size]);
                at += used;
            }
            None => {
                if in_block {
                    host.picture_left_out(document.id);
                }
                out.copy_within(at + used..length, at);
                length -= used;
            }
        }
    }
    Ok(length)
}

/// Rows and the closing `end` of a host-owned list, whole rows only.
fn list(
    out: &mut [u8],
    rows_cap: usize,
    run: impl FnOnce(&mut dyn FnMut(Record<'_>) -> ControlFlow<()>) -> Result<RowsEnd, i32>,
) -> Result<usize, i32> {
    let mut end = wire::ServiceEnd {
        flags: 0,
        total: 0,
        generation: 0,
        next: 0,
        cursor: None,
    };
    let reserved = codec::framed_len(&Record::ServiceEnd(end))?;
    let available = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
    let mut used = 0;
    let mut rows = 0;
    let mut failure = None;
    let position = run(&mut |record| {
        if rows == rows_cap {
            return ControlFlow::Break(());
        }
        match codec::framed_len(&record) {
            Ok(length) if length <= available - used => {
                match codec::encode_one(&record, &mut out[used..available]) {
                    Ok(written) => {
                        used += written;
                        rows += 1;
                        ControlFlow::Continue(())
                    }
                    Err(error) => {
                        failure = Some(error);
                        ControlFlow::Break(())
                    }
                }
            }
            Ok(_) => ControlFlow::Break(()),
            Err(error) => {
                failure = Some(error);
                ControlFlow::Break(())
            }
        }
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    // The generation is never 0; the last page says so.
    require(position.generation != 0)?;
    end.total = position.total;
    end.generation = position.generation;
    end.next = position.next;
    if position.next >= position.total {
        end.flags = 1;
    } else if rows == 0 {
        // A row that fits no page would loop its reader for ever.
        return Err(ERR_NO_SPACE);
    }
    Ok(used + codec::encode_one(&Record::ServiceEnd(end), &mut out[used..])?)
}

fn paged(input: &[u8], scope_allowed: bool) -> Result<wire::ServiceDocPage<'_>, i32> {
    let page: wire::ServiceDocPage<'_> = codec::single(input)?;
    // No document here, and `scope` is op 21's format filter alone.
    require(page.document == 0 && (scope_allowed || page.scope.is_empty()))?;
    args::page_state(page.cursor, page.generation)?;
    Ok(page)
}

fn formats(host: &dyn DocHost, input: &[u8], rows: usize, out: &mut [u8]) -> Result<usize, i32> {
    let page = paged(input, false)?;
    list(out, rows, |visit| {
        host.formats(page.cursor, page.generation, visit)
    })
}

fn readers(host: &dyn DocHost, input: &[u8], rows: usize, out: &mut [u8]) -> Result<usize, i32> {
    let page = paged(input, true)?;
    require(page.scope.is_empty() || args::id_valid(page.scope))?;
    list(out, rows, |visit| {
        host.readers(page.scope, page.cursor, page.generation, visit)
    })
}

fn format_for(host: &dyn DocHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let path: wire::ServiceDocPath<'_> = codec::single(input)?;
    // Bit 0: sniff when no extension matches.
    require(
        path.root <= 1
            && path.flags & !1 == 0
            && args::path_valid(path.path)
            && args::reachable(path.root, path.path),
    )?;
    host.root_access(path.root)?;
    let mut written = Err(ERR_NOT_FOUND);
    host.format_for(path.root, path.path, path.flags & 1 != 0, &mut |record| {
        written = encode(&record, out);
    })?;
    written
}

#[cfg(test)]
#[path = "doc_service/tests.rs"]
pub(crate) mod tests;
