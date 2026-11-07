//! Writing a new database file.
//!
//! Pages are appended to `<path>.part` as trees are built: leaves first,
//! then the levels above them. [`Writer::finish`] writes page 1 (header
//! and schema) to `<path>` and copies the rest after it, so a reader never
//! sees a half-written database.
//!
//! Cells are written only when their page is: payloads too long for a
//! cell go to overflow pages then, so no page is ever left unused.
//!
//! Every buffer is allocated fallibly: memory refused is
//! [`Error::OutOfMemory`] from the call that needed it (and from
//! [`Writer::finish`] after it), never an abort.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::{Storage, StorageError};

use crate::record::{Value, try_encode_record};
use crate::{
    Error, INDEX_INTERIOR, INDEX_LEAF, SchemaEntry, TABLE_INTERIOR, TABLE_LEAF, local_size, varint,
};

const PAGE: usize = 4096;
/// SQLite 3.45.0, the format this writer follows.
const SQLITE_VERSION: u32 = 3_045_000;

/// Builds a database file.
pub struct Writer<'s> {
    storage: &'s dyn Storage,
    path: String,
    part: String,
    /// The next page's number.
    next: u32,
    failed: Option<Error>,
}

impl core::fmt::Debug for Writer<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Writer")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// A cell before it is written: what precedes the payload (child page,
/// sizes, row id) and the payload.
struct Cell {
    prefix: Vec<u8>,
    payload: Vec<u8>,
    table_leaf: bool,
}

impl Cell {
    /// Its size in the page, with the link to overflow pages if any.
    fn len(&self) -> usize {
        let local = local_size(self.payload.len(), PAGE, self.table_leaf);
        let overflow = if local < self.payload.len() { 4 } else { 0 };
        self.prefix.len() + local + overflow
    }
}

/// Room for `more` items in `items`, or [`Error::OutOfMemory`].
fn reserve<T>(items: &mut Vec<T>, more: usize) -> Result<(), Error> {
    items.try_reserve(more).map_err(|_| Error::OutOfMemory)
}

/// Appends `item`, allocating fallibly.
fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), Error> {
    reserve(items, 1)?;
    items.push(item);
    Ok(())
}

/// `bytes` copied, allocated fallibly.
fn copy(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| Error::OutOfMemory)?;
    out.extend_from_slice(bytes);
    Ok(out)
}

/// `parts` joined, allocated fallibly.
fn text(parts: &[&str]) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())
        .map_err(|_| Error::OutOfMemory)?;
    for part in parts {
        out.push_str(part);
    }
    Ok(out)
}

/// `len` zero bytes, allocated fallibly.
fn zeroed(len: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| Error::OutOfMemory)?;
    out.resize(len, 0);
    Ok(out)
}

/// `head` (a child page number, or nothing) and `values` as varints.
fn prefix(head: Option<u32>, values: &[u64]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    // Four bytes for the page, nine at most a varint.
    out.try_reserve_exact(4 + 9 * values.len())
        .map_err(|_| Error::OutOfMemory)?;
    if let Some(page) = head {
        out.extend_from_slice(&page.to_be_bytes());
    }
    for &value in values {
        varint::write(&mut out, value);
    }
    Ok(out)
}

fn header_len(kind: u8) -> usize {
    if kind == TABLE_INTERIOR || kind == INDEX_INTERIOR {
        12
    } else {
        8
    }
}

/// Whether cells of `lens` fit on a page of `kind` whose header starts at
/// `start`.
fn fits(kind: u8, start: usize, lens: impl Iterator<Item = usize>) -> bool {
    let used: usize = lens.map(|l| l + 2).sum();
    start + header_len(kind) + used <= PAGE
}

/// How cells `0..n` split into the nodes of an interior level: each node
/// holds cells `s..e` and child `e` as its right-most pointer; the next
/// one starts at `e + 1`. Every node keeps at least one cell.
fn plan(kind: u8, lens: &[usize]) -> Result<Vec<(usize, usize)>, Error> {
    let n = lens.len();
    let mut nodes = Vec::new();
    let mut start = 0;
    let mut used = 0;
    let room = PAGE - header_len(kind);
    let mut i = 0;
    while i < n {
        if used + lens[i] + 2 <= room || i == start {
            used += lens[i] + 2;
            i += 1;
            continue;
        }
        // Full: cell `i` is the boundary, unless that left the last node
        // without a cell; then the one before is.
        let end = if i + 1 == n && i - 1 > start {
            i - 1
        } else {
            i
        };
        push(&mut nodes, (start, end))?;
        start = end + 1;
        i = start;
        used = 0;
    }
    push(&mut nodes, (start, n))?;
    Ok(nodes)
}

