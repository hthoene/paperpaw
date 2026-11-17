//! The production host of `core.document` (`docs/abi/document.md` 1 to 3): the
//! one table of open documents, the format index over the declaration store and
//! what binds them to a call, behind [`DocHost`].
//!
//! Two views of one [`DocState`]:
//!
//! - **The state itself** serves what needs no frame and is also what the
//!   host's own lifecycle calls (`owner_retired`) reach: the table, the reader
//!   hold, the conversion key, a retiring owner's formats. It cannot call a
//!   plugin, so closing a document at that point does not tell the format
//!   (op 4 `close`); the format's state for it goes with the format.
//! - **The bound host** ([`Bound`], made per call by [`DocHost::serve`]) adds
//!   what the caller's frame gives: the settings (the user's format choice),
//!   the declarations (`FormatIndex::sync`), the translator, the stack (who
//!   shows a screen) and the directed stage call to the format
//!   ([`Frame::call_native`]). The index is brought in line with the
//!   declarations when the registry generation moved (one walk, never per
//!   call); documents of a format that went end as the format stopped.
//!
//! The preparing loop (`doc_pump`, `doc_prepare`): a `prepared` format whose
//! open answered state 1 gets op 3 `prepare` from the service pump, a step a
//! pass, never inside a callback; the opener hears `event.document_changed`
//! (88) with the latest state.
//!
//! Memory: the state is the table (about 40 bytes a document), one small
//! index (under 100 bytes a format) and one `Book` per open document
//! (its path, reserved with the table); a bound host is four references.
use super::{
    doc_service::{
        Document, DocumentTable, FormatCall, FormatRef, Limits, Options, format_stopped,
    },
    fmt_call::{FrameRoute, ImageMap, Opener, Route, Stage},
    format_index::FormatIndex,
    format_scope::BookKind,
    host_services::Caller,
    owner_paths::OwnerPaths,
    paths_in_use::PathsInUse,
    registrations::{self as wire, Record},
    registry_service::fixed::{Fixed, caps, clip, translated},
    stages_wire::core_format_v1 as local,
};
use crate::abi7::context::{Frame, RowsEnd};
use crate::native_host::codec;
use alloc::{rc::Rc, string::String, vec::Vec};
use core::{cell::Cell, cell::RefCell, ops::ControlFlow};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE,
    ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_platform::Storage;

/// The most of a file a format is told by (`document.md` 3, op 1).
const SNIFF_BYTES: usize = 512;

/// `format.spec.flags` bit 4: root 3 is the book's folder and below.
const SIBLINGS: u32 = 1 << 4;

/// `format.spec.flags` bit 2: the book is a folder.
const FOLDER: u32 = 1 << 2;
/// `format.spec.flags` bit 0 `prepared`: its books are converted before they
/// are read (DOCX, MOBI); only these are opened with `details`.
const PREPARED: u32 = 1;

/// What an open document's reader hold needs to be given back.
struct Book {
    document: u32,
    /// The root the book was opened on (1: a book of the library).
    root: u32,
    /// The path as the host named it to the in-use table (root 1: the card
    /// path); empty when the book is not held.
    ///
    /// A `String`, not a `Box<str>`: boxing would shrink it, and a shrink
    /// cannot be refused.
    path: String,
    held: bool,
    /// The owner key of the plugin whose folder the book is in (root 5), else 0.
    maker: u64,
    /// The cache the format reads and writes as root 6, once the open began
    /// to make it.
    cache: Option<Cache>,
    /// The open's batch (its CSS file and options) while its `open` waits
    /// for the format plugin to be made (`doc_service::pending`); empty else.
    open: Vec<u8>,
}

/// What a document's cache is: its identity and its folder.
struct Cache {
    folder: String,
    plugin: String,
    version: String,
    book: [u8; paperpaw_sdk::library::KEY_LEN],
    conversion: u64,
    /// What the book's file and the CSS file were when the open began
    /// (`doc_cache::content`).
    content: String,
    /// The stamp is current: a finished cache found at the open, or the
    /// format answered done and the stamp was made.
    finished: bool,
    /// The bytes the stamp says the cache holds.
    stamped: u64,
    /// Opened with bit 3 `details`: never stamped by this document, an
    /// unstamped folder never removed for it (`document.md` 2.1).
    details: bool,
    /// The card path of the user's CSS file the open named (root 4 of the
    /// format's calls that may read it); `""` for none.
    css: String,
}

impl Cache {
    fn identity(&self) -> super::doc_cache::Identity<'_> {
        super::doc_cache::Identity {
            book: core::str::from_utf8(&self.book).unwrap_or_default(),
            plugin: &self.plugin,
            version: &self.version,
            conversion: self.conversion,
            content: &self.content,
        }
    }
}

/// A copy of `text`, or `ERR_NO_MEMORY`.
fn copy(text: &str) -> Result<String, i32> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(text);
    Ok(copy)
}

