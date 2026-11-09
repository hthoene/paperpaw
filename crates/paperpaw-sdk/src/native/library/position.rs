//! A book's identity for sync (`book_key`, op 25) and its reading position
//! (`position` and `set_position`, ops 13 and 14).
use super::{BookRef, Error, Host, Library, one, schema, wire};

/// Bit of `lib_position.flags`: the position carries an XPointer.
const HAS_XPOINTER: u32 = 1;
/// Bit of `lib_position.flags`: the position carries a conversion.
const HAS_CONVERSION: u32 = 1 << 1;
/// Bit of `lib_position.flags`: the last write was a sync write.
const FROM_SYNC: u32 = 1 << 2;
/// Bit of `lib_position_set.flags`: the write comes from sync.
const SYNC: u32 = 1;
/// Bit of `lib_position_set.flags`: the user confirmed a sync write.
const CONFIRMED: u32 = 1 << 1;
/// Most millionths of progress.
const PROGRESS_MAX: u32 = 1_000_000;

/// A book's key and fingerprint, borrowed from the caller's reply buffer. Keys
/// are the host's: the SDK has no function that computes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookKey<'a>(wire::ServiceLibKey<'a>);
impl<'a> BookKey<'a> {
    /// The 16 hex digits plugins name the book's own folders by. A move by the
    /// host changes it.
    #[must_use]
    pub const fn key(&self) -> &'a str {
        self.0.key
    }
    /// The 32 hex digits of KOReader's partial MD5 of the file; with
    /// [`size`](Self::size) it is the book's identity on other devices.
    #[must_use]
    pub const fn fingerprint(&self) -> &'a str {
        self.0.fingerprint
    }
    /// The file's size in bytes.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.0.size
    }
}

/// A stored reading position, borrowed from the caller's reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Position<'a> {
    /// The source section.
    pub section: u32,
    /// The source offset in the section.
    pub offset: u32,
    /// The ordinal of inserted content at the offset, 0 for none.
    pub insert: u32,
    /// Progress through the book in millionths.
    pub progress_ppm: u32,
    /// The conversion the section and offset belong to, if known (`None` and
    /// `Some(0)` differ).
    pub conversion: Option<u64>,
    /// Seconds since 1970 of the write, 0 for an unknown clock.
    pub updated: u64,
    /// The KOReader XPointer, if one is stored.
    pub xpointer: Option<&'a str>,
    /// Whether the last write came from sync (and was not confirmed).
    pub from_sync: bool,
}

/// A position to store. A local write (no [`sync`](Self::sync)) always wins; a
/// sync write wins only over an older sync position, else it is refused with
/// `State` and stores nothing, unless the user [`confirmed`](Self::confirmed) it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PositionWrite<'a> {
    position: Position<'a>,
    sync: bool,
    confirmed: bool,
}
impl<'a> PositionWrite<'a> {
    /// A local write of this place, with progress 0, no conversion, no XPointer
    /// and an unknown clock.
    #[must_use]
    pub const fn new(section: u32, offset: u32) -> Self {
        Self {
            position: Position {
                section,
                offset,
                insert: 0,
                progress_ppm: 0,
                conversion: None,
                updated: 0,
                xpointer: None,
                from_sync: false,
            },
            sync: false,
            confirmed: false,
        }
    }
    /// The ordinal of inserted content at the offset.
    #[must_use]
    pub const fn insert(mut self, insert: u32) -> Self {
        self.position.insert = insert;
        self
    }
    /// Progress in millionths, at most 1,000,000 (checked when written).
    #[must_use]
    pub const fn progress_ppm(mut self, progress_ppm: u32) -> Self {
        self.position.progress_ppm = progress_ppm;
        self
    }
    /// The conversion the section and offset belong to.
    #[must_use]
    pub const fn conversion(mut self, conversion: u64) -> Self {
        self.position.conversion = Some(conversion);
        self
    }
    /// Seconds since 1970 of the position; leave 0 for an unknown clock.
    #[must_use]
    pub const fn updated(mut self, updated: u64) -> Self {
        self.position.updated = updated;
        self
    }
    /// The KOReader XPointer, at most 1,024 bytes (checked when written).
    #[must_use]
    pub const fn xpointer(mut self, xpointer: &'a str) -> Self {
        self.position.xpointer = Some(xpointer);
        self
    }
    /// The write comes from sync (kosync pulling another device's place).
    #[must_use]
    pub const fn sync(mut self) -> Self {
        self.sync = true;
        self
    }
    /// The user chose to go to the other device's place: a sync write that counts
    /// as local.
    #[must_use]
    pub const fn confirmed(mut self) -> Self {
        self.confirmed = true;
        self
    }
}

