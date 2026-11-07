//! Reading tables from a database file.
//!
//! A file may be corrupt or made to harm: a scan reads each page of it at
//! most once (interior pages again for each child, rather than keeping
//! them), holds a single page and the row at hand, and allocates fallibly,
//! so its work and memory stay bounded by the file's size.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;

use crate::record::{Value, decode};
use crate::{Error, TABLE_INTERIOR, TABLE_LEAF, local_size, varint};

/// Deepest B-tree followed: interior pages above a leaf.
const MAX_DEPTH: usize = 20;

/// A row of `sqlite_schema`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaEntry {
    /// `table`, `index`, `view` or `trigger`.
    pub kind: String,
    pub name: String,
    pub table: String,
    pub root: u32,
    pub sql: String,
}

/// A row [`Database::scan_bounded`] visits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Row<'v> {
    /// The row's values.
    Values(&'v [Value]),
    /// A row passed over: its payload (the record as stored) is this many
    /// bytes, more than the scan's limit. Nothing of it was allocated and
    /// its overflow pages were not read.
    Oversized(u64),
}

/// A row as a scan reads it: its values, or the size of a payload past
/// the limit.
enum Record {
    Values(Vec<Value>),
    Oversized(u64),
}

/// An open database file.
pub struct Database<'s> {
    storage: &'s dyn Storage,
    path: String,
    page_size: usize,
    usable: usize,
    /// Pages in the file.
    pages: u32,
    pub user_version: u32,
}

impl core::fmt::Debug for Database<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl<'s> Database<'s> {
    pub fn open(storage: &'s dyn Storage, path: &str) -> Result<Self, Error> {
        let mut header = [0u8; 100];
        if storage.read_at(path, 0, &mut header)? != 100 || &header[..16] != b"SQLite format 3\0" {
            return Err(Error::NotSqlite);
        }
        let be32 = |at: usize| {
            u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
        };
        let page_size = match u16::from_be_bytes([header[16], header[17]]) {
            1 => 65_536,
            n if n >= 512 && n.is_power_of_two() => usize::from(n),
            _ => return Err(Error::Corrupt("page size")),
        };
        // SQLite's own minimum.
        let usable = page_size - usize::from(header[20]);
        if usable < 480 {
            return Err(Error::Corrupt("reserved bytes"));
        }
        // UTF-8 only.
        if be32(56) > 1 {
            return Err(Error::NotSqlite);
        }
        // The header's page count holds if the version that wrote the file
        // kept it; pages past it are left over.
        let in_file = storage.size(path)? / page_size as u64;
        let in_file = u32::try_from(in_file).unwrap_or(u32::MAX);
        let counted = be32(28);
        let pages = if counted != 0 && be32(24) == be32(92) {
            counted.min(in_file)
        } else {
            in_file
        };
        let mut owned = String::new();
        owned
            .try_reserve_exact(path.len())
            .map_err(|_| Error::OutOfMemory)?;
        owned.push_str(path);
        Ok(Self {
            storage,
            path: owned,
            page_size,
            usable,
            pages,
            user_version: be32(60),
        })
    }

    /// Where page `number` starts in the file.
    fn offset(&self, number: u32) -> Result<u64, Error> {
        if number == 0 || number > self.pages {
            return Err(Error::Corrupt("page number"));
        }
        Ok(u64::from(number - 1) * self.page_size as u64)
    }

    /// Fills `buf` from `offset` on.
    fn read_exact(&self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        if self.storage.read_at(&self.path, offset, buf)? != buf.len() {
            return Err(Error::Corrupt("short page"));
        }
        Ok(())
    }

    /// The tables, indexes, views and triggers.
    pub fn schema(&self) -> Result<Vec<SchemaEntry>, Error> {
        let mut entries = Vec::new();
        let mut failed = None;
        self.rows(1, usize::MAX, &mut |_, record| {
            // Nothing is past no limit.
            let Record::Values(mut values) = record else {
                return true;
            };
            let mut text = |i: usize| match values.get_mut(i) {
                Some(Value::Text(text)) => core::mem::take(text),
                _ => String::new(),
            };
            let (kind, name, table, sql) = (text(0), text(1), text(2), text(4));
            let root = values
                .get(3)
                .and_then(Value::as_int)
                .and_then(|root| u32::try_from(root).ok())
                .unwrap_or(0);
            if entries.try_reserve(1).is_err() {
                failed = Some(Error::OutOfMemory);
                return false;
            }
            entries.push(SchemaEntry {
                kind,
                name,
                table,
                root,
                sql,
            });
            true
        })?;
        failed.map_or(Ok(entries), Err)
    }

