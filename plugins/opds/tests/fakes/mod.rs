//! Stand-ins for the data folder and the secrets, for the tests of the
//! engines (no host needed).
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test fixtures"
)]
use paperpaw_plugin_opds::native_catalogs::Vault;
use paperpaw_plugin_opds::native_files::Files;
use paperpaw_plugin_opds::native_keep::Secret;
use paperpaw_sdk::native::Error;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

/// The data folder in memory.
#[derive(Debug, Default)]
pub struct Mem {
    pub files: RefCell<BTreeMap<String, Vec<u8>>>,
    /// Writes fail with `Full` from this one on (counting writes and appends).
    pub full_after: Cell<Option<usize>>,
    pub writes: Cell<usize>,
}

impl Mem {
    pub fn put(&self, name: &str, bytes: &[u8]) {
        self.files
            .borrow_mut()
            .insert(name.to_string(), bytes.to_vec());
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.files
            .borrow()
            .get(name)
            .map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn has(&self, name: &str) -> bool {
        self.files.borrow().contains_key(name)
    }

    fn may_write(&self) -> Result<(), Error> {
        let n = self.writes.get();
        self.writes.set(n + 1);
        match self.full_after.get() {
            Some(limit) if n >= limit => Err(Error::Full),
            _ => Ok(()),
        }
    }
}

impl Files for Mem {
    fn size(&self, name: &str) -> Result<u64, Error> {
        self.files
            .borrow()
            .get(name)
            .map(|b| b.len() as u64)
            .ok_or(Error::NotFound)
    }

    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        let files = self.files.borrow();
        let bytes = files.get(name).ok_or(Error::NotFound)?;
        let from = usize::try_from(offset).unwrap().min(bytes.len());
        let n = out.len().min(bytes.len() - from);
        out[..n].copy_from_slice(&bytes[from..from + n]);
        Ok(n)
    }

    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.may_write()?;
        self.put(name, data);
        Ok(())
    }

    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.may_write()?;
        self.files
            .borrow_mut()
            .entry(name.to_string())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        self.files
            .borrow_mut()
            .remove(name)
            .map(|_| ())
            .ok_or(Error::NotFound)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        let mut files = self.files.borrow_mut();
        let bytes = files.remove(from).ok_or(Error::NotFound)?;
        files.insert(to.to_string(), bytes);
        Ok(())
    }
}

/// The secrets in memory: name to (label, value).
#[derive(Debug, Default)]
pub struct MemVault {
    pub secrets: RefCell<BTreeMap<String, (String, String)>>,
    /// A device that keeps none.
    pub unsupported: bool,
    /// At most this many are kept (the plugin's 32 in the host).
    pub room: Cell<Option<usize>>,
}

impl MemVault {
    pub fn with(name: &str, value: &str) -> Self {
        let vault = Self::default();
        vault
            .secrets
            .borrow_mut()
            .insert(name.to_string(), ("label".to_string(), value.to_string()));
        vault
    }

    pub fn value(&self, name: &str) -> Option<String> {
        self.secrets.borrow().get(name).map(|(_, v)| v.clone())
    }

    pub fn names(&self) -> Vec<String> {
        self.secrets.borrow().keys().cloned().collect()
    }
}

impl Vault for MemVault {
    fn read(&self, name: &str) -> Result<Option<Secret>, Error> {
        if self.unsupported {
            return Err(Error::Unsupported);
        }
        self.secrets
            .borrow()
            .get(name)
            .map(|(_, value)| Secret::copy(value))
            .transpose()
    }

    fn keep(&self, name: &str, label: &str, value: &str) -> Result<(), Error> {
        if self.unsupported {
            return Err(Error::Unsupported);
        }
        let mut secrets = self.secrets.borrow_mut();
        if !secrets.contains_key(name) && self.room.get().is_some_and(|room| secrets.len() >= room)
        {
            return Err(Error::Full);
        }
        secrets.insert(name.to_string(), (label.to_string(), value.to_string()));
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        self.secrets.borrow_mut().remove(name);
        Ok(())
    }

    fn others(&self, wanted: &[String]) -> Result<Vec<String>, Error> {
        Ok(self
            .secrets
            .borrow()
            .keys()
            .filter(|k| !wanted.contains(k))
            .cloned()
            .collect())
    }
}
