//! Files of the library: the delete preflight, delete and import
//! (`docs/abi/library.md` 2.3).
use super::*;

/// Most paths of one `can_delete` or `delete` call.
pub const MAX_DELETE_PATHS: usize = wire::SERVICE_LIB_PATHS_PATHS_MAX_ITEMS;
/// Bit of `lib_paths.flags`: delete books that have annotations too.
const WITH_NOTES: u32 = 1;
/// Bits of `lib_result.flags` of `can_delete`.
const HAS_ANNOTATIONS: u32 = 1;
const HAS_POSITION: u32 = 1 << 1;
const HAS_BOOK_DATA: u32 = 1 << 2;
/// Bits of `lib_import.flags`.
const REPLACE: u32 = 1;
const FREE_NAME: u32 = 1 << 1;
/// The longest path segment the card allows, in UTF-16 units.
const SEGMENT_UNITS: usize = 255;

/// One book's answer to `can_delete` or `delete`, borrowed from the reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteRow<'a>(wire::ServiceLibResult<'a>);
impl<'a> DeleteRow<'a> {
    fn decode(record: RawRecord<'a>) -> Result<Self, Error> {
        let row = record
            .decode::<wire::ServiceLibResult<'a>>()
            .map_err(|_| Error::Invalid)?;
        Ok(Self(row))
    }
    /// The book's path.
    #[must_use]
    pub const fn path(&self) -> &'a str {
        self.0.path
    }
    /// What happened to this book. For `can_delete`: `Ok` it can be deleted;
    /// `Invalid` it is not a book; `NotFound` it is gone already (a delete only
    /// forgets it); `Io` the card cannot. For `delete`: `Ok` it is deleted;
    /// `Invalid` not a book; `State` it has annotations and `with_notes` was off;
    /// `Busy` the host uses the path; otherwise the card's refusals.
    ///
    /// # Errors
    /// The book's own refusal as listed.
    pub fn status(&self) -> Result<(), Error> {
        if self.0.status == 0 {
            Ok(())
        } else {
            Err(Error::from_status(self.0.status))
        }
    }
    /// The book has annotations (`can_delete` only): ask before deleting them.
    #[must_use]
    pub const fn has_annotations(&self) -> bool {
        self.0.flags & HAS_ANNOTATIONS != 0
    }
    /// The book has a stored reading position (`can_delete` only).
    #[must_use]
    pub const fn has_position(&self) -> bool {
        self.0.flags & HAS_POSITION != 0
    }
    /// Plugins keep data for the book (`can_delete` only).
    #[must_use]
    pub const fn has_book_data(&self) -> bool {
        self.0.flags & HAS_BOOK_DATA != 0
    }
}

/// The answers of a delete call, one per path in the order asked, validated in
/// full before any row is exposed.
#[derive(Debug)]
pub struct DeleteReport<'a>(LibraryPage<'a, DeleteRow<'a>>);
impl<'a> DeleteReport<'a> {
    /// Number of books answered (the number of paths asked).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether no book was answered (never the case for a successful call).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Iterate the answers, in the order of the paths asked, without allocation.
    pub fn rows(&self) -> impl Iterator<Item = DeleteRow<'a>> + 'a {
        self.0.rows().flatten()
    }
}

/// What import does when the target name exists already.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Existing {
    /// Refuse with `State`; nothing changes.
    #[default]
    Refuse,
    /// Replace a file this plugin imported and nobody moved since (the book keeps
    /// its key, position and annotations); anything else is `Denied`.
    Replace,
    /// Take the next free name, `name (2).ext`, `name (3).ext` and so on.
    FreeName,
}

