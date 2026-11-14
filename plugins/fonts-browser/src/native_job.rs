//! The download of one font as data without the host: which file is next,
//! and whether a file that arrived is the one the catalog named (its size and
//! SHA-1, read in pieces, never whole), and clearing what was staged.
use crate::native_plan::{Plan, Step};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Storage, StorageError, digest::sha1::Sha1, native::Error};
use paperpaw_ui_kit::mem::buffer;

/// Bytes of a file read at a time to check it.
const PIECE: usize = 4096;

/// A font being downloaded.
#[derive(Debug)]
pub struct Job {
    /// What is downloaded.
    pub plan: Plan,
    /// The font's name, for what is said.
    pub name: String,
    /// The font is installed already: the host replaces it.
    pub update: bool,
    /// The file being downloaded.
    next: usize,
    /// Every file is there and the host was asked to install the folder.
    installing: bool,
    /// The request number of this job's install question.
    request: u32,
}

impl Job {
    /// A job at its first file.
    #[must_use]
    pub const fn new(plan: Plan, name: String, update: bool, request: u32) -> Self {
        Self {
            plan,
            name,
            update,
            next: 0,
            installing: false,
            request,
        }
    }

    /// The file being downloaded; `None` once every file is there.
    #[must_use]
    pub fn current(&self) -> Option<&Step> {
        self.plan.steps.get(self.next)
    }

    /// The current file is checked and kept: on to the next. `true` while
    /// there is one.
    pub fn advance(&mut self) -> bool {
        self.next += 1;
        self.current().is_some()
    }

    /// How many files are here (checked and kept).
    #[must_use]
    pub const fn done(&self) -> usize {
        self.next
    }

    /// How many files the font has.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.plan.steps.len()
    }

    /// Notes that the host was asked to install the folder.
    pub const fn install_asked(&mut self) {
        self.installing = true;
    }

    /// The request number of the install question of this job.
    #[must_use]
    pub const fn request(&self) -> u32 {
        self.request
    }

    /// Whether `install_done` for `request` ends this job: its own question,
    /// asked, and no other (a stale answer of a stopped or earlier job is not).
    #[must_use]
    pub const fn ends_with(&self, request: u32) -> bool {
        self.installing && self.request == request
    }

    /// Whether the host was asked to install it.
    #[must_use]
    pub const fn installing(&self) -> bool {
        self.installing
    }
}

/// The error a storage failure is.
fn io(error: StorageError) -> Error {
    match error {
        StorageError::NotFound => Error::NotFound,
        StorageError::Other(0x6e6d) => Error::NoMemory,
        _ => Error::Io,
    }
}

/// Whether the file `step` saved is the size and SHA-1 the catalog said.
///
/// # Errors
/// `NoMemory`; an unreadable file is not a match, it is `Ok(false)`.
pub fn verify(storage: &dyn Storage, step: &Step) -> Result<bool, Error> {
    match storage.size(&step.save) {
        Ok(size) if size == step.size => {}
        Ok(_) => return Ok(false),
        Err(error) => return fallible(error),
    }
    let mut piece = buffer(PIECE)?;
    let mut sha1 = Sha1::new();
    let mut at = 0u64;
    loop {
        let n = match storage.read_at(&step.save, at, &mut piece) {
            Ok(n) => n,
            Err(error) => return fallible(error),
        };
        if n == 0 {
            break;
        }
        sha1.update(piece.get(..n).unwrap_or(&[]));
        at += n as u64;
    }
    Ok(at == step.size
        && paperpaw_sdk::digest::hex::encode(&sha1.finish()) == step.sha1.to_ascii_lowercase())
}

/// A refused allocation is the caller's; any other failure is a file that does
/// not check out.
fn fallible(error: StorageError) -> Result<bool, Error> {
    match io(error) {
        Error::NoMemory => Err(Error::NoMemory),
        _ => Ok(false),
    }
}

/// Removes the files the plan staged (what is not there is not an error) and
/// the folders they were in.
pub fn clear(storage: &dyn Storage, plan: &Plan) {
    let mut folders: Vec<&str> = Vec::new();
    for step in &plan.steps {
        storage.remove(&step.save).ok();
        let mut path = step.save.as_str();
        while let Some(cut) = path.rfind('/') {
            path = &path[..cut];
            if !folders.contains(&path) && folders.try_reserve(1).is_ok() {
                folders.push(path);
            }
        }
    }
    // Deepest first, up to the plugin's own staging folder.
    folders.sort_unstable_by_key(|p| core::cmp::Reverse(p.len()));
    for folder in folders {
        storage.remove(folder).ok();
    }
}

#[cfg(test)]
mod tests;
