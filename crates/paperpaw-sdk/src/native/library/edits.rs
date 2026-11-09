//! Staged annotation edits (`docs/abi/library.md` 2.2, ops 17-21 and 26): a plugin
//! that saves a whole merged set at once (KOReader) opens a transaction on a book
//! at the annotation generation it read, stages edits, and commits them all or
//! none.
use super::{annotations::AnnotationKind, *};

/// Most edits one [`Library::edit_add`] call takes (`library.md` 2.2).
pub const MAX_EDITS_PER_CALL: usize = 3;
/// Longest note an edit carries (3 KiB, as in interface 6).
pub const NOTE_MAX_BYTES: usize = 3 * 1024;
/// Longest text or note sent inline; longer ones travel in parts.
const INLINE_MAX_BYTES: usize = 512;
/// Most bytes of one part (`lib_edit_text.data`).
const PART_MAX_BYTES: usize = wire::SERVICE_LIB_EDIT_TEXT_DATA_MAX_BYTES;
/// Bits of `lib_edit.flags`, as the annotation rows.
const NOTED: u32 = 1;
const HAS_CONVERSION: u32 = 1 << 3;
const HAS_XPOINTERS: u32 = 1 << 4;
/// `lib_edit_text.field` numbers.
const FIELD_TEXT: u32 = 0;
const FIELD_NOTE: u32 = 1;
const FIELD_START: u32 = 2;
const FIELD_END: u32 = 3;

/// What one edit does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Add,
    Replace(u32),
    Remove(u32),
    Upsert,
}

/// One edit of a staged transaction: add an annotation, replace or remove one by
/// its id, or upsert one by its `client_key`. Built with [`add`](Self::add),
/// [`upsert`](Self::upsert), [`replace`](Self::replace) or
/// [`remove`](Self::remove); every limit is checked when the edit is staged.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct AnnotationEdit<'a> {
    action: Action,
    kind: AnnotationKind,
    start: (u32, u32),
    end: (u32, u32),
    percent: u32,
    conversion: Option<u64>,
    created: u64,
    updated: u64,
    client_key: &'a str,
    text: &'a str,
    note: &'a str,
    xpointers: Option<(&'a str, &'a str)>,
}

impl<'a> AnnotationEdit<'a> {
    const fn new(action: Action, kind: AnnotationKind, start: (u32, u32), end: (u32, u32)) -> Self {
        Self {
            action,
            kind,
            start,
            end,
            percent: 0,
            conversion: None,
            created: 0,
            updated: 0,
            client_key: "",
            text: "",
            note: "",
            xpointers: None,
        }
    }

    /// Add an annotation of `kind` from `start` to `end` (section and offset
    /// each, in the conversion given by [`conversion`](Self::conversion)).
    #[must_use]
    pub const fn add(kind: AnnotationKind, start: (u32, u32), end: (u32, u32)) -> Self {
        Self::new(Action::Add, kind, start, end)
    }

    /// Add the annotation named by `client_key` (at most 64 bytes, unique per
    /// book), or update it if the book has it already: a repeated import adds
    /// nothing twice.
    #[must_use]
    pub const fn upsert(
        client_key: &'a str,
        kind: AnnotationKind,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Self {
        let mut edit = Self::new(Action::Upsert, kind, start, end);
        edit.client_key = client_key;
        edit
    }

    /// Replace the annotation with this id (from [`Annotation::id`]).
    ///
    /// # Errors
    /// Invalid for the id 0.
    pub const fn replace(
        id: u32,
        kind: AnnotationKind,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Result<Self, Error> {
        if id == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self::new(Action::Replace(id), kind, start, end))
    }

    /// Remove the annotation with this id.
    ///
    /// # Errors
    /// Invalid for the id 0.
    pub const fn remove(id: u32) -> Result<Self, Error> {
        if id == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self::new(
            Action::Remove(id),
            AnnotationKind::Bookmark,
            (0, 0),
            (0, 0),
        ))
    }

    /// The reading progress at the annotation, 0 to 100 percent.
    #[must_use]
    pub const fn percent(mut self, percent: u32) -> Self {
        self.percent = percent;
        self
    }

