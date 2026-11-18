//! A volume in memory for the tests: the storage calls over files in a map.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::files::Volume;

/// The storage calls over files in memory.
#[derive(Default)]
pub(crate) struct Mem {
    pub(crate) files: RefCell<BTreeMap<String, Vec<u8>>>,
    /// Every append is at most one call's data.
    pub(crate) longest: RefCell<usize>,
}

impl Mem {
    pub(crate) fn with(name: &str, bytes: &[u8]) -> Self {
        let mem = Self::default();
        mem.files.borrow_mut().insert(name.into(), bytes.into());
        mem
    }

    pub(crate) fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(name).cloned()
    }
}

impl Volume for Mem {
    fn size(&self, name: &str) -> Result<u64, Error> {
        self.files
            .borrow()
            .get(name)
            .map(|f| f.len() as u64)
            .ok_or(Error::NotFound)
    }

    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let files = self.files.borrow();
        let file = files.get(name).ok_or(Error::NotFound)?;
        let from = usize::try_from(at).unwrap().min(file.len());
        let n = out.len().min(file.len() - from);
        out[..n].copy_from_slice(&file[from..from + n]);
        Ok(n)
    }

    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.files.borrow_mut().insert(name.into(), data.into());
        Ok(())
    }

    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        assert!(data.len() <= MAX_DATA);
        let mut longest = self.longest.borrow_mut();
        *longest = (*longest).max(data.len());
        self.files
            .borrow_mut()
            .entry(name.into())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        let file = self
            .files
            .borrow_mut()
            .remove(from)
            .ok_or(Error::NotFound)?;
        self.files.borrow_mut().insert(to.into(), file);
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        self.files
            .borrow_mut()
            .remove(name)
            .map(|_| ())
            .ok_or(Error::NotFound)
    }

    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
}
