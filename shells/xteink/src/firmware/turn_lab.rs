//! LAB ONLY (#40, feature `turn-lab`, off by default and in no release): how
//! a page turn (`RefreshMode::Partial`) is shown, by the preset the setting
//! `x4.turn-lab` names (Settings > Display; the presets of the panel's
//! controller are in `lab_presets`). The setting is read before each turn, so
//! a change shows at once. Every turn logs
//! `turn-lab: <preset> turn K [clean]: shown in X ms`; the time also goes to
//! the lab's screen.

#[cfg(not(feature = "turn-lab"))]
pub(super) use off::*;
#[cfg(feature = "turn-lab")]
pub(super) use on::*;

#[cfg(feature = "turn-lab")]
mod on {
    use paperpaw_platform::RefreshMode;
    use paperpaw_x4::display::Refresh;
    use paperpaw_x4::panel::Experiment;

    use crate::firmware::Firmware;
    use crate::lab_presets;
    use log::info;

    /// Which preset runs and how many turns it has shown since the last clean.
    #[derive(Debug, Default)]
    pub(in crate::firmware) struct Lab {
        name: &'static str,
        turns: u32,
    }

    /// What the lab does with the turn being shown.
    #[derive(Debug, Clone, Copy)]
    pub(in crate::firmware) struct Turn {
        name: &'static str,
        number: u32,
        experiment: Option<Experiment>,
    }

    impl Firmware {
        /// The next page turn, if it is one of a fast refresh and a preset is
        /// set: a change of preset starts the count again; a clean turn is a
        /// half refresh.
        pub(in crate::firmware) fn turn_lab(
            &mut self,
            mode: RefreshMode,
            refresh: &mut Refresh,
        ) -> Option<Turn> {
            if mode != RefreshMode::Partial {
                return None;
            }
            if *refresh != Refresh::Fast {
                // Every pixel is driven anyway: nothing is left to clean.
                self.state.turn_lab.turns = 0;
                return None;
            }
            let value = self.kernel.settings().read(&lab_presets::PRESET);
            let name = lab_presets::choices()
                .iter()
                .copied()
                .find(|c| *c == value)?;
            let preset = lab_presets::preset(name);
            let lab = &mut self.state.turn_lab;
            if lab.name != name {
                lab.name = name;
                lab.turns = 0;
            }
            lab.turns += 1;
            let number = lab.turns;
            if preset.clean_every > 0 && number.is_multiple_of(preset.clean_every) {
                *refresh = Refresh::Half;
                lab.turns = 0;
            }
            Some(Turn {
                name,
                number,
                experiment: preset.experiment,
            })
        }

        /// Sets the preset's waveform for a page turn shown as a fast refresh.
        pub(in crate::firmware) fn turn_lab_waveform(
            &mut self,
            turn: Option<Turn>,
            refresh: Refresh,
        ) {
            if refresh == Refresh::Fast
                && let Some(turn) = turn
                && turn.experiment.is_some()
            {
                let _supported = self.panel.set_experiment(turn.experiment);
            }
        }
    }

    /// Logs the page turn and the time the panel took.
    pub(in crate::firmware) fn log(turn: Option<Turn>, refresh: Refresh, shown_ms: u64) {
        if let Some(turn) = turn {
            paperpaw_core::turn_lab::set_last_ms(u32::try_from(shown_ms).unwrap_or(u32::MAX));
            info!(
                "turn-lab: {} turn {}{}: shown in {shown_ms} ms",
                turn.name,
                turn.number,
                if refresh == Refresh::Fast {
                    ""
                } else {
                    " clean"
                },
            );
        }
    }
}

#[cfg(not(feature = "turn-lab"))]
mod off {
    use paperpaw_platform::RefreshMode;
    use paperpaw_x4::display::Refresh;

    use crate::firmware::Firmware;

    /// Never made: without the lab there is no turn to speak of.
    pub(in crate::firmware) type Turn = core::convert::Infallible;

    impl Firmware {
        #[inline(always)]
        pub(in crate::firmware) fn turn_lab(
            &mut self,
            _: RefreshMode,
            _: &mut Refresh,
        ) -> Option<Turn> {
            None
        }

        #[inline(always)]
        pub(in crate::firmware) fn turn_lab_waveform(&mut self, _: Option<Turn>, _: Refresh) {}
    }

    #[inline(always)]
    pub(in crate::firmware) fn log(_: Option<Turn>, _: Refresh, _: u64) {}
}