/// The device's document service state.
pub(crate) struct DocState {
    table: DocumentTable,
    index: RefCell<FormatIndex>,
    long_side: u32,
    /// The registry generation the index was last brought in line at; 0 never.
    synced: Cell<u32>,
    books: RefCell<Vec<Book>>,
    in_use: Rc<PathsInUse>,
    /// Formats the index refused after the store admitted them: 0 unless the
    /// two disagree (see `FormatIndex::sync`).
    skipped: Cell<u32>,
    /// Formats that went whose documents could not all be ended because the
    /// table was borrowed: the next call ends the rest (the index has
    /// forgotten them already).
    unfinished: RefCell<Vec<FormatRef>>,
    /// The kernel's clock as the event pump last gave it: events are stamped
    /// with it.
    now: Cell<u64>,
    /// Steps of the cover job that could not be made, in a row (`doc_cover_job`).
    job_later: Cell<u8>,
    /// Whether the kernel's start has returned: before, no cover job begins.
    jobs_allowed: Cell<bool>,
    /// The cover job's marker is on the card (`cover_job_marker`).
    job_guarded: Cell<bool>,
    /// A `cover` asked for a job where none may begin (before that, or with a
    /// book open for reading), or the job was displaced: it begins again when
    /// released (`cover_jobs_released`).
    job_held_back: Cell<bool>,
    /// The details job's run (`doc_details_job`).
    details: super::doc_service::details::DetailsRun,
    /// Not before this time (the kernel's clock, ms): what displaced the job
    /// is done by then.
    job_resume_at: Cell<u64>,
    /// A library list screen was up until this time (the kernel's clock, ms)
    /// at the latest: the job takes no step before the quiet moment after it
    /// is over (`cover_job_list`).
    list_quiet_until: Cell<u64>,
    /// The user's last input, the start and the last wake, plus the quiet
    /// time (the kernel's clock, ms): no cover work begins before it
    /// (`cover_job_list`).
    input_quiet_until: Cell<u64>,
    /// The key of the book of the job that runs (or ran last).
    job_key: Cell<u64>,
    /// The book the job was displaced for last, and how many times in a row;
    /// at [`DISPLACED_MAX`] the book is given up until the next start.
    job_displaced: Cell<(u64, u8)>,
    /// When the job that runs began (the kernel's clock, ms), and the time
    /// the books' jobs have run in all (key, ms): the cap per book and start
    /// ([`JOB_AWAKE_MAX_MS`]).
    job_started: Cell<u64>,
    job_used: Cell<(u64, u64)>,
    /// The book given up (0: none), for the library's failure ring.
    job_given_up: Cell<u64>,
    /// The job is to end at the next pump pass (a suspend that came inside a
    /// callback).
    job_suspend: Cell<bool>,
    /// The books whose cover the cover job made (or found none of) since the
    /// start, by path key, the oldest written over (`doc_cover_make`).
    covers_made: Cell<[u64; cover_make::COVERS_MADE]>,
    /// Which of `covers_made` were asked for since the last cover was made
    /// (bit `n` for entry `n`): the books on screen.
    covers_shown: Cell<u32>,
    /// The card, for the first bytes a file is told by (`with_storage`).
    storage: Option<Rc<dyn Storage>>,
    /// The most bytes one document's cache may hold.
    cache_cap: u64,
    /// The id of the document whose preparation was stepped last (the
    /// turn goes on after it).
    last_prepared: Cell<u32>,
    /// The books kept at most: the device's places for documents.
    books_max: usize,
    /// Whether the screen keeps colour (a picture's colour flag).
    colour: bool,
    /// Documents that ended where the image table could not be reached (the
    /// host ended them, or the table was busy): their image ids end with the
    /// next call.
    pending_images: RefCell<Vec<u32>>,
    /// A document's image ids could not be recorded as ended (logged once).
    images_lost: Cell<bool>,
    /// The document a left-out picture was last logged for (0 none): one
    /// line per run of the same document, not one a picture (a reader
    /// switching between two documents logs each switch).
    pictures_logged: Cell<u32>,
    /// An event was raised since the pump's last pass: it asks for one. An
    /// event whose owner does not run stays queued without asking again, so
    /// an idle device is not woken for it.
    fresh: Cell<bool>,
    /// The reading session and the events it raises (`doc_reading.rs`).
    reading: super::doc_reading::Reading,
}

/// How long after a displacement (a book to show, a ready cover) the job waits
/// before it begins again.
const RESUME_AFTER_MS: u64 = 2_000;
/// A job displaced this many times in a row for the same book is not started
/// again until the next start.
const DISPLACED_MAX: u8 = 3;
/// The most one book's job (its cover or its details) may run in all (awake
/// time, ms) between two starts of the device, wherever it was displaced and
/// began again; then the book counts as failed (no cover until the next start).
const JOB_AWAKE_MAX_MS: u64 = 10 * 60 * 1_000;

/// Nothing is drawn from a format's picture yet: it is left out.
struct NoImages;
impl ImageMap for NoImages {
    fn map(
        &self,
        _owner: u64,
        _document: u32,
        _image: &local::FmtImage<'_>,
        _in_block: bool,
    ) -> Result<Option<wire::ServiceDocImage>, i32> {
        Ok(None)
    }
}

/// A format's pictures as the opener's image ids: the cache files of the
/// document's folder, checked, in the image table (`document.md` 5).
struct DocImages<'a> {
    table: &'a super::image_service::ImageTable,
    storage: &'a dyn Storage,
    state: &'a DocState,
}

impl ImageMap for DocImages<'_> {
    fn map(
        &self,
        owner: u64,
        document: u32,
        image: &local::FmtImage<'_>,
        in_block: bool,
    ) -> Result<Option<wire::ServiceDocImage>, i32> {
        let books = self.state.books.try_borrow().map_err(|_| ERR_BUSY)?;
        let Some(cache) = books
            .iter()
            .find(|book| book.document == document)
            .and_then(|book| book.cache.as_ref())
        else {
            // A document without a cache has no picture to vouch for.
            return Ok(None);
        };
        self.table.map_fmt_image(
            self.storage,
            owner,
            document,
            &cache.folder,
            image,
            in_block,
            self.state.colour,
        )
    }
}

impl DocState {
    /// Whether the reader's books hold their room (tests of `release_rooms`).
    #[cfg(test)]
    pub(crate) fn books_reserved(&self) -> bool {
        self.books
            .try_borrow()
            .is_ok_and(|books| books.capacity() != 0)
    }

