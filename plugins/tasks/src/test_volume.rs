//! Storage in memory for the tests: every call that changes something is
//! counted, and one of them can be where the power is cut (it and every later
//! one refused, nothing applied). A write or a rename is whole or not at all,
//! as the host's are.
#![allow(clippy::unwrap_used, reason = "fixtures")]
use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::cell::{Cell, RefCell};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::files::Volume;

#[derive(Debug, Default)]
pub struct Fake {
    pub files: RefCell<BTreeMap<String, Vec<u8>>>,
    ops: Cell<usize>,
    cut_at: Cell<usize>,
}

impl Fake {
    /// How many changing calls were made.
    pub fn ops(&self) -> usize {
        self.ops.get()
    }

    /// The power is cut at the `n`th changing call from now (1 is the next).
    pub fn cut_at(&self, n: usize) {
        self.ops.set(0);
        self.cut_at.set(n);
    }

    /// The power is back.
    pub fn restart(&self) {
        self.ops.set(0);
        self.cut_at.set(0);
    }

    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(name).cloned()
    }

    pub fn text(&self, name: &str) -> String {
        String::from_utf8(self.get(name).unwrap()).unwrap()
    }

    pub fn put(&self, name: &str, data: &[u8]) {
        self.files.borrow_mut().insert(name.into(), data.into());
    }

    fn step(&self) -> Result<(), Error> {
        let n = self.ops.get() + 1;
        self.ops.set(n);
        let at = self.cut_at.get();
        if at != 0 && n >= at {
            return Err(Error::Io);
        }
        Ok(())
    }
}

impl Volume for Fake {
    fn size(&self, name: &str) -> Result<u64, Error> {
        self.files
            .borrow()
            .get(name)
            .map(|data| data.len() as u64)
            .ok_or(Error::NotFound)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let files = self.files.borrow();
        let data = files.get(name).ok_or(Error::NotFound)?;
        let rest = data.get(usize::try_from(at).unwrap()..).unwrap_or(&[]);
        let n = rest.len().min(out.len());
        out[..n].copy_from_slice(&rest[..n]);
        Ok(n)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.step()?;
        assert!(data.len() <= 4096, "one call takes 4 KiB");
        self.put(name, data);
        Ok(())
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.step()?;
        assert!(data.len() <= 4096, "one call takes 4 KiB");
        self.files
            .borrow_mut()
            .entry(name.into())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        self.step()?;
        let mut files = self.files.borrow_mut();
        let data = files.remove(from).ok_or(Error::NotFound)?;
        files.insert(to.into(), data);
        Ok(())
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        self.step()?;
        self.files
            .borrow_mut()
            .remove(name)
            .map(|_| ())
            .ok_or(Error::NotFound)
    }
    fn sync(&self) -> Result<(), Error> {
        self.step()
    }
}
