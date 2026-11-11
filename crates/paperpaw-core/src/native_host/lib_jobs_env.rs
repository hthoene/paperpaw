//! The library's delete and import jobs ([`Jobs`](super::lib_jobs::Jobs)) as
//! one step of the main loop has them: the kernel's catalog ([`EnvCarry`]:
//! the card, the plugins' book-data folders, the queue the events go to) and
//! the catalog's own parts (`RemovalCarry`, `ImportCarry`, which Services
//! implements over the same `Env`).
use super::{
    book_data_steps::{self, Progress},
    lib_jobs::{ImportCarry, JobHost, RemovalCarry, RemovalPhase},
    lib_moves_carry::{EnvCarry, STEP_PIECES},
    lib_service::moves::own,
};
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::Storage;
use paperpaw_sdk::builtin::events;

/// The catalog's parts, for a build that has none yet: nothing to forget and
/// nothing to note (the files and the data are still handled).
pub(crate) struct NoCatalog;

impl RemovalCarry for NoCatalog {
    fn forget(&self, _: &str, _: bool, phase: RemovalPhase) -> Result<RemovalPhase, i32> {
        Ok(match phase {
            RemovalPhase::Details => RemovalPhase::Marks,
            _ => RemovalPhase::Done,
        })
    }
}

impl ImportCarry for NoCatalog {
    fn catalogue(&self, _: &str, _: &str) -> Result<(), i32> {
        Ok(())
    }
}

/// A step's view of what the jobs need.
pub(crate) struct EnvJobs<'a> {
    carry: &'a EnvCarry<'a>,
    removal: &'a dyn RemovalCarry,
    import: &'a dyn ImportCarry,
}

impl<'a> EnvJobs<'a> {
    pub(crate) fn new(
        carry: &'a EnvCarry<'a>,
        removal: &'a dyn RemovalCarry,
        import: &'a dyn ImportCarry,
    ) -> Self {
        Self {
            carry,
            removal,
            import,
        }
    }
}

impl RemovalCarry for EnvJobs<'_> {
    fn forget(
        &self,
        path: &str,
        with_notes: bool,
        phase: RemovalPhase,
    ) -> Result<RemovalPhase, i32> {
        self.removal.forget(path, with_notes, phase)
    }
}

impl ImportCarry for EnvJobs<'_> {
    fn catalogue(&self, path: &str, importer: &str) -> Result<(), i32> {
        self.import.catalogue(path, importer)
    }
}

impl JobHost for EnvJobs<'_> {
    fn storage(&self) -> &dyn Storage {
        self.carry.storage()
    }

    fn remove_kept(&self, path: &str) -> Result<Progress, i32> {
        // One budget for the book and its `.bak` sibling: a step stays bounded.
        let mut budget = STEP_PIECES;
        let (storage, data) = (self.carry.storage(), self.carry.book_data());
        let mut bak = alloc::string::String::new();
        bak.try_reserve_exact(path.len() + 4)
            .map_err(|_| ERR_NO_MEMORY)?;
        bak.push_str(path);
        bak.push_str(".bak");
        let mut progress = Progress::Done;
        for name in [path, bak.as_str()] {
            if book_data_steps::remove_data_within(storage, data, name, &mut budget)?
                == Progress::More
                || book_data_steps::remove_doc_cache(storage, name, &mut budget)? == Progress::More
            {
                progress = Progress::More;
            }
        }
        Ok(progress)
    }

    fn remove_cache(&self, path: &str) -> Result<Progress, i32> {
        let mut budget = STEP_PIECES;
        book_data_steps::remove_doc_cache(self.carry.storage(), path, &mut budget)
    }

    fn removed(&self, path: &str) -> Result<(), i32> {
        let path = own(path)?;
        self.carry
            .send(events::BOOK_REMOVED.emit(events::BookRemoved::new(path)))
    }

    fn changed(&self) -> Result<(), i32> {
        self.carry.send(events::LIBRARY_CHANGED.emit(()))
    }
}
