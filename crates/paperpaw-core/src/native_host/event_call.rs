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
    screen_stage::ScreenStage,
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
    /// What a screen's callback stages besides (empty for a service's event,
    /// whose stage admits none of it).
    pub(crate) screen: ScreenEffects,
}

/// The effects a screen callback's stage holds beyond commands and pictures,
/// applied after them in this order (`Kernel::apply_staged`).
#[derive(Default)]
pub(crate) struct ScreenEffects {
    pub(crate) policies: Vec<super::commands::PolicyChange>,
    pub(crate) item_states: (
        Vec<super::item_state::ItemPush>,
        Option<super::commands::ItemRoom>,
    ),
    pub(crate) presets: Vec<alloc::boxed::Box<str>>,
    /// The answers to plugin requests (`commands_answers.rs`).
    pub(crate) answers: Vec<super::plugin_requests::Answer>,
    pub(crate) hints: Vec<super::hint_labels::HintSet>,
    /// The widgets it said changed (their makers hear of it from the pump).
    pub(crate) widgets: Vec<u32>,
}

impl ScreenEffects {
    fn is_empty(&self) -> bool {
        self.policies.is_empty()
            && self.item_states.0.is_empty()
            && self.presets.is_empty()
            && self.answers.is_empty()
            && self.hints.is_empty()
            && self.widgets.is_empty()
    }
}

impl Staged {
    /// Whether it holds nothing to apply.
    pub(crate) fn is_empty(&self) -> bool {
        self.commands.is_empty()
            && self.redraw.is_none()
            && self.images.is_empty()
            && self.screen.is_empty()
    }

    /// A batch of commands and pictures, with no screen effects.
    pub(crate) fn new(
        owner: u64,
        commands: Vec<Command>,
        redraw: Option<Redraw>,
        images: Vec<ImageCommand>,
    ) -> Self {
        Self {
            owner,
            commands,
            redraw,
            images,
            screen: ScreenEffects::default(),
        }
    }
}

/// The one batch that waits for the kernel to run it.
#[derive(Default)]
pub(crate) struct Outbox(RefCell<Option<Staged>>);

impl Outbox {
    /// Whether a batch waits (no callback is called until it ran).
    pub(crate) fn full(&self) -> bool {
        self.0.try_borrow().map_or(true, |slot| slot.is_some())
    }

    pub(crate) fn put(&self, staged: Staged) -> Result<(), i32> {
        let mut slot = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Err(ERR_BUSY);
        }
        *slot = Some(staged);
        Ok(())
    }

    pub(crate) fn take(&self) -> Option<Staged> {
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
        self.native_staged_call(owner, |env, stage| {
            lease.service_event_staged(bytes, env, stage)
        })
    }

    /// As [`Self::native_event_call`], for the `input` callback of a widget
    /// at `token` with the `input.widget` record `bytes`.
    ///
    /// # Errors
    /// As [`Self::native_event_call`].
    pub(crate) fn native_input_call(
        &self,
        owner: u64,
        lease: &NativeLease,
        token: u32,
        bytes: &[u8],
    ) -> Result<i32, i32> {
        self.native_staged_call(owner, |env, stage| {
            lease.input_staged(token, bytes, env, stage)
        })
    }

    /// The staged call of `owner`'s callback that `call` makes.
    fn native_staged_call(
        &self,
        owner: u64,
        call: impl FnOnce(&paperpaw_sdk::Env<'_>, &mut CommandStage<'_>) -> Result<i32, i32>,
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
            .with_ui(&env, self.event_screen(id), 0, self.screen_keys_remaining())
            .with_widgets(instances.ui_services(), owner);
            stage.set_images_held(self.image_held(owner));
            stage.set_speech_held(self.io.speech.owned_texts(id.as_str()));
            stage.set_requests_open(
                self.native_requests_open(),
                self.native_requests_open_by(id.as_str()),
            );
            stage.set_answer_view(self.native_answer_view(owner));
            // A run's callback is quiet (`work.md` section 2).
            stage.set_quiet(
                instances
                    .work_services()
                    .is_some_and(|work| work.sched.quiet(owner)),
            );
            let mut status = call(&env, &mut stage);
            let staged = if matches!(status, Ok(0 | 1)) {
                let images = stage.take_image_commands();
                // The widgets it said changed; their makers hear of it from
                // the pump. The table is not held across a callback, so a
                // failed borrow is a host fault: the callback fails
                // (`ERR_BUSY`, as in the shell path) and its batch is dropped.
                let widgets = stage.take_widget_changes();
                let answers = stage.take_answers();
                let marked = match instances.ui_services() {
                    Some(ui) if !widgets.is_empty() => ui.widgets_changed(owner, &widgets),
                    _ => Ok(()),
                };
                if let Err(error) = marked {
                    status = Err(error);
                    None
                } else {
                    let (commands, redraw, _hop) = stage.into_ui_output();
                    (!commands.is_empty()
                        || redraw.is_some()
                        || !images.is_empty()
                        || !answers.is_empty())
                    .then(|| {
                        let mut staged = Staged::new(owner, commands, redraw, images);
                        staged.screen.answers = answers;
                        staged
                    })
                }
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

    /// Delivers the actions native plugins ran since the last call
    /// (`event.action_run`), each as a staged callback whose batch is run
    /// before the next: the plugin's `open` reaches the kernel in the call
    /// that ran the action. `true` when one was delivered (its commands wait
    /// for the next round of `run_commands`, the caller's, which bounds a
    /// chain of actions that run one another). Never inside a callback.
    // Its own frame (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_actions_run(&mut self) -> bool {
        if self.shared.native_instances.depth() != 0 {
            return false;
        }
        let Some(queue) = self.shared.native_instances.action_queue_made() else {
            return false;
        };
        for _ in 0..queue.refused() {
            self.warn_runtime("a native plugin's action was not run: too many wait");
        }
        let mut delivered = false;
        // Only what waits now: what a delivered action's commands run in turn
        // (an action that runs itself, or two that run each other) is queued
        // behind, for the next round of `run_commands`, which bounds them.
        for _ in 0..queue.waiting() {
            let Some(pending) = queue.take() else {
                break;
            };
            if self.shared.native_instances.outbox.full() {
                self.native_outbox_run();
            }
            let status = self.native_staged_call(pending.owner, |env, stage| {
                pending
                    .lease
                    .event_staged(pending.token, &pending.bytes, env, stage)
            });
            if let Err(status) | Ok(status) = status
                && !matches!(status, 0 | 1)
            {
                // A stopped or failing plugin leaves the action without
                // effect; the report says why.
                use super::native_screen::Dispatch as _;
                pending.lease.dropped(pending.token, status);
            }
            self.native_outbox_run();
            delivered = true;
        }
        delivered
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
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_outbox_run(&mut self) {
        // The service callbacks' batch first, then the screens' (in the order
        // their callbacks returned).
        while let Some(staged) = self.shared.native_instances.outbox.take().or_else(|| {
            self.shared
                .native_instances
                .screen_stage()
                .and_then(ScreenStage::take)
        }) {
            let Some(owner) = self
                .shared
                .native_instances
                .id(staged.owner)
                .and_then(|id| paperpaw_sdk::Id::try_parse(id.as_str()).ok())
            else {
                continue;
            };
            // Its warnings say what the callback's own status could not.
            if self.apply_staged(&owner, staged).is_err() {
                self.warn_runtime("a native screen's labels or widgets could not be kept");
            }
        }
    }
}

#[cfg(test)]
#[path = "event_call_tests.rs"]
mod tests;
