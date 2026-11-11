//! The host's document table (`docs/abi/document.md` 2.1): ids, leases, limits.
//!
//! A document id is a `u32` from a checked counter: never 0 and never reused
//! while the host runs. The opener owns it; every later call names it and the
//! host checks the caller. The table is allocated once, fallibly, at its
//! device limit (X4: 2 documents, about 40 bytes each), so `open` and `close`
//! allocate nothing.
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE,
    ERR_NOT_FOUND, ERR_STATE, ERR_TIMEOUT,
};

/// `doc_state.state`.
pub(crate) const READY: u32 = 0;
pub(crate) const PREPARING: u32 = 1;
pub(crate) const FAILED: u32 = 2;
/// A preparation that answers "more" this many times in a row without
/// rising above its best progress has failed (`ERR_TIMEOUT`).
pub(crate) const STALL_CAP: u8 = 32;

/// How many taken documents are remembered as `ERR_CANCELLED` (48 bytes).
const EVICTED: usize = 4;
/// `doc_open.flags` bit 2: an open for an index build, never for a book to show.
pub(crate) const BACKGROUND: u32 = 1 << 2;
/// The owner of the host's own background job (the library's covers, book
/// details): no opener, so it takes no opener's cap and hears no
/// `event.document_closed`. No plugin has this key (`ui_store` checks it
/// against the host's other keys).
pub(crate) const HOST_JOB: u64 = u64::MAX - 2;
/// `event.document_closed.reason` 4 `needed`: its place went to a book to show.
pub(crate) const NEEDED: u32 = 4;

/// How many `event.document_closed` records the table keeps for each place it
/// has: a document that closes while its last one is still undelivered. An
/// open is admitted only while a record can be kept for every document that
/// could close (`DocumentTable::open`), so raising the event never fails.
const EVENTS_PER_PLACE: usize = 2;

/// Three malformed answers in a row close the document (`document.md` 3).
const STRIKES_TO_CLOSE: u8 = 3;

/// How many documents may be open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    pub(crate) per_owner: usize,
    pub(crate) device: usize,
}
impl Limits {
    /// What one opener may hold. Where the device has no more slots than one
    /// opener may take (the X4), an opener keeps off the last one, so that
    /// the reader's shown book can always open: a background index of the
    /// same or another plugin cannot take both.
    pub(crate) const fn per_owner(&self) -> usize {
        if self.device <= self.per_owner {
            let kept = self.device.saturating_sub(1);
            if kept == 0 { 1 } else { kept }
        } else {
            self.per_owner
        }
    }

    /// The most a book's cache may hold (`document.md` 3, Cache quota): 64
    /// MiB where the device has the X4's two places, else 4 GiB (FAT's file
    /// size bound is not the cache's).
    pub(crate) const fn cache_bytes(&self) -> u64 {
        if self.device <= self.per_owner {
            64 << 20
        } else {
            4 << 30
        }
    }

    /// The device prepares one book at a time, in open order (`document.md`
    /// 2.1: the X4's rule); elsewhere the preparing documents take turns.
    pub(crate) const fn one_preparation(&self) -> bool {
        self.device <= self.per_owner
    }

    /// At most 2 per opener and 2 device-wide: the shown book and an index build.
    pub(crate) const X4: Self = Self {
        per_owner: 2,
        device: 2,
    };
    /// At most 2 per opener and 8 device-wide.
    pub(crate) const LARGE: Self = Self {
        per_owner: 2,
        device: 8,
    };
}

/// One `event.document_closed` waiting for its opener (`document.md` 2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Closed {
    pub(crate) owner: u64,
    pub(crate) document: u32,
    pub(crate) reason: u32,
}

/// The host's opaque name of a format (an index into its declared formats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FormatRef(pub(crate) u32);

