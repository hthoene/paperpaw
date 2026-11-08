//! What the tests of this crate share: a storage in memory that counts
//! its reads (and can stop answering after a budget, so a reader caught
//! in a loop fails instead of hanging), helpers to build databases, and
//! files on disk for real SQLite.

// Not every test file uses every helper.
#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sqlite::{Database, SchemaEntry, Value, Writer, encode_record};

/// Files in memory.
#[derive(Default)]
pub struct Memory {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    /// Reads so far.
    pub reads: Cell<usize>,
    /// Reads answered before every further one fails.
    pub budget: Cell<Option<usize>>,
    /// Appends fail (a full card).
    pub full: Cell<bool>,
    /// Files cannot be moved (a storage without `rename`).
    pub fixed: Cell<bool>,
}

impl Memory {
    pub fn with(path: &str, data: Vec<u8>) -> Self {
        let memory = Self::default();
        memory.files.borrow_mut().insert(path.to_string(), data);
        memory
    }

    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(path).cloned()
    }

    pub fn put(&self, path: &str, data: Vec<u8>) {
        self.files.borrow_mut().insert(path.to_string(), data);
    }

    pub fn paths(&self) -> Vec<String> {
        self.files.borrow().keys().cloned().collect()
    }
}

impl Storage for Memory {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.reads.set(self.reads.get() + 1);
        if self.budget.get().is_some_and(|b| self.reads.get() > b) {
            return Err(StorageError::Io);
        }
        let files = self.files.borrow();
        let data = files.get(path).ok_or(StorageError::NotFound)?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files
            .borrow()
            .get(path)
            .map(|d| d.len() as u64)
            .ok_or(StorageError::NotFound)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.put(path, data.to_vec());
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.full.get() {
            return Err(StorageError::Full);
        }
        self.files
            .borrow_mut()
            .entry(path.to_string())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.borrow_mut().remove(path);
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if self.fixed.get() {
            return Err(StorageError::Unsupported);
        }
        let mut files = self.files.borrow_mut();
        let data = files.remove(from).ok_or(StorageError::NotFound)?;
        files.insert(to.to_string(), data);
        Ok(())
    }
}

/// The page size the writer uses.
pub const PAGE: usize = 4096;

pub fn text(t: &str) -> Value {
    Value::Text(t.to_string())
}

/// A schema entry of a table `name` with `columns` (no types: values are
/// stored as they are given).
pub fn table_entry(name: &str, columns: &[&str], root: u32) -> SchemaEntry {
    SchemaEntry {
        kind: "table".to_string(),
        name: name.to_string(),
        table: name.to_string(),
        root,
        sql: format!("CREATE TABLE {name}({})", columns.join(", ")),
    }
}

pub fn index_entry(name: &str, table: &str, columns: &[&str], root: u32) -> SchemaEntry {
    SchemaEntry {
        kind: "index".to_string(),
        name: name.to_string(),
        table: table.to_string(),
        root,
        sql: format!("CREATE INDEX {name} ON {table}({})", columns.join(", ")),
    }
}

/// A database `db` with table `t(a, b, c, d, e, f)` holding `rows`.
pub fn write_table(storage: &Memory, rows: &[(i64, Vec<Value>)]) -> u32 {
    let mut writer = Writer::new(storage, "db");
    let root = writer
        .table(rows.iter().map(|(id, v)| (*id, encode_record(v))))
        .unwrap();
    writer
        .finish(&[table_entry("t", &COLUMNS, root)], 1)
        .unwrap();
    root
}

pub const COLUMNS: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

/// Every row of the table at `root`: its id and its record as stored.
pub fn read_rows(
    db: &Database<'_>,
    root: u32,
) -> Result<Vec<(i64, Vec<u8>)>, paperpaw_sqlite::Error> {
    let mut rows = Vec::new();
    db.scan(root, &mut |id, values| {
        rows.push((id, encode_record(values)));
        true
    })?;
    Ok(rows)
}

/// A file path for real SQLite, removed when dropped.
pub struct TempFile(pub PathBuf);

impl TempFile {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("paperpaw-sqlite-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut other = self.0.clone().into_os_string();
            other.push(suffix);
            let _ = std::fs::remove_file(other);
        }
    }
}
