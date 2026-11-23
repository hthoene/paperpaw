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
use crate::Ask;
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_STATE};

/// The commands one callback staged, for its plugin.
pub(crate) struct Staged {
    pub(crate) owner: u64,
    pub(crate) commands: Vec<Ask>,
    pub(crate) redraw: Option<Redraw>,
    pub(crate) images: Vec<ImageCommand>,
    /// The hand-overs its opens name, and the screens they replace: for the
    /// drain (`Kernel::drain_native_opens`).
    pub(crate) opens: super::commands::Opens,
    /// Its power and Wi-Fi commands, applied in their place among the
    /// commands (`commands_dev.rs`).
    pub(crate) dev: Option<alloc::boxed::Box<super::commands::dev::DevBatch>>,
    /// What a callback stages besides: a screen's, an event's and an action's
    /// alike (`screen_stage::staged_of`); what a stage does not admit (a
    /// screen policy outside an action or an answer) stays empty.
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
    /// The ready lookups, as their asks' hashes (`commands_lookup.rs`).
    pub(crate) lookups: Vec<u64>,
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
            && self.lookups.is_empty()
            && self.hints.is_empty()
            && self.widgets.is_empty()
    }
}

impl Staged {
    /// Whether it holds nothing to apply.
    pub(crate) fn is_empty(&self) -> bool {
        self.commands.is_empty()
            && self.dev.is_none()
            && self.redraw.is_none()
            && self.images.is_empty()
            && self.screen.is_empty()
    }