/// One open document. Plain data: the format plugin's parse state is its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Document {
    pub(crate) id: u32,
    pub(crate) owner: u64,
    pub(crate) format: FormatRef,
    pub(crate) state: u32,
    pub(crate) progress_ppm: u32,
    pub(crate) status: i32,
    pub(crate) flags: u32,
    pub(crate) conversion: u64,
    /// The longest side of a page picture and of the cover, as opened.
    pub(crate) image_side: u32,
    pub(crate) cover_side: u32,
    /// 0 while open for use, else `event.document_closed.reason`: the host
    /// closed it and every call but `close` is `ERR_CANCELLED`.
    pub(crate) closed_by_host: u32,
    /// Opened with `BACKGROUND` (an index build): its place may be taken for
    /// a book to show. `flags` is the format's state, so this is kept apart.
    pub(crate) background: bool,
    /// The library's cover job (`lib_cover`): a preparing book kept open so
    /// that the preparing loop converts it, ended by the host when it is
    /// ready or failed. One per device; always `background` as well.
    pub(crate) cover_job: bool,
    /// The host's `reader_opened` accepted this document and `reader_closed`
    /// has not been called yet: the one record of whether the book is still
    /// held, whichever path ends the document.
    pub(crate) reader_held: bool,
    /// The section count the format's `meta` answered, once it was asked
    /// (the reader's report is checked against it).
    pub(crate) sections: Option<u32>,
    /// The format plugin is gone (it stopped): it is never called for this
    /// document again, not even by its opener's `close`.
    pub(crate) format_gone: bool,
    /// The block whose fragments are being read: what the earlier ones said.
    pub(crate) block: Option<Pending>,
    /// The state or the progress moved since the opener last heard of it:
    /// `event.document_changed` (88) waits for it, one per document (the
    /// latest values are read when it is delivered).
    changed: bool,
    /// The highest progress a preparation answered, and how many answers in a
    /// row did not rise above it (`STALL_CAP`).
    best_ppm: u32,
    stalls: u8,
    strikes: u8,
}

/// A block stream in progress (`document.md` 4): the address and identity the
/// next fragment must name, and the checks' running totals (about 60 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pending {
    pub(crate) section: u32,
    pub(crate) offset: u32,
    pub(crate) generation: u32,
    pub(crate) next: u32,
    pub(crate) fragment: super::check::Fragment,
}

impl Pending {
    /// Whether a call names the continuation this expects.
    pub(crate) fn is(&self, section: u32, offset: u32, generation: u32, cursor: u32) -> bool {
        self.section == section
            && self.offset == offset
            && self.generation == generation
            && self.next == cursor
    }
}

#[derive(Debug)]
pub(crate) struct DocumentTable {
    limits: Limits,
    next_id: Cell<u32>,
    documents: RefCell<Vec<Document>>,
    /// The last documents taken for a book to show, as (id, owner): later
    /// calls on such an id are `ERR_CANCELLED`, not `ERR_NOT_FOUND`
    /// (`document.md` 2.1). Fixed size, oldest forgotten first, never grows.
    evicted: Cell<[(u32, u64); EVICTED]>,
    evicted_next: Cell<usize>,
    /// `event.document_closed` records not yet delivered, oldest first. Never
    /// grows: its room is reserved once, and an open leaves room for the close
    /// of every document that is live (see `open`).
    closed: RefCell<Vec<Closed>>,
    /// A library list screen is up (or was a moment ago): the cover job takes
    /// no step, as with a book open for reading (`DocHost::cover_job_list`).
    list_hold: Cell<bool>,
}

impl DocumentTable {
    /// # Errors
    /// `ERR_NO_MEMORY` when the table cannot be allocated.
    pub(crate) fn try_new(limits: Limits) -> Result<Self, i32> {
        // The room is reserved at the first `open` (`reserve`): a device that
        // never opens a document keeps none.
        Ok(Self {
            limits,
            closed: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
            documents: RefCell::new(Vec::new()),
            evicted: Cell::new([(0, 0); EVICTED]),
            list_hold: Cell::new(false),
            evicted_next: Cell::new(0),
        })
    }

    /// Reserves the room of the table and of its event queue, once, at the
    /// first open: every later call finds it, and never allocates.
    fn reserve(&self, rows: &mut Vec<Document>) -> Result<(), i32> {
        if rows.capacity() == 0 {
            let mut closed = self.closed.try_borrow_mut().map_err(|_| ERR_BUSY)?;
            closed
                .try_reserve_exact(self.limits.device * EVENTS_PER_PLACE)
                .map_err(|_| ERR_NO_MEMORY)?;
            if rows.try_reserve_exact(self.limits.device).is_err() {
                // Both or none, so the next open starts from the same state.
                *closed = Vec::new();
                return Err(ERR_NO_MEMORY);
            }
        }
        Ok(())
    }

