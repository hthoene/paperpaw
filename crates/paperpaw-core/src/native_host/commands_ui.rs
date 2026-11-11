//! Host-only UI authority and deferred redraw effects for one callback.
use super::*;
use crate::native_host::stack_rows::StackRows;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_platform::RefreshMode;
use paperpaw_sdk::{Env, IdError, Params, ScreenKey};

pub(super) struct UiContext<'a> {
    env: &'a Env<'a>,
    screen: Option<ScreenKey>,
    hop: u8,
    open_budget: u32,
    /// The labels of `command.set_hint` and whether the device has a row of
    /// hints (without one the command is checked and does nothing).
    labels: Option<(
        &'a core::cell::RefCell<crate::native_host::hint_labels::HintLabels>,
        bool,
    )>,
    /// The widget table and the caller's owner key, to check
    /// `command.widget_changed` (without it the command is `ERR_UNSUPPORTED`).
    widgets: Option<(&'a crate::native_host::ui_service::UiServices, u64)>,
    /// The kernel's stack and the submitter's owner id, for the navigation
    /// commands (`commands_nav.rs`); without it they are `ERR_UNSUPPORTED`.
    stack: Option<super::nav::NavStack<'a>>,
}

impl<'a> UiContext<'a> {
    pub(super) fn env(&self) -> &'a Env<'a> {
        self.env
    }

    pub(super) fn hop(&self) -> u8 {
        self.hop
    }

    pub(super) fn stack(&self) -> Option<super::nav::NavStack<'a>> {
        self.stack
    }
}

/// Deferred redraw retains the original screen instance, never a later top one.
pub(crate) struct Redraw {
    pub(crate) screen: ScreenKey,
    pub(crate) mode: Option<RefreshMode>,
}

impl<'a> CommandStage<'a> {
    pub(crate) fn with_ui(
        mut self,
        env: &'a Env<'a>,
        screen: Option<ScreenKey>,
        hop: u8,
        open_budget: u32,
    ) -> Self {
        self.ui = Some(UiContext {
            env,
            screen,
            hop,
            open_budget,
            labels: None,
            widgets: None,
            stack: None,
        });
        self
    }

    /// Admits `command.widget_changed` for owner `owner_key`: the instance
    /// must be one of its own widgets in `ui`'s table.
    #[must_use]
    pub(crate) fn with_widgets(
        mut self,
        ui: Option<&'a crate::native_host::ui_service::UiServices>,
        owner_key: u64,
    ) -> Self {
        if let (Some(context), Some(ui)) = (&mut self.ui, ui) {
            context.widgets = Some((ui, owner_key));
        }
        self
    }

    /// The checked widget changes, in order, for the host to apply after the
    /// callback ([`UiServices::widgets_changed`]); call it before
    /// [`Self::into_commands`].
    #[must_use]
    pub(crate) fn take_widget_changes(&mut self) -> Vec<u32> {
        core::mem::take(&mut self.staged_widgets)
    }

    /// Admits `command.set_hint`: `labels` keeps what instances relabel,
    /// `row` says the device has a row of button hints. Without it the
    /// command is `ERR_UNSUPPORTED`.
    #[must_use]
    pub(crate) fn with_hint_labels(
        mut self,
        labels: &'a core::cell::RefCell<crate::native_host::hint_labels::HintLabels>,
        row: bool,
    ) -> Self {
        if let Some(ui) = &mut self.ui {
            ui.labels = Some((labels, row));
        }
        self
    }

    /// Admits the navigation commands (`close`, `back_to`, `set_key_layer`,
    /// `show_hints`, `raise_input`): `rows` is the kernel's stack, `owner` the
    /// submitter's host owner id.
    #[must_use]
    pub(crate) fn with_stack(mut self, rows: &'a StackRows, owner: u64) -> Self {
        if let Some(ui) = &mut self.ui {
            ui.stack = Some(super::nav::NavStack { rows, owner });
        }
        self
    }

