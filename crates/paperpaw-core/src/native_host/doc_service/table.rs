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
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_FULL, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND,
    ERR_STATE,
};

/// `doc_state.state`.
pub(crate) const READY: u32 = 0;
pub(crate) const PREPARING: u32 = 1;
pub(crate) const FAILED: u32 = 2;

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
    /// The block whose fragments are being read: what the earlier ones said.
    pub(crate) block: Option<Pending>,
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
}

impl DocumentTable {
    /// # Errors
    /// `ERR_NO_MEMORY` when the table cannot be allocated.
    pub(crate) fn try_new(limits: Limits) -> Result<Self, i32> {
        let mut documents = Vec::new();
        documents
            .try_reserve_exact(limits.device)
            .map_err(|_| ERR_NO_MEMORY)?;
        Ok(Self {
            limits,
            next_id: Cell::new(1),
            documents: RefCell::new(documents),
        })
    }

    fn rows(&self) -> Result<core::cell::RefMut<'_, Vec<Document>>, i32> {
        self.documents.try_borrow_mut().map_err(|_| ERR_BUSY)
    }

    /// Give `owner` a new document in `state`. Never allocates.
    ///
    /// # Errors
    /// `ERR_FULL` over a limit, `ERR_NO_SPACE` when ids are exhausted.
    pub(crate) fn open(
        &self,
        owner: u64,
        format: FormatRef,
        state: u32,
        flags: u32,
    ) -> Result<u32, i32> {
        let mut rows = self.rows()?;
        if rows.len() >= self.limits.device
            || rows.iter().filter(|d| d.owner == owner).count() >= self.limits.per_owner()
        {
            return Err(ERR_FULL);
        }
        let id = self.next_id.get();
        // Never 0 and never reused: when the counter is spent, no more opens.
        let next = id.checked_add(1).ok_or(ERR_NO_SPACE)?;
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
            block: None,
            strikes: 0,
        });
        self.next_id.set(next);
        Ok(id)
    }

    /// The document `id` as `owner` may use it.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for an id never given or already closed, `ERR_DENIED`
    /// for another opener's, `ERR_CANCELLED` once the host closed it.
    pub(crate) fn lease(&self, owner: u64, id: u32) -> Result<Document, i32> {
        let rows = self.rows()?;
        let document = rows
            .iter()
            .find(|d| d.id == id)
            .copied()
            .ok_or(ERR_NOT_FOUND)?;
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
        Ok(())
    }

    pub(crate) fn set_sides(&self, id: u32, image_side: u32, cover_side: u32) -> Result<(), i32> {
        let mut rows = self.rows()?;
        let document = rows.iter_mut().find(|d| d.id == id).ok_or(ERR_NOT_FOUND)?;
        document.image_side = image_side;
        document.cover_side = cover_side;
        Ok(())
    }

    /// `close`: free the id. A host-closed document may be closed too.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND`, `ERR_DENIED`.
    pub(crate) fn close(&self, owner: u64, id: u32) -> Result<Document, i32> {
        let mut rows = self.rows()?;
        let at = rows.iter().position(|d| d.id == id).ok_or(ERR_NOT_FOUND)?;
        if rows[at].owner != owner {
            return Err(ERR_DENIED);
        }
        Ok(rows.remove(at))
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
    pub(crate) fn close_owner(&self, owner: u64, mut ended: impl FnMut(&Document)) -> usize {
        let mut count = 0;
        loop {
            let document = {
                let Ok(mut rows) = self.documents.try_borrow_mut() else {
                    break;
                };
                let Some(at) = rows.iter().position(|d| d.owner == owner) else {
                    break;
                };
                rows.remove(at)
            };
            ended(&document);
            count += 1;
        }
        count
    }

    /// The format plugin stopped (or was uninstalled): each of its documents
    /// that is not ended yet is ended by the host with `reason`
    /// (`event.document_closed`) and stays until its opener closes it; `ended`
    /// gets each, with the table released. Returns how many.
    pub(crate) fn cancel_format(
        &self,
        format: FormatRef,
        reason: u32,
        mut ended: impl FnMut(&Document),
    ) -> usize {
        let mut count = 0;
        loop {
            let document = {
                let Ok(mut rows) = self.documents.try_borrow_mut() else {
                    break;
                };
                let Some(document) = rows
                    .iter_mut()
                    .find(|d| d.format == format && d.closed_by_host == 0)
                else {
                    break;
                };
                document.closed_by_host = if (1..=3).contains(&reason) { reason } else { 1 };
                *document
            };
            ended(&document);
            count += 1;
        }
        count
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