    /// A batch of commands and pictures, with no screen effects.
    pub(crate) fn new(
        owner: u64,
        commands: Vec<Ask>,
        redraw: Option<Redraw>,
        images: Vec<ImageCommand>,
    ) -> Self {
        Self {
            owner,
            commands,
            redraw,
            images,
            opens: super::commands::Opens::default(),
            dev: None,
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
        self.native_event_call_nav(owner, lease, bytes, false)
    }

    /// As [`Self::native_event_call`] for `event.answered`: the answer of a
    /// question may close the asker's own screen (the dialog closed with the
    /// answer, so the asker's screen is on top again), as an action may: the
    /// guide, the lock and the settings leave their screen with it (#433). No
    /// other event stages navigation commands.
    ///
    /// # Errors
    /// As [`Self::native_event_call`].
    pub(crate) fn native_answer_call(
        &self,
        owner: u64,
        lease: &NativeLease,
        bytes: &[u8],
    ) -> Result<i32, i32> {
        self.native_event_call_nav(owner, lease, bytes, true)
    }

    fn native_event_call_nav(
        &self,
        owner: u64,
        lease: &NativeLease,
        bytes: &[u8],
        navigates: bool,
    ) -> Result<i32, i32> {
        let status = self.native_staged_call(owner, navigates, |env, stage| {
            lease.service_event_staged(bytes, env, stage)
        });
        // A failure of the plugin's callback is noted, for a warning
        // (`event_failures.rs`): the callers discard the status.
        if let Err(code) = status {
            let tag =
                super::registrations::RawRecord::single(bytes).map_or(0, |record| record.tag());
            self.shared
                .native_instances
                .event_failures
                .note(owner, tag, code);
        }
        status
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
        self.native_staged_call(owner, false, |env, stage| {
            lease.input_staged(token, bytes, env, stage)
        })
    }

    /// The staged call of `owner`'s callback that `call` makes. Every one may
    /// set its screens' policies; an action's and an answer's (`navigates`)
    /// may also stage the navigation commands (`close` ..., `commands_nav.rs`)
    /// as a screen's callback does.
    fn native_staged_call(
        &self,
        owner: u64,
        navigates: bool,
        call: impl FnOnce(&crate::Place<'_>, &mut CommandStage<'_>) -> Result<i32, i32>,
    ) -> Result<i32, i32> {
        let instances = &self.shared.native_instances;
        if instances.outbox.full() {
            return Err(ERR_BUSY);
        }
        let id = instances.id(owner).ok_or(ERR_STATE)?;
        let env = self.shared.place(self.shared.now);
        let mut stage = self.callback_stage(owner, &id, &env);
        stage = if navigates {
            stage.with_stack(&self.shared.stack_rows, owner)
        } else {
            stage.with_policy_stack(&self.shared.stack_rows, owner)
        };
        let status = call(&env, &mut stage);
        self.callback_staged(owner, status, &mut stage)
    }

    /// The command stage a callback of `owner` (`id`) stages into. Its own
    /// frame, gone before the callback runs: making the stage copies it, and
    /// the callback's frame stays under the firmware's limit for host code.
    #[inline(never)]
    fn callback_stage<'a>(
        &'a self,
        owner: u64,
        id: &'a paperpaw_sdk::Id,
        env: &'a crate::Place<'a>,
    ) -> CommandStage<'a> {
        let instances = &self.shared.native_instances;
        let mut stage = CommandStage::new(
            id,
            env.settings(),
            Some(self.command_settings_authority(id)),
            CALLBACK_COMMANDS,
            CALLBACK_COMMAND_BYTES,
        )
        .with_ui(env, self.event_screen(id), 0, self.screen_keys_remaining())
        // Its own top screen's button hints, as a screen's callback sets them
        // (`ui.md` section 3): the words of a guide follow a language chosen.
        .with_hint_labels(
            self.shared.stack_rows.labels(),
            !self.hint.buttons.is_empty(),
        )
        .with_widgets(instances.ui_services(), owner)
        .with_handover_store(instances.ui_services().map(|ui| &ui.store));
        // An event's or an action's callback sets its items' states too, as
        // a screen's does (`update` marks its menu item).
        if let Some(directory) = self.shared.registry.native_declarations.as_ref() {
            stage = stage.with_item_states(super::commands::ItemAdmit { directory });
        }
        stage.set_images_held(self.image_held(owner));
        stage.set_speech_held(self.io.speech.owned_texts(id.as_str()));
        stage.set_requests_open(
            self.native_requests_open(),
            self.native_requests_open_by(id.as_str()),
        );
        stage.set_answer_view(self.native_answer_view(owner));
        stage.set_preview_held(self.preview_held(id.as_str()));
        stage.set_go_online_granted(self.go_online_granted());
        // A run's callback is quiet (`work.md` section 2).
        stage.set_quiet(
            instances
                .work_services()
                .is_some_and(|work| work.sched.quiet(owner)),
        );
        stage
    }

    /// What the callback of `owner` staged, into the outbox when it returned
    /// `0` or `1` (an error drops the whole batch with the stage); the
    /// callback's status, or why its batch could not be kept. Its own frame,
    /// after the callback returned (see [`Self::callback_stage`]).
    #[inline(never)]
    fn callback_staged(
        &self,
        owner: u64,
        status: Result<i32, i32>,
        stage: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        if !matches!(status, Ok(0 | 1)) {
            return status;
        }
        let mut staged = super::screen_stage::staged_of(stage, owner);
        // The widgets it said changed are marked now; their makers hear of it
        // from the pump. The table is not held across a callback, so a failed
        // borrow is a host fault: the callback fails (`ERR_BUSY`, as in the
        // shell path) and its batch is dropped.
        let widgets = core::mem::take(&mut staged.screen.widgets);
        if let Some(ui) = self.shared.native_instances.ui_services()
            && !widgets.is_empty()
        {
            ui.widgets_changed(owner, &widgets)?;
        }
        if !staged.is_empty() {
            // Empty, checked before the callback: this cannot refuse.
            self.shared.native_instances.outbox.put(staged)?;
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
            // A parked plugin is made first; one that cannot be made now
            // keeps its action, behind those waiting, until it gave up after
            // its longest wait without memory (#248).
            if pending.lease.is_parked() && pending.lease.live().err() == Some(ERR_BUSY) {
                if pending.lease.slot().give_up() {
                    // This one and every other of its plugin, each said once.
                    for _ in 0..=queue.drop_owner(pending.owner) {
                        self.warn_runtime("a native plugin's action was not run: no memory");
                    }
                } else if queue.put_back(pending).is_err() {
                    self.warn_runtime("a native plugin's action was not run: too many wait");
                }
                continue;
            }
            if self.shared.native_instances.outbox.full() {
                self.native_outbox_run();
            }
            self.deliver_action(pending);
            self.native_outbox_run();
            delivered = true;
        }
        delivered
    }

    /// One waiting action to its plugin. Its own frame, gone before the
    /// batch it staged runs (`native_outbox_run`): the commands applied
    /// there go deep, the X4's stack is small (#250).
    #[inline(never)]
    fn deliver_action(&mut self, pending: super::native_action::Pending) {
        let status = self.native_staged_call(pending.owner, true, |env, stage| {
            pending
                .lease
                .event_staged(pending.token, &pending.bytes, env, stage)
        });
        let (Ok(status) | Err(status)) = status;
        if !matches!(status, 0 | 1) {
            // A stopped or failing plugin leaves the action without
            // effect; the report says why.
            use super::native_screen::Dispatch as _;
            pending.lease.dropped(pending.token, status);
        }
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