impl<'a> Position<'a> {
    fn decode(row: &wire::ServiceLibPosition<'a>) -> Self {
        Self {
            section: row.section,
            offset: row.offset,
            insert: row.insert,
            progress_ppm: row.progress_ppm,
            conversion: (row.flags & HAS_CONVERSION != 0).then_some(row.conversion),
            updated: row.updated,
            xpointer: (row.flags & HAS_XPOINTER != 0).then_some(row.xpointer),
            from_sync: row.flags & FROM_SYNC != 0,
        }
    }
}

impl Library {
    /// Read a book's key, KOReader fingerprint and file size. `input` is 1,024
    /// bytes. `out` is the operation's reply capacity or more (4,096 bytes always
    /// do), else NoSpace.
    ///
    /// # Errors
    /// NotFound for an unknown book, State for a stale catalog handle, Invalid for a
    /// malformed reply; otherwise the host's refusals.
    pub fn book_key<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<BookKey<'a>, Error> {
        let n = self.call(host, schema::OP_BOOK_KEY, &[book.record()], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceLibKey<'a>>()
            .map_err(|_| Error::Invalid)?;
        Ok(BookKey(row))
    }

    /// Read a book's stored reading position. `input` is 1,024 bytes; `out` is
    /// 4,096 bytes or more.
    ///
    /// # Errors
    /// NotFound for an unknown book or one with no stored position, State for a
    /// stale catalog handle, Invalid for a malformed reply; otherwise the host's
    /// refusals.
    pub fn position<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Position<'a>, Error> {
        let n = self.call(host, schema::OP_POSITION, &[book.record()], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceLibPosition<'a>>()
            .map_err(|_| Error::Invalid)?;
        Ok(Position::decode(&row))
    }

    /// Store a reading position, atomically. A local write always wins. A sync
    /// write ([`PositionWrite::sync`]) wins only over an older sync position, else
    /// it answers `State` and stores nothing (ask the user, then write again with
    /// [`PositionWrite::confirmed`]). `input` is 2,048 bytes.
    ///
    /// # Errors
    /// Invalid for progress above 1,000,000, an XPointer over 1,024 bytes, or a
    /// confirmation without sync (nothing is sent); State for a refused sync write
    /// or a stale handle; Invalid for a malformed reply; otherwise the host's
    /// refusals.
    pub fn set_position(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        write: &PositionWrite<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let p = &write.position;
        if p.progress_ppm > PROGRESS_MAX
            || p.xpointer
                .is_some_and(|x| x.len() > wire::SERVICE_LIB_POSITION_XPOINTER_MAX_BYTES)
            || (write.confirmed && !write.sync)
        {
            return Err(Error::Invalid);
        }
        let position = wire::ServiceLibPosition {
            section: p.section,
            offset: p.offset,
            insert: p.insert,
            progress_ppm: p.progress_ppm,
            conversion: p.conversion.unwrap_or(0),
            updated: p.updated,
            xpointer: p.xpointer.unwrap_or(""),
            flags: if p.xpointer.is_some() {
                HAS_XPOINTER
            } else {
                0
            } | if p.conversion.is_some() {
                HAS_CONVERSION
            } else {
                0
            },
        };
        let set = wire::ServiceLibPositionSet {
            flags: if write.sync { SYNC } else { 0 } | if write.confirmed { CONFIRMED } else { 0 },
        };
        let n = self.call(
            host,
            schema::OP_SET_POSITION,
            &[book.record(), &position, &set],
            input,
            out,
        )?;
        let result = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceLibResult<'_>>()
            .map_err(|_| Error::Invalid)?;
        if result.status == 0 {
            Ok(())
        } else {
            Err(Error::from_status(result.status))
        }
    }
}
