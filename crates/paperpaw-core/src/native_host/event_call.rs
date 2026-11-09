//! A host-raised callback with its command stage (`docs/abi/screens-input.md`
//! 2.1: every callback the host lends a stage, every callback but `draw`):
//! the events of the host's services (`image_ready`, work 103 to 105,
//! `core.net` and `core.sockets`) are delivered through [`Kernel::native_event_call`].
//!
//! What the callback submitted with `commands.submit` is staged within the
//! callback-command limits (64 commands, 4,096 bytes, fallibly reserved: a
//! refusal is the plugin's `ERR_NO_MEMORY`). When the callback returns `0` or
//! `1` the staged batch is kept in the **outbox**, one place, and the kernel
//! runs it ([`Kernel::native_outbox_run`]) outside any callback and before the
//! next callback is called; a callback that returns an error drops its whole
//! batch. A pump that finds the outbox full does not call another callback:
//! it stops, and asks for another pass.
use super::{
    commands::{CALLBACK_COMMAND_BYTES, CALLBACK_COMMANDS, CommandStage, Redraw},
    image_service::commands::ImageCommand,
    native_instances::NativeLease,
};
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_STATE};
use paperpaw_sdk::Command;

/// The commands one callback staged, for its plugin.
pub(crate) struct Staged {
    pub(crate) owner: u64,
    pub(crate) commands: Vec<Command>,
    pub(crate) redraw: Option<Redraw>,
    pub(crate) images: Vec<ImageCommand>,
}

/// The one batch that waits for the kernel to run it.
#[derive(Default)]
pub(crate) struct Outbox(RefCell<Option<Staged>>);

impl Outbox {
    /// Whether a batch waits (no callback is called until it ran).
    pub(crate) fn full(&self) -> bool {
        self.0.try_borrow().map_or(true, |slot| slot.is_some())
    }

    fn put(&self, staged: Staged) -> Result<(), i32> {
        let mut slot = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Err(ERR_BUSY);
        }
        *slot = Some(staged);
        Ok(())
    }

    fn take(&self) -> Option<Staged> {
        self.0.try_borrow_mut().ok()?.take()
    }
}

impl crate::Kernel {
    /// Calls the callback of `owner` registered for the event in `bytes`, with
    /// a command stage, and keeps what it staged in the outbox when it
    /// returned `0` or `1`. The caller runs the outbox
    /// ([`Self::native_outbox_run`]) before it calls another callback.
    ///
    /// # Errors
    /// `ERR_BUSY` when a batch still waits (nothing is called), `ERR_STATE`
    /// for an owner that is not known; otherwise the callback's own status.
    pub(crate) fn native_event_call(
        &self,
        owner: u64,
        lease: &NativeLease,
        bytes: &[u8],
    ) -> Result<i32, i32> {
        let instances = &self.shared.native_instances;
        if instances.outbox.full() {
            return Err(ERR_BUSY);
        }
        let id = instances.id(owner).ok_or(ERR_STATE)?;
        let (status, staged) = {
            let env = self.shared.env(self.now);
            let mut stage = CommandStage::new(
                id,
                env.settings(),
                Some(self.command_settings_authority(id)),
                CALLBACK_COMMANDS,
                CALLBACK_COMMAND_BYTES,
            )
            .with_ui(&env, self.event_screen(id), 0, self.screen_keys_remaining());
            // A run's callback is quiet (`work.md` section 2).
            stage.set_quiet(
                instances
                    .work_services()
                    .is_some_and(|work| work.sched.quiet(owner)),
            );
            let status = lease.service_event_staged(bytes, &env, &mut stage);
            let staged = if matches!(status, Ok(0 | 1)) {
                let images = stage.take_image_commands();
                let (commands, redraw, _hop) = stage.into_ui_output();
                (!commands.is_empty() || redraw.is_some() || !images.is_empty()).then_some(Staged {
                    owner,
                    commands,
                    redraw,
                    images,
                })
            } else {
                // An error drops the whole batch with the stage.
                None
            };
            (status, staged)
        };
        if let Some(staged) = staged {
            // Empty and checked above: this cannot refuse.
            instances.outbox.put(staged)?;
        }
        status
    }

    /// The screen a background callback of `owner` speaks for: its own
    /// topmost one on the stack (a redraw is for it), none when it has none.
    fn event_screen(&self, owner: &paperpaw_sdk::Id) -> Option<paperpaw_sdk::ScreenKey> {
        self.shared
            .stack
            .iter()
            .rev()
            .find(|entry| entry.screen.namespace() == owner.as_str())
            .map(|entry| entry.key)
    }

    /// Runs the batch in the outbox: its commands and picture requests, for
    /// the plugin that staged them (one that is gone loses them).
    pub(crate) fn native_outbox_run(&mut self) {
        let Some(staged) = self.shared.native_instances.outbox.take() else {
            return;
        };
        let Some(owner) = self
            .shared
            .native_instances
            .id(staged.owner)
            .and_then(|id| paperpaw_sdk::Id::try_parse(id.as_str()).ok())
        else {
            return;
        };
        self.drain_native_ui(&owner, staged.commands, staged.redraw);
        self.native_images_apply(staged.owner, staged.images);
    }
}

#[cfg(test)]
#[path = "event_call_tests.rs"]
mod tests;
