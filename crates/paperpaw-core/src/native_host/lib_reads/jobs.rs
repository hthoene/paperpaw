//! The catalog's parts of the library's delete and import jobs
//! (`lib_jobs`): what [`RemovalCarry`] and [`ImportCarry`] ask of the card for
//! a book that is gone, or that has arrived. Bound to one step of the main
//! loop (the card of the step); every call is idempotent and bounded.
use super::{importer, removal};
use crate::native_host::lib_jobs::{ImportCarry, RemovalCarry, RemovalPhase};
use paperpaw_platform::Storage;

/// The catalog's parts over the card of one step.
pub(crate) struct CatalogJobs<'a> {
    storage: &'a dyn Storage,
}

impl<'a> CatalogJobs<'a> {
    pub(crate) fn new(storage: &'a dyn Storage) -> Self {
        Self { storage }
    }
}

impl RemovalCarry for CatalogJobs<'_> {
    /// `Details` (the catalog line, details with the fields, cover, lists), then
    /// `Marks` (position, annotations, importer record). Annotations always go:
    /// the service refused a book that has them unless the view said `with_notes`.
    fn forget(
        &self,
        path: &str,
        _with_notes: bool,
        phase: RemovalPhase,
    ) -> Result<RemovalPhase, i32> {
        match phase {
            RemovalPhase::Details => {
                removal::details(self.storage, path).map(|()| RemovalPhase::Marks)
            }
            RemovalPhase::Marks => removal::marks(self.storage, path).map(|()| RemovalPhase::Done),
            RemovalPhase::Done => Ok(RemovalPhase::Done),
        }
    }
}

impl ImportCarry for CatalogJobs<'_> {
    /// Keeps who imported the file and what it is (size and partial MD5), so a
    /// later `replace` import of the same name is allowed for the same plugin
    /// while the file is still the one imported. The book itself is catalogued
    /// by the walk the job's `library_changed` starts.
    fn catalogue(&self, path: &str, importer: &str) -> Result<(), i32> {
        importer::set(self.storage, path, importer)
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
