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