    /// The conversion the offsets belong to.
    #[must_use]
    pub const fn conversion(mut self, conversion: u64) -> Self {
        self.conversion = Some(conversion);
        self
    }

    /// Creation and update times, seconds since 1970 (0 unknown).
    #[must_use]
    pub const fn times(mut self, created: u64, updated: u64) -> Self {
        self.created = created;
        self.updated = updated;
        self
    }

    /// The caller's own identity of the annotation, at most 64 bytes.
    #[must_use]
    pub const fn client_key(mut self, client_key: &'a str) -> Self {
        self.client_key = client_key;
        self
    }

    /// The highlighted text; one over 512 bytes travels in parts.
    #[must_use]
    pub const fn text(mut self, text: &'a str) -> Self {
        self.text = text;
        self
    }

    /// The note, at most 3 KiB; one over 512 bytes travels in parts.
    #[must_use]
    pub const fn note(mut self, note: &'a str) -> Self {
        self.note = note;
        self
    }

    /// The start and end KOReader XPointers, each 1 to 1,024 bytes.
    #[must_use]
    pub const fn xpointers(mut self, start: &'a str, end: &'a str) -> Self {
        self.xpointers = Some((start, end));
        self
    }

    /// Check every limit; nothing is sent for an edit that fails.
    fn check(&self) -> Result<(), Error> {
        if matches!(self.action, Action::Remove(_)) {
            return Ok(());
        }
        if self.note.len() > NOTE_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        if self.percent > 100
            || self.client_key.len() > wire::SERVICE_LIB_EDIT_CLIENT_KEY_MAX_BYTES
            || matches!(self.action, Action::Upsert) && self.client_key.is_empty()
            || matches!(self.kind, AnnotationKind::Highlight) && self.start > self.end
            || matches!(self.kind, AnnotationKind::Other(n) if n.checked_add(2).is_none())
            || u32::try_from(self.text.len()).is_err()
        {
            return Err(Error::Invalid);
        }
        if let Some((start, end)) = self.xpointers {
            let cap = wire::SERVICE_LIB_ANCHOR_START_MAX_BYTES;
            if start.is_empty() || end.is_empty() || start.len() > cap || end.len() > cap {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }

    /// The `lib_edit` record: texts over the inline limit are left out and
    /// their lengths given, the XPointers are always left to their parts.
    fn record(&self) -> wire::ServiceLibEdit<'a> {
        let (action, id) = match self.action {
            Action::Add => (0, 0),
            Action::Replace(id) => (1, id),
            Action::Remove(id) => (2, id),
            Action::Upsert => (3, 0),
        };
        if matches!(self.action, Action::Remove(_)) {
            return wire::ServiceLibEdit {
                action,
                id,
                kind: 0,
                start_section: 0,
                start_offset: 0,
                end_section: 0,
                end_offset: 0,
                flags: 0,
                percent: 0,
                conversion: 0,
                created: 0,
                updated: 0,
                client_key: "",
                text: "",
                note: "",
                text_bytes: 0,
                note_bytes: 0,
            };
        }
        let inline = |text: &'a str| {
            if text.len() > INLINE_MAX_BYTES {
                ""
            } else {
                text
            }
        };
        let bytes = |text: &str| {
            if text.len() > INLINE_MAX_BYTES {
                u32::try_from(text.len()).unwrap_or(u32::MAX)
            } else {
                0
            }
        };
        wire::ServiceLibEdit {
            action,
            id,
            kind: match self.kind {
                AnnotationKind::Bookmark => 0,
                AnnotationKind::Highlight => 1,
                AnnotationKind::Other(n) => n.saturating_add(2),
            },
            start_section: self.start.0,
            start_offset: self.start.1,
            end_section: self.end.0,
            end_offset: self.end.1,
            flags: if self.note.is_empty() { 0 } else { NOTED }
                | if self.conversion.is_some() {
                    HAS_CONVERSION
                } else {
                    0
                }
                | if self.xpointers.is_some() {
                    HAS_XPOINTERS
                } else {
                    0
                },
            percent: self.percent,
            conversion: self.conversion.unwrap_or(0),
            created: self.created,
            updated: self.updated,
            client_key: self.client_key,
            text: inline(self.text),
            note: inline(self.note),
            text_bytes: bytes(self.text),
            note_bytes: bytes(self.note),
        }
    }
}