    /// Hold the table as a running call does (tests of a busy table).
    #[cfg(test)]
    pub(crate) fn hold(&self) -> Result<core::cell::RefMut<'_, Vec<Document>>, i32> {
        self.rows()
    }

    fn rows(&self) -> Result<core::cell::RefMut<'_, Vec<Document>>, i32> {
        self.documents.try_borrow_mut().map_err(|_| ERR_BUSY)
    }

    /// Give `owner` a new document in `state`. Never allocates. Where the
    /// device-wide places are full and the open is not `BACKGROUND`, it takes
    /// the place of another opener's document (`document.md` 2.1): the first
    /// one opened with `BACKGROUND`, else the first whose opener `shows` no
    /// screen, oldest first. That document is returned, already out of the
    /// table (later calls on its id are `ERR_CANCELLED`), for the caller to
    /// end with the table released.
    ///
    /// Only an opener that shows a screen (`caller_shows`) may take a place;
    /// any other open that finds the device full is `ERR_FULL`.
    ///
    /// # Errors
    /// `ERR_FULL` over the opener's own cap, or full with no document to
    /// take; `ERR_NO_SPACE` when ids are exhausted (nothing is taken then).
    pub(crate) fn open(
        &self,
        owner: u64,
        caller_shows: bool,
        format: FormatRef,
        state: u32,
        flags: u32,
        shows: &dyn Fn(u64) -> bool,
    ) -> Result<(u32, Option<Document>), i32> {
        let mut rows = self.rows()?;
        self.reserve(&mut rows)?;
        if rows.iter().filter(|d| d.owner == owner).count() >= self.limits.per_owner() {
            return Err(ERR_FULL);
        }
        // A close that finds no room for its event would leave the opener
        // without it: so an open waits (`ERR_BUSY`) while undelivered events
        // and live documents (each may close) already fill the room.
        let live = rows.iter().filter(|d| d.closed_by_host == 0).count();
        let waiting = self.closed.try_borrow().map_err(|_| ERR_BUSY)?.len();
        if live + waiting >= self.limits.device * EVENTS_PER_PLACE {
            return Err(ERR_BUSY);
        }
        let id = self.next_id.get();
        // Never 0 and never reused: when the counter is spent, no more opens.
        let next = id.checked_add(1).ok_or(ERR_NO_SPACE)?;
        let mut taken = None;
        if rows.len() >= self.limits.device {
            if flags & BACKGROUND != 0 || !caller_shows {
                return Err(ERR_FULL);
            }
            let live = |d: &&Document| d.owner != owner && d.closed_by_host == 0;
            // The host's own job goes first, then an opener's background
            // document, then one whose opener shows no screen.
            let at = rows
                .iter()
                .position(|d| live(&d) && d.cover_job)
                .or_else(|| rows.iter().position(|d| live(&d) && d.background))
                .or_else(|| rows.iter().position(|d| live(&d) && !shows(d.owner)))
                .ok_or(ERR_FULL)?;
            let mut victim = rows.remove(at);
            victim.closed_by_host = NEEDED;
            self.remember(&victim);
            taken = Some(victim);
        }
        rows.push(Document {
            id,
            owner,
            format,
            state,
            progress_ppm: 0,
            status: 0,
            flags,
            conversion: 0,
            image_side: 0,
            cover_side: 0,
            closed_by_host: 0,
            background: flags & BACKGROUND != 0,
            cover_job: false,
            reader_held: false,
            format_gone: false,
            block: None,
            changed: false,
            best_ppm: 0,
            stalls: 0,
            sections: None,
            strikes: 0,
        });
        self.next_id.set(next);
        Ok((id, taken))
    }

    /// Makes the open document `id` of `owner` the cover job: kept open for
    /// the preparing loop, its place the first to be taken for a book to show.
    /// `false` (the document stays as it was) where a job runs already, or the
    /// table is held.
    pub(crate) fn start_cover_job(&self, owner: u64, id: u32) -> bool {
        let Ok(mut rows) = self.rows() else {
            return false;
        };
        if rows.iter().any(|d| d.cover_job && d.closed_by_host == 0) {
            return false;
        }
        match rows.iter_mut().find(|d| d.id == id && d.owner == owner) {
            Some(document) if document.closed_by_host == 0 => {
                document.cover_job = true;
                document.background = true;
                // The host's own job: no opener's cap, no close event.
                document.owner = HOST_JOB;
                true
            }
            _ => false,
        }
    }

    /// The cover job's document, wherever it stands (a host-closed one is
    /// still the job's, to be cleaned up).
    pub(crate) fn cover_job(&self) -> Option<Document> {
        self.documents
            .try_borrow()
            .ok()?
            .iter()
            .find(|d| d.cover_job)
            .copied()
    }

    /// Makes the open, ready document `id` of `owner` the host's **cover to
    /// make** (`doc_cover_make`): kept open past the asking call, beside the
    /// cover job, until the pump's next look asks its format for the cover and
    /// closes it. A background row of the host's (`HOST_JOB`, no opener's
    /// cap, no close event; a book to show takes its place first after the
    /// job's). `false` (the document stays as it was) where one waits already,
    /// or the table is held.
    pub(crate) fn start_cover_make(&self, owner: u64, id: u32) -> bool {
        let Ok(mut rows) = self.rows() else {
            return false;
        };
        if rows.iter().any(|d| d.owner == HOST_JOB && !d.cover_job) {
            return false;
        }
        match rows.iter_mut().find(|d| d.id == id && d.owner == owner) {
            Some(document) if document.closed_by_host == 0 && document.state == READY => {
                document.background = true;
                document.owner = HOST_JOB;
                true
            }
            _ => false,
        }
    }

    /// The cover to make's document ([`Self::start_cover_make`]), wherever it
    /// stands.
    pub(crate) fn cover_make(&self) -> Option<Document> {
        self.documents
            .try_borrow()
            .ok()?
            .iter()
            .find(|d| d.owner == HOST_JOB && !d.cover_job)
            .copied()
    }

    /// The live cover to make, taken out of the table for sleep (as
    /// [`Self::take_cover_job`]).
    pub(crate) fn take_cover_make(&self) -> Option<Document> {
        let mut rows = self.rows().ok()?;
        let at = rows
            .iter()
            .position(|d| d.owner == HOST_JOB && !d.cover_job && d.closed_by_host == 0)?;
        let mut row = rows.remove(at);
        row.closed_by_host = NEEDED;
        self.remember(&row);
        Some(row)
    }

    /// The live cover job, taken out of the table for a book to show or for
    /// sleep: as a place taken in [`Self::open`] (later calls on its id are
    /// `ERR_CANCELLED`), for the caller to end with the table released.
    pub(crate) fn take_cover_job(&self) -> Option<Document> {
        let mut rows = self.rows().ok()?;
        let at = rows
            .iter()
            .position(|d| d.cover_job && d.closed_by_host == 0)?;
        let mut job = rows.remove(at);
        job.closed_by_host = NEEDED;
        self.remember(&job);
        Some(job)
    }

    fn remember(&self, document: &Document) {
        let mut ring = self.evicted.get();
        let at = self.evicted_next.get() % EVICTED;
        ring[at] = (document.id, document.owner);
        self.evicted.set(ring);
        self.evicted_next.set((at + 1) % EVICTED);
    }

    /// Whether `id` is a document of `owner` taken for a book to show.
    fn was_taken(&self, owner: u64, id: u32) -> bool {
        self.evicted.get().contains(&(id, owner))
    }

    /// The document `id` as `owner` may use it.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for an id never given or already closed, `ERR_DENIED`
    /// for another opener's, `ERR_CANCELLED` once the host closed it.
    pub(crate) fn lease(&self, owner: u64, id: u32) -> Result<Document, i32> {
        let rows = self.rows()?;
        let Some(document) = rows.iter().find(|d| d.id == id).copied() else {
            return Err(if self.was_taken(owner, id) {
                ERR_CANCELLED
            } else {
                ERR_NOT_FOUND
            });
        };
        if document.owner != owner {
            return Err(ERR_DENIED);
        }
        if document.closed_by_host != 0 {
            return Err(ERR_CANCELLED);
        }
        Ok(document)
    }

    /// Like [`Self::lease`], and the document must be ready to read.
    ///
    /// # Errors
    /// Those of `lease`; `ERR_BUSY` while preparing, `ERR_STATE` once failed.
    pub(crate) fn ready(&self, owner: u64, id: u32) -> Result<Document, i32> {
        let document = self.lease(owner, id)?;
        match document.state {
            READY => Ok(document),
            PREPARING => Err(ERR_BUSY),
            _ => Err(ERR_STATE),
        }
    }

    /// Record the format's answer to `open` or `prepare`.
    pub(crate) fn set_state(
        &self,
        id: u32,
        state: u32,
        progress_ppm: u32,
        status: i32,
        flags: u32,
        conversion: u64,
    ) -> Result<(), i32> {
        let mut rows = self.rows()?;
        let document = rows.iter_mut().find(|d| d.id == id).ok_or(ERR_NOT_FOUND)?;
        document.state = state;
        document.progress_ppm = progress_ppm;
        document.status = status;
        document.flags = flags;
        document.conversion = conversion;
        // A preparation starts here.
        document.best_ppm = progress_ppm;
        document.stalls = 0;
        Ok(())
    }

    pub(crate) fn set_sides(&self, id: u32, image_side: u32, cover_side: u32) -> Result<(), i32> {
        let mut rows = self.rows()?;
        let document = rows.iter_mut().find(|d| d.id == id).ok_or(ERR_NOT_FOUND)?;
        document.image_side = image_side;
        document.cover_side = cover_side;
        Ok(())
    }

    /// The document whose preparation takes the next step: the oldest
    /// preparing one where the device prepares one at a time, else the first
    /// after `after` (the id of the last one served), round the table.
    pub(crate) fn next_preparing(&self, after: u32) -> Option<Document> {
        let rows = self.documents.try_borrow().ok()?;
        // The cover job waits while a book is open for reading.
        let held = self.list_hold.get() || Self::reading(&rows);
        let preparing =
            |d: &&Document| d.state == PREPARING && d.closed_by_host == 0 && !(d.cover_job && held);
        let oldest = rows.iter().find(preparing);
        if self.limits.one_preparation() {
            return oldest.copied();
        }
        rows.iter()
            .find(|d| preparing(d) && d.id > after)
            .or(oldest)
            .copied()
    }

    /// Whether any document is preparing (a held table counts as one: the
    /// step will find out).
    pub(crate) fn any_preparing(&self) -> bool {
        self.documents.try_borrow().map_or(true, |rows| {
            let held = self.list_hold.get() || Self::reading(&rows);
            rows.iter()
                .any(|d| d.state == PREPARING && d.closed_by_host == 0 && !(d.cover_job && held))
        })
    }

    /// Whether the cover job is held back for a list screen.
    pub(crate) fn list_held(&self) -> bool {
        self.list_hold.get()
    }

    /// Sets whether the cover job is held back for a list screen; `true` when
    /// that changed (the caller moves the job's clock).
    pub(crate) fn set_list_hold(&self, hold: bool) -> bool {
        self.list_hold.replace(hold) != hold
    }

    /// Whether a book is open for reading or showing: a live document that is
    /// neither the cover job nor a background one (an index build). The cover
    /// job takes no step then (`doc_cover_job`).
    fn reading(rows: &[Document]) -> bool {
        rows.iter()
            .any(|d| !d.cover_job && !d.background && d.closed_by_host == 0)
    }

    /// Whether a book is open for reading or showing, besides the document
    /// `besides` (0: none): a table that is held counts as one.
    pub(crate) fn reading_besides(&self, besides: u32) -> bool {
        self.documents.try_borrow().map_or(true, |rows| {
            rows.iter()
                .any(|d| d.id != besides && !d.cover_job && !d.background && d.closed_by_host == 0)
        })
    }

    /// Whether every preparing document is the cover job (and one is): the
    /// preparing loop's "more work" then does not hold the device awake.
    pub(crate) fn only_cover_job_preparing(&self) -> bool {
        self.documents.try_borrow().is_ok_and(|rows| {
            let mut any = false;
            for d in rows
                .iter()
                .filter(|d| d.state == PREPARING && d.closed_by_host == 0)
            {
                if !d.cover_job {
                    return false;
                }
                any = true;
            }
            any
        })
    }

    /// The format's answer to a `prepare`: the document's state, progress,
    /// status and flags. A preparation that answers "more" [`STALL_CAP`] times
    /// in a row without rising above its best progress has failed with
    /// `ERR_TIMEOUT` (`document.md` 2.1), as if the format had said so.
    /// Returns whether the state or the progress moved (the opener hears of it
    /// with `event.document_changed`), and the state and status now held.
    ///
    /// # Errors
    /// `ERR_BUSY` (the table is borrowed), `ERR_NOT_FOUND`.
    pub(crate) fn set_prepared(
        &self,
        id: u32,
        state: u32,
        progress_ppm: u32,
        status: i32,
        flags: u32,
    ) -> Result<(bool, u32, i32), i32> {
        let mut rows = self.rows()?;
        let document = rows.iter_mut().find(|d| d.id == id).ok_or(ERR_NOT_FOUND)?;
        let (mut state, mut status) = (state, status);
        if state == PREPARING {
            if progress_ppm > document.best_ppm {
                document.best_ppm = progress_ppm;
                document.stalls = 0;
            } else {
                document.stalls = document.stalls.saturating_add(1);
                if document.stalls >= STALL_CAP {
                    state = FAILED;
                    status = ERR_TIMEOUT;
                }
            }
        }
        let moved = document.state != state || document.progress_ppm != progress_ppm;
        document.state = state;
        document.progress_ppm = progress_ppm;
        document.status = status;
        document.flags = flags;
        document.changed |= moved;
        Ok((moved, state, status))
    }

    /// The state or the progress of `owner`'s oldest changed document, given
    /// to `encode`; the document counts as told when `encode` answers `Ok`
    /// (later changes raise it again), and as still waiting when it fails.
    ///
    /// # Errors
    /// `ERR_BUSY` when the table is borrowed; what `encode` answers.
    pub(crate) fn take_changed(
        &self,
        owner: u64,
        encode: impl FnOnce(&Document) -> Result<usize, i32>,
    ) -> Result<Option<usize>, i32> {
        let mut rows = self.rows()?;
        let Some(document) = rows
            .iter_mut()
            .find(|d| d.owner == owner && d.changed && d.closed_by_host == 0)
        else {
            return Ok(None);
        };
        let written = encode(document)?;
        document.changed = false;
        Ok(Some(written))
    }

    /// Whether an `event.document_changed` waits for any opener.
    pub(crate) fn changed_waiting(&self) -> bool {
        self.documents
            .try_borrow()
            .map_or(true, |rows| rows.iter().any(|d| d.changed))
    }

    /// `close`: free the id. A host-closed document may be closed too.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND`, `ERR_DENIED`; `ERR_CANCELLED` for a document taken for a
    /// book to show: it is already ended and the `close` succeeds with nothing
    /// more to do.
    pub(crate) fn close(&self, owner: u64, id: u32) -> Result<Document, i32> {
        let mut rows = self.rows()?;
        let Some(at) = rows.iter().position(|d| d.id == id) else {
            return Err(if self.was_taken(owner, id) {
                ERR_CANCELLED
            } else {
                ERR_NOT_FOUND
            });
        };
        if rows[at].owner != owner {
            return Err(ERR_DENIED);
        }
        Ok(rows.remove(at))
    }

    /// Keep `event.document_closed` for `document`'s opener. Never fails for
    /// a document that was live: `open` left room for it.
    pub(crate) fn raise_closed(&self, document: &Document, reason: u32) {
        // The host's own job has no opener to tell.
        if document.owner == HOST_JOB {
            return;
        }
        let Ok(mut events) = self.closed.try_borrow_mut() else {
            debug_assert!(false, "the event queue is never held across a call");
            return;
        };
        if events.len() < events.capacity() {
            events.push(Closed {
                owner: document.owner,
                document: document.id,
                reason,
            });
        } else {
            debug_assert!(false, "an open leaves room for each live document's close");
        }
    }

    /// The oldest undelivered event of `owner`, given to `encode`; it is
    /// forgotten when `encode` answers `Ok`, and kept when it fails.
    ///
    /// # Errors
    /// `ERR_BUSY` when the queue is borrowed; what `encode` answers.
    pub(crate) fn take_closed(
        &self,
        owner: u64,
        encode: impl FnOnce(&Closed) -> Result<usize, i32>,
    ) -> Result<Option<usize>, i32> {
        let mut events = self.closed.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let Some(at) = events.iter().position(|event| event.owner == owner) else {
            return Ok(None);
        };
        let written = encode(&events[at])?;
        events.remove(at);
        Ok(Some(written))
    }

    /// Whether any event waits for its opener.
    #[cfg(test)]
    pub(crate) fn has_closed(&self) -> bool {
        self.closed
            .try_borrow()
            .map_or(true, |events| !events.is_empty())
    }

    /// `reader_opened` accepted `id`: its book is held until `release_reader`.
    pub(crate) fn hold_reader(&self, id: u32) {
        if let Ok(mut rows) = self.documents.try_borrow_mut()
            && let Some(document) = rows.iter_mut().find(|d| d.id == id)
        {
            document.reader_held = true;
        }
    }

    /// `reader_closed` was called for `id`: it is not held any more.
    pub(crate) fn release_reader(&self, id: u32) {
        if let Ok(mut rows) = self.documents.try_borrow_mut()
            && let Some(document) = rows.iter_mut().find(|d| d.id == id)
        {
            document.reader_held = false;
        }
    }

    /// Free an id that never reached its opener (a failed `open`).
    pub(crate) fn discard(&self, id: u32) {
        if let Ok(mut rows) = self.documents.try_borrow_mut() {
            rows.retain(|d| d.id != id);
        }
    }

    /// The opener stopped: free all its ids; `ended` gets each document, with
    /// the table released (the host tells the format to close it). Returns how
    /// many.
    ///
    /// # Errors
    /// `ERR_BUSY` when the table is borrowed: what is left stays, and a retry
    /// ends it.
    pub(crate) fn close_owner(
        &self,
        owner: u64,
        mut ended: impl FnMut(&Document),
    ) -> Result<usize, i32> {
        let mut count = 0;
        // Its opener is gone: no one is left to hear an event.
        self.closed
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .retain(|event| event.owner != owner);
        loop {
            let document = {
                let mut rows = self.rows()?;
                let Some(at) = rows.iter().position(|d| d.owner == owner) else {
                    break;
                };
                rows.remove(at)
            };
            ended(&document);
            count += 1;
        }
        Ok(count)
    }

    /// The format plugin stopped (or was uninstalled): each of its documents
    /// that is not ended yet is ended by the host with `reason`
    /// (`event.document_closed`) and stays until its opener closes it; `ended`
    /// gets each, with the table released. Returns how many.
    ///
    /// `retiring` is the owner that is going away with the format (a plugin
    /// that is both opener and format): its documents are marked gone and
    /// closed like the others, but `ended` does not get them (it hears no
    /// event) and they are not counted; its opener cleanup follows.
    ///
    /// # Errors
    /// `ERR_BUSY` when the table is borrowed: a retry finishes the rest.
    pub(crate) fn cancel_format(
        &self,
        format: FormatRef,
        reason: u32,
        retiring: Option<u64>,
        mut ended: impl FnMut(&Document),
    ) -> Result<usize, i32> {
        let mut count = 0;
        // Every document of the format, also one the host closed earlier (three
        // malformed answers), never reaches the stopped format again.
        self.rows()?
            .iter_mut()
            .filter(|d| d.format == format)
            .for_each(|d| d.format_gone = true);
        loop {
            let document = {
                let mut rows = self.rows()?;
                let Some(document) = rows
                    .iter_mut()
                    .find(|d| d.format == format && d.closed_by_host == 0)
                else {
                    break;
                };
                document.closed_by_host = if (1..=3).contains(&reason) { reason } else { 1 };
                document.format_gone = true;
                *document
            };
            if retiring == Some(document.owner) {
                continue;
            }
            ended(&document);
            count += 1;
        }
        Ok(count)
    }

    /// The host ends a document (format stopped, card gone, book moved): the
    /// opener sees `reason` in `event.document_closed` and every later call
    /// but `close` is `ERR_CANCELLED`. Reasons 0 and above 3 read as 1.
    pub(crate) fn cancel(&self, id: u32, reason: u32) -> Result<Document, i32> {
        let mut rows = self.rows()?;
        let document = rows.iter_mut().find(|d| d.id == id).ok_or(ERR_NOT_FOUND)?;
        document.closed_by_host = if (1..=3).contains(&reason) { reason } else { 1 };
        Ok(*document)
    }

    /// The host ends one document on its own with `reason` (1, 2 or 3): as
    /// [`Self::cancel`], but `None` for one that is already ended, so no end
    /// path runs twice.
    ///
    /// # Errors
    /// `ERR_INVALID` for another reason, `ERR_NOT_FOUND`, `ERR_BUSY`.
    pub(crate) fn end_by_host(&self, id: u32, reason: u32) -> Result<Option<Document>, i32> {
        if !(1..=3).contains(&reason) {
            return Err(ERR_INVALID);
        }
        let mut rows = self.rows()?;
        let document = rows.iter_mut().find(|d| d.id == id).ok_or(ERR_NOT_FOUND)?;
        if document.closed_by_host != 0 {
            return Ok(None);
        }
        document.closed_by_host = reason;
        Ok(Some(*document))
    }

    /// A format answered with a malformed or foreign row. Returns true when
    /// this was the third in a row and the document is now closed (reason 1).
    pub(crate) fn strike(&self, id: u32) -> bool {
        let Ok(mut rows) = self.documents.try_borrow_mut() else {
            return false;
        };
        let Some(document) = rows.iter_mut().find(|d| d.id == id) else {
            return false;
        };
        document.strikes = document.strikes.saturating_add(1);
        if document.strikes >= STRIKES_TO_CLOSE && document.closed_by_host == 0 {
            document.closed_by_host = 1;
            return true;
        }
        false
    }

    /// Remember (or forget) the block stream in progress for `id`.
    pub(crate) fn keep_block(&self, id: u32, block: Option<Pending>) {
        if let Ok(mut rows) = self.documents.try_borrow_mut()
            && let Some(document) = rows.iter_mut().find(|d| d.id == id)
        {
            document.block = block;
        }
    }

    /// The format said the document has `sections` sections.
    pub(crate) fn set_sections(&self, id: u32, sections: u32) {
        if let Ok(mut rows) = self.documents.try_borrow_mut()
            && let Some(document) = rows.iter_mut().find(|d| d.id == id)
        {
            document.sections = Some(sections);
        }
    }

    /// A good answer ends a run of bad ones.
    pub(crate) fn pardon(&self, id: u32) {
        if let Ok(mut rows) = self.documents.try_borrow_mut()
            && let Some(document) = rows.iter_mut().find(|d| d.id == id)
        {
            document.strikes = 0;
        }
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.documents.borrow().capacity()
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.documents.borrow().len()
    }
    #[cfg(test)]
    pub(crate) fn set_next_id(&self, id: u32) {
        self.next_id.set(id);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;
    use paperpaw_native::host_glue::ERR_BUSY;

    #[test]
    fn a_borrowed_table_is_reported_not_skipped() {
        let table = DocumentTable::try_new(Limits::LARGE).unwrap();
        let format = FormatRef(1);
        let id = table.open(7, false, format, 0, 0, &|_| false).unwrap().0;
        let held = table.documents.borrow();
        assert_eq!(table.close_owner(7, |_| ()), Err(ERR_BUSY));
        assert_eq!(table.cancel_format(format, 1, None, |_| ()), Err(ERR_BUSY));
        drop(held);
        // Nothing was lost: a retry ends it.
        assert_eq!(table.cancel_format(format, 1, None, |_| ()), Ok(1));
        assert_eq!(table.close_owner(7, |d| assert_eq!(d.id, id)), Ok(1));
    }
}
