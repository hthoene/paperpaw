//! Host-only UI authority and deferred redraw effects for one callback.
use super::*;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_platform::RefreshMode;
use paperpaw_sdk::{Env, IdError, Params, ScreenKey};

pub(super) struct UiContext<'a> {
    env: &'a Env<'a>,
    screen: Option<ScreenKey>,
    hop: u8,
}

/// Deferred redraw retains the original screen instance, never a later top one.
pub(crate) struct Redraw {
    pub(crate) screen: ScreenKey,
    pub(crate) mode: Option<RefreshMode>,
}

impl<'a> CommandStage<'a> {
    pub(crate) fn with_ui(mut self, env: &'a Env<'a>, screen: Option<ScreenKey>, hop: u8) -> Self {
        self.ui = Some(UiContext { env, screen, hop });
        self
    }

    /// UI callers drain this form so redraw-only never becomes a refresh.
    pub(crate) fn into_ui_output(self) -> (Vec<Command>, Option<Redraw>, u8) {
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

impl crate::Kernel {
    /// Drain after foreign code and all borrowed callback scopes have returned.
    pub(crate) fn drain_native_ui(
        &mut self,
        owner: &Id,
        commands: Vec<Command>,
        redraw: Option<Redraw>,
    ) {
        for command in commands {
            if let Command::Close(key) = &command
                && !self
                    .shared
                    .stack
                    .iter()
                    .any(|entry| entry.key == *key && entry.screen.namespace() == owner.as_str())
            {
                self.warn_runtime("native close dropped: its screen no longer exists");
                continue;
            }
            self.apply(command);
        }
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

    #[test]
    fn open_then_close_uses_original_target_or_warns_when_already_closed() {
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
        for already_closed in [false, true] {
            let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
                .plugin(Screens)
                .build()
                .unwrap();
            for id in ["probe.base", "probe.old"] {
                kernel.apply(Command::OpenScreen(Id::new_static(id), Params::new()));
            }
            let original = kernel.shared.stack.last().unwrap().key;
            let owner = Id::new_static("probe");
            let (commands, redraw, _) =
                {
                    let env = kernel.shared.env(Instant(0));
                    let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
                        .with_ui(&env, Some(original), 0);
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
        assert_eq!(redraw.mode, Some(RefreshMode::Partial));
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