/// The `count` cells of an interior level, made by `cell`, and their sizes.
fn level_cells(
    count: usize,
    mut cell: impl FnMut(usize) -> Result<Cell, Error>,
) -> Result<(Vec<Cell>, Vec<usize>), Error> {
    let mut cells = Vec::new();
    let mut lens = Vec::new();
    reserve(&mut cells, count)?;
    reserve(&mut lens, count)?;
    for i in 0..count {
        let cell = cell(i)?;
        lens.push(cell.len());
        cells.push(cell);
    }
    Ok((cells, lens))
}

impl<'s> Writer<'s> {
    /// A writer of a new file at `path`. Memory refused for the names is
    /// told by the first call that writes ([`Error::OutOfMemory`]).
    pub fn new(storage: &'s dyn Storage, path: &str) -> Self {
        let names = text(&[path]).and_then(|owned| Ok((owned, text(&[path, ".part"])?)));
        let (path, part, failed) = match names {
            Ok((path, part)) => {
                let failed = storage.write(&part, &[]).err().map(Error::Storage);
                (path, part, failed)
            }
            Err(error) => (String::new(), String::new(), Some(error)),
        };
        Self {
            storage,
            path,
            part,
            // Page 1 comes last.
            next: 2,
            failed,
        }
    }

    /// Appends a page; returns its number.
    fn append(&mut self, page: &[u8]) -> u32 {
        if self.failed.is_none()
            && let Err(e) = self.storage.append(&self.part, page)
        {
            self.failed = Some(Error::Storage(e));
        }
        self.next += 1;
        self.next - 1
    }