/// A file to import into the library: a downloaded file in the caller's root 0
/// and the name it gets below the library folder.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Import<'a> {
    from: &'a str,
    folder: &'a str,
    name: &'a str,
    existing: Existing,
}
impl<'a> Import<'a> {
    /// Import `from` (a file in root 0) as `name` in the library folder itself.
    #[must_use]
    pub const fn new(from: &'a str, name: &'a str) -> Self {
        Self {
            from,
            folder: "",
            name,
            existing: Existing::Refuse,
        }
    }
    /// Put the book in this folder below the library folder, `/` between the
    /// segments; the host makes missing folders.
    #[must_use]
    pub const fn folder(mut self, folder: &'a str) -> Self {
        self.folder = folder;
        self
    }
    /// What to do when the name exists already.
    #[must_use]
    pub const fn existing(mut self, existing: Existing) -> Self {
        self.existing = existing;
        self
    }
    fn check(&self) -> Result<(), Error> {
        if self.from.len() > wire::SERVICE_LIB_IMPORT_FROM_MAX_BYTES
            || self.folder.len() > wire::SERVICE_LIB_IMPORT_FOLDER_MAX_BYTES
            || self.name.len() > wire::SERVICE_LIB_IMPORT_NAME_MAX_BYTES
        {
            return Err(Error::NoSpace);
        }
        if self.from.is_empty() || self.from.chars().any(char::is_control) {
            return Err(Error::Invalid);
        }
        if !self.folder.is_empty() {
            for segment in self.folder.split('/') {
                segment_ok(segment)?;
            }
        }
        segment_ok(self.name)?;
        if self.name.contains('/') || self.name.starts_with(['.', '~']) {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

/// One folder or file name the card takes, checked as the host does before
/// anything is written (`docs/abi/library.md` 2.3, "Import names come from
/// outside").
fn segment_ok(segment: &str) -> Result<(), Error> {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.ends_with(['.', ' '])
        || segment
            .chars()
            .any(|c| c.is_control() || "\\\"*:<>?|".contains(c))
        || segment.encode_utf16().count() > SEGMENT_UNITS
        || reserved(segment)
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// A device name the card's hosts reserve, with or without an extension.
fn reserved(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment);
    let b = stem.as_bytes();
    if stem.eq_ignore_ascii_case("CON")
        || stem.eq_ignore_ascii_case("PRN")
        || stem.eq_ignore_ascii_case("AUX")
        || stem.eq_ignore_ascii_case("NUL")
    {
        return true;
    }
    matches!(b, [a, b2, c, b'1'..=b'9'] if [*a, *b2, *c].eq_ignore_ascii_case(b"COM")
        || [*a, *b2, *c].eq_ignore_ascii_case(b"LPT"))
}

impl Library {
    /// Ask which books can be deleted and what deleting them would take along
    /// (op 9). It changes nothing. `paths` holds 1 to 7 complete card paths. `input`
    /// is 4,096 bytes; `out` is 4,096 bytes and holds the joined paths until the
    /// call, so it must be 3,600 bytes or more.
    ///
    /// # Errors
    /// Invalid for no path, more than 7, an empty path or one with a control
    /// character (nothing is sent); NoSpace for a path over 512 bytes or a short
    /// `out`; Invalid for a reply that does not answer each path, by its path, once
    /// and in order; otherwise
    /// the host's refusals. Each book's own answer is in its row.
    pub fn can_delete<'a>(
        &self,
        host: &Host<'_>,
        paths: &[&str],
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DeleteReport<'a>, Error> {
        self.paths_call(host, schema::OP_CAN_DELETE, paths, 0, input, out)
    }
    /// Delete books and everything kept for them (op 10; the plugin needs
    /// `uses.storage` besides `uses.library`). Only books are removed. A path whose first
    /// segment is `plugins` or `.paperpaw` is refused by the host. A book with annotations is `State` in its row unless
    /// `with_notes`, which a plugin passes only for the books it downloaded itself
    /// or after asking the user. Buffers and refusals as for
    /// [`can_delete`](Self::can_delete).
    ///
    /// # Errors
    /// As [`can_delete`](Self::can_delete).
    pub fn delete<'a>(
        &self,
        host: &Host<'_>,
        paths: &[&str],
        with_notes: bool,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DeleteReport<'a>, Error> {
        let flags = if with_notes { WITH_NOTES } else { 0 };
        self.paths_call(host, schema::OP_DELETE, paths, flags, input, out)
    }
    fn paths_call<'a>(
        &self,
        host: &Host<'_>,
        op: u32,
        paths: &[&str],
        flags: u32,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DeleteReport<'a>, Error> {
        if paths.is_empty() || paths.len() > MAX_DELETE_PATHS {
            return Err(Error::Invalid);
        }
        let mut joined = 0usize;
        for (i, path) in paths.iter().enumerate() {
            if path.is_empty() || path.chars().any(char::is_control) {
                return Err(Error::Invalid);
            }
            if path.len() > wire::SERVICE_LIB_PATHS_PATHS_ITEM_MAX_BYTES {
                return Err(Error::NoSpace);
            }
            let end = joined + usize::from(i > 0) + path.len();
            let room = out.get_mut(joined..end).ok_or(Error::NoSpace)?;
            let text = if i > 0 {
                let (sep, text) = room.split_at_mut(1);
                sep.copy_from_slice(b"\n");
                text
            } else {
                room
            };
            text.copy_from_slice(path.as_bytes());
            joined = end;
        }
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let len = {
            let text = core::str::from_utf8(out.get(..joined).ok_or(Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
            let record = wire::ServiceLibPaths { paths: text, flags };
            wire::records_encode_counted_of(
                [&record as &dyn WireEncode].into_iter(),
                usize::from(operation.input_rows),
                operation.max_input,
                input,
            )
            .map_err(Error::from_status)?
        };
        let n = self
            .service
            .call(host, op, input.get(..len).ok_or(Error::Invalid)?, out)?;
        let page = rows::page(
            out.get(..n).ok_or(Error::Invalid)?,
            None,
            MAX_DELETE_PATHS,
            DeleteRow::decode,
        )?;
        if page.len() != paths.len()
            || !page
                .rows()
                .zip(paths)
                .all(|(row, asked)| row.is_ok_and(|row| row.path() == *asked))
        {
            return Err(Error::Invalid);
        }
        Ok(DeleteReport(page))
    }
    /// Move a book to another path in the library (op 11; the plugin needs
    /// `uses.storage` besides `uses.library`). The file and everything kept for
    /// it (position, annotations, fields, plugins' book data) follow, each through
    /// the card's journalled move; the book's key changes. With `replace` an
    /// existing `to` is replaced. `from` and `to` are complete card paths of the
    /// library root. `input` is 2,048 bytes and `out` 4,096.
    ///
    /// # Errors
    /// Invalid for an empty path or one with a control character, or a malformed
    /// reply; NoSpace for a path over 512 bytes (nothing is sent in both cases);
    /// `State` for a `to` that exists without `replace`; `Denied` for a path in
    /// `plugins/` or `.paperpaw/`; `Busy` while the host uses a path or a move job
    /// runs; otherwise the host's refusals. A refusal changes nothing.
    pub fn move_to(
        &self,
        host: &Host<'_>,
        from: &str,
        to: &str,
        replace: bool,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if from.len() > wire::SERVICE_LIB_MOVE_FROM_MAX_BYTES
            || to.len() > wire::SERVICE_LIB_MOVE_TO_MAX_BYTES
        {
            return Err(Error::NoSpace);
        }
        if [from, to]
            .iter()
            .any(|p| p.is_empty() || p.chars().any(char::is_control))
        {
            return Err(Error::Invalid);
        }
        let record = wire::ServiceLibMove {
            from,
            to,
            flags: u32::from(replace),
        };
        let n = self.call(host, schema::OP_MOVE, &[&record], input, out)?;
        let result = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceLibResult<'_>>()
            .map_err(|_| Error::Invalid)?;
        if result.status == 0 {
            Ok(())
        } else {
            Err(Error::from_status(result.status))
        }
    }
    /// Put a downloaded file into the library (op 12) and catalogue it. The file
    /// moves where root 0 and the library share a card, else it is copied and the
    /// source removed. Returns the book's path in the library (the path taken
    /// with [`Existing::FreeName`]), borrowed from `out`. Downloaders need no
    /// storage permission for it. `input` is 2,048 bytes and `out` 4,096.
    ///
    /// # Errors
    /// Invalid for an empty source, a name or folder segment the card does not take
    /// (empty, `.` or `..`, a path separator in the name, control characters or
    /// `"*:<>?|`, a trailing dot or space, a reserved device name, over 255
    /// UTF-16 units, a name starting with `.` or `~`), or a malformed reply;
    /// NoSpace over the wire limits (nothing is sent); `State` for a name that
    /// exists under [`Existing::Refuse`]; `Denied` for a replace of a book this
    /// plugin did not import or one moved since, or a target outside the library
    /// folder; `Busy` while the host uses a path or a move job runs; `Full` for a
    /// full card; otherwise the host's refusals.
    pub fn import<'a>(
        &self,
        host: &Host<'_>,
        file: &Import<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<&'a str, Error> {
        file.check()?;
        let record = wire::ServiceLibImport {
            from: file.from,
            folder: file.folder,
            name: file.name,
            flags: match file.existing {
                Existing::Refuse => 0,
                Existing::Replace => REPLACE,
                Existing::FreeName => FREE_NAME,
            },
        };
        let n = self.call(host, schema::OP_IMPORT, &[&record], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceLibResult<'a>>()
            .map_err(|_| Error::Invalid)?;
        if row.status != 0 {
            return Err(Error::from_status(row.status));
        }
        Ok(row.path)
    }
}
