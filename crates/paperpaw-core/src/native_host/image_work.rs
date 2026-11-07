//! One deferred command batch per owner; no borrow crosses plugin callbacks.
use super::image_service::{
    ImageTable,
    commands::{ImageCommand, ImageRoots},
};
use super::shell_images::OwnerRoots;
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

struct Pending {
    owner: u64,
    plugin: Id,
    storage_root: bool,
    active: bool,
    commands: Vec<ImageCommand>,
}

#[derive(Default)]
pub(super) struct ImageWork(RefCell<Vec<Pending>>);

impl ImageWork {
    pub(super) const fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
    #[allow(
        dead_code,
        reason = "callback command runner remains gated pending native activation"
    )]
    pub(super) fn begin(
        &self,
        owner: u64,
        plugin: &Id,
        storage_root: bool,
    ) -> Result<ImageBatch<'_>, i32> {
        let mut pending = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if pending.iter().any(|batch| batch.owner == owner) {
            return Err(ERR_BUSY);
        }
        let plugin = plugin.try_clone().ok_or(ERR_NO_MEMORY)?;
        pending.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        pending.push(Pending {
            owner,
            plugin,
            storage_root,
            active: true,
            commands: Vec::new(),
        });
        Ok(ImageBatch {
            work: self,
            owner,
            committed: false,
        })
    }

    pub(super) fn owner_stopped(&self, owner: u64) -> Result<(), i32> {
        self.0
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .retain(|batch| batch.owner != owner);
        Ok(())
    }

    pub(super) fn pending(&self) -> bool {
        self.0
            .try_borrow()
            .map_or(true, |pending| !pending.is_empty())
    }

    pub(super) fn apply(&self, images: &ImageTable) -> Result<(), i32> {
        let mut pending = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let mut first_error = None;
        for batch in pending.iter_mut().filter(|batch| !batch.active) {
            let roots = OwnerRoots::new(batch.owner, &batch.plugin, batch.storage_root);
            if let Err(error) = Self::apply_batch(images, batch.owner, &mut batch.commands, &roots)
            {
                first_error.get_or_insert(error);
            }
        }
        pending.retain(|batch| batch.active || !batch.commands.is_empty());
        first_error.map_or(Ok(()), Err)
    }

    fn apply_batch(
        images: &ImageTable,
        owner: u64,
        commands: &mut Vec<ImageCommand>,
        roots: &dyn ImageRoots,
    ) -> Result<(), i32> {
        while let Some(command) = commands.first() {
            images.apply(owner, command, roots)?;
            commands.remove(0);
        }
        Ok(())
    }
}

#[allow(
    dead_code,
    reason = "callback command runner remains gated pending native activation"
)]
pub(super) struct ImageBatch<'a> {
    work: &'a ImageWork,
    owner: u64,
    committed: bool,
}
#[allow(
    dead_code,
    reason = "callback command runner remains gated pending native activation"
)]
impl ImageBatch<'_> {
    pub(super) fn finish(mut self, commands: Vec<ImageCommand>) -> Result<(), i32> {
        let mut pending = self.work.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let batch = pending
            .iter_mut()
            .find(|batch| batch.owner == self.owner)
            .ok_or(ERR_STATE)?;
        batch.commands = commands;
        batch.active = false;
        pending.retain(|batch| batch.active || !batch.commands.is_empty());
        self.committed = true;
        Ok(())
    }
}
impl Drop for ImageBatch<'_> {
    fn drop(&mut self) {
        if !self.committed {
            // No callback or table borrow can span guard destruction. If an
            // internal misuse holds one, an empty active row refuses new work.
            let _ = self.work.owner_stopped(self.owner);
        }
    }
}
