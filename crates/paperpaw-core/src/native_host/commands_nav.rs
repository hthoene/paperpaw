//! The navigation commands of slot 15 on the kernel's own screen stack
//! (`docs/abi/ui.md` 1.3): `close` (1548), `back_to` (1549), `set_key_layer`
//! (1550), `show_hints` (1551) and `raise_input` (1552). The guard rules are
//! [`ui_guard`](crate::native_host::ui_guard)'s, read from the stack the
//! kernel keeps ([`StackRows`]); the stage judges at submit and the drain
//! applies as the interface-6 commands (`Close`, `BackTo`, `SetKeyLayer`,
//! `ShowHints`, `Input`). A command whose effect is nothing (the root
//! screen, a `back_to` with nothing open above) is `OK` and stages nothing.
//!
//! Not here: `screen_policy` (1563, a run-time change of a screen's flags)
//! stays `ERR_UNSUPPORTED`.
use super::{CommandStage, WireCommand, ui};
use crate::native_host::{
    stack_rows::StackRows,
    ui_guard::{self, Close},
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::{Command, ScreenKey};

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
            _ => Err(paperpaw_native::host_glue::ERR_UNSUPPORTED),
        }
    }

    /// Whose input `input` is: the Power key and the escape are the user's
    /// alone, `input.<plugin>.<name>` and `gesture.<plugin>.<name>` their
    /// plugin's own; the host's are anyone's.
    fn raise_target(&self, input: &str) -> Result<(), i32> {
        if input == "button.power" || input.starts_with("escape") || input.starts_with("chord.") {
            return Err(ERR_DENIED);
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