    /// The column names and root page of table `name`.
    pub fn table(&self, name: &str) -> Result<(Vec<String>, u32), Error> {
        let entry = self
            .schema()?
            .into_iter()
            .find(|e| e.kind == "table" && e.name.eq_ignore_ascii_case(name))
            .ok_or(Error::Missing)?;
        Ok((columns(&entry.sql)?, entry.root))
    }

    /// Calls `visit` with every row's id and values of the table at page
    /// `root`, in order of their ids, until it returns `false`.
    pub fn scan(
        &self,
        root: u32,
        visit: &mut dyn FnMut(i64, &[Value]) -> bool,
    ) -> Result<(), Error> {
        self.rows(root, usize::MAX, &mut |id, record| match record {
            Record::Values(values) => visit(id, &values),
            Record::Oversized(_) => true,
        })
    }

    /// [`Database::scan`], but a row whose payload (the record as stored)
    /// is more than `max_payload` bytes is visited as [`Row::Oversized`]:
    /// nothing of it is allocated and its overflow pages are not read. A
    /// row read takes about twice its payload while it is decoded, so one
    /// huge row cannot fail the scan for want of memory.
    ///
    /// A payload larger than the file is still [`Error::Corrupt`].
    pub fn scan_bounded(
        &self,
        root: u32,
        max_payload: usize,
        visit: &mut dyn FnMut(i64, Row<'_>) -> bool,
    ) -> Result<(), Error> {
        self.rows(root, max_payload, &mut |id, record| match record {
            Record::Values(values) => visit(id, Row::Values(&values)),
            Record::Oversized(size) => visit(id, Row::Oversized(size)),
        })
    }

    /// The rows of the table at `root`, handing the values over; payloads
    /// over `max_payload` bytes are not read.
    fn rows(
        &self,
        root: u32,
        max_payload: usize,
        visit: &mut dyn FnMut(i64, Record) -> bool,
    ) -> Result<(), Error> {
        let mut seen = Seen::new(self.pages)?;
        let mut buffer = zeroed(self.page_size)?;
        // The interior pages above the current one: page number, the child
        // taken last and how many cells it has.
        let mut path = [(0u32, 0usize, 0usize); MAX_DEPTH];
        let mut depth = 0;
        let mut number = root;
        loop {
            seen.insert(number)?;
            self.read_exact(self.offset(number)?, &mut buffer)?;
            let page = &buffer[..self.usable];
            let node = Node::parse(page, number)?;
            if node.kind == TABLE_INTERIOR {
                let top = path.get_mut(depth).ok_or(Error::Corrupt("too deep"))?;
                *top = (number, 0, node.cells);
                depth += 1;
                number = node.child(page, 0)?;
                continue;
            }
            if !self.leaf(page, node, max_payload, &mut seen, visit)? {
                return Ok(());
            }
            // Up to the next child not visited yet, read again from its
            // parent.
            number = loop {
                let Some(top) = depth.checked_sub(1) else {
                    return Ok(());
                };
                let (parent, taken, cells) = &mut path[top];
                if *taken < *cells {
                    *taken += 1;
                    self.read_exact(self.offset(*parent)?, &mut buffer)?;
                    let page = &buffer[..self.usable];
                    break Node::parse(page, *parent)?.child(page, *taken)?;
                }
                depth = top;
            };
        }
    }

    /// Visits the rows of leaf page `page`; `Ok(false)` once `visit` asked
    /// to stop.
    fn leaf(
        &self,
        page: &[u8],
        node: Node,
        max_payload: usize,
        seen: &mut Seen,
        visit: &mut dyn FnMut(i64, Record) -> bool,
    ) -> Result<bool, Error> {
        // The cells must fit in the page side by side, so overlapping ones
        // cannot multiply the work.
        let mut room = page.len() - node.content_start();
        for i in 0..node.cells {
            let cell = &page[node.cell(page, i)?..];
            let (size, n) = varint::read(cell).ok_or(Error::Corrupt("cell"))?;
            let (rowid, m) = cell
                .get(n..)
                .and_then(varint::read)
                .ok_or(Error::Corrupt("cell"))?;
            // A payload is no longer than the file.
            if size > u64::from(self.pages) * self.usable as u64 {
                return Err(Error::Corrupt("payload size"));
            }
            let stored_size = size;
            let size = usize::try_from(size).map_err(|_| Error::OutOfMemory)?;
            let local = local_size(size, self.usable, true);
            let link = if local < size { 4 } else { 0 };
            room = room
                .checked_sub(n + m + local + link)
                .ok_or(Error::Corrupt("cell sizes"))?;
            let record = if size > max_payload {
                // What is in the page is checked as for a row read; the
                // overflow chain is not followed.
                let stored = cell
                    .get(n + m..n + m + local + link)
                    .ok_or(Error::Corrupt("payload"))?;
                if let Some(&[a, b, c, d]) = stored.get(local..) {
                    self.offset(u32::from_be_bytes([a, b, c, d]))?;
                }
                Record::Oversized(stored_size)
            } else {
                let payload = self.payload(&cell[n + m..], size, seen)?;
                let values = decode(&payload)?;
                drop(payload);
                Record::Values(values)
            };
            if !visit(rowid as i64, record) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A table leaf cell's payload of `size` bytes: what `cell` (the cell
    /// from its payload on) holds, the rest from overflow pages.
    fn payload(&self, cell: &[u8], size: usize, seen: &mut Seen) -> Result<Vec<u8>, Error> {
        let mut payload = zeroed(size)?;
        let local = local_size(size, self.usable, true);
        let (start, overflow) = payload.split_at_mut(local);
        start.copy_from_slice(cell.get(..local).ok_or(Error::Corrupt("payload"))?);
        if overflow.is_empty() {
            return Ok(payload);
        }
        let mut next = cell
            .get(local..local + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or(Error::Corrupt("overflow"))?;
        // Each overflow page: the next one's number, then content.
        for chunk in overflow.chunks_mut(self.usable - 4) {
            if next == 0 {
                return Err(Error::Corrupt("overflow chain"));
            }
            seen.insert(next)?;
            let offset = self.offset(next)?;
            let mut link = [0u8; 4];
            self.read_exact(offset, &mut link)?;
            self.read_exact(offset + 4, chunk)?;
            next = u32::from_be_bytes(link);
        }
        Ok(payload)
    }
}

/// The header of a table B-tree page.
#[derive(Clone, Copy)]
struct Node {
    /// Where the header starts: 100 on page 1, after the file header.
    at: usize,
    kind: u8,
    cells: usize,
}

impl Node {
    /// The header of page `number`, whose usable part is `page`.
    fn parse(page: &[u8], number: u32) -> Result<Self, Error> {
        // The header fits: pages hold at least 480 usable bytes.
        let at = if number == 1 { 100 } else { 0 };
        let kind = page[at];
        if kind != TABLE_INTERIOR && kind != TABLE_LEAF {
            return Err(Error::Corrupt("not a table page"));
        }
        let node = Self {
            at,
            kind,
            cells: usize::from(u16::from_be_bytes([page[at + 3], page[at + 4]])),
        };
        if node.content_start() > page.len() {
            return Err(Error::Corrupt("cell count"));
        }
        Ok(node)
    }

    fn pointers(self) -> usize {
        self.at + if self.kind == TABLE_INTERIOR { 12 } else { 8 }
    }

    /// Where cells may start: after the header and the cell pointers.
    fn content_start(self) -> usize {
        self.pointers() + 2 * self.cells
    }

    /// Where cell `i` starts.
    fn cell(self, page: &[u8], i: usize) -> Result<usize, Error> {
        let at = self.pointers() + 2 * i;
        let offset = page
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
            .ok_or(Error::Corrupt("cell pointer"))?;
        if offset < self.content_start() || offset >= page.len() {
            return Err(Error::Corrupt("cell offset"));
        }
        Ok(offset)
    }

    /// An interior page's child `i`: left of cell `i`, or the right-most
    /// one after the last cell.
    fn child(self, page: &[u8], i: usize) -> Result<u32, Error> {
        let at = if i == self.cells {
            self.at + 8
        } else {
            self.cell(page, i)?
        };
        page.get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or(Error::Corrupt("child pointer"))
    }
}

/// The pages a scan has read: in a sound file, each page has one place.
struct Seen(Vec<u8>);

impl Seen {
    fn new(pages: u32) -> Result<Self, Error> {
        zeroed(pages as usize / 8 + 1).map(Self)
    }

    fn insert(&mut self, number: u32) -> Result<(), Error> {
        let bit = 1 << (number % 8);
        let byte = self
            .0
            .get_mut((number / 8) as usize)
            .ok_or(Error::Corrupt("page number"))?;
        if *byte & bit != 0 {
            return Err(Error::Corrupt("page used twice"));
        }
        *byte |= bit;
        Ok(())
    }
}

/// `len` zero bytes, allocated fallibly.
fn zeroed(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| Error::OutOfMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// The column names of a `CREATE TABLE` statement, in order. SQLite
/// keeps the statement as it was written: comments and quoted names and
/// texts are passed over, with the commas and brackets in them. Allocated
/// fallibly.
///
/// # Errors
/// [`Error::OutOfMemory`] where the names' room cannot be had.
pub fn columns(sql: &str) -> Result<Vec<String>, Error> {
    // The definitions between the outermost brackets, without comments.
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut depth = 0u32;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '-' if chars.peek() == Some(&'-') => {
                chars.find(|&c| c == '\n');
                push_char(&mut part, ' ')?;
                continue;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                chars.find(|&c| core::mem::replace(&mut last, c) == '*' && c == '/');
                push_char(&mut part, ' ')?;
                continue;
            }
            '\'' | '"' | '`' | '[' => {
                // Up to the closing quote; a quote doubled stands for
                // itself.
                let close = if c == '[' { ']' } else { c };
                let mut quoted = String::new();
                push_char(&mut quoted, c)?;
                while let Some(q) = chars.next() {
                    push_char(&mut quoted, q)?;
                    if q == close {
                        if c != '['
                            && let Some(again) = chars.next_if_eq(&close)
                        {
                            push_char(&mut quoted, again)?;
                        } else {
                            break;
                        }
                    }
                }
                if depth > 0 {
                    push_str(&mut part, &quoted)?;
                }
                continue;
            }
            '(' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    push_part(&mut parts, core::mem::take(&mut part))?;
                    break;
                }
            }
            ',' if depth == 1 => {
                push_part(&mut parts, core::mem::take(&mut part))?;
                continue;
            }
            _ => {}
        }
        if depth > 0 {
            push_char(&mut part, c)?;
        }
    }
    if depth > 0 {
        // Not closed: no statement.
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for part in parts {
        let Some(word) = part.split_whitespace().next() else {
            continue;
        };
        let name = word.trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']');
        let constraint = ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"]
            .iter()
            .any(|k| name.eq_ignore_ascii_case(k));
        if !constraint && !name.is_empty() {
            let mut owned = String::new();
            push_str(&mut owned, name)?;
            push_part(&mut names, owned)?;
        }
    }
    Ok(names)
}

/// Appends `c`, allocating fallibly.
fn push_char(text: &mut String, c: char) -> Result<(), Error> {
    text.try_reserve(c.len_utf8())
        .map_err(|_| Error::OutOfMemory)?;
    text.push(c);
    Ok(())
}

/// Appends `more`, allocating fallibly.
fn push_str(text: &mut String, more: &str) -> Result<(), Error> {
    text.try_reserve(more.len())
        .map_err(|_| Error::OutOfMemory)?;
    text.push_str(more);
    Ok(())
}

/// Appends `part`, allocating fallibly.
fn push_part(parts: &mut Vec<String>, part: String) -> Result<(), Error> {
    parts.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
    parts.push(part);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_come_from_create_table() {
        let sql = "CREATE TABLE page_stat_data\n (\n id_book integer, -- the book\n page integer NOT NULL DEFAULT 0,\n start_time integer,\n UNIQUE (id_book, page, start_time),\n FOREIGN KEY(id_book) REFERENCES book(id)\n )";
        assert_eq!(columns(sql).unwrap(), ["id_book", "page", "start_time"]);
        assert_eq!(
            columns("CREATE TABLE \"t\"(\"a\" text, [b], c DEFAULT (1+2))").unwrap(),
            ["a", "b", "c"]
        );
    }

    use crate::Writer;
    use crate::memory::Memory;
    use crate::record::encode_record;
    use alloc::string::ToString;
    use alloc::vec;
    use paperpaw_platform::Storage;

    /// The writer's page size.
    const PAGE: usize = 4096;

    /// A database whose table `t` holds `rows`, padded with empty pages to
    /// `pages` pages; and the table's root page.
    fn database(rows: Vec<(i64, Vec<u8>)>, pages: u32) -> (Memory, u32) {
        let storage = Memory::default();
        let mut writer = Writer::new(&storage, "db");
        let root = writer.table(rows).unwrap();
        let schema = [SchemaEntry {
            kind: "table".to_string(),
            name: "t".to_string(),
            table: "t".to_string(),
            root,
            sql: "CREATE TABLE t(a)".to_string(),
        }];
        writer.finish(&schema, 0).unwrap();
        let mut data = storage.bytes("db");
        let pages = pages.max((data.len() / PAGE) as u32);
        data.resize(pages as usize * PAGE, 0);
        data[28..32].copy_from_slice(&pages.to_be_bytes());
        storage.write("db", &data).unwrap();
        (storage, root)
    }

    /// Replaces page `number`.
    fn set_page(storage: &Memory, number: u32, page: &[u8]) {
        let mut data = storage.bytes("db");
        let at = (number as usize - 1) * PAGE;
        data[at..at + PAGE].copy_from_slice(page);
        storage.write("db", &data).unwrap();
    }

    /// A table page of `kind` holding `cells` as they are, with `right` as
    /// an interior page's right-most child.
    fn table_page(kind: u8, cells: &[Vec<u8>], right: u32) -> Vec<u8> {
        let mut page = vec![0u8; PAGE];
        page[0] = kind;
        page[3..5].copy_from_slice(&(cells.len() as u16).to_be_bytes());
        let header = if kind == TABLE_INTERIOR {
            page[8..12].copy_from_slice(&right.to_be_bytes());
            12
        } else {
            8
        };
        let mut content = PAGE;
        for (i, cell) in cells.iter().enumerate() {
            content -= cell.len();
            page[content..content + cell.len()].copy_from_slice(cell);
            let pointer = header + 2 * i;
            page[pointer..pointer + 2].copy_from_slice(&(content as u16).to_be_bytes());
        }
        page[5..7].copy_from_slice(&(content as u16).to_be_bytes());
        page
    }

    /// An interior cell: the child left of `key`.
    fn interior_cell(child: u32, key: u64) -> Vec<u8> {
        let mut cell = child.to_be_bytes().to_vec();
        varint::write(&mut cell, key);
        cell
    }

    /// A leaf cell of row `rowid` claiming a payload of `size` bytes; it
    /// holds `local` of them and, if given, links to overflow page `next`.
    fn leaf_cell(size: u64, rowid: u64, local: &[u8], next: Option<u32>) -> Vec<u8> {
        let mut cell = Vec::new();
        varint::write(&mut cell, size);
        varint::write(&mut cell, rowid);
        cell.extend_from_slice(local);
        if let Some(next) = next {
            cell.extend_from_slice(&next.to_be_bytes());
        }
        cell
    }

    /// An overflow page linking to `next`.
    fn overflow_page(next: u32, content: &[u8]) -> Vec<u8> {
        let mut page = vec![0u8; PAGE];
        page[..4].copy_from_slice(&next.to_be_bytes());
        page[4..4 + content.len()].copy_from_slice(content);
        page
    }

    /// The row ids of the table at `root`.
    fn ids(db: &Database<'_>, root: u32) -> Result<Vec<i64>, Error> {
        let mut ids = Vec::new();
        db.scan(root, &mut |id, _| {
            ids.push(id);
            true
        })?;
        Ok(ids)
    }

    fn one_row() -> Vec<(i64, Vec<u8>)> {
        vec![(1, encode_record(&[Value::Int(1)]))]
    }

    #[test]
    fn deep_trees_read_back() {
        // Four rows a leaf, 600 leaves: more than an interior page holds.
        let rows: Vec<(i64, Vec<u8>)> = (1..=2400)
            .map(|i| (i, encode_record(&[Value::Blob(vec![i as u8; 900])])))
            .collect();
        let (storage, root) = database(rows.clone(), 0);
        let data = storage.bytes("db");
        let page = |n: u32| &data[(n as usize - 1) * PAGE..n as usize * PAGE];
        let child = Node::parse(page(root), root)
            .and_then(|node| node.child(page(root), 0))
            .unwrap();
        assert_eq!(page(child)[0], TABLE_INTERIOR, "three levels");
        let db = Database::open(&storage, "db").unwrap();
        let mut read = Vec::new();
        db.scan(root, &mut |id, values| {
            read.push((id, encode_record(values)));
            true
        })
        .unwrap();
        assert_eq!(read, rows);
        // Stopping early.
        let mut seen = 0;
        db.scan(root, &mut |_, _| {
            seen += 1;
            seen < 700
        })
        .unwrap();
        assert_eq!(seen, 700);
    }

    #[test]
    fn interior_cell_at_the_page_end_is_corrupt() {
        let (storage, root) = database(one_row(), 3);
        let mut page = table_page(TABLE_INTERIOR, &[interior_cell(3, 1)], 3);
        page[12..14].copy_from_slice(&(PAGE as u16 - 1).to_be_bytes());
        set_page(&storage, root, &page);
        let db = Database::open(&storage, "db").unwrap();
        assert!(matches!(ids(&db, root), Err(Error::Corrupt(_))));
    }

    #[test]
    fn payload_larger_than_the_file_is_corrupt() {
        let (storage, root) = database(one_row(), 3);
        let size = 1u64 << 40;
        let local = local_size(size as usize, PAGE, true);
        let cell = leaf_cell(size, 1, &vec![0; local], Some(3));
        set_page(&storage, root, &table_page(TABLE_LEAF, &[cell], 0));
        // An overflow page that is its own successor.
        set_page(&storage, 3, &overflow_page(3, &[]));
        let db = Database::open(&storage, "db").unwrap();
        storage.limit_reads(100);
        assert!(matches!(ids(&db, root), Err(Error::Corrupt(_))));
    }

    #[test]
    fn overflow_loops_are_corrupt() {
        let (storage, root) = database(one_row(), 6);
        // Two overflow pages' worth, but the first links back to itself.
        let payload = encode_record(&[Value::Blob(vec![7; 10_000])]);
        let local = local_size(payload.len(), PAGE, true);
        assert!(payload.len() - local > PAGE - 4);
        let cell = leaf_cell(payload.len() as u64, 1, &payload[..local], Some(3));
        set_page(&storage, root, &table_page(TABLE_LEAF, &[cell], 0));
        set_page(&storage, 3, &overflow_page(3, &payload[local..PAGE - 4]));
        let db = Database::open(&storage, "db").unwrap();
        assert!(matches!(ids(&db, root), Err(Error::Corrupt(_))));
    }

    #[test]
    fn shared_children_are_corrupt() {
        // Three interior levels whose 300 cells all lead to the same page:
        // 27 million visits of the leaf if followed.
        let (storage, _) = database(one_row(), 5);
        for level in 2..=4u32 {
            let cells: Vec<Vec<u8>> = (0..300).map(|i| interior_cell(level + 1, i)).collect();
            set_page(
                &storage,
                level,
                &table_page(TABLE_INTERIOR, &cells, level + 1),
            );
        }
        let row = encode_record(&[Value::Int(1)]);
        let leaf = leaf_cell(row.len() as u64, 1, &row, None);
        set_page(&storage, 5, &table_page(TABLE_LEAF, &[leaf], 0));
        let db = Database::open(&storage, "db").unwrap();
        storage.limit_reads(10_000);
        assert!(matches!(ids(&db, 2), Err(Error::Corrupt(_))));
        // The leaf on its own reads.
        storage.limit_reads(usize::MAX);
        assert_eq!(ids(&db, 5), Ok(vec![1]));
    }

    #[test]
    fn overlapping_cells_are_corrupt() {
        let (storage, root) = database(one_row(), 0);
        let row = encode_record(&[Value::Int(1)]);
        let mut page = table_page(TABLE_LEAF, &[leaf_cell(row.len() as u64, 1, &row, None)], 0);
        // A thousand pointers to the same cell.
        let cell = [page[8], page[9]];
        page[3..5].copy_from_slice(&1000u16.to_be_bytes());
        for i in 0..1000 {
            page[8 + 2 * i..10 + 2 * i].copy_from_slice(&cell);
        }
        set_page(&storage, root, &page);
        let db = Database::open(&storage, "db").unwrap();
        assert!(matches!(ids(&db, root), Err(Error::Corrupt(_))));
    }

    #[test]
    fn pages_past_the_end_are_corrupt() {
        let (storage, root) = database(one_row(), 0);
        let page = table_page(TABLE_INTERIOR, &[interior_cell(1000, 1)], 1001);
        set_page(&storage, root, &page);
        let db = Database::open(&storage, "db").unwrap();
        assert!(matches!(ids(&db, root), Err(Error::Corrupt(_))));
        assert!(matches!(ids(&db, 0), Err(Error::Corrupt(_))));
    }

    #[test]
    fn too_few_usable_bytes_are_refused() {
        let (storage, _) = database(one_row(), 0);
        let mut data = storage.bytes("db");
        data[16..18].copy_from_slice(&512u16.to_be_bytes());
        data[20] = 255;
        storage.write("db", &data).unwrap();
        assert!(matches!(
            Database::open(&storage, "db"),
            Err(Error::Corrupt(_))
        ));
    }
}
