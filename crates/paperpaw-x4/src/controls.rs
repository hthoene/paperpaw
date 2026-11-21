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
//! - `cpu_clock`: on the ESP32-C3 devices the processor waits at a slow clock
//!   (slower in Saving and Offline), and the Performance mode keeps the full
//!   one (#452); the ESP32-S3 devices stay at the maximum.

use paperpaw_sdk::power::PowerControls;

/// The power mode that keeps the processor at its full clock (a built-in
/// preset of the kind `core.power-mode`, `paperpaw_core`'s `POWER_MODES`).
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
