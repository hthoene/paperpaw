//! The navigation commands of slot 15 on the kernel's own screen stack
//! (`docs/abi/ui.md` 1.3): `close` (1548), `back_to` (1549), `set_key_layer`
//! (1550), `show_hints` (1551) and `raise_input` (1552). The guard rules are
//! [`ui_guard`](crate::native_host::ui_guard)'s, read from the stack the
//! kernel keeps ([`StackRows`]); the stage judges at submit and the drain
//! applies as the interface-6 commands (`Close`, `BackTo`, `SetKeyLayer`,
//! `ShowHints`, `Input`). A command whose effect is nothing (the root
//! screen, a `back_to` with nothing open above) is `OK` and stages nothing.
//!
//! Only a screen callback (`input`, an action, a gesture) has the stack: the
//! callback of an event or a service event stages without it, so these
//! commands are `ERR_UNSUPPORTED` there. The kernel's mirror of the stack
//! ([`StackRows`]) holds 16 rows; a stack deeper than that judges nothing
//! (the rows past it read as the host's own guard: `close` is `ERR_STATE`,
//! `raise_input` `ERR_DENIED`, nothing is applied).
//!
//! The drain applies the commands of the guard's own plugin as the guard's
//! own (the screen it closes or opens above itself, the input it raises, the
//! screens it closes with `back_to`); every other plugin's are held below
//! the guard as the stage judged.
//!
//! `screen_policy` (1563) is checked here and staged apart from the commands:
//! the host applies it first ([`Kernel::native_screen_policies`](crate::Kernel)).
use super::{CommandBatch, CommandStage, WireCommand, ui};
use crate::native_host::{
    stack_rows::StackRows,
    ui_guard::{self, Close},
};
use crate::screens::{POLICY_BITS, POLICY_GUARDS};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::{Command, ScreenKey};

/// One run-time change of a screen's policy bits (`command.screen_policy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PolicyChange {
    /// The screen instance (never 0: the own top one is named here).
    pub(crate) instance: u32,
    /// The bits it ends with (`guards`, `secret`, `holds_edges`).
    pub(crate) flags: u32,
    /// Not a policy: `command.screen_ready` (1557) for the instance, which
    /// `flags` then does not read.
    pub(crate) ready: bool,
}

/// The most bytes of an input name (`command.raise_input.input`).
const INPUT_MAX: usize = 64;

/// The stack the stage judges against and the submitter's host owner id.
#[derive(Clone, Copy)]
pub(super) struct NavStack<'a> {
    pub(super) rows: &'a StackRows,
    pub(super) owner: u64,
}

/// Whether `command` is one of the navigation commands of this module.
pub(super) fn is_nav(command: &WireCommand<'_>) -> bool {
    matches!(
        command,
        WireCommand::Close { .. }
            | WireCommand::BackTo { .. }
            | WireCommand::SetKeyLayer { .. }
            | WireCommand::ShowHints {}
            | WireCommand::RaiseInput { .. }
            | WireCommand::ScreenPolicy { .. }
            | WireCommand::ScreenReady {}
    )
}

/// Characters of an input name as key maps write it.
fn input_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
}