/// An open staged transaction on one book. At most one is open per plugin and two
/// on the device; it lives across callbacks until it is committed, aborted, left
/// idle for 10 minutes or the plugin stops. After an error while staging, abort
/// it: the transaction refuses to commit and answers `State`.
#[derive(Debug)]
#[must_use = "commit or abort the transaction"]
pub struct Transaction {
    id: u32,
    edits: u32,
    broken: bool,
    finished: bool,
}

impl Transaction {
    /// The number of edits staged so far.
    #[must_use]
    pub const fn edits(&self) -> u32 {
        self.edits
    }
}

/// What a commit did (`lib_committed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Committed {
    transaction: u32,
    generation: u32,
    edits: u32,
    added: u32,
}

impl Committed {
    /// The transaction's number, to read its receipt with
    /// [`Library::edit_receipt`].
    #[must_use]
    pub const fn transaction(&self) -> u32 {
        self.transaction
    }

    /// The book's annotation generation after the commit (at least 2).
    #[must_use]
    pub const fn annotation_generation(&self) -> u32 {
        self.generation
    }

    /// How many edits were applied.
    #[must_use]
    pub const fn edits(&self) -> u32 {
        self.edits
    }

    /// How many new annotations they added.
    #[must_use]
    pub const fn added(&self) -> u32 {
        self.added
    }
}

/// What was done for one edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReceiptAction {
    /// A new annotation.
    Added,
    /// An annotation replaced.
    Replaced,
    /// An annotation removed.
    Removed,
    /// An annotation updated by its `client_key`.
    Updated,
    /// An action a newer host added.
    Other(u32),
}

/// The annotation an edit made or changed (`lib_receipt`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt(wire::ServiceLibReceipt);

impl Receipt {
    /// The index of the edit in the transaction.
    #[must_use]
    pub const fn edit(&self) -> u32 {
        self.0.edit
    }

    /// The annotation's id.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.0.id
    }

    /// What was done.
    #[must_use]
    pub const fn action(&self) -> ReceiptAction {
        match self.0.action {
            0 => ReceiptAction::Added,
            1 => ReceiptAction::Replaced,
            2 => ReceiptAction::Removed,
            3 => ReceiptAction::Updated,
            n => ReceiptAction::Other(n),
        }
    }
}

/// Where a page of receipt rows continues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReceiptCursor(u32);

impl ReceiptCursor {
    /// The first page.
    #[must_use]
    pub const fn start() -> Self {
        Self(0)
    }
}

/// A page of receipt rows, validated in full before any row is exposed.
#[derive(Debug)]
pub struct ReceiptPage<'a> {
    bytes: &'a [u8],
    count: usize,
    total: u32,
    next: Option<ReceiptCursor>,
}

impl<'a> ReceiptPage<'a> {
    fn decode(bytes: &'a [u8], cursor: ReceiptCursor) -> Result<Self, Error> {
        let mut rest = bytes;
        let mut count = 0usize;
        let end = loop {
            let (raw, n) = wire::RawRecord::prefix(rest).map_err(|_| Error::Invalid)?;
            rest = rest.get(n..).ok_or(Error::Invalid)?;
            if raw.tag() == wire::SERVICE_END_TAG {
                break raw
                    .decode::<wire::ServiceEnd>()
                    .map_err(|_| Error::Invalid)?;
            }
            count += 1;
            if count > schema::PAGE_MAX_ROWS {
                return Err(Error::Invalid);
            }
            raw.decode::<wire::ServiceLibReceipt>()
                .map_err(|_| Error::Invalid)?;
        };
        let expected = cursor
            .0
            .checked_add(u32::try_from(count).map_err(|_| Error::Invalid)?)
            .ok_or(Error::Invalid)?;
        let last = end.flags == 1;
        if !rest.is_empty()
            || end.flags > 1
            || end.cursor.is_some()
            || end.next != expected
            || end.next > end.total
            || last != (end.next == end.total)
            || !last && count == 0
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            bytes,
            count,
            total: end.total,
            next: (!last).then_some(ReceiptCursor(end.next)),
        })
    }

    /// The number of edits with a receipt row.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// Where the next page starts, or `None` after the last.
    #[must_use]
    pub const fn next(&self) -> Option<ReceiptCursor> {
        self.next
    }

    /// Number of rows in this page.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether this page has no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterate the rows without allocation.
    pub fn rows(&self) -> impl Iterator<Item = Receipt> + use<'a> {
        let mut bytes = self.bytes;
        let mut left = self.count;
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let (raw, n) = wire::RawRecord::prefix(bytes).ok()?;
            bytes = bytes.get(n..)?;
            raw.decode::<wire::ServiceLibReceipt>().ok().map(Receipt)
        })
    }
}

