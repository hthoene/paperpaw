//! Host-only UI authority and deferred redraw effects for one callback.
use super::*;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_sdk::{Env, IdError, Params, ScreenKey};

pub(super) struct UiContext<'a> {
    env: &'a Env<'a>,
    screen: Option<ScreenKey>,
    hop: u8,
}

/// Deferred redraw retains the original screen instance, never a later top one.
pub(crate) struct Redraw {
    pub(crate) screen: ScreenKey,
    pub(crate) mode: u32,
}

impl<'a> CommandStage<'a> {
    pub(crate) fn with_ui(mut self, env: &'a Env<'a>, screen: Option<ScreenKey>, hop: u8) -> Self {
        self.ui = Some(UiContext { env, screen, hop });
        self
    }

    /// UI callers drain this form so redraw-only never becomes a refresh.
    pub(crate) fn into_ui_output(self) -> (Vec<Command>, Option<Redraw>, u8) {
        let screen = self.ui.as_ref().and_then(|ui| ui.screen);
        let redraw = self
            .redraw
            .zip(screen)
            .map(|(mode, screen)| Redraw { screen, mode });
        (self.staged, redraw, self.ui.map_or(0, |ui| ui.hop))
    }

    fn own_top(&self) -> Result<ScreenKey, i32> {
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
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    pub(super) fn owned_ui(&self, command: WireCommand<'_>) -> Result<Command, i32> {
        match command {
            WireCommand::OpenScreen { screen } => {
                Ok(Command::OpenScreen(copy_id(screen)?, Params::new()))
            }
            WireCommand::RunAction { action } => {
                Ok(Command::RunAction(copy_id(action)?, Params::new()))
            }
            WireCommand::CloseScreen => Ok(Command::Close(self.own_top()?)),
            _ => Err(ERR_UNSUPPORTED),
        }
    }
}

pub(super) fn copy_id(text: &str) -> Result<Id, i32> {
    Id::try_parse(text).map_err(|error| match error {
        IdError::NoMemory => ERR_NO_MEMORY,
        _ => ERR_INVALID,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
    use super::*;
    use paperpaw_platform::Instant;

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
        let mut stage =
            CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(&env, Some(key), 0);
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
        assert_eq!(redraw.mode, 2);
        assert_eq!(hop, 0);

        let foreign = Id::new_static("other");
        let mut stage =
            CommandStage::new(&foreign, env.settings(), None, 8, 4096).with_ui(&env, Some(key), 0);
        assert_eq!(
            submit(&mut stage, &[WireCommand::Redraw { mode: 2 }]),
            ERR_STATE
        );
        assert_eq!(submit(&mut stage, &[WireCommand::CloseScreen]), ERR_STATE);
        let mut stage =
            CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(&env, Some(key), 8);
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
}
