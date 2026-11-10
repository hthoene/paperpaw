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
//! half-done: the images of a document (`map_image` leaves a picture out, so
//! no image id exists to end), `event.document_changed` (88: nothing raises
//! it before the preparing loop), sniffing a file's first 512 bytes,
//! root 5, the preparing loop and the cache (`format.spec.flags` `prepared`
//! formats open and answer as their op 2 does), the root-6 cache stamps.
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
    host_services::Caller,
    paths_in_use::PathsInUse,
    registrations::{self as wire, Record},
    registry_service::fixed::{Fixed, caps, clip, translated},
    stages_wire::core_format_v1 as local,
};
use crate::abi7::context::{Frame, RowsEnd};
use crate::native_host::codec;
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::{cell::Cell, cell::RefCell, ops::ControlFlow};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};

/// `format.spec.flags` bit 4: root 3 is the book's folder and below.
const SIBLINGS: u32 = 1 << 4;

/// What an open document's reader hold needs to be given back.
struct Book {
    document: u32,
    /// The path as the host named it to the in-use table (root 1: the card
    /// path); empty when the book is not held.
    path: Box<str>,
    held: bool,
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
    /// An event was raised since the pump's last pass: it asks for one. An
    /// event whose owner does not run stays queued without asking again, so
    /// an idle device is not woken for it.
    fresh: Cell<bool>,
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
            fresh: Cell::new(false),
        })
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

    fn reader_opened(&self, document: u32, root: u32, path: &str) -> Result<(), i32> {
        let mut books = self.books.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if books.len() >= books.capacity() {
            return Err(ERR_FULL);
        }
        // Root 1 is the card's top: the path is the card path, without a
        // leading slash. The data and install folders (roots 0, 5) are
        // resolved by the files service's own rules: not held yet.
        let held = root == 1;
        let shown = if held {
            path.trim_start_matches('/')
        } else {
            path
        };
        let mut copy = String::new();
        copy.try_reserve_exact(shown.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        copy.push_str(shown);
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
            path: copy.into_boxed_str(),
            held,
        });
        Ok(())
    }

    fn reader_closed(&self, document: u32) {
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
        fn end_images(&self, _document: &Document) {
            // No image id exists until pictures are mapped.
        }
        fn document_closed(&self, document: &Document, reason: u32) {
            self.state().table.raise_closed(document, reason);
            self.state().fresh.set(true);
        }
        fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<usize>, i32> {
            self.state().next_event(owner, out)
        }
        fn events_waiting(&self) -> bool {
            self.state().fresh.get()
        }
        fn set_now(&self, now_ms: u64) {
            // The pass that takes the events starts here: what was raised
            // before it has had its turn.
            self.state().fresh.set(false);
            self.state().now.set(now_ms);
        }
        fn reader_opened(&self, document: u32, root: u32, path: &str) -> Result<(), i32> {
            self.state().reader_opened(document, root, path)
        }
        fn reader_closed(&self, document: u32) {
            self.state().reader_closed(document);
        }
        fn map_image(
            &self,
            _document: &Document,
            _image: &wire::ServiceDocImage,
            _in_block: bool,
        ) -> Result<Option<u32>, i32> {
            Ok(None)
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
    fn state(&self) -> &DocState {
        self.state
    }

    /// The index in line with the declarations, if the registry changed since
    /// it was. The documents of a format that went end as it stopped.
    fn sync(&self) -> Result<(), i32> {
        self.end_unfinished();
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

    fn root_access(&self, root: u32) -> Result<(), i32> {
        match root {
            0 => Ok(()),
            // The card's top needs `uses.storage` (decision 125).
            1 => self.who.features.feature("uses.storage"),
            // The handed file is not wired yet.
            5 => Err(ERR_UNSUPPORTED),
            _ => Err(ERR_INVALID),
        }
    }

    fn select_format(&self, _root: u32, path: &str, requested: &str) -> Result<FormatRef, i32> {
        self.sync()?;
        let chosen = self
            .state
            .index()?
            .select(path, requested, &|ext, visit| self.choice(ext, visit))?;
        // No extension matches: the formats that sniff are asked (op 1) once
        // the host reads a file's first bytes; until then none reads it.
        chosen.ok_or(ERR_NOT_FOUND)
    }

    fn call_format(&self, call: &FormatCall<'_>, out: &mut [u8]) -> Result<usize, i32> {
        let (target, siblings) = {
            let index = self.state.index()?;
            (
                index.target(call.format).ok_or(ERR_CANCELLED)?,
                index.has_flag(call.format, SIBLINGS),
            )
        };
        let route = FrameRoute(self.frame);
        let stage = Stage {
            route: &route,
            images: &NoImages,
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
        result
    }

    fn close_format(&self, document: &Document) {
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
        _root: u32,
        path: &str,
        _sniff: bool,
        visit: &mut dyn FnMut(Record<'_>),
    ) -> Result<(), i32> {
        self.sync()?;
        let index = self.state.index()?;
        let format = index
            .select(path, "", &|ext, visit| self.choice(ext, visit))?
            .ok_or(ERR_NOT_FOUND)?;
        index.row(format, &mut |row| visit(self.translate_format(row)))
    }
}

#[cfg(test)]
#[path = "doc_host_tests.rs"]
mod tests;