    /// The state for a device with `limits` open documents and a screen whose
    /// long side is `long_side` pixels; reader holds go to `in_use`.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`.
    pub(crate) fn try_new(
        limits: Limits,
        long_side: u32,
        in_use: Rc<PathsInUse>,
    ) -> Result<Self, i32> {
        let table = DocumentTable::try_new(limits)?;
        // The lists are reserved at first use (`hold_book`, `keep_unfinished`,
        // `ended_images`): a device that never opens a document keeps none.
        let (unfinished, images_pending, books) = (Vec::new(), Vec::new(), Vec::new());
        Ok(Self {
            table,
            index: RefCell::new(FormatIndex::new()),
            long_side,
            synced: Cell::new(0),
            books: RefCell::new(books),
            in_use,
            skipped: Cell::new(0),
            unfinished: RefCell::new(unfinished),
            now: Cell::new(0),
            job_later: Cell::new(0),
            jobs_allowed: Cell::new(false),
            job_guarded: Cell::new(false),
            job_held_back: Cell::new(false),
            details: super::doc_service::details::DetailsRun::default(),
            job_resume_at: Cell::new(0),
            list_quiet_until: Cell::new(0),
            input_quiet_until: Cell::new(0),
            job_key: Cell::new(0),
            job_displaced: Cell::new((0, 0)),
            job_given_up: Cell::new(0),
            job_started: Cell::new(0),
            job_used: Cell::new((0, 0)),
            job_suspend: Cell::new(false),
            covers_made: Cell::new([0; cover_make::COVERS_MADE]),
            covers_shown: Cell::new(0),
            storage: None,
            cache_cap: limits.cache_bytes(),
            last_prepared: Cell::new(0),
            books_max: limits.device,
            colour: false,
            pending_images: RefCell::new(images_pending),
            images_lost: Cell::new(false),
            pictures_logged: Cell::new(0),
            fresh: Cell::new(false),
            reading: super::doc_reading::Reading::new(),
        })
    }

    /// The state reading files' first bytes from `storage` (the formats that
    /// sniff are asked about a file whose extension no format has).
    #[must_use]
    pub(crate) fn with_storage(mut self, storage: Option<Rc<dyn Storage>>) -> Self {
        self.storage = storage;
        self
    }

    /// The state for a screen that keeps colour (or not): a picture's
    /// `doc_image.flags` bit 0.
    #[must_use]
    pub(crate) fn with_colour(mut self, colour: bool) -> Self {
        self.colour = colour;
        self
    }

    /// The image ids of `document` end with the next call (the image table is
    /// not at hand here).
    fn ended_images(&self, document: u32) {
        if let Ok(mut pending) = self.pending_images.try_borrow_mut()
            && pending.try_reserve(1).is_ok()
        {
            pending.push(document);
        } else {
            // Not recorded: the ids live until their plugin stops (at most 8
            // each). The next call with a frame says so, once.
            self.images_lost.set(true);
        }
    }

    /// The image ids of the documents that ended out of reach of the table
    /// end now (`images` is the device's table).
    fn end_pending_images(&self, images: &super::image_service::ImageTable) {
        if let Ok(mut pending) = self.pending_images.try_borrow_mut() {
            pending.retain(|document| images.document_closed(*document).is_err());
        }
    }