    /// A cell's bytes; what does not fit goes to overflow pages written
    /// now, linked from the cell's last four bytes.
    fn write_cell(&mut self, cell: &Cell) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        reserve(&mut out, cell.len())?;
        out.extend_from_slice(&cell.prefix);
        let local = local_size(cell.payload.len(), PAGE, cell.table_leaf);
        out.extend_from_slice(&cell.payload[..local]);
        if local < cell.payload.len() {
            let rest = &cell.payload[local..];
            let count = rest.len().div_ceil(PAGE - 4);
            let first = self.next;
            let mut page = zeroed(PAGE)?;
            for (i, chunk) in rest.chunks(PAGE - 4).enumerate() {
                page.fill(0);
                let next = if i + 1 < count {
                    first + i as u32 + 1
                } else {
                    0
                };
                page[..4].copy_from_slice(&next.to_be_bytes());
                page[4..4 + chunk.len()].copy_from_slice(chunk);
                self.append(&page);
            }
            out.extend_from_slice(&first.to_be_bytes());
        }
        Ok(out)
    }

    /// A page of `kind` with `cells`; `right` is an interior page's
    /// right-most child; `start` where the header begins (100 on page 1).
    fn page_bytes(
        &mut self,
        kind: u8,
        start: usize,
        cells: &[Cell],
        right: u32,
    ) -> Result<Vec<u8>, Error> {
        let mut page = zeroed(PAGE)?;
        page[start] = kind;
        page[start + 3..start + 5].copy_from_slice(&(cells.len() as u16).to_be_bytes());
        let mut content = PAGE;
        let mut pointer = start + header_len(kind);
        for cell in cells {
            let bytes = self.write_cell(cell)?;
            content -= bytes.len();
            page[content..content + bytes.len()].copy_from_slice(&bytes);
            page[pointer..pointer + 2].copy_from_slice(&(content as u16).to_be_bytes());
            pointer += 2;
        }
        page[start + 5..start + 7].copy_from_slice(&(content as u16).to_be_bytes());
        if header_len(kind) == 12 {
            page[start + 8..start + 12].copy_from_slice(&right.to_be_bytes());
        }
        Ok(page)
    }

    fn write_page(&mut self, kind: u8, cells: &[Cell], right: u32) -> Result<u32, Error> {
        let page = self.page_bytes(kind, 0, cells, right)?;
        Ok(self.append(&page))
    }

    /// `build`'s result; a failure (also memory refused) is kept, so that
    /// [`Writer::finish`] tells it too.
    fn step(&mut self, build: impl FnOnce(&mut Self) -> Result<u32, Error>) -> Result<u32, Error> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        let root = build(self);
        if let Err(error) = &root {
            self.failed.get_or_insert_with(|| error.clone());
        }
        match &self.failed {
            Some(error) => Err(error.clone()),
            None => root,
        }
    }

    /// Writes a table of `(rowid, record)` rows, given in order of their
    /// ids; returns its root page.
    pub fn table(&mut self, rows: impl IntoIterator<Item = (i64, Vec<u8>)>) -> Result<u32, Error> {
        self.step(|writer| writer.build_table(rows))
    }

    fn build_table(
        &mut self,
        rows: impl IntoIterator<Item = (i64, Vec<u8>)>,
    ) -> Result<u32, Error> {
        // Leaves: (page, largest id in it).
        let mut children: Vec<(u32, i64)> = Vec::new();
        let mut leaf: Vec<Cell> = Vec::new();
        let mut last = 0;
        for (rowid, record) in rows {
            let cell = Cell {
                prefix: prefix(None, &[record.len() as u64, rowid as u64])?,
                payload: record,
                table_leaf: true,
            };
            let lens = leaf.iter().map(Cell::len).chain([cell.len()]);
            if !leaf.is_empty() && !fits(TABLE_LEAF, 0, lens) {
                let page = self.write_page(TABLE_LEAF, &leaf, 0)?;
                push(&mut children, (page, last))?;
                leaf.clear();
            }
            push(&mut leaf, cell)?;
            last = rowid;
        }
        let page = self.write_page(TABLE_LEAF, &leaf, 0)?;
        push(&mut children, (page, last))?;
        // Levels above: a cell per child (its page and largest id) but the
        // right-most.
        while children.len() > 1 {
            let n = children.len() - 1;
            let (cells, lens) = level_cells(n, |i| {
                let (page, key) = children[i];
                Ok(Cell {
                    prefix: prefix(Some(page), &[key as u64])?,
                    payload: Vec::new(),
                    table_leaf: false,
                })
            })?;
            let mut parents = Vec::new();
            for (start, end) in plan(TABLE_INTERIOR, &lens)? {
                let (right, key) = children[end];
                let page = self.write_page(TABLE_INTERIOR, &cells[start..end], right)?;
                push(&mut parents, (page, key))?;
            }
            children = parents;
        }
        Ok(children[0].0)
    }

    /// Writes an index of `entries` (records whose last value is the row
    /// id), given in order; returns its root page.
    pub fn index(&mut self, entries: impl IntoIterator<Item = Vec<u8>>) -> Result<u32, Error> {
        self.step(|writer| writer.build_index(entries))
    }

    fn build_index(&mut self, entries: impl IntoIterator<Item = Vec<u8>>) -> Result<u32, Error> {
        let cell = |entry: Vec<u8>| -> Result<Cell, Error> {
            Ok(Cell {
                prefix: prefix(None, &[entry.len() as u64])?,
                payload: entry,
                table_leaf: false,
            })
        };
        // Leaves and the entries between them: an index keeps each entry
        // once, so the entry after a full leaf goes up as the separator.
        let mut children: Vec<u32> = Vec::new();
        let mut separators: Vec<Vec<u8>> = Vec::new();
        let mut leaf: Vec<Cell> = Vec::new();
        let mut entries = entries.into_iter().peekable();
        while let Some(entry) = entries.next() {
            let c = cell(entry)?;
            let lens = leaf.iter().map(Cell::len).chain([c.len()]);
            if leaf.is_empty() || fits(INDEX_LEAF, 0, lens) {
                push(&mut leaf, c)?;
                continue;
            }
            // Full. The entry after it separates this leaf from the next;
            // if there is none, the leaf's own last entry does, so the
            // last leaf is never empty.
            let separator = if entries.peek().is_some() {
                c.payload
            } else {
                let last = leaf.pop().map(|l| l.payload).unwrap_or_default();
                let page = self.write_page(INDEX_LEAF, &leaf, 0)?;
                push(&mut children, page)?;
                push(&mut separators, last)?;
                leaf.clear();
                push(&mut leaf, c)?;
                continue;
            };
            let page = self.write_page(INDEX_LEAF, &leaf, 0)?;
            push(&mut children, page)?;
            push(&mut separators, separator)?;
            leaf.clear();
        }
        let page = self.write_page(INDEX_LEAF, &leaf, 0)?;
        push(&mut children, page)?;
        while children.len() > 1 {
            let pairs = separators.len().min(children.len());
            let (cells, lens) = level_cells(pairs, |i| {
                let separator = &separators[i];
                Ok(Cell {
                    prefix: prefix(Some(children[i]), &[separator.len() as u64])?,
                    payload: copy(separator)?,
                    table_leaf: false,
                })
            })?;
            let mut up_children = Vec::new();
            let mut up_separators = Vec::new();
            for (start, end) in plan(INDEX_INTERIOR, &lens)? {
                let page = self.write_page(INDEX_INTERIOR, &cells[start..end], children[end])?;
                push(&mut up_children, page)?;
                if end < separators.len() {
                    push(&mut up_separators, copy(&separators[end])?)?;
                }
            }
            children = up_children;
            separators = up_separators;
        }
        Ok(children[0])
    }

    /// Writes the file: page 1 with the header and the `schema`, then the
    /// pages built. `user_version` is the application's schema version.
    pub fn finish(mut self, schema: &[SchemaEntry], user_version: u32) -> Result<(), Error> {
        let result = match self.failed.take() {
            Some(e) => Err(e),
            None => self.write_file(schema, user_version),
        };
        // The temporary pages go, whether the file was written or not (no
        // name where memory for it was refused).
        if !self.part.is_empty() {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "a temporary file, gone either way; the write's result is the one told"
            )]
            let _ = self.storage.remove(&self.part);
        }
        result
    }

    /// Writes page 1 and copies the pages built after it.
    fn write_file(&mut self, schema: &[SchemaEntry], user_version: u32) -> Result<(), Error> {
        let mut cells = Vec::new();
        reserve(&mut cells, schema.len())?;
        for (i, entry) in schema.iter().enumerate() {
            let sql = if entry.sql.is_empty() {
                Value::Null
            } else {
                Value::Text(text(&[&entry.sql])?)
            };
            let record = try_encode_record(&[
                Value::Text(text(&[&entry.kind])?),
                Value::Text(text(&[&entry.name])?),
                Value::Text(text(&[&entry.table])?),
                Value::Int(i64::from(entry.root)),
                sql,
            ])?;
            if local_size(record.len(), PAGE, true) < record.len() {
                return Err(Error::SchemaTooLarge);
            }
            cells.push(Cell {
                prefix: prefix(None, &[record.len() as u64, i as u64 + 1])?,
                payload: record,
                table_leaf: true,
            });
        }
        if !fits(TABLE_LEAF, 100, cells.iter().map(Cell::len)) {
            return Err(Error::SchemaTooLarge);
        }
        let mut first = self.page_bytes(TABLE_LEAF, 100, &cells, 0)?;
        let pages = self.next - 1;
        let header = &mut first[..100];
        header[..16].copy_from_slice(b"SQLite format 3\0");
        header[16..18].copy_from_slice(&(PAGE as u16).to_be_bytes());
        // File format versions (rollback journal), no reserved bytes,
        // payload fractions 64, 32, 32.
        header[18..24].copy_from_slice(&[1, 1, 0, 64, 32, 32]);
        let be = |v: u32| v.to_be_bytes();
        header[24..28].copy_from_slice(&be(1)); // change counter
        header[28..32].copy_from_slice(&be(pages));
        header[40..44].copy_from_slice(&be(1)); // schema cookie
        header[44..48].copy_from_slice(&be(4)); // schema format
        header[56..60].copy_from_slice(&be(1)); // UTF-8
        header[60..64].copy_from_slice(&be(user_version));
        header[92..96].copy_from_slice(&be(1)); // version valid for
        header[96..100].copy_from_slice(&be(SQLITE_VERSION));
        // Put together beside the file, then moved over it: a write that
        // fails part way leaves the file there as it was.
        let new = text(&[&self.path, ".new"])?;
        let mut chunk = zeroed(PAGE * 8)?;
        let written = self.assemble(&new, &first, &mut chunk).and_then(|()| {
            match self.storage.rename(&new, &self.path) {
                // Storages that cannot move files: copied over it, and
                // removed again where that fails, so no part of a
                // database is left there.
                Err(StorageError::Unsupported) => {
                    let copied = self.assemble(&self.path, &first, &mut chunk);
                    if copied.is_err() {
                        #[expect(clippy::let_underscore_must_use, reason = "cleaning up after a failed write, whose own error is the one told")]
                        let _ = self.storage.remove(&self.path);
                    }
                    copied
                }
                moved => moved,
            }
        });
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a temporary file, gone either way; the write's result is the one told"
        )]
        let _ = self.storage.remove(&new);
        written.map_err(Error::from)
    }

    /// Writes page 1 (`first`) and the pages built after it to `path`,
    /// through `chunk`.
    fn assemble(&self, path: &str, first: &[u8], chunk: &mut [u8]) -> Result<(), StorageError> {
        self.storage.write(path, first)?;
        let mut at = 0u64;
        loop {
            let n = self.storage.read_at(&self.part, at, chunk)?;
            if n == 0 {
                return Ok(());
            }
            self.storage.append(path, &chunk[..n])?;
            at += n as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Memory;
    use crate::record::encode_record;
    use crate::{Database, compare_records, decode_record};
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn plans_keep_a_cell_in_every_node() {
        // 1000 bytes a cell: four fit in a page.
        let lens = vec![1000; 9];
        let nodes = plan(INDEX_INTERIOR, &lens).unwrap();
        assert!(nodes.iter().all(|(s, e)| e > s), "{nodes:?}");
        // Cells and boundaries cover every cell once.
        let covered: usize = nodes.iter().map(|(s, e)| e - s).sum::<usize>() + nodes.len() - 1;
        assert_eq!(covered, 9);
        assert_eq!(nodes.last().map(|n| n.1), Some(9));
    }

    #[test]
    fn tables_and_indexes_read_back() {
        let storage = Memory::default();
        let mut writer = Writer::new(&storage, "db");
        let long = "x".repeat(9000);
        let rows: Vec<(i64, Vec<u8>)> = (1..=3000)
            .map(|i| {
                let text = if i == 1500 {
                    long.clone()
                } else {
                    alloc::format!("row {i}")
                };
                (i, encode_record(&[Value::Int(i * 7), Value::Text(text)]))
            })
            .collect();
        let table = writer.table(rows.clone()).unwrap();
        let mut entries: Vec<Vec<u8>> = (1..=3000i64)
            .map(|i| encode_record(&[Value::Int((i * 37) % 1000), Value::Int(i)]))
            .collect();
        entries.sort_by(|a, b| compare_records(a, b));
        let index = writer.index(entries).unwrap();
        let schema = [
            SchemaEntry {
                kind: "table".to_string(),
                name: "t".to_string(),
                table: "t".to_string(),
                root: table,
                sql: "CREATE TABLE t(a integer, b text)".to_string(),
            },
            SchemaEntry {
                kind: "index".to_string(),
                name: "t_a".to_string(),
                table: "t".to_string(),
                root: index,
                sql: "CREATE INDEX t_a ON t(a)".to_string(),
            },
        ];
        writer.finish(&schema, 7).unwrap();
        let db = Database::open(&storage, "db").unwrap();
        assert_eq!(db.user_version, 7);
        assert_eq!(db.schema().unwrap(), schema);
        let (columns, root) = db.table("t").unwrap();
        assert_eq!(columns, ["a", "b"]);
        let mut read = Vec::new();
        db.scan(root, &mut |id, values| {
            read.push((id, encode_record(values)));
            true
        })
        .unwrap();
        assert_eq!(read, rows);
        assert!(decode_record(&read[1499].1).is_some());
        assert!(storage.size("db.part").is_err(), "the part file is gone");
    }
}

/// Writes a sample database for SQLite itself to check:
/// `PAPERPAW_SQLITE_OUT=x.db cargo test -p paperpaw-sqlite -- --ignored`.
#[cfg(test)]
mod check {
    extern crate std;

    use super::*;
    use crate::{compare_records, encode_record};
    use alloc::string::ToString;

    #[test]
    #[ignore]
    fn sample_for_sqlite() {
        let Ok(out) = std::env::var("PAPERPAW_SQLITE_OUT") else {
            return;
        };
        let storage = crate::memory::Memory::default();
        let mut writer = Writer::new(&storage, "db");
        let long = "y".repeat(12_000);
        let rows: Vec<(i64, Vec<u8>)> = (1..=20_000)
            .map(|i| {
                let text = if i % 5000 == 0 {
                    long.clone()
                } else {
                    alloc::format!("row {i}")
                };
                (i, encode_record(&[Value::Int(i % 977), Value::Text(text)]))
            })
            .collect();
        let table = writer.table(rows).unwrap();
        let mut entries: Vec<Vec<u8>> = (1..=20_000i64)
            .map(|i| {
                let text = if i % 5000 == 0 {
                    "y".repeat(12_000)
                } else {
                    alloc::format!("row {i}")
                };
                encode_record(&[Value::Int(i % 977), Value::Text(text), Value::Int(i)])
            })
            .collect();
        entries.sort_by(|a, b| compare_records(a, b));
        let index = writer.index(entries).unwrap();
        let schema = [
            SchemaEntry {
                kind: "table".to_string(),
                name: "t".to_string(),
                table: "t".to_string(),
                root: table,
                sql: "CREATE TABLE t(a integer, b text)".to_string(),
            },
            SchemaEntry {
                kind: "index".to_string(),
                name: "t_ab".to_string(),
                table: "t".to_string(),
                root: index,
                sql: "CREATE INDEX t_ab ON t(a, b)".to_string(),
            },
        ];
        writer.finish(&schema, 20_221_111).unwrap();
        std::fs::write(out, storage.bytes("db")).unwrap();
    }
}