impl CommandStage<'_> {
    fn nav(&self) -> Result<NavStack<'_>, i32> {
        self.ui
            .as_ref()
            .and_then(|ui| ui.stack())
            .ok_or(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }

    /// The index on the stack of the topmost instance of `target` (a screen
    /// id or a slot id), if one is open.
    fn back_to_index(&self, target: &str) -> Option<usize> {
        let ui = self.ui.as_ref()?;
        ui.env().stack().iter().rposition(|entry| {
            entry.screen.as_str() == target
                || entry
                    .slot
                    .as_ref()
                    .is_some_and(|slot| slot.as_str() == target)
        })
    }

    /// Whether a checked command changes nothing (`OK`, nothing staged).
    pub(super) fn nav_no_effect(&self, command: &WireCommand<'_>) -> bool {
        match *command {
            WireCommand::Close { instance } => self.nav().is_ok_and(|nav| {
                matches!(
                    ui_guard::close_allowed(nav.rows, nav.owner, instance),
                    Ok(Close::NoEffect)
                )
            }),
            WireCommand::BackTo { target } => self.back_to_index(target).is_none_or(|index| {
                self.nav()
                    .is_ok_and(|nav| index + 1 >= ui_guard::UiStackView::len(nav.rows))
            }),
            _ => false,
        }
    }

    /// Check one navigation command against the stack as it is now; the bytes
    /// it will copy.
    pub(super) fn validate_nav(&self, command: WireCommand<'_>) -> Result<usize, i32> {
        let nav = self.nav()?;
        let hop = self.ui.as_ref().map_or(0, |ui| ui.hop());
        match command {
            WireCommand::Close { instance } => {
                ui_guard::close_allowed(nav.rows, nav.owner, instance)?;
                Ok(0)
            }
            WireCommand::BackTo { target } => {
                crate::native_host::declarations::full_id(target)?;
                if let Some(index) = self.back_to_index(target) {
                    ui_guard::back_to_allowed(nav.rows, nav.owner, index)?;
                }
                Ok(target.len())
            }
            WireCommand::SetKeyLayer { layer } => {
                if !layer.is_empty() {
                    crate::native_host::declarations::full_id(layer)?;
                }
                self.own_top()?;
                Ok(layer.len())
            }
            WireCommand::ShowHints {} => Ok(0),
            WireCommand::RaiseInput { input } => {
                // A raised input is a chain step like an open.
                if hop >= 8 {
                    return Err(ERR_BUSY);
                }
                if input.is_empty() || input.len() > INPUT_MAX || !input.bytes().all(input_byte) {
                    return Err(ERR_INVALID);
                }
                self.raise_target(input)?;
                ui_guard::raise_allowed(nav.rows, nav.owner)?;
                Ok(input.len())
            }
            WireCommand::ScreenPolicy { instance, flags } => {
                self.policy_change(&nav, instance, flags).map(|_| 0)
            }
            // The submitter's own top instance, else `ERR_STATE`.
            WireCommand::ScreenReady {} => self.own_top().map(|_| 0),
            _ => Err(paperpaw_native::host_glue::ERR_UNSUPPORTED),
        }
    }

    /// The change `command.screen_policy` asks for, checked against the stack
    /// as it is now: the instance (0 is the submitter's own top one) and the
    /// policy bits it ends with.
    pub(super) fn policy_change(
        &self,
        nav: &NavStack<'_>,
        instance: u32,
        flags: u32,
    ) -> Result<PolicyChange, i32> {
        if flags & !POLICY_BITS != 0 {
            return Err(ERR_INVALID);
        }
        let instance = if instance == 0 {
            self.own_top()?.serial()
        } else {
            instance
        };
        ui_guard::policy_allowed(nav.rows, nav.owner, instance, flags & POLICY_GUARDS != 0)?;
        Ok(PolicyChange {
            instance,
            flags,
            ready: false,
        })
    }

    /// Makes room for the policy changes of `batch`.
    #[inline(never)]
    pub(super) fn reserve_policies(&mut self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let count = batch
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    WireCommand::ScreenPolicy { .. } | WireCommand::ScreenReady {}
                )
            })
            .count();
        self.staged_policies
            .try_reserve(count)
            .map_err(|_| paperpaw_native::host_glue::ERR_NO_MEMORY)
    }

    /// Stages the checked changes of a batch (room was reserved).
    #[inline(never)]
    pub(super) fn stage_policy(&mut self, command: &WireCommand<'_>) -> Result<(), i32> {
        let nav = self.nav()?;
        let change = match *command {
            WireCommand::ScreenPolicy { instance, flags } => {
                self.policy_change(&nav, instance, flags)?
            }
            WireCommand::ScreenReady {} => PolicyChange {
                instance: self.own_top()?.serial(),
                flags: 0,
                ready: true,
            },
            _ => return Ok(()),
        };
        self.staged_policies.push(change);
        Ok(())
    }

    /// The checked policy changes, in order, for the host to apply before the
    /// callback's other commands ([`Kernel::native_screen_policies`]).
    #[must_use]
    #[allow(
        dead_code,
        reason = "the command runner (shell_commands) calls it, wired with the shell"
    )]
    pub(crate) fn take_policies(&mut self) -> Vec<PolicyChange> {
        core::mem::take(&mut self.staged_policies)
    }

    /// Whose input `input` is: the Power key and the escape are the user's
    /// alone, `input.<plugin>.<name>` and `gesture.<plugin>.<name>` their
    /// plugin's own; the host's are anyone's.
    fn raise_target(&self, input: &str) -> Result<(), i32> {
        // Every form of the Power key (`button.`, `long.`, `double.`) and the
        // escape chord; a name no key map reads is `ERR_INVALID`.
        if input.starts_with("escape") || input.starts_with("chord.") {
            return Err(ERR_DENIED);
        }
        match crate::keymap::InputKey::parse(input) {
            Some(
                crate::keymap::InputKey::Button(name)
                | crate::keymap::InputKey::LongButton(name)
                | crate::keymap::InputKey::DoubleButton(name),
            ) if name == "power" => return Err(ERR_DENIED),
            Some(_) => {}
            None if input.starts_with("input.") || input.starts_with("gesture.") => {}
            None => return Err(ERR_INVALID),
        }
        for kind in ["input.", "gesture."] {
            if let Some(rest) = input.strip_prefix(kind) {
                let plugin = rest.split('.').next().unwrap_or_default();
                return if plugin == self.owner.as_str() {
                    Ok(())
                } else {
                    Err(ERR_DENIED)
                };
            }
        }
        Ok(())
    }

    /// The staged form of a checked navigation command.
    pub(super) fn owned_nav(&self, command: WireCommand<'_>) -> Result<Command, i32> {
        match command {
            WireCommand::Close { instance } => {
                let nav = self.nav()?;
                match ui_guard::close_allowed(nav.rows, nav.owner, instance)? {
                    Close::Instance(index) => {
                        let row = ui_guard::UiStackView::row(nav.rows, index).ok_or(ERR_STATE)?;
                        Ok(Command::Close(ScreenKey::new(row.instance)))
                    }
                    Close::NoEffect => Err(ERR_STATE),
                }
            }
            WireCommand::BackTo { target } => {
                let index = self.back_to_index(target).ok_or(ERR_NOT_FOUND)?;
                let ui = self.ui.as_ref().ok_or(ERR_STATE)?;
                let entry = ui.env().stack().get(index).ok_or(ERR_NOT_FOUND)?;
                Ok(Command::BackTo(ui::copy_id(entry.screen.as_str())?))
            }
            WireCommand::SetKeyLayer { layer } => {
                let key = self.own_top()?;
                let layer = if layer.is_empty() {
                    None
                } else {
                    Some(ui::copy_id(layer)?)
                };
                Ok(Command::SetKeyLayer(key, layer))
            }
            WireCommand::ShowHints {} => Ok(Command::ShowHints),
            WireCommand::RaiseInput { input } => {
                let mut name = alloc::string::String::new();
                name.try_reserve_exact(input.len())
                    .map_err(|_| paperpaw_native::host_glue::ERR_NO_MEMORY)?;
                name.push_str(input);
                Ok(Command::Input(name))
            }
            _ => Err(paperpaw_native::host_glue::ERR_UNSUPPORTED),
        }
    }
}

#[cfg(test)]
#[path = "commands_nav_tests.rs"]
mod tests;