    /// The checked `command.set_hint` copies, in order, for the host to apply
    /// after the callback ([`HintLabels::apply`](crate::native_host::hint_labels::HintLabels::apply));
    /// call it before [`Self::into_commands`].
    #[must_use]
    pub(crate) fn take_hint_sets(&mut self) -> Vec<crate::native_host::hint_labels::HintSet> {
        core::mem::take(&mut self.staged_hints)
    }

    /// Copies the `set_hint` commands of a validated batch, checks that they
    /// fit (at most 8 actions per instance, counting what is staged) and takes
    /// the room for them ahead; nothing is listed on a refusal. On a device
    /// without a row of hints they do nothing.
    #[inline(never)]
    pub(super) fn reserve_hints(&mut self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let Some((labels, true)) = self.ui.as_ref().and_then(|ui| ui.labels) else {
            return Ok(());
        };
        let count = batch
            .iter()
            .filter(|command| matches!(command, WireCommand::SetHint { .. }))
            .count();
        if count == 0 {
            return Ok(());
        }
        let instance = self.own_top()?.serial();
        let owner = self.owner.as_str();
        let small = self
            .ui
            .as_ref()
            .is_some_and(|ui| crate::native_host::ui_store::is_x4(ui.env.device().memory));
        let budget = crate::native_host::hint_labels::label_budget(small);
        let start = self.staged_hints.len();
        self.staged_hints
            .try_reserve(count)
            .map_err(|_| ERR_NO_MEMORY)?;
        for command in batch.iter() {
            let WireCommand::SetHint {
                action,
                kind,
                label,
            } = command
            else {
                continue;
            };
            let checked = (|| {
                let set =
                    crate::native_host::hint_labels::make(owner, instance, action, kind, label)?;
                labels.try_borrow().map_err(|_| ERR_BUSY)?.admits(
                    &set,
                    &self.staged_hints,
                    budget,
                )?;
                Ok::<_, i32>(set)
            })();
            match checked {
                // Capacity was reserved above.
                Ok(set) => self.staged_hints.push(set),
                Err(error) => {
                    self.staged_hints.truncate(start);
                    return Err(error);
                }
            }
        }
        let made = &self.staged_hints[start..];
        let reserved = labels
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)
            .and_then(|mut labels| labels.reserve(made));
        if let Err(error) = reserved {
            self.staged_hints.truncate(start);
            return Err(error);
        }
        Ok(())
    }

    #[inline(never)]
    pub(super) fn validate_open_budget(&self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let Some(ui) = &self.ui else {
            return Ok(());
        };
        let queued = self
            .staged
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    Command::OpenScreen(..)
                        | Command::OpenSlot(..)
                        | Command::OpenInSlot(..)
                        | Command::Replace(..)
                )
            })
            .count();
        let incoming = batch
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    WireCommand::OpenScreen { .. } | WireCommand::Open { .. }
                )
            })
            .count();
        if queued.saturating_add(incoming) > ui.open_budget as usize {
            return Err(ERR_NO_SPACE);
        }
        Ok(())
    }

    /// UI callers drain this form so redraw-only never becomes a refresh.
    pub(crate) fn into_ui_output(mut self) -> (Vec<Command>, Option<Redraw>, u8) {
        let screen = self.ui.as_ref().and_then(|ui| ui.screen);
        let redraw = self.redraw.zip(screen).map(|(mode, screen)| Redraw {
            screen,
            mode: match mode {
                1 => Some(RefreshMode::Fast),
                2 => Some(RefreshMode::Partial),
                3 => Some(RefreshMode::Full),
                _ => None,
            },
        });
        (
            core::mem::take(&mut self.staged),
            redraw,
            self.ui.as_ref().map_or(0, |ui| ui.hop),
        )
    }

    pub(super) fn own_top(&self) -> Result<ScreenKey, i32> {
        let ui = self.ui.as_ref().ok_or(ERR_UNSUPPORTED)?;
        let top = ui.env.stack().last().ok_or(ERR_STATE)?;
        if Some(top.key) != ui.screen || top.screen.namespace() != self.owner.as_str() {
            return Err(ERR_STATE);
        }
        Ok(top.key)
    }

    pub(super) fn validate_ui(&self, command: WireCommand<'_>) -> Result<usize, i32> {
        let ui = self.ui.as_ref().ok_or(ERR_UNSUPPORTED)?;
        match command {
            WireCommand::OpenScreen { screen } => {
                super::super::declarations::full_id(screen)?;
                if ui.hop >= 8 {
                    return Err(ERR_BUSY);
                }
                if !ui.env.screens().iter().any(|(id, _)| id.as_str() == screen) {
                    return Err(ERR_NOT_FOUND);
                }
                Ok(screen.len())
            }
            WireCommand::Open {
                target, mode, slot, ..
            } => self.validate_open(target, mode, slot),
            WireCommand::RunAction { action } => {
                super::super::declarations::full_id(action)?;
                if ui.hop >= 8 {
                    return Err(ERR_BUSY);
                }
                if !ui.env.actions().contains(action) {
                    return Err(ERR_NOT_FOUND);
                }
                Ok(action.len())
            }
            WireCommand::Redraw { .. } | WireCommand::CloseScreen => self.own_top().map(|_| 0),
            command if super::nav::is_nav(&command) => self.validate_nav(command),
            WireCommand::SetHint {
                action,
                kind,
                label,
            } => {
                if ui.labels.is_none() {
                    return Err(ERR_UNSUPPORTED);
                }
                crate::native_host::hint_labels::check(action, kind, label)?;
                self.own_top()?;
                Ok(action.len() + label.len())
            }
            WireCommand::WidgetChanged { instance } => {
                let (ui, owner) = ui.widgets.ok_or(ERR_UNSUPPORTED)?;
                ui.widget_check_changed(owner, instance)?;
                Ok(0)
            }
            // Bit 0 `announce`; the other bits are `ERR_INVALID` (ui.md, messages).
            WireCommand::Notify { text, flags } => {
                if flags & !NOTIFY_ANNOUNCE != 0 {
                    return Err(ERR_INVALID);
                }
                Ok(text.len())
            }
            // Speech (`speech.rs`): a device without it says status 3 at once.
            WireCommand::Speak {
                text,
                language,
                rate,
                flags,
                ..
            } => {
                crate::native_host::speech::check(rate, flags)?;
                Ok(text.len() + language.len())
            }
            WireCommand::StopSpeaking {} => Ok(0),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    /// At most `speech::MAX_TEXTS` texts per plugin: a queued `command.speak`
    /// beyond is `ERR_FULL`, the batch refused whole (what is staged already
    /// and what the batch holds count with the kernel's own).
    pub(super) fn validate_speech_room(&self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        use crate::native_host::speech::{QUEUE, count, is_reply, is_stop};
        let mut texts = self.speech_held;
        for command in &self.staged {
            if let Command::Speak(speech) = command {
                if is_reply(speech.reply()) {
                    let _ = count(&mut texts, Some(speech.is_queued()));
                } else if is_stop(speech.reply()) {
                    let _ = count(&mut texts, None);
                }
            }
        }
        for command in batch.iter() {
            match command {
                WireCommand::Speak { flags, .. } => count(&mut texts, Some(flags & QUEUE != 0))?,
                WireCommand::StopSpeaking {} => count(&mut texts, None)?,
                _ => {}
            }
        }
        Ok(())
    }

    #[inline(never)]
    pub(super) fn owned_ui(&self, command: WireCommand<'_>) -> Result<Command, i32> {
        match command {
            WireCommand::OpenScreen { screen } => {
                Ok(Command::OpenScreen(copy_id(screen)?, Params::new()))
            }
            WireCommand::Open {
                target, mode, slot, ..
            } => super::open::command(target, mode, slot),
            WireCommand::RunAction { action } => {
                Ok(Command::RunAction(copy_id(action)?, Params::new()))
            }
            WireCommand::CloseScreen => Ok(Command::Close(self.own_top()?)),
            command if super::nav::is_nav(&command) => self.owned_nav(command),
            WireCommand::Notify { text, flags } => {
                let mut owned = alloc::string::String::new();
                owned
                    .try_reserve_exact(text.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                owned.push_str(text);
                Ok(if flags & NOTIFY_ANNOUNCE != 0 {
                    Command::Announce(owned)
                } else {
                    Command::Notify(owned)
                })
            }
            WireCommand::Speak {
                cookie,
                text,
                language,
                rate,
                flags,
            } => crate::native_host::speech::speak(
                self.owner,
                (cookie, text, language),
                (rate, flags),
            ),
            WireCommand::StopSpeaking {} => crate::native_host::speech::stop(self.owner),
            _ => Err(ERR_UNSUPPORTED),
        }
    }
}

/// `command.notify` flags bit 0: also from background work and when no screen
/// of the plugin shows.
pub(super) const NOTIFY_ANNOUNCE: u32 = 1;

pub(super) fn copy_id(text: &str) -> Result<Id, i32> {
    Id::try_parse(text).map_err(|error| match error {
        IdError::NoMemory => ERR_NO_MEMORY,
        _ => ERR_INVALID,
    })
}

impl crate::Kernel {
    /// Drain after foreign code and all borrowed callback scopes have returned.
    pub(crate) fn drain_native_ui(
        &mut self,
        owner: &Id,
        commands: Vec<Command>,
        redraw: Option<Redraw>,
    ) {
        self.drain_native_opens(owner, commands, redraw, super::Opens::default());
    }

    /// As [`Self::drain_native_ui`], for a stage whose opens name hand-overs:
    /// each goes to the instance its open shows (docs/abi/ui.md 1.1 step 5),
    /// and one whose open did not apply (a guard refused, a stale target) is
    /// freed with the old one staying (step 4).
    pub(crate) fn drain_native_opens(
        &mut self,
        owner: &Id,
        commands: Vec<Command>,
        redraw: Option<Redraw>,
        opens: super::Opens,
    ) {
        let super::Opens { at, claims } = opens;
        for (index, command) in commands.into_iter().enumerate() {
            let staged = at.iter().find(|open| open.index == index);
            // A replace is the submitter's own top screen's, nobody else's: if
            // another is on top by now (an earlier open of the same batch), it
            // does not apply and its hand-over is freed.
            let displaced = staged.and_then(|open| open.replaces).is_some_and(|key| {
                self.stack
                    .last()
                    .is_none_or(|top| ScreenKey::new(top.serial) != key)
            });
            if displaced {
                self.warn_runtime("native replace dropped: its screen is no longer on top");
            } else {
                self.shared
                    .native_instances
                    .set_pending_handover(staged.map(|open| open.handover));
                self.drain_native_command(owner, command);
            }
            // The open that did not apply (a guard refused, a stale target):
            // its claim is freed and the instance's old hand-over stays.
            self.release_pending_handover();
        }
        // Nothing stays claimed once the batch is applied.
        if let Some(store) = self.shared.native_instances.ui_store() {
            for claims in claims {
                let _ = store.rollback(claims);
            }
        }
        self.finish_native_redraw(owner, redraw);
    }

    fn release_pending_handover(&mut self) {
        if let Some(Some(id)) = self.shared.native_instances.take_pending_handover()
            && let Some(store) = self.shared.native_instances.ui_store()
        {
            let _ = store.release(id);
        }
    }

    /// Apply one staged command of `owner`; a close of a screen that is gone
    /// is dropped with a warning.
    pub(crate) fn drain_native_command(&mut self, owner: &Id, command: Command) {
        // The guard's own plugin's commands are the guard's own, as the
        // interface-6 guard's are: its screens open and close above and
        // including itself, and its raised input is not held (ui.md 4.3). Any
        // other plugin's are held below it. Judged for each command, as an
        // earlier one of the batch may have closed the guard. The marks the
        // caller had are put back.
        let own_guard = self
            .guard()
            .and_then(|guard| self.stack.get(guard))
            .is_some_and(|guard| guard.id.namespace() == owner.as_str());
        let before = (self.from_guard, self.from_guard_own);
        (self.from_guard, self.from_guard_own) = (own_guard, own_guard);
        self.drain_native_command_as(owner, command);
        (self.from_guard, self.from_guard_own) = before;
    }

    fn drain_native_command_as(&mut self, owner: &Id, command: Command) {
        // A close names an instance still open that is the owner's own or an
        // overlay (`command.close`, ui.md 1.3).
        if let Command::Close(key) = &command
            && !self.shared.stack.iter().any(|entry| {
                entry.key == *key && (entry.screen.namespace() == owner.as_str() || entry.overlay)
            })
        {
            self.warn_runtime("native close dropped: its screen no longer exists");
            return;
        }
        // A notify without the announce bit is for a plugin that shows a
        // screen: otherwise it is dropped (`OK` at submit, logged here).
        if matches!(command, Command::Notify(_))
            && !self
                .shared
                .stack
                .iter()
                .any(|entry| entry.screen.namespace() == owner.as_str())
        {
            self.warn_runtime("native notify dropped: the plugin shows no screen");
            return;
        }
        // A key map layer is laid for the owner's own screen on top, the one
        // that asked, if it still is.
        if let Command::SetKeyLayer(key, layer) = command {
            if self.native_top(owner, key) {
                self.set_key_layer(key, layer);
            } else {
                self.warn_runtime("native key layer dropped: its screen is no longer on top");
            }
            return;
        }
        self.apply(command);
    }

    /// Applies the policy changes a callback of `owner` (host owner id
    /// `owner_key`) asked for (`command.screen_policy`, ui.md 1.2), before its
    /// other commands. A change for a screen that is gone, or that turns
    /// `guards` on where the screen is no longer the top one, is dropped with
    /// a warning; the stage judged it at submit.
    #[allow(
        dead_code,
        reason = "the command runner (shell_commands) calls it, wired with the shell"
    )]
    pub(crate) fn native_screen_policies(
        &mut self,
        owner: &Id,
        owner_key: u64,
        changes: Vec<super::nav::PolicyChange>,
    ) {
        use crate::screens::POLICY_GUARDS;
        let mut changed = false;
        for change in changes {
            let Some(index) = self.stack.iter().position(|active| {
                active.serial == change.instance && active.id.namespace() == owner.as_str()
            }) else {
                self.warn_runtime(if change.ready {
                    "native screen_ready dropped: its screen is gone"
                } else {
                    "native screen policy dropped: its screen is gone"
                });
                continue;
            };
            if change.ready {
                // Not a policy: the screen says it is ready (ui.md 7).
                self.native_command_ready(index);
                continue;
            }
            let turns_on = change.flags & POLICY_GUARDS != 0 && !self.stack[index].guards();
            if turns_on
                && crate::native_host::ui_guard::guard_on_allowed(
                    &*self.shared.stack_rows,
                    owner_key,
                    change.instance,
                )
                .is_err()
            {
                self.warn_runtime("native screen policy dropped: its screen is no longer on top");
                continue;
            }
            self.stack[index].policy = Some(change.flags);
            changed = true;
        }
        if changed {
            // The stack rows and the guard follow; what the guard held waits
            // no longer once none guards.
            self.sync_stack();
            self.release_held_notice();
            self.output.redraw = true;
        }
    }

    /// The deferred redraw, if its screen is still on top.
    pub(crate) fn finish_native_redraw(&mut self, owner: &Id, redraw: Option<Redraw>) {
        if let Some(redraw) = redraw {
            if !self.native_top(owner, redraw.screen) {
                self.warn_runtime("native redraw dropped: its screen is no longer on top");
                return;
            }
            match redraw.mode {
                Some(mode) => self.apply(Command::Redraw(mode)),
                None => self.output.redraw = true,
            }
        }
    }

    fn native_top(&self, owner: &Id, key: ScreenKey) -> bool {
        self.shared
            .stack
            .last()
            .is_some_and(|top| top.key == key && top.screen.namespace() == owner.as_str())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
    use super::*;
    use paperpaw_platform::Instant;

    fn screens_kernel() -> crate::Kernel {
        use paperpaw_sdk::{Manifest, Plugin, Registrar, Screen, ScreenSpec, Version};
        struct Empty;
        impl Screen for Empty {
            fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &Env<'_>) {}
            fn on_action(
                &mut self,
                _: &Id,
                _: &mut paperpaw_sdk::Cx<'_, '_>,
            ) -> paperpaw_sdk::Handled {
                paperpaw_sdk::Handled::No
            }
        }
        struct Screens;
        impl Plugin for Screens {
            fn manifest(&self) -> Manifest {
                Manifest::new(Id::new_static("probe"), "Probe", Version::new(1, 0, 0))
                    .uses(paperpaw_sdk::Uses::NOTHING)
            }
            fn register(&self, r: &mut Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
                for id in ["probe.base", "probe.old", "probe.new"] {
                    r.add_screen(ScreenSpec::new(
                        Id::new_static(id),
                        "probe.title",
                        alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Empty)),
                    ))?;
                }
                Ok(())
            }
        }
        crate::Kernel::builder(crate::tests::DEVICE)
            .plugin(Screens)
            .build()
            .unwrap()
    }

    #[test]
    fn screen_keys_exhaust_without_wrap_or_reuse() {
        let mut kernel = screens_kernel();
        kernel.next_serial = u32::MAX - 1;
        kernel.apply(Command::OpenScreen(
            Id::new_static("probe.old"),
            Params::new(),
        ));
        assert_eq!(
            kernel.shared.stack.last().unwrap().key,
            ScreenKey::new(u32::MAX - 1)
        );
        assert_eq!(kernel.next_serial, u32::MAX);
        kernel.take_warnings();
        for _ in 0..2 {
            kernel.apply(Command::OpenScreen(
                Id::new_static("probe.new"),
                Params::new(),
            ));
            assert_eq!(kernel.next_serial, u32::MAX);
            assert_eq!(kernel.shared.stack.len(), 1);
            assert_eq!(kernel.shared.stack[0].key, ScreenKey::new(u32::MAX - 1));
        }
        let warnings = kernel.take_warnings();
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .all(|warning| warning.message.contains("screen key space exhausted"))
        );
    }

    #[test]
    fn exhausted_screen_key_budget_returns_slot_status_without_mutating_stage() {
        let kernel = screens_kernel();
        let env = kernel.shared.env(Instant(0));
        let owner = Id::new_static("probe");
        for remaining in [0, 1] {
            let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
                .with_ui(&env, None, 0, remaining);
            let scope = core::pin::pin!(CommandScope::new());
            let submit = |stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]| {
                let mut bytes = [0; 192];
                let len = commands_encode(commands, &mut bytes).unwrap();
                scope
                    .as_ref()
                    .scope(stage, || {
                        // SAFETY: this initialized encoded input remains readable
                        // and disjoint from stage until the synchronous call ends.
                        unsafe { scope.submit(bytes.as_ptr(), len) }
                    })
                    .unwrap()
            };
            let open = WireCommand::OpenScreen {
                screen: "probe.new",
            };
            assert_eq!(submit(&mut stage, &[open, open]), ERR_NO_SPACE);
            assert!(stage.staged.is_empty());
            if remaining == 1 {
                assert_eq!(submit(&mut stage, &[open]), OK);
            }
            for _ in 0..2 {
                assert_eq!(submit(&mut stage, &[open]), ERR_NO_SPACE);
                assert_eq!(stage.staged.len(), remaining as usize);
                assert_eq!(stage.accepted, remaining as usize);
            }
            assert!(kernel.shared.stack.is_empty());
            assert!(kernel.output.commands.is_empty());
        }
    }

    #[test]
    fn drain_exhaustion_keeps_key_and_stack_and_warns_for_missing_close() {
        let mut kernel = screens_kernel();
        kernel.apply(Command::OpenScreen(
            Id::new_static("probe.base"),
            Params::new(),
        ));
        kernel.next_serial = u32::MAX - 1;
        let owner = Id::new_static("probe");
        let (mut commands, redraw, _) = {
            let env = kernel.shared.env(Instant(0));
            let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(
                &env,
                None,
                0,
                u32::MAX - kernel.next_serial,
            );
            let mut bytes = [0; 192];
            let len = commands_encode(
                &[WireCommand::OpenScreen {
                    screen: "probe.new",
                }],
                &mut bytes,
            )
            .unwrap();
            assert_eq!(stage.stage(&commands_decode(&bytes[..len]).unwrap()), OK);
            stage.into_ui_output()
        };
        // Another callback consumes the last available key before this drain.
        kernel.apply(Command::OpenScreen(
            Id::new_static("probe.old"),
            Params::new(),
        ));
        kernel.take_warnings();
        // A native close normally stamps an existing screen at submission.
        // Exercise the drain's defensive refusal for a target that never opened.
        commands.push(Command::Close(ScreenKey::new(u32::MAX)));
        kernel.drain_native_ui(&owner, commands, redraw);
        assert_eq!(kernel.next_serial, u32::MAX);
        assert_eq!(kernel.shared.stack.len(), 2);
        assert_eq!(
            kernel.shared.stack.last().unwrap().screen.as_str(),
            "probe.old"
        );
        assert_eq!(
            kernel.shared.stack.last().unwrap().key,
            ScreenKey::new(u32::MAX - 1)
        );
        let warnings = kernel.take_warnings();
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].message.contains("screen key space exhausted"));
        assert!(warnings[1].message.contains("native close dropped"));
    }

    #[test]
    fn open_then_close_uses_original_target_or_warns_when_already_closed() {
        for already_closed in [false, true] {
            let mut kernel = screens_kernel();
            for id in ["probe.base", "probe.old"] {
                kernel.apply(Command::OpenScreen(Id::new_static(id), Params::new()));
            }
            let original = kernel.shared.stack.last().unwrap().key;
            let owner = Id::new_static("probe");
            let (commands, redraw, _) =
                {
                    let env = kernel.shared.env(Instant(0));
                    let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
                        .with_ui(&env, Some(original), 0, u32::MAX - kernel.next_serial);
                    let mut bytes = [0; 192];
                    let len = commands_encode(
                        &[
                            WireCommand::OpenScreen {
                                screen: "probe.new",
                            },
                            WireCommand::CloseScreen,
                        ],
                        &mut bytes,
                    )
                    .unwrap();
                    assert_eq!(stage.stage(&commands_decode(&bytes[..len]).unwrap()), OK);
                    stage.into_ui_output()
                };
            if already_closed {
                kernel.apply(Command::Close(original));
            }
            kernel.take_warnings();
            kernel.drain_native_ui(&owner, commands, redraw);
            let warnings = kernel.take_warnings();
            assert_eq!(warnings.len(), usize::from(already_closed));
            if already_closed {
                assert!(warnings[0].message.contains("native close dropped"));
            }
            assert_eq!(kernel.shared.stack.len(), 2);
            assert_eq!(
                kernel.shared.stack.last().unwrap().screen.as_str(),
                "probe.new"
            );
            assert!(
                !kernel
                    .shared
                    .stack
                    .iter()
                    .any(|entry| entry.key == original)
            );
        }
    }

    #[test]
    fn ui_batches_preserve_redraw_only_and_refuse_foreign_screens_and_hop_eight() {
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let key = ScreenKey::new(7);
        kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
            Id::new_static("probe.screen"),
            None,
            false,
            key,
        ));
        let env = kernel.shared.env(Instant(0));
        let owner = Id::new_static("probe");
        let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(
            &env,
            Some(key),
            0,
            u32::MAX - kernel.next_serial,
        );
        let submit = |stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]| {
            let mut bytes = [0; 512];
            let len = commands_encode(commands, &mut bytes).unwrap();
            stage.stage(&commands_decode(&bytes[..len]).unwrap())
        };
        assert_eq!(submit(&mut stage, &[WireCommand::Redraw { mode: 0 }]), OK);
        assert_eq!(
            stage.redraw,
            Some(0),
            "redraw only must not become a refresh"
        );
        assert_eq!(
            submit(
                &mut stage,
                &[
                    WireCommand::Redraw { mode: 3 },
                    WireCommand::OpenScreen {
                        screen: "missing.screen"
                    },
                ]
            ),
            ERR_NOT_FOUND
        );
        assert_eq!(
            stage.redraw,
            Some(0),
            "whole refused batch leaves the prior effect intact"
        );
        assert_eq!(
            submit(
                &mut stage,
                &[WireCommand::Redraw { mode: 2 }, WireCommand::CloseScreen]
            ),
            OK
        );
        let (commands, redraw, hop) = stage.into_ui_output();
        assert!(matches!(commands.as_slice(), [Command::Close(k)] if *k == key));
        let redraw = redraw.unwrap();
        assert_eq!(redraw.screen, key);
        assert_eq!(redraw.mode, Some(RefreshMode::Partial));
        assert_eq!(hop, 0);

        let foreign = Id::new_static("other");
        let mut stage = CommandStage::new(&foreign, env.settings(), None, 8, 4096).with_ui(
            &env,
            Some(key),
            0,
            u32::MAX - kernel.next_serial,
        );
        assert_eq!(
            submit(&mut stage, &[WireCommand::Redraw { mode: 2 }]),
            ERR_STATE
        );
        assert_eq!(submit(&mut stage, &[WireCommand::CloseScreen]), ERR_STATE);
        let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(
            &env,
            Some(key),
            8,
            u32::MAX - kernel.next_serial,
        );
        assert_eq!(
            submit(
                &mut stage,
                &[WireCommand::OpenScreen {
                    screen: "missing.screen"
                }]
            ),
            ERR_BUSY
        );
        assert_eq!(submit(&mut stage, &[WireCommand::Redraw { mode: 0 }]), OK);
    }
    #[test]
    fn redraw_only_preserves_refresh_and_stale_instance_effects_are_dropped() {
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let owner = Id::new_static("probe");
        let key = ScreenKey::new(7);
        kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
            Id::new_static("probe.screen"),
            None,
            false,
            key,
        ));
        kernel.output.redraw = false;
        kernel.output.refresh = None;
        kernel.drain_native_ui(
            &owner,
            Vec::new(),
            Some(Redraw {
                screen: key,
                mode: None,
            }),
        );
        assert!(kernel.output.redraw);
        assert_eq!(
            kernel.output.refresh, None,
            "mode zero never requests refresh"
        );
        kernel.output.redraw = false;
        kernel.shared.stack.clear();
        // Same id, different instance: the old stamp is still stale.
        kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
            Id::new_static("probe.screen"),
            None,
            false,
            ScreenKey::new(8),
        ));
        kernel.drain_native_ui(
            &owner,
            alloc::vec![Command::Close(key)],
            Some(Redraw {
                screen: key,
                mode: Some(RefreshMode::Full),
            }),
        );
        assert!(!kernel.output.redraw);
        assert_eq!(kernel.output.refresh, None);
        assert_eq!(kernel.shared.stack.last().unwrap().key, ScreenKey::new(8));
    }
}