    /// The oldest `event.document_closed` of `owner`, encoded into `out` as
    /// the one-record batch a callback decodes (`codec::encode_event`). It
    /// is taken when it is encoded: a callback that refuses it does not get
    /// it again (as every service event).
    fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<usize>, i32> {
        let closed = self.table.take_closed(owner, |event| {
            codec::encode_event(
                &Record::EventDocumentClosed(wire::EventDocumentClosed {
                    now_ms: self.now.get(),
                    document: event.document,
                    reason: event.reason,
                }),
                out,
            )
        })?;
        if closed.is_some() {
            return Ok(closed);
        }
        // The state or the progress of a preparing document moved: its latest
        // values, one event for however many steps it took since (`document.md`
        // 2.1).
        self.table.take_changed(owner, |document| {
            codec::encode_event(
                &Record::EventDocumentChanged(wire::EventDocumentChanged {
                    now_ms: self.now.get(),
                    document: document.id,
                    state: document.state,
                    progress_ppm: document.progress_ppm,
                }),
                out,
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn skipped(&self) -> u32 {
        self.skipped.get()
    }

    fn index(&self) -> Result<core::cell::RefMut<'_, FormatIndex>, i32> {
        self.index.try_borrow_mut().map_err(|_| ERR_BUSY)
    }

    fn hold_book(
        &self,
        document: u32,
        root: u32,
        path: &str,
        plugin: Option<&str>,
    ) -> Result<(), i32> {
        let mut books = self.books.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if books.capacity() == 0 {
            books
                .try_reserve_exact(self.books_max)
                .map_err(|_| ERR_NO_MEMORY)?;
        }
        if books.len() >= books.capacity() {
            return Err(ERR_FULL);
        }
        // The in-use table names the card path: root 1 is the card's top
        // (without a leading slash), root 0 the opener's data folder, root 5
        // the handed file's card path.
        // Root 5's path is the card path the hand-over resolved to.
        let held = root == 1 || root == 0 || root == super::doc_service::ROOT_HANDED;
        let copy = if root == 0 {
            // Without a caller there is no data folder to name.
            if plugin.is_none() {
                return Err(ERR_UNSUPPORTED);
            }
            super::owner_paths::OwnerPaths::join(plugin, 0, path)?
        } else {
            let shown = if held {
                path.trim_start_matches('/')
            } else {
                path
            };
            let mut copy = String::new();
            copy.try_reserve_exact(shown.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            copy.push_str(shown);
            copy
        };
        if held {
            self.in_use
                .claim(super::busy_adapters::READER_OWNER, &copy)
                .map_err(|status| {
                    if status == ERR_NO_MEMORY {
                        ERR_FULL
                    } else {
                        status
                    }
                })?;
        }
        books.push(Book {
            document,
            root,
            path: copy,
            held,
            maker: 0,
            cache: None,
            open: Vec::new(),
        });
        Ok(())
    }

    /// The document ended at `now_ms`: its reading session (if it had one)
    /// ends, and the reader hold is given back.
    fn reader_closed_at(&self, document: u32, now_ms: u64) {
        self.reading.document_ended(now_ms, document);
        let Ok(mut books) = self.books.try_borrow_mut() else {
            return;
        };
        let Some(at) = books.iter().position(|book| book.document == document) else {
            return;
        };
        let book = books.remove(at);
        if book.held {
            // An unbalanced release is the in-use table's to count.
            let _ = self
                .in_use
                .release(super::busy_adapters::READER_OWNER, &book.path);
        }
    }

    /// The book's file name, for a `siblings` format.
    fn sibling(&self, document: u32, visit: &mut dyn FnMut(&str)) {
        if let Ok(books) = self.books.try_borrow()
            && let Some(book) = books.iter().find(|book| book.document == document)
        {
            visit(book.path.rsplit('/').next().unwrap_or(""));
        }
    }

    /// The conversion key: a hash of everything that changes the output of an
    /// open (the book's path, the format, the flags, the sides, the CSS, the
    /// options). The book's content fingerprint (LIB) joins it when the cache
    /// is made.
    fn conversion_key(
        &self,
        open: &wire::ServiceDocOpen<'_>,
        format: FormatRef,
        options: Options<'_>,
    ) -> Result<u64, i32> {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut put = |bytes: &[u8]| {
            for byte in bytes.iter().chain(&[0xff]) {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        };
        put(&open.root.to_le_bytes());
        put(open.path.as_bytes());
        self.index()?.id(format, &mut |id| put(id.as_bytes()))?;
        // Bit 3 `details` reads the book's own conversion, a reader's
        // (`document.md` 2.1): it and the background bit with it are not part
        // of the key.
        let details = super::doc_service::DETAILS;
        let flags = if open.flags & details == 0 {
            open.flags
        } else {
            open.flags & !(details | super::doc_service::BACKGROUND)
        };
        put(&flags.to_le_bytes());
        put(&open.image_side.to_le_bytes());
        put(&open.cover_side.to_le_bytes());
        put(&open.css_root.to_le_bytes());
        put(open.css_path.as_bytes());
        options.each(|option| {
            put(option.key.as_bytes());
            put(option.value.as_bytes());
        });
        Ok(hash)
    }

    fn keep_unfinished(&self, format: FormatRef) {
        // The room is reserved for every format the device can hold at the
        // first reference, and a reference is kept once: a full list cannot
        // happen. If it did, or the room cannot be had, the reference is
        // dropped and the format's documents stay until their reader closes
        // them.
        if let Ok(mut pending) = self.unfinished.try_borrow_mut() {
            if pending.capacity() == 0
                && pending
                    .try_reserve_exact(super::format_index::DEVICE)
                    .is_err()
            {
                return;
            }
            if pending.len() < pending.capacity() {
                pending.push(format);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn unfinished(&self) -> usize {
        self.unfinished.borrow().len()
    }

    fn formats_of_owner(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        if let Ok(mut index) = self.index() {
            index.remove_formats_of(owner, visit);
        }
    }

    /// The formats of `owner`, left in the index (a busy index lists none).
    fn formats_owned(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        if let Ok(index) = self.index() {
            index.formats_of(owner, visit);
        }
    }
}

macro_rules! frameless {
    () => {
        fn format_prepared(&self, format: FormatRef) -> bool {
            self.state()
                .index()
                .is_ok_and(|index| index.has_flag(format, PREPARED))
        }
        fn table(&self) -> &DocumentTable {
            &self.state().table
        }
        fn screen_long_side(&self) -> u32 {
            self.state().long_side
        }
        fn format_id(&self, format: FormatRef, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
            self.state().index()?.id(format, visit)
        }
        fn conversion_key(
            &self,
            open: &wire::ServiceDocOpen<'_>,
            format: FormatRef,
            options: Options<'_>,
        ) -> Result<u64, i32> {
            self.state().conversion_key(open, format, options)
        }
        fn document_closed(&self, document: &Document, reason: u32) {
            self.state().table.raise_closed(document, reason);
            self.state().fresh.set(true);
        }
        fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<usize>, i32> {
            self.state().next_event(owner, out)
        }
        fn events_waiting(&self) -> bool {
            self.state().fresh.get() || self.state().reading.waiting()
        }
        fn set_now(&self, now_ms: u64) {
            // The pass that takes the events starts here: what was raised
            // before it has had its turn.
            self.state().fresh.set(false);
            self.state().now.set(now_ms);
        }
        fn take_reading_edge(&self) -> Option<super::doc_reading::Edge> {
            self.state().reading.take_edge()
        }
        fn reading_sleep(&self, now_ms: u64) {
            self.state().reading.sleep(now_ms);
        }
        fn cover_jobs_allowed(&self) -> bool {
            self.state().jobs_allowed.get()
        }
        fn cover_job_held_back(&self) {
            self.state().job_held_back.set(true);
        }
        fn cover_job_displaced(&self, kind: super::doc_service::JobKind) {
            self.state().displaced(kind);
        }
        fn details_job(&self) -> Option<&dyn super::doc_service::details::DetailsJob> {
            Some(self.state())
        }
        fn cover_job_list(&self, up: bool, quiet_until: u64) -> Option<u64> {
            self.state().list_hold(up, quiet_until)
        }
        fn cover_job_input(&self, quiet_until: u64) {
            self.state().input_quiet_until.set(quiet_until);
        }
        fn cover_quiet(&self) -> bool {
            self.state().now.get() >= self.state().input_quiet_until.get()
        }
        fn cover_job_guard(&self, path: &str) -> Result<(), i32> {
            self.state().guard(path)
        }
        fn cover_job_unguard(&self) {
            self.state().unguard();
        }
        fn handed_maker(&self, document: u32, maker: u64) {
            if let Ok(mut books) = self.state().books.try_borrow_mut()
                && let Some(book) = books.iter_mut().find(|book| book.document == document)
            {
                book.maker = maker;
            }
        }
        fn document_of_maker(&self, maker: u64) -> Option<u32> {
            let books = self.state().books.try_borrow().ok()?;
            books
                .iter()
                .find(|book| maker != 0 && book.maker == maker)
                .map(|book| book.document)
        }
        fn map_image(
            &self,
            _document: &Document,
            image: &wire::ServiceDocImage,
            _in_block: bool,
        ) -> Result<Option<u32>, i32> {
            // The stage adapter mapped the picture when it translated the
            // format's answer (`DocImages`: the cache check and the id), so
            // the record carries the opener's id (0 where all are out, in a
            // block stream); a picture that failed the check is not here.
            Ok(Some(image.image))
        }
        fn formats_of_owner(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
            self.state().formats_of_owner(owner, visit);
        }
        fn formats_owned(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
            self.state().formats_owned(owner, visit);
        }
    };
}

/// The host as the lifecycle sees it: no frame, no plugin is called.
impl DocState {
    fn state(&self) -> &Self {
        self
    }
}
impl super::doc_service::DocHost for DocState {
    fn as_dyn(&self) -> &dyn super::doc_service::DocHost {
        self
    }
    fn serve(
        &self,
        frame: &dyn Frame,
        who: &Caller<'_>,
        run: &mut dyn FnMut(&dyn super::doc_service::DocHost),
    ) {
        run(&Bound {
            state: self,
            frame,
            who,
            route: None,
        });
    }
    frameless!();
    fn end_images(&self, document: &Document) {
        self.ended_images(document.id);
    }
    fn flush_images(&self, images: &super::image_service::ImageTable) {
        self.end_pending_images(images);
    }
    fn reader_opened(&self, document: u32, root: u32, path: &str) -> Result<(), i32> {
        self.hold_book(document, root, path, None)
    }
    fn reader_closed(&self, document: u32) {
        // Without a frame the pump's clock stamps the close of a session.
        self.reader_closed_at(document, self.now.get());
    }
    fn root_access(&self, _root: u32) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn select_format(&self, _: u32, _: &str, _: &str) -> Result<FormatRef, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn preparing(&self) -> bool {
        self.table.any_preparing()
    }
    fn prepare_step(
        &self,
        hosts: &super::host_services::HostServices,
        route: &dyn super::fmt_call::Route,
    ) -> super::doc_service::PrepareStep {
        let step = self.prepare_next(hosts, route);
        self.cover_job_stepped(step);
        step
    }
    fn cover_job_step(
        &self,
        hosts: &super::host_services::HostServices,
        route: &dyn super::fmt_call::Route,
        keep: &mut super::doc_service::details::Keep<'_>,
    ) -> super::doc_service::JobStep {
        self.cover_job_next(hosts, route, keep)
    }
    fn cover_at(&self, path: &str) -> super::doc_host::CoverAt {
        self.cover_at(path)
    }
    fn cover_job_active(&self) -> bool {
        // A marker with no job: the pump's look removes it.
        self.table.cover_job().is_some()
            || self.table.cover_make().is_some()
            || self.job_guarded.get()
    }
    fn only_cover_job_preparing(&self) -> bool {
        self.table.only_cover_job_preparing()
    }
    fn cover_job_suspend_later(&self) {
        self.job_suspend.set(true);
    }
    fn cover_job_given_up(&self) -> u64 {
        self.job_given_up.replace(0)
    }
    fn cover_jobs_released(&self) -> bool {
        if !self.job_held_back.get() || self.job_may_begin().is_err() {
            return false;
        }
        self.job_held_back.set(false);
        // The screens ask for their covers first: no details job meanwhile.
        self.job_resume_at
            .set(self.now.get().saturating_add(RESUME_AFTER_MS));
        true
    }
    fn release_rooms(&self) -> bool {
        if !self.table.release_rooms() {
            return false;
        }
        // No document: no book is held either.
        if let Ok(mut books) = self.books.try_borrow_mut()
            && books.is_empty()
        {
            *books = Vec::new();
        }
        true
    }
    fn allow_cover_jobs(&self) -> u64 {
        self.jobs_allowed.set(true);
        // A marker from before: the reset happened in that book's job.
        self.storage
            .as_deref()
            .map_or(0, super::cover_job_marker::take)
    }
    fn cover_job_abort(
        &self,
        hosts: &super::host_services::HostServices,
        route: &dyn super::fmt_call::Route,
    ) {
        self.cover_job_end(hosts, route);
    }
    fn call_format(&self, _: &FormatCall<'_>, _: &mut [u8]) -> Result<usize, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn close_format(&self, _document: &Document) {
        // No frame, no call: the format's state for it goes with the format.
    }
    fn formats(
        &self,
        _: u32,
        _: u32,
        _: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn readers(
        &self,
        _: &str,
        _: u32,
        _: u32,
        _: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn format_for(
        &self,
        _: u32,
        _: &str,
        _: bool,
        _: &mut dyn FnMut(Record<'_>),
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
}

/// The host of one call: the state, the caller's frame and the caller; the
/// details job's open (`doc_details_job`) calls the format through the pump's
/// `route` instead of the frame.
struct Bound<'a> {
    state: &'a DocState,
    frame: &'a dyn Frame,
    who: &'a Caller<'a>,
    route: Option<&'a dyn Route>,
}

impl Bound<'_> {
    /// The kernel's clock now, milliseconds (the pump's where the frame has
    /// no environment).
    fn clock(&self) -> u64 {
        self.frame
            .kernel_env()
            .map_or_else(|| self.state.now.get(), |env| env.now().millis())
    }

    fn state(&self) -> &DocState {
        self.state
    }

    /// A document's cache is made ready as its open begins (`document.md`
    /// 2.1): a folder that is not finished is removed, the book's older
    /// conversions go until there is room, and the folder is kept with the
    /// book for its calls (root 6). Without a card there is no cache.
    ///
    /// # Errors
    /// `ERR_BUSY` where no conversion of the book can go, as `doc_cache`
    /// says; the registry's.
    fn prepare_cache(&self, call: &FormatCall<'_>, css: &str) -> Result<(), i32> {
        let Some(storage) = self.state.storage.as_deref() else {
            return Ok(());
        };
        let details = call.flags & super::doc_service::DETAILS != 0;
        let mut plugin = Err(ERR_STATE);
        self.state.index()?.plugin_id(call.format, &mut |id| {
            plugin = copy(id);
        })?;
        let plugin = plugin?;
        let version = self.format_version(&plugin)?;
        let mut digits = [0_u8; paperpaw_sdk::library::KEY_LEN];
        let (made, content) = {
            let books = self.state.books.try_borrow().map_err(|_| ERR_BUSY)?;
            let book = books
                .iter()
                .find(|book| book.document == call.document)
                .ok_or(ERR_CANCELLED)?;
            if book.path.is_empty() {
                return Ok(());
            }
            let content = super::doc_cache::content(storage, &book.path, css)?;
            let key = paperpaw_sdk::library::key_into(&book.path, &mut digits);
            let id = super::doc_cache::Identity {
                book: key,
                plugin: &plugin,
                version: &version,
                conversion: call.conversion,
                content: &content,
            };
            let made = if details {
                // A `details` open reads what is there: no removal of its own
                // or of another conversion (no room made), no stamp
                // (`document.md` 2.1); not while another document of the book
                // and its format plugin prepares, whatever its conversion.
                let making = books.iter().any(|b| {
                    b.document != call.document
                        && b.path == book.path
                        && b.cache.as_ref().is_some_and(|c| c.plugin == plugin)
                        && self.state.table.is_preparing(b.document)
                });
                if making {
                    return Err(ERR_BUSY);
                }
                let folder = id.folder()?;
                super::doc_cache::Prepared {
                    bytes: super::doc_cache::current(storage, &id)?,
                    folder,
                    fresh: false,
                }
            } else {
                super::doc_cache::prepare(storage, &id, &|folder| {
                    books
                        .iter()
                        .any(|b| b.cache.as_ref().is_some_and(|c| c.folder == folder))
                })?
            };
            (made, content)
        };
        // What the cache holds is counted per folder: new, it starts at 0; a
        // finished one found on the card starts at what its stamp says.
        let scope = self.who.hosts.format_scope();
        if made.fresh {
            scope.reset(&made.folder);
        } else if let Some(bytes) = made.bytes {
            scope.seed(&made.folder, bytes);
        }
        let mut books = self.state.books.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if let Some(book) = books.iter_mut().find(|book| book.document == call.document) {
            book.cache = Some(Cache {
                folder: made.folder,
                plugin,
                version,
                book: digits,
                conversion: call.conversion,
                content,
                finished: made.bytes.is_some(),
                stamped: made.bytes.unwrap_or(0),
                details,
                css: copy(css)?,
            });
        }
        Ok(())
    }

    fn finish_cache(&self, document: u32) {
        self.state.finish_cache(self.who.hosts, document);
    }

    fn discard_cache(&self, document: u32) {
        self.state.discard_cache(self.who.hosts, document);
    }

    /// The version of the plugin `plugin` as the registry says it (a new
    /// version of a format makes its caches stale).
    fn format_version(&self, plugin: &str) -> Result<String, i32> {
        let asker = super::registry_service::Asker {
            owner: self.who.owner,
            plugin: self.who.plugin,
        };
        // The row is encoded into a buffer of the heap, not of the opener's
        // stack (a frame on the X4 is small), and freed with this call.
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(1024).map_err(|_| ERR_NO_MEMORY)?;
        bytes.resize(1024, 0);
        let mut version = Err(ERR_NOT_FOUND);
        self.frame
            .registry()?
            .detail(6, plugin, &asker, &mut |row| {
                if version.is_ok() {
                    return Ok(());
                }
                let used = row.encode(&mut bytes)?;
                // One bare record, exactly (the frame is the whole encoding).
                let (raw, length) = wire::RawRecord::prefix(&bytes[..used])?;
                if length != used {
                    return Err(ERR_INVALID);
                }
                if raw.tag() == wire::SERVICE_REG_PLUGIN_TAG {
                    let row: wire::ServiceRegPlugin<'_> = raw.decode()?;
                    version = copy(row.version);
                }
                Ok(())
            })?;
        version
    }

    /// The card path of the user's CSS file of an open (`""`: none, or a call
    /// that does not read it), root 4 of the format's call.
    fn css_card_path(&self, call: &FormatCall<'_>) -> Result<String, i32> {
        if call.op != super::doc_service::fmt::OPEN || call.css_path.is_empty() {
            return Ok(String::new());
        }
        if call.css_root == 1 {
            let path = call.css_path.trim_start_matches('/');
            let mut copy = String::new();
            copy.try_reserve_exact(path.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            copy.push_str(path);
            return Ok(copy);
        }
        super::owner_paths::OwnerPaths::join_for(self.who, call.css_root, call.css_path)
    }

    /// The format's call for `call.document` begins: its roots 3, 4 and 6 are
    /// the document's, until the guard is dropped.
    fn enter_scope(
        &self,
        owner: u64,
        call: &FormatCall<'_>,
        kind: BookKind,
        css: &str,
    ) -> Result<super::format_scope::Guard<'_>, i32> {
        self.state
            .enter_scope(self.who.hosts, owner, call.document, kind, css)
    }

    /// The index in line with the declarations, if the registry changed since
    /// it was. The documents of a format that went end as it stopped.
    fn sync(&self) -> Result<(), i32> {
        self.end_unfinished();
        self.end_pending_images();
        let generation = self.frame.registry()?.generation();
        if self.state.synced.get() == generation {
            return Ok(());
        }
        let mut gone = [FormatRef(0); super::format_index::DEVICE];
        let mut count = 0;
        let mut result = Err(ERR_STATE);
        self.frame.with_declarations(&mut |any| {
            let Some(store) = any.downcast_ref::<super::declaration_store::DeclarationStore>()
            else {
                return;
            };
            result = self.state.index().and_then(|mut index| {
                index.sync(store, |format| {
                    if let Some(slot) = gone.get_mut(count) {
                        *slot = format;
                        count += 1;
                    }
                })
            });
        })?;
        let skipped = result?;
        self.state
            .skipped
            .set(self.state.skipped.get().saturating_add(skipped));
        self.state.synced.set(generation);
        // The index is no longer borrowed: the documents of what went end.
        for format in gone.iter().take(count) {
            if format_stopped(self, *format).is_err() {
                // A table was borrowed: the next call ends the rest. The
                // room is reserved at the first reference, for every format
                // the device can hold.
                self.state.keep_unfinished(*format);
            }
        }
        Ok(())
    }

    /// The image ids of documents that ended out of reach of the table end.
    fn end_pending_images(&self) {
        self.state.end_pending_images(self.who.hosts.images());
        if self.state.images_lost.replace(false) {
            self.frame.log(format_args!(
                "document: image ids of an ended document kept"
            ));
        }
    }

    /// Ends the documents of formats an earlier call could not finish.
    fn end_unfinished(&self) {
        let Ok(mut pending) = self.state.unfinished.try_borrow_mut() else {
            return;
        };
        pending.retain(|format| format_stopped(self, *format).is_err());
    }

    /// Op 1's choice: the user's `core.formats.<extension>` setting, a
    /// claimant's format id.
    fn choice(&self, extension: &str, visit: &mut dyn FnMut(&str)) {
        let Some(key) = Fixed::<64>::of(&["core.formats.", extension]) else {
            return;
        };
        let wanted = self.frame.settings().text(key.as_str());
        if !wanted.is_empty() {
            visit(wanted);
        }
    }

    /// The format that reads the file `path` of `root` by its first bytes:
    /// the `sniffs` formats in priority order, each asked (op 1) with at most
    /// 512 bytes, the first to say yes. A format that fails is not asked
    /// again for this file: it is not the one that reads it.
    fn sniff_format(&self, root: u32, path: &str) -> Result<FormatRef, i32> {
        let storage = self.state.storage.as_ref().ok_or(ERR_NOT_FOUND)?;
        let joined = OwnerPaths::join_for(self.who, root, path)?;
        // The bytes are made once, off the stack, and only when a format
        // sniffs.
        let mut candidates = [FormatRef(0); super::format_index::DEVICE];
        let mut count = 0;
        self.state.index()?.sniffers(|format| {
            if let Some(slot) = candidates.get_mut(count) {
                *slot = format;
                count += 1;
            }
            ControlFlow::Continue(())
        });
        if count == 0 {
            return Err(ERR_NOT_FOUND);
        }
        let mut first = Vec::new();
        first
            .try_reserve_exact(SNIFF_BYTES)
            .map_err(|_| ERR_NO_MEMORY)?;
        first.resize(SNIFF_BYTES, 0);
        let read = storage
            .read_at(&joined, 0, &mut first)
            .map_err(super::storage::status)?;
        first.truncate(read);
        let frame_route = FrameRoute(self.frame);
        let stage = Stage {
            route: self.route.unwrap_or(&frame_route),
            images: &NoImages,
        };
        for format in candidates.iter().take(count) {
            let Some(target) = self.state.index()?.target(*format) else {
                continue;
            };
            let mut reply = [0_u8; 64];
            if stage.sniff(target, &first, &mut reply) == Ok(true) {
                return Ok(*format);
            }
        }
        Err(ERR_NOT_FOUND)
    }

    /// Whether the caller's plugin declares a reading app.
    fn declares_reader(&self) -> bool {
        let mut declares = false;
        let _ = self.frame.with_declarations(&mut |any| {
            if let Some(store) = any.downcast_ref::<super::declaration_store::DeclarationStore>() {
                store.each_reader(&mut |reader| declares |= reader.plugin == self.who.plugin);
            }
        });
        declares
    }

    fn translate_format<'r>(&'r self, row: Record<'r>) -> Record<'r> {
        match row {
            Record::ServiceDocFormat(format) => Record::ServiceDocFormat(wire::ServiceDocFormat {
                title: translated(self.frame.translator(), format.title, caps::TITLE),
                ..format
            }),
            other => other,
        }
    }
}

impl super::doc_service::DocHost for Bound<'_> {
    fn as_dyn(&self) -> &dyn super::doc_service::DocHost {
        self
    }
    frameless!();
    fn picture_left_out(&self, document: u32) {
        if self.state.pictures_logged.replace(document) != document {
            self.frame.log(format_args!(
                "document {document}: a picture was left out, its block keeps its text"
            ));
        }
    }
    fn end_images(&self, document: &Document) {
        // The ids end now where the table can be had, else with the next call.
        if self
            .who
            .hosts
            .images()
            .document_closed(document.id)
            .is_err()
        {
            self.state.ended_images(document.id);
        }
    }
    fn reader_opened(&self, document: u32, root: u32, path: &str) -> Result<(), i32> {
        self.state
            .hold_book(document, root, path, Some(self.who.plugin))
    }
    fn reader_closed(&self, document: u32) {
        self.state.reader_closed_at(document, self.clock());
    }
    fn reading_report(
        &self,
        document: &Document,
        report: &wire::ServiceDocReport<'_>,
    ) -> Result<(), i32> {
        // Only a book of the library that its opener, a reading app (it
        // declares a `reader.spec`) on top of the screen stack, reports drives
        // the session (`document.md` 2.4). Which of its screens is on top is
        // not told by the stack rows.
        let books = self.state.books.try_borrow().map_err(|_| ERR_BUSY)?;
        let book = books.iter().find(|book| book.document == document.id);
        let library = book.is_some_and(|book| book.root == 1) && !document.background;
        let facts = super::doc_reading::Facts {
            document: document.id,
            counts: library && self.declares_reader() && self.shows_screen(document.owner),
            path: book.map_or("", |book| book.path.as_str()),
        };
        self.state.reading.report(self.clock(), &facts, report)
    }

    fn root_access(&self, root: u32) -> Result<(), i32> {
        match root {
            0 => Ok(()),
            // The card's top needs `uses.storage` (decision 125).
            1 => self.who.features.feature("uses.storage"),
            // The handed file is resolved by `handed_open`.
            5 => Err(ERR_UNSUPPORTED),
            _ => Err(ERR_INVALID),
        }
    }

    #[inline(never)]
    fn handed_open(&self, path: &str) -> Result<super::doc_service::HandedBook, i32> {
        // Until the `core.ui` store is wired no file is handed.
        if self.who.hosts.ui().is_none() {
            return Err(ERR_UNSUPPORTED);
        }
        // An instance that is not a reader screen has no hand-over.
        let instance = self.frame.calling_instance().map_err(|_| ERR_DENIED)?;
        let mut handed = Err(ERR_DENIED);
        self.who.hosts.handed_file(instance, &mut |file| {
            // Only the one file the hand-over names, in a folder of its maker.
            let ours = file.path == path && matches!(file.root, 0 | 2);
            handed = if ours && file.path.len() <= 512 {
                copy(file.path).map(|text| (file.maker, file.root, text))
            } else {
                Err(ERR_DENIED)
            };
        })?;
        let (maker, root, file_path) = handed?;
        // The maker may be gone: its key never names another plugin.
        let mut id: Result<String, i32> = Err(ERR_NOT_FOUND);
        self.frame.with_declarations(&mut |any| {
            let Some(store) = any.downcast_ref::<super::declaration_store::DeclarationStore>()
            else {
                return;
            };
            if let Some(name) = store.plugin_id_of_owner(maker) {
                id = copy(name);
            }
        })?;
        let joined = OwnerPaths::join(Some(&id?), root, &file_path)?;
        let storage = self.state.storage.as_ref().ok_or(ERR_UNSUPPORTED)?;
        match storage.size(&joined) {
            Ok(_) => Ok(super::doc_service::HandedBook {
                card_path: joined,
                maker,
            }),
            Err(paperpaw_platform::StorageError::NotFound) => Err(ERR_NOT_FOUND),
            Err(error) => Err(super::storage::status(error)),
        }
    }

    fn select_format(&self, root: u32, path: &str, requested: &str) -> Result<FormatRef, i32> {
        self.sync()?;
        let chosen = self
            .state
            .index()?
            .select(path, requested, &|ext, visit| self.choice(ext, visit))?;
        // No extension matches: the formats that sniff are asked (op 1) about
        // the file's first bytes.
        match chosen {
            Some(format) => Ok(format),
            None => self.sniff_format(root, path),
        }
    }

    fn call_format(&self, call: &FormatCall<'_>, out: &mut [u8]) -> Result<usize, i32> {
        let (target, kind) = self.state.format_target(call.format)?;
        let siblings = kind == BookKind::Siblings;
        // The format's roots 3 and 4 are this call's book and CSS file for as
        // long as the guard lives.
        let css = self.css_card_path(call)?;
        let _scope = self.enter_scope(target.plugin, call, kind, &css)?;
        let frame_route = FrameRoute(self.frame);
        let route = self.route.unwrap_or(&frame_route);
        let pictures = self.state.storage.as_deref().map(|storage| DocImages {
            table: self.who.hosts.images(),
            storage,
            state: self.state,
        });
        let stage = Stage {
            route,
            images: match &pictures {
                Some(pictures) => pictures,
                None => &NoImages,
            },
        };
        let mut result = Err(ERR_NOT_FOUND);
        let mut run = |sibling: &str| {
            result = stage.call(
                Opener {
                    owner: self.who.owner,
                    sibling,
                },
                target,
                call,
                out,
            );
        };
        if siblings {
            self.state.sibling(call.document, &mut |name| run(name));
        } else {
            run("");
        }
        if result == Err(ERR_NO_SPACE) {
            self.discard_cache(call.document);
        }
        result
    }

    fn open_cache(&self, call: &FormatCall<'_>) -> Result<(), i32> {
        let css = self.css_card_path(call)?;
        self.prepare_cache(call, &css)
    }

    fn keep_open(&self, document: u32, batch: &[u8]) -> Result<(), i32> {
        self.state.keep_open(document, batch)
    }

    fn close_format(&self, document: &Document) {
        self.finish_cache(document.id);
        let Ok(target) = self
            .state
            .index()
            .and_then(|index| index.target(document.format).ok_or(ERR_CANCELLED))
        else {
            return;
        };
        // Op 4 answers nothing: the open's room, as the cover job's close
        // (`doc_cover_job`), not 4 KiB (#248).
        let room = super::doc_service::details::OPEN_REPLY;
        let mut scratch = Vec::new();
        if scratch.try_reserve_exact(room).is_err() {
            return;
        }
        scratch.resize(room, 0);
        let frame_route = FrameRoute(self.frame);
        Stage {
            route: self.route.unwrap_or(&frame_route),
            images: &NoImages,
        }
        .close(target, document.id, &mut scratch);
    }

    fn document_ready(&self, document: u32) {
        self.state.document_ready(self.who.hosts, document);
    }

    fn shows_screen(&self, owner: u64) -> bool {
        let mut shows = false;
        let _ = self.frame.with_stack(&mut |view| {
            shows = view
                .len()
                .checked_sub(1)
                .and_then(|top| view.row(top))
                .is_some_and(|row| !row.host && row.owner == owner);
        });
        shows
    }

    fn formats(
        &self,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.sync()?;
        self.state.index()?.formats(cursor, generation, &mut |row| {
            visit(self.translate_format(row))
        })
    }

    fn readers(
        &self,
        scope: &str,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.sync()?;
        self.state
            .index()?
            .readers(scope, cursor, generation, &mut |row| {
                let Record::ServiceDocReader(reader) = row else {
                    return visit(row);
                };
                // The row's title is the app's plugin's name: the index keeps
                // the plugin's id there.
                let mut flow = None;
                let named = self.frame.plugin_name(reader.title, &mut |name| {
                    flow = Some(visit(Record::ServiceDocReader(wire::ServiceDocReader {
                        title: clip(name, caps::PLUGIN_NAME),
                        ..reader
                    })));
                });
                match (named, flow) {
                    (Ok(()), Some(flow)) => flow,
                    _ => visit(Record::ServiceDocReader(reader)),
                }
            })
    }

    fn format_for(
        &self,
        root: u32,
        path: &str,
        sniff: bool,
        visit: &mut dyn FnMut(Record<'_>),
    ) -> Result<(), i32> {
        self.sync()?;
        let chosen = self
            .state
            .index()?
            .select(path, "", &|ext, visit| self.choice(ext, visit))?;
        let format = match chosen {
            Some(format) => format,
            None if sniff => self.sniff_format(root, path)?,
            None => return Err(ERR_NOT_FOUND),
        };
        self.state
            .index()?
            .row(format, &mut |row| visit(self.translate_format(row)))
    }
}

#[path = "doc_cover_job.rs"]
mod cover_job;
#[path = "doc_cover_make.rs"]
mod cover_make;
#[path = "doc_details_job.rs"]
mod details_job;
pub(crate) use cover_make::CoverAt;
#[path = "doc_prepare.rs"]
mod prepare;

#[cfg(test)]
#[path = "doc_host_tests.rs"]
mod tests;
