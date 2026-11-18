//! The members of the stages a plugin runs, frozen for one of its callbacks
//! (`docs/abi/stages-services.md` section 1.2): the first `stage.run` of a
//! stage in a callback takes the list from the declaration store, and every
//! cursor of that callback reads the same list. A member that stops or traps
//! meanwhile stays in it, and its reply is `ERR_CANCELLED`: no member is
//! ever skipped silently. The next callback takes a fresh list.
//!
//! This is not a second registry: it copies what the store held at one
//! moment, bounded (4 stages of at most 32 members) and dropped by the next
//! callback.
use super::native_declarations::NativeDeclarations;
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_MEMORY};

/// The stages one callback may run, each with its own frozen list.
const STAGES: usize = 4;

/// One member, as the store held it.
pub(crate) struct Member {
    /// The subscribing plugin's host owner id.
    pub(crate) owner: u64,
    pub(crate) token: u32,
    pub(crate) signature: u64,
    pub(crate) id: Box<str>,
}

#[derive(Default)]
pub(crate) struct StageMembers {
    epoch: u64,
    stages: Vec<(Box<str>, Vec<Member>)>,
}

/// A copy of `text`, allocated fallibly.
pub(crate) fn boxed(text: &str) -> Result<Box<str>, i32> {
    let mut bytes = String::new();
    bytes
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.push_str(text);
    Ok(bytes.into_boxed_str())
}

impl StageMembers {
    /// The member of `stage` at `cursor` in the callback `epoch`: `None` past
    /// the last. The first ask of a stage in a callback freezes the list with
    /// `freeze`; a failure leaves nothing frozen.
    ///
    /// # Errors
    /// What `freeze` fails with, `ERR_NO_MEMORY`, and `ERR_FULL` for a fifth
    /// stage in one callback.
    pub(crate) fn member(
        &mut self,
        epoch: u64,
        stage: &str,
        cursor: usize,
        freeze: impl FnOnce() -> Result<Vec<Member>, i32>,
    ) -> Result<Option<&Member>, i32> {
        if epoch == 0 {
            return Err(ERR_INVALID);
        }
        if self.epoch != epoch {
            self.stages.clear();
            self.epoch = epoch;
        }
        let at = match self.stages.iter().position(|(key, _)| &**key == stage) {
            Some(at) => at,
            None => {
                if self.stages.len() >= STAGES {
                    return Err(ERR_FULL);
                }
                self.stages.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                let key = boxed(stage)?;
                self.stages.push((key, freeze()?));
                self.stages.len() - 1
            }
        };
        Ok(self.stages[at].1.get(cursor))
    }
}

impl StageMembers {
    /// Frees what the callback `epoch` froze; another callback's list stays.
    pub(crate) fn end(&mut self, epoch: u64) {
        if self.epoch == epoch {
            self.epoch = 0;
            self.stages = Vec::new();
        }
    }

    /// Frees every list, whatever callback froze it.
    pub(crate) fn end_all(&mut self) {
        self.epoch = 0;
        self.stages = Vec::new();
    }

    /// Whether nothing is frozen.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }
}

/// Frees the lists a callback froze when it ends, so that nothing stays held
/// between callbacks (up to 32 members of each of 4 stages). A nested
/// callback has an epoch of its own and leaves its caller's lists alone.
pub(crate) struct Release<'a> {
    directory: Option<&'a NativeDeclarations>,
    epoch: u64,
}

impl<'a> Release<'a> {
    /// For the callback `epoch`. `outermost` is a callback that starts with no
    /// other running: whatever an earlier one left frozen (a path that did not
    /// release, a callback that ended abnormally) is freed at once.
    pub(crate) fn new(
        directory: Option<&'a NativeDeclarations>,
        epoch: u64,
        outermost: bool,
    ) -> Self {
        if outermost
            && let Some(directory) = directory
            && let Ok(mut members) = directory.members.try_borrow_mut()
        {
            members.end_all();
        }
        Self { directory, epoch }
    }
}

impl Drop for Release<'_> {
    fn drop(&mut self) {
        // The list is only borrowed inside `stage.run`; a refusal here leaves
        // it to the next callback, which starts from a fresh list anyway.
        if let Some(directory) = self.directory
            && let Ok(mut members) = directory.members.try_borrow_mut()
        {
            members.end(self.epoch);
        }
    }
}
