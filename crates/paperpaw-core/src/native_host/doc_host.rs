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
//! Not served yet, each answered as the contract's "none" and never
//! half-done: `event.document_changed` (88: nothing raises it before the
//! preparing loop), root 5, and the preparing loop itself (`format.spec.flags`
//! `prepared` formats open and answer as their op 2 does).
//!
//! Memory: the state is the table (about 40 bytes a document), one small
//! index (under 100 bytes a format) and one `Book` per open document
//! (its path, reserved with the table); a bound host is four references.
use super::{
    doc_service::{
        Document, DocumentTable, FormatCall, FormatRef, Limits, Options, format_stopped,
    },
    fmt_call::{FrameRoute, ImageMap, Opener, Stage},
    format_index::FormatIndex,
    format_scope::{BookKind, FormatBook},
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
    /// The card, for the first bytes a file is told by (`with_storage`).
    storage: Option<Rc<dyn Storage>>,
    /// The most bytes one document's cache may hold.
    cache_cap: u64,
    /// Whether the screen keeps colour (a picture's colour flag).
    colour: bool,
    /// Documents that ended where the image table could not be reached (the
    /// host ended them, or the table was busy): their image ids end with the
    /// next call.
    pending_images: RefCell<Vec<u32>>,
    /// A document's image ids could not be recorded as ended (logged once).
    images_lost: Cell<bool>,
    /// An event was raised since the pump's last pass: it asks for one. An
    /// event whose owner does not run stays queued without asking again, so
    /// an idle device is not woken for it.
    fresh: Cell<bool>,
    /// The reading session and the events it raises (`doc_reading.rs`).
    reading: super::doc_reading::Reading,
}

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
        let mut unfinished = Vec::new();
        unfinished
            .try_reserve_exact(super::format_index::DEVICE)
            .map_err(|_| ERR_NO_MEMORY)?;
        let mut images_pending = Vec::new();
        images_pending
            .try_reserve_exact(limits.device * 2)
            .map_err(|_| ERR_NO_MEMORY)?;
        let mut books = Vec::new();
        books
            .try_reserve_exact(limits.device)
            .map_err(|_| ERR_NO_MEMORY)?;
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
            storage: None,
            cache_cap: limits.cache_bytes(),
            colour: false,
            pending_images: RefCell::new(images_pending),
            images_lost: Cell::new(false),
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

    /// The oldest `event.document_closed` of `owner`, encoded into `out`. It
    /// is taken when it is encoded: a callback that refuses it does not get
    /// it again (as every service event).
    fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<usize>, i32> {
        self.table.take_closed(owner, |event| {
            codec::encode_one(
                &Record::EventDocumentClosed(wire::EventDocumentClosed {
                    now_ms: self.now.get(),
                    document: event.document,
                    reason: event.reason,
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
        put(&open.flags.to_le_bytes());
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
        // The room is reserved for every format the device can hold, and a
        // reference is kept once: a full list cannot happen. If it did, the
        // format's documents would stay until their reader closes them.
        if let Ok(mut pending) = self.unfinished.try_borrow_mut()
            && pending.len() < pending.capacity()
        {
            pending.push(format);
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
}

macro_rules! frameless {
    () => {
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

/// The host of one call: the state, the caller's frame and the caller.
struct Bound<'a> {
    state: &'a DocState,
    frame: &'a dyn Frame,
    who: &'a Caller<'a>,
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
            let made = super::doc_cache::prepare(storage, &id, &|folder| {
                books
                    .iter()
                    .any(|b| b.cache.as_ref().is_some_and(|c| c.folder == folder))
            })?;
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
            });
        }
        Ok(())
    }

    /// The last document of a cache closes: what a format wrote after the
    /// stamp was made is put in the stamp (so a reopen counts it), and the
    /// folder's count goes (it is made again from the stamp at the next open).
    /// A cache another open document uses is left as it is.
    fn finish_cache(&self, document: u32) {
        let (Some(storage), Ok(books)) =
            (self.state.storage.as_deref(), self.state.books.try_borrow())
        else {
            return;
        };
        let Some(cache) = books
            .iter()
            .find(|book| book.document == document)
            .and_then(|book| book.cache.as_ref())
        else {
            return;
        };
        let shared = books.iter().any(|b| {
            b.document != document && b.cache.as_ref().is_some_and(|c| c.folder == cache.folder)
        });
        if shared {
            return;
        }
        let scope = self.who.hosts.format_scope();
        let bytes = scope.total(&cache.folder);
        if cache.finished && bytes != cache.stamped {
            let _ = super::doc_cache::complete(storage, &cache.identity(), bytes);
        }
        scope.reset(&cache.folder);
    }

    /// A call answered `ERR_NO_SPACE` while the cache was being made: the
    /// partial cache is removed (`document.md` 3, Cache quota), unless another
    /// open document uses the folder. What a bounded removal leaves is
    /// removed with the next open, as an unfinished folder.
    fn discard_cache(&self, document: u32) {
        let (Some(storage), Ok(books)) =
            (self.state.storage.as_deref(), self.state.books.try_borrow())
        else {
            return;
        };
        let Some(cache) = books
            .iter()
            .find(|book| book.document == document)
            .and_then(|book| book.cache.as_ref())
            .filter(|cache| !cache.finished)
        else {
            return;
        };
        let shared = books.iter().any(|b| {
            b.document != document && b.cache.as_ref().is_some_and(|c| c.folder == cache.folder)
        });
        if !shared {
            let _ = super::doc_cache::clear(storage, &cache.folder);
            self.who.hosts.format_scope().reset(&cache.folder);
        }
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

    /// The format's call for `call.document` begins: its roots 3, 4 (and later
    /// 6) are the document's, until the guard is dropped.
    fn enter_scope(
        &self,
        owner: u64,
        call: &FormatCall<'_>,
        kind: BookKind,
        css: &str,
    ) -> Result<super::format_scope::Guard<'_>, i32> {
        let books = self.state.books.try_borrow().map_err(|_| ERR_BUSY)?;
        let book = books
            .iter()
            .find(|book| book.document == call.document)
            .ok_or(ERR_CANCELLED)?;
        let opened = if kind == BookKind::Siblings {
            book.path.rsplit('/').next().unwrap_or("")
        } else {
            ""
        };
        self.who.hosts.format_scope().enter(
            owner,
            &FormatBook {
                path: &book.path,
                kind,
                opened,
                document: call.document,
                cache: book.cache.as_ref().map_or("", |cache| &cache.folder),
                css,
                cache_cap: self.state.cache_cap,
                quota: None,
            },
        )
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
                // room is reserved for every format the device can hold.
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
        let route = FrameRoute(self.frame);
        let stage = Stage {
            route: &route,
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
        let (target, kind) = {
            let index = self.state.index()?;
            let kind = if index.has_flag(call.format, FOLDER) {
                BookKind::Folder
            } else if index.has_flag(call.format, SIBLINGS) {
                BookKind::Siblings
            } else {
                BookKind::File
            };
            (index.target(call.format).ok_or(ERR_CANCELLED)?, kind)
        };
        let siblings = kind == BookKind::Siblings;
        // The format's roots 3 and 4 are this call's book and CSS file for as
        // long as the guard lives.
        let css = self.css_card_path(call)?;
        if call.op == super::doc_service::fmt::OPEN {
            self.prepare_cache(call, &css)?;
        }
        let _scope = self.enter_scope(target.plugin, call, kind, &css)?;
        let route = FrameRoute(self.frame);
        let pictures = self.state.storage.as_deref().map(|storage| DocImages {
            table: self.who.hosts.images(),
            storage,
            state: self.state,
        });
        let stage = Stage {
            route: &route,
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

    fn close_format(&self, document: &Document) {
        self.finish_cache(document.id);
        let Ok(target) = self
            .state
            .index()
            .and_then(|index| index.target(document.format).ok_or(ERR_CANCELLED))
        else {
            return;
        };
        let mut scratch = Vec::new();
        if scratch.try_reserve_exact(4096).is_err() {
            return;
        }
        scratch.resize(4096, 0);
        let route = FrameRoute(self.frame);
        Stage {
            route: &route,
            images: &NoImages,
        }
        .close(target, document.id, &mut scratch);
    }

    fn document_ready(&self, document: u32) {
        let Some(storage) = self.state.storage.as_deref() else {
            return;
        };
        let stamped = {
            let Ok(books) = self.state.books.try_borrow() else {
                return;
            };
            let Some(cache) = books
                .iter()
                .find(|book| book.document == document)
                .and_then(|book| book.cache.as_ref())
                // A finished cache found at the open keeps its stamp.
                .filter(|cache| !cache.finished)
            else {
                return;
            };
            let bytes = self.who.hosts.format_scope().total(&cache.folder);
            // A stamp that cannot be made leaves the cache unfinished: the next
            // open makes it again.
            if super::doc_cache::complete(storage, &cache.identity(), bytes).is_err() {
                return;
            }
            bytes
        };
        if let Ok(mut books) = self.state.books.try_borrow_mut()
            && let Some(cache) = books
                .iter_mut()
                .find(|book| book.document == document)
                .and_then(|book| book.cache.as_mut())
        {
            cache.finished = true;
            cache.stamped = stamped;
        }
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

#[cfg(test)]
#[path = "doc_host_tests.rs"]
mod tests;