impl Library {
    /// Open a staged transaction on a book (op 17), at the annotation generation
    /// the caller read ([`AnnotationPageEnd::generation`]). `input` is 1,024
    /// bytes and `out` 4,096.
    ///
    /// # Errors
    /// Invalid for the generation 0; `Full` when the plugin or the device has its
    /// transactions open already; `NotFound` for an unknown book; `State` for a
    /// stale catalog handle; otherwise the host's refusals.
    pub fn edit_begin(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        annotation_generation: u32,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Transaction, Error> {
        if annotation_generation == 0 {
            return Err(Error::Invalid);
        }
        let head = wire::ServiceLibEditHead {
            annotation_generation,
        };
        let n = self.annotation_call(host, schema::OP_EDIT_BEGIN, &[&book.0, &head], input, out)?;
        let row = wire::RawRecord::one(out.get(..n).ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)?
            .decode::<wire::ServiceLibTransaction>()
            .map_err(|_| Error::Invalid)?;
        Ok(Transaction {
            id: row.transaction,
            edits: 0,
            broken: false,
            finished: false,
        })
    }

    /// Stage up to 3 edits (op 18) and send their long texts, notes and their
    /// XPointers in parts (op 19). `input` is 4,096 bytes and `out` 4,096.
    ///
    /// # Errors
    /// `State` for a transaction that is finished or broken; Invalid or `NoSpace`
    /// for an edit over a limit or more than 3 edits (nothing is sent); the host's
    /// refusals, such as `NoSpace` over the transaction's cap. After any error
    /// from the host, abort the transaction.
    pub fn edit_add(
        &self,
        host: &Host<'_>,
        transaction: &mut Transaction,
        edits: &[AnnotationEdit<'_>],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if transaction.finished || transaction.broken {
            return Err(Error::State);
        }
        let Some(blank) = edits.first() else {
            return Ok(());
        };
        if edits.len() > MAX_EDITS_PER_CALL {
            return Err(Error::NoSpace);
        }
        for edit in edits {
            edit.check()?;
        }
        let before = transaction.edits;
        let added = u32::try_from(edits.len()).map_err(|_| Error::NoSpace)?;
        let next = before.checked_add(added).ok_or(Error::NoSpace)?;
        let reference = wire::ServiceLibTransactionRef {
            transaction: transaction.id,
        };
        // The array always holds 3 rows; only the first `edits.len()` are sent.
        let rows: [wire::ServiceLibEdit<'_>; MAX_EDITS_PER_CALL] =
            core::array::from_fn(|at| edits.get(at).unwrap_or(blank).record());
        let mut records: [&dyn wire::WireEncode; 4] = [&reference, &rows[0], &rows[1], &rows[2]];
        let records = &mut records[..=edits.len()];
        // From here a failure leaves a half-staged transaction.
        transaction.broken = true;
        let n = self.annotation_call(host, schema::OP_EDIT_ADD, records, input, out)?;
        if n != 0 {
            return Err(Error::Invalid);
        }
        transaction.edits = next;
        for (at, edit) in edits.iter().enumerate() {
            let index = before + u32::try_from(at).map_err(|_| Error::NoSpace)?;
            self.send_parts(host, transaction.id, index, edit, input, out)?;
        }
        transaction.broken = false;
        Ok(())
    }

    /// Send the parts an edit's record left out: its text and note over the inline
    /// limit, then its start and end XPointers.
    fn send_parts(
        &self,
        host: &Host<'_>,
        transaction: u32,
        edit: u32,
        staged: &AnnotationEdit<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if matches!(staged.action, Action::Remove(_)) {
            return Ok(());
        }
        let mut field = |number: u32, data: &str| -> Result<(), Error> {
            let mut offset = 0usize;
            while offset < data.len() {
                let mut end = offset.saturating_add(PART_MAX_BYTES).min(data.len());
                while !data.is_char_boundary(end) {
                    end -= 1;
                }
                let part = wire::ServiceLibEditText {
                    transaction,
                    edit,
                    field: number,
                    offset: u32::try_from(offset).map_err(|_| Error::NoSpace)?,
                    flags: u32::from(end == data.len()),
                    data: data.get(offset..end).ok_or(Error::Invalid)?,
                };
                let n = self.annotation_call(host, schema::OP_EDIT_TEXT, &[&part], input, out)?;
                if n != 0 {
                    return Err(Error::Invalid);
                }
                offset = end;
            }
            Ok(())
        };
        if staged.text.len() > INLINE_MAX_BYTES {
            field(FIELD_TEXT, staged.text)?;
        }
        if staged.note.len() > INLINE_MAX_BYTES {
            field(FIELD_NOTE, staged.note)?;
        }
        if let Some((start, end)) = staged.xpointers {
            field(FIELD_START, start)?;
            field(FIELD_END, end)?;
        }
        Ok(())
    }

    /// Apply every staged edit or none (op 20). A plugin advances its own sync
    /// state only after this answers `Ok`. `input` is 256 bytes and `out` 4,096.
    ///
    /// # Errors
    /// `State` when the book's annotation generation moved since
    /// [`edit_begin`](Self::edit_begin) (read again, merge, begin again) or the
    /// transaction is finished or broken; `NotFound` for an unknown annotation id;
    /// Invalid for a range outside the sections or parts that did not arrive whole;
    /// Invalid for a reply that does not belong to this transaction. The host
    /// ends the transaction on an error.
    pub fn edit_commit(
        &self,
        host: &Host<'_>,
        transaction: &mut Transaction,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Committed, Error> {
        if transaction.finished || transaction.broken {
            return Err(Error::State);
        }
        let reference = wire::ServiceLibTransactionRef {
            transaction: transaction.id,
        };
        let result = self.annotation_call(host, schema::OP_EDIT_COMMIT, &[&reference], input, out);
        transaction.finished = true;
        let n = result?;
        let row = wire::RawRecord::one(out.get(..n).ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)?
            .decode::<wire::ServiceLibCommitted>()
            .map_err(|_| Error::Invalid)?;
        if row.transaction != transaction.id || row.annotation_generation < 2 {
            return Err(Error::Invalid);
        }
        Ok(Committed {
            transaction: row.transaction,
            generation: row.annotation_generation,
            edits: row.edits,
            added: row.added,
        })
    }

    /// Give a staged transaction up (op 21); nothing is applied. Abort after an
    /// error. `input` is 256 bytes and `out` 64.
    ///
    /// # Errors
    /// `State` for a transaction that is finished already; otherwise the host's
    /// refusals.
    pub fn edit_abort(
        &self,
        host: &Host<'_>,
        transaction: &mut Transaction,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if transaction.finished {
            return Err(Error::State);
        }
        let reference = wire::ServiceLibTransactionRef {
            transaction: transaction.id,
        };
        let result = self.annotation_call(host, schema::OP_EDIT_ABORT, &[&reference], input, out);
        transaction.finished = true;
        if result? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// Read the annotation ids a committed transaction made (op 26), in pages of
    /// at most 32 rows. The receipt is kept until the plugin's next
    /// [`edit_begin`](Self::edit_begin), 10 minutes, or the plugin stops. `input`
    /// is 256 bytes and `out` 4,096.
    ///
    /// # Errors
    /// `NotFound` once the receipt is gone; Invalid for a malformed page;
    /// otherwise the host's refusals.
    pub fn edit_receipt<'a>(
        &self,
        host: &Host<'_>,
        committed: &Committed,
        cursor: ReceiptCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<ReceiptPage<'a>, Error> {
        let ask = wire::ServiceLibReceiptAsk {
            transaction: committed.transaction,
            cursor: cursor.0,
        };
        let n = self.annotation_call(host, schema::OP_EDIT_RECEIPT, &[&ask], input, out)?;
        ReceiptPage::decode(out.get(..n).ok_or(Error::Invalid)?, cursor)
    }
}
