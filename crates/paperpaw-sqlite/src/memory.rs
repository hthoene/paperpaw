//! Files in memory for the tests: the in-memory storage of the SDK is not
//! reachable from here.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use paperpaw_platform::{DirEntry, Storage, StorageError};

#[derive(Default)]
pub(crate) struct Memory {
    files: RefCell<Vec<(String, Vec<u8>)>>,
    /// How many more reads succeed, if limited: a bound on the work of a
    /// reader that would otherwise go on for very long.
    reads: Cell<Option<usize>>,
}

impl Memory {
    pub(crate) fn limit_reads(&self, reads: usize) {
        self.reads.set(Some(reads));
    }

    /// The contents of file `path`.
    pub(crate) fn bytes(&self, path: &str) -> Vec<u8> {
        let files = self.files.borrow();
        files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, d)| d.clone())
            .unwrap_or_default()
    }
}

impl Storage for Memory {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        match self.reads.get() {
            Some(0) => return Err(StorageError::Io),
            Some(n) => self.reads.set(Some(n - 1)),
            None => {}
        }
        let files = self.files.borrow();
        let (_, data) = files
            .iter()
            .find(|(p, _)| p == path)
            .ok_or(StorageError::NotFound)?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let files = self.files.borrow();
        files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, d)| d.len() as u64)
            .ok_or(StorageError::NotFound)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut files = self.files.borrow_mut();
        files.retain(|(p, _)| p != path);
        files.push((path.to_string(), data.to_vec()));
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut files = self.files.borrow_mut();
        match files.iter_mut().find(|(p, _)| p == path) {
            Some((_, d)) => d.extend_from_slice(data),
            None => files.push((path.to_string(), data.to_vec())),
        }
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let mut files = self.files.borrow_mut();
        let at = files
            .iter()
            .position(|(p, _)| p == from)
            .ok_or(StorageError::NotFound)?;
        let (_, data) = files.remove(at);
        files.retain(|(p, _)| p != to);
        files.push((to.to_string(), data));
        Ok(())
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.borrow_mut().retain(|(p, _)| p != path);
        Ok(())
    }
}
