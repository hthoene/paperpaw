//! What the X4 shells change by the power level (ADR 0025), as plugins see
//! it in `Capabilities::power`, so that the power settings show only what
//! applies on the device.
//!
//! - `buttons`: how often the buttons are read follows the level (every
//!   50 ms in Saving and Offline), on all of them.
//! - `frontlight`: the X4 Pro's light follows it, dimmer and off sooner
//!   (the modes' presets set `x4.frontlight`); only the Pro has a light.
//! - Not `radio`: the radio's own saving follows whether the network is
//!   busy, not the level.
//! - `cpu_clock`: on the ESP32-C3 devices (X4, X3) the processor works at
//!   160 MHz and waits at a slow clock, slower in Saving and Offline; the
//!   Performance mode keeps 160 MHz ([`clock_policy`], #452). The ESP32-S3
//!   devices stay at their maximum.

use paperpaw_sdk::power::{Power, PowerControls, PowerLevel};

/// The power mode that keeps the processor at its full clock: a built-in
/// preset of the kind `core.power-mode` at the level Normal
/// (`paperpaw_core`'s `POWER_MODES`).
pub const PERFORMANCE_MODE: &str = "core.performance";

/// The controls of a device, `frontlight` where it has a front light,
/// `cpu_clock` where the clock follows the work.
pub fn power_controls(frontlight: bool, cpu_clock: bool) -> PowerControls {
    let mut controls = PowerControls::default();
    controls.buttons = true;
    controls.frontlight = frontlight;
    controls.cpu_clock = cpu_clock;
    controls
}

/// How the processor's clock follows the work, by the power mode worn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockPolicy {
    /// Always the full clock: the Performance mode.
    Full,
    /// The full clock while working, the crystal's divided by
    /// [`waiting_divider`](Self::waiting_divider) while waiting.
    OnDemand {
        /// 2 in Normal (20 MHz from the X4's 40 MHz crystal), 4 in Saving
        /// and Offline (10 MHz).
        divider: u32,
    },
}

impl ClockPolicy {
    /// What the crystal is divided by while waiting; `None`: the full clock.
    #[must_use]
    pub const fn waiting_divider(self) -> Option<u32> {
        match self {
            Self::Full => None,
            Self::OnDemand { divider } => Some(divider),
        }
    }
}

/// The clock's policy for `power`: Performance keeps the full clock, Saving
/// and Offline wait at a slower one than Normal (and than any level added
/// later, which is not slowed).
#[must_use]
pub fn clock_policy(power: &Power) -> ClockPolicy {
    if power.mode.as_str() == PERFORMANCE_MODE {
        return ClockPolicy::Full;
    }
    let divider = match power.level {
        PowerLevel::Saving | PowerLevel::Offline => 4,
        _ => 2,
    };
    ClockPolicy::OnDemand { divider }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::Id;

    #[test]
    fn every_device_slows_its_buttons_and_only_the_pro_dims_a_light() {
        let plain = power_controls(false, true);
        assert!(plain.any() && plain.buttons && !plain.frontlight && plain.cpu_clock);
        let pro = power_controls(true, false);
        assert!(pro.buttons && pro.frontlight && !pro.cpu_clock);
        for controls in [plain, pro] {
            assert!(!controls.radio && !controls.system_saver);
        }
    }

    #[test]
    fn performance_keeps_the_full_clock_and_saving_waits_slower_than_normal() {
        let power = |level, mode: &'static str| Power::new(level, Id::new_static(mode), false, 1);
        let wait = |level, mode| clock_policy(&power(level, mode)).waiting_divider();
        assert_eq!(wait(PowerLevel::Normal, PERFORMANCE_MODE), None);
        assert_eq!(wait(PowerLevel::Normal, "core.normal"), Some(2));
        assert_eq!(wait(PowerLevel::Saving, "core.saving"), Some(4));
        assert_eq!(wait(PowerLevel::Offline, "core.offline"), Some(4));
        // A mode of a data plugin's at the level Normal: on demand, as Normal.
        assert_eq!(wait(PowerLevel::Normal, "night.reading"), Some(2));
    }
}
